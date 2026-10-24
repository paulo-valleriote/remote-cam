//! Virtual camera registration driver (T031/T032).
//!
//! **NON-PRODUCTION** — F0 risk-reduction spike.
//!
//! Registers the spike camera per-user (`HKCU`), reports where each bitness was
//! written and whether an administrator was needed, optionally creates the
//! camera instance and keeps it alive long enough for a consumer to enumerate
//! it, then removes everything again. `register.ps1` calls this so the Q-1 and
//! Q-8 answers are produced by the same code the product would ship.
//!
//! ```text
//! cargo run -p remotecam-injection --example vcam_register -- \
//!     --register-only --dll target/x86_64-pc-windows-msvc/debug/remotecam_injection.dll
//! cargo run -p remotecam-injection --example vcam_register -- \
//!     --dll target/x86_64-pc-windows-msvc/debug/remotecam_injection.dll \
//!     --hold-seconds 60 --json spikes/evidence/injection/registration.json
//! cargo run -p remotecam-injection --example vcam_register -- --unregister
//! ```

#[cfg(not(windows))]
fn main() {
    eprintln!(
        "vcam_register only runs on Windows: a virtual camera cannot be registered \
         on this host. Cross-check with: cargo check -p remotecam-injection \
         --target x86_64-pc-windows-msvc"
    );
    std::process::exit(2);
}

#[cfg(windows)]
fn main() {
    if let Err(error) = windows_impl::run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

#[cfg(windows)]
mod windows_impl {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use remotecam_injection::registration::{
        FRIENDLY_NAME, register_com_current_user, register_com_machine_wide, register_current_user,
        register_machine_wide, unregister_current_user, unregister_machine_wide,
    };

    const USAGE: &str = "\
    Usage: vcam_register [options]

      --dll <path>         the 64-bit remotecam_injection.dll to register
      --dll32 <path>       the optional 32-bit build, for 32-bit consumers
      --friendly-name <s>  camera name shown to consumers
      --register-only      write the COM registration and exit
      --machine-wide       use HKLM instead of HKCU (requires an administrator)
      --hold-seconds <s>   keep the started camera alive for s seconds
      --unregister         remove both registrations and exit
      --json <path>        write the registration report as JSON
      --help               show this message";

    #[derive(Default)]
    struct Options {
        dll: Option<PathBuf>,
        dll32: Option<PathBuf>,
        friendly_name: Option<String>,
        register_only: bool,
        machine_wide: bool,
        hold_seconds: u64,
        unregister: bool,
        json_path: Option<PathBuf>,
    }

    pub fn run() -> Result<(), String> {
        let options = parse_options(std::env::args_os().skip(1))?;

        if options.unregister {
            unregister_current_user().map_err(|error| error.to_string())?;
            unregister_machine_wide().map_err(|error| error.to_string())?;
            println!("{{\"unregistered\": true}}");
            return Ok(());
        }

        let dll = options
            .dll
            .clone()
            .ok_or("--dll is required to register the media source")?;
        let friendly = options
            .friendly_name
            .clone()
            .unwrap_or_else(|| FRIENDLY_NAME.to_string());
        let dll32 = options.dll32.as_deref();

        if options.register_only {
            let report = if options.machine_wide {
                register_com_machine_wide(&dll, dll32)
            } else {
                register_com_current_user(&dll, dll32)
            }
            .map_err(|error| error.to_string())?;
            let summary = report_summary(&friendly, &report, false);
            write_summary(&options, &summary)?;
            print!("{summary}");
            return Ok(());
        }

        let (report, camera) = if options.machine_wide {
            register_machine_wide(&friendly, &dll, dll32)
        } else {
            register_current_user(&friendly, &dll, dll32).map_err(|error| {
                eprintln!(
                    "per-user registration was refused ({error}); a matrix cell cannot \
                     be concluded from an elevated fallback without recording it"
                );
                error
            })
        }
        .map_err(|error| error.to_string())?;

        let source = camera.media_source().map_err(|error| error.to_string())?;
        let _ = &source;
        let summary = report_summary(&friendly, &report, true);
        write_summary(&options, &summary)?;
        print!("{summary}");

        if options.hold_seconds > 0 {
            println!(
                "the camera stays registered for {} s so a consumer can enumerate it",
                options.hold_seconds
            );
            std::thread::sleep(std::time::Duration::from_secs(options.hold_seconds));
        }
        camera.remove().map_err(|error| error.to_string())?;
        Ok(())
    }

    fn report_summary(
        friendly: &str,
        report: &remotecam_injection::registration::RegistrationReport,
        media_source_created: bool,
    ) -> String {
        format!(
            "{{\n  \"backend\": \"mf_virtual_camera\",\n  \"friendly_name\": \"{}\",\n  \
             \"registration\": {},\n  \"media_source_created\": {}\n}}\n",
            friendly,
            report.to_json(),
            media_source_created,
        )
    }

    fn write_summary(options: &Options, summary: &str) -> Result<(), String> {
        if let Some(path) = &options.json_path {
            std::fs::write(path, summary)
                .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        }
        Ok(())
    }

    fn parse_options(args: impl Iterator<Item = OsString>) -> Result<Options, String> {
        let mut options = Options::default();
        let mut args = args;

        while let Some(argument) = args.next() {
            let mut value = || {
                args.next()
                    .ok_or_else(|| format!("{} needs a value", argument.to_string_lossy()))
            };
            match argument.to_string_lossy().as_ref() {
                "--dll" => options.dll = Some(PathBuf::from(value()?)),
                "--dll32" => options.dll32 = Some(PathBuf::from(value()?)),
                "--friendly-name" => {
                    options.friendly_name = Some(value()?.to_string_lossy().into_owned());
                }
                "--register-only" => options.register_only = true,
                "--machine-wide" => options.machine_wide = true,
                "--hold-seconds" => {
                    options.hold_seconds = value()?
                        .to_string_lossy()
                        .parse()
                        .map_err(|_| "invalid --hold-seconds")?;
                }
                "--unregister" => options.unregister = true,
                "--json" => options.json_path = Some(PathBuf::from(value()?)),
                "--help" | "-h" => {
                    println!("{USAGE}");
                    std::process::exit(0);
                }
                other => return Err(format!("unknown option {other}")),
            }
        }

        if options.unregister && (options.register_only || options.machine_wide) {
            return Err("--unregister removes every registration and takes no other mode".into());
        }
        if options.register_only && options.hold_seconds > 0 {
            return Err("--register-only does not start a camera, so it cannot hold one".into());
        }
        Ok(options)
    }
}
