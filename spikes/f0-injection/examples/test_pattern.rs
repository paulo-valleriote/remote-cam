//! Test-pattern producer for the virtual camera handoff (T033).
//!
//! Produces timestamped NV12 frames into the injection ring buffer at the
//! requested size and rate, with knobs for an injected stall and for duplicated
//! frames. A consumer thread drains the buffer and reports what it observed, so
//! the ring buffer contract is exercised end to end without a camera, a driver
//! or a Windows session.
//!
//! **NON-PRODUCTION** — F0 risk-reduction spike. This binary never feeds the
//! product: it is the stand-in for the Media Foundation media source.
//!
//! ```text
//! cargo run -p remotecam-injection --example test_pattern -- \
//!     --width 1280 --height 720 --duration-s 5 --json /tmp/pattern.json
//! cargo run -p remotecam-injection --example test_pattern -- \
//!     --stall-at 60 --stall-ms 2000 --duplicate-at 90
//! ```

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use remotecam_injection::ring_buffer::RingBuffer;
use remotecam_injection::test_pattern::{read_marker, render_frame};

const WRAP: u32 = 1 << 16;

#[derive(Debug, Clone)]
struct Options {
    width: u32,
    height: u32,
    fps: u32,
    duration_s: f64,
    capacity: usize,
    stall_at: Option<u32>,
    stall_ms: u64,
    duplicate_at: Option<u32>,
    json_path: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            fps: 30,
            duration_s: 5.0,
            capacity: 4,
            stall_at: None,
            stall_ms: 0,
            duplicate_at: None,
            json_path: None,
        }
    }
}

fn main() {
    let options = match parse_options(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("error: {message}\n\n{USAGE}");
            std::process::exit(2);
        }
    };

    if let Err(error) = run(&options) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

const USAGE: &str = "\
Usage: test_pattern [options]

  --width <px>          frame width (default 1280)
  --height <px>         frame height (default 720)
  --fps <n>             frames per second (default 30)
  --duration-s <s>      run length in seconds (default 5)
  --capacity <n>        ring buffer capacity in frames (default 4)
  --stall-at <frame>    inject a delivery stall before this frame
  --stall-ms <ms>       stall duration (default 2000)
  --duplicate-at <frame> publish this frame twice
  --json <path>         write the observation summary as JSON
  --help                show this message";

fn parse_options(args: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut options = Options::default();
    let mut args = args;

    while let Some(argument) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("{argument} needs a value"))
        };
        match argument.as_str() {
            "--width" => options.width = value()?.parse().map_err(|_| "invalid --width")?,
            "--height" => options.height = value()?.parse().map_err(|_| "invalid --height")?,
            "--fps" => options.fps = value()?.parse().map_err(|_| "invalid --fps")?,
            "--duration-s" => {
                options.duration_s = value()?.parse().map_err(|_| "invalid --duration-s")?
            }
            "--capacity" => {
                options.capacity = value()?.parse().map_err(|_| "invalid --capacity")?
            }
            "--stall-at" => {
                options.stall_at = Some(value()?.parse().map_err(|_| "invalid --stall-at")?)
            }
            "--stall-ms" => {
                options.stall_ms = value()?.parse().map_err(|_| "invalid --stall-ms")?
            }
            "--duplicate-at" => {
                options.duplicate_at = Some(value()?.parse().map_err(|_| "invalid --duplicate-at")?)
            }
            "--json" => options.json_path = Some(value()?),
            "--help" | "-h" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("unknown option {other}")),
        }
    }

    if options.width % 2 != 0 || options.height % 2 != 0 {
        return Err("NV12 requires even dimensions".into());
    }
    if options.fps == 0 {
        return Err("--fps must be positive".into());
    }
    if options.capacity == 0 {
        return Err("--capacity must be positive".into());
    }
    if options.stall_ms == 0 && options.stall_at.is_some() {
        options.stall_ms = 2_000;
    }
    Ok(options)
}

#[derive(Debug)]
struct Observation {
    consumed: u64,
    missing: u64,
    duplicates: u64,
    longest_stall_ms: f64,
    observed_duration_s: f64,
}

fn run(options: &Options) -> Result<(), String> {
    let buffer = Arc::new(RingBuffer::new(options.capacity).map_err(|error| error.to_string())?);
    let writer = buffer.writer();
    let mut reader = buffer.reader();
    let total_frames = (options.duration_s * options.fps as f64).round() as u32;
    let frame_interval = Duration::from_secs_f64(1.0 / options.fps as f64);

    let consumer = thread::spawn(move || {
        let mut observation = Observation {
            consumed: 0,
            missing: 0,
            duplicates: 0,
            longest_stall_ms: 0.0,
            observed_duration_s: 0.0,
        };
        let mut previous_marker: Option<u32> = None;
        let mut previous_arrival: Option<Instant> = None;
        let mut first_arrival: Option<Instant> = None;

        while let Some(frame) = reader.next() {
            let now = Instant::now();
            first_arrival.get_or_insert(now);
            if let Some(previous) = previous_arrival {
                let gap_ms = now.duration_since(previous).as_secs_f64() * 1000.0;
                observation.longest_stall_ms = observation.longest_stall_ms.max(gap_ms);
            }
            previous_arrival = Some(now);

            match read_marker(&frame) {
                Some(marker) => {
                    if let Some(previous) = previous_marker {
                        if marker == previous {
                            observation.duplicates += 1;
                        } else {
                            let expected = (previous + 1) % WRAP;
                            if marker != expected {
                                let skipped = (marker.wrapping_sub(expected)) % WRAP;
                                observation.missing += u64::from(skipped);
                            }
                        }
                    }
                    previous_marker = Some(marker);
                }
                None => return Err("a produced frame lost its marker".to_string()),
            }
            observation.consumed += 1;
        }

        if let (Some(first), Some(last)) = (first_arrival, previous_arrival) {
            observation.observed_duration_s = last.duration_since(first).as_secs_f64();
        }
        Ok(observation)
    });

    let started = Instant::now();
    let mut frame_no = 0u32;
    while frame_no < total_frames {
        let scheduled = started + frame_interval.mul_f64(frame_no as f64);
        let now = Instant::now();
        if scheduled > now {
            thread::sleep(scheduled - now);
        }

        if options.stall_at == Some(frame_no) {
            // Nothing is published while the producer is stalled.
            thread::sleep(Duration::from_millis(options.stall_ms));
        }

        let mut frame = render_frame(options.width, options.height, frame_no)
            .map_err(|error| error.to_string())?;
        frame.timestamp_ns = started.elapsed().as_nanos() as u64;
        writer.publish(frame).expect("buffer is open");

        if options.duplicate_at == Some(frame_no) {
            // The same picture is delivered twice, as a decoder would repeat a
            // frame after a lost packet.
            let repeat = render_frame(options.width, options.height, frame_no)
                .map_err(|error| error.to_string())?;
            writer.publish(repeat).expect("buffer is open");
        }

        frame_no += 1;
    }

    // Let the consumer drain the last published frame before releasing the
    // buffer, so the summary reports delivery rather than shutdown timing.
    thread::sleep(frame_interval * 2);
    buffer.shutdown();
    let observation = consumer
        .join()
        .map_err(|_| "the consumer thread panicked".to_string())??;
    let stats = buffer.stats();

    let summary = format!(
        concat!(
            "{{\n",
            "  \"width\": {},\n",
            "  \"height\": {},\n",
            "  \"fps\": {},\n",
            "  \"published\": {},\n",
            "  \"consumed\": {},\n",
            "  \"discarded_frames\": {},\n",
            "  \"missing_frames\": {},\n",
            "  \"duplicate_frames\": {},\n",
            "  \"observed_duration_s\": {:.3},\n",
            "  \"longest_stall_ms\": {:.3},\n",
            "  \"stall_injected_ms\": {},\n",
            "  \"duplicate_injected\": {}\n",
            "}}\n"
        ),
        options.width,
        options.height,
        options.fps,
        stats.published,
        observation.consumed,
        stats.discarded,
        observation.missing,
        observation.duplicates,
        observation.observed_duration_s,
        observation.longest_stall_ms,
        if options.stall_at.is_some() {
            options.stall_ms
        } else {
            0
        },
        options.duplicate_at.is_some(),
    );

    if let Some(path) = &options.json_path {
        std::fs::write(path, &summary).map_err(|error| format!("cannot write {path}: {error}"))?;
    }
    print!("{summary}");
    Ok(())
}
