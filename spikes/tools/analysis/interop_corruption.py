#!/usr/bin/env python3
"""
Corruption detection for codec interoperability evidence.

Detects:
- Black frames (all pixels zero)
- Green/magenta frames (swapped chroma)
- Frozen frames (consecutive identical frames)
- Longest freeze duration

A negotiation success with visible corruption is a failure.
"""

import json
import numpy as np
import cv2
import argparse
from pathlib import Path
import sys


class CorruptionDetector:
    """Detects various types of video corruption in captured frames."""

    def __init__(
        self,
        black_threshold: float = 1.0,
        green_magenta_threshold: float = 50.0,
        freeze_threshold: float = 0.995,
        min_freeze_frames: int = 3
    ):
        self.black_threshold = black_threshold
        self.green_magenta_threshold = green_magenta_threshold
        self.freeze_threshold = freeze_threshold
        self.min_freeze_frames = min_freeze_frames

    def analyze_video(self, video_path: str) -> dict:
        """
        Analyze a video file for corruption.

        Returns:
            Dictionary with corruption analysis results.
        """
        cap = cv2.VideoCapture(video_path)
        if not cap.isOpened():
            raise ValueError(f"Cannot open video: {video_path}")

        frame_count = int(cap.get(cv2.CAP_PROP_FRAME_COUNT))
        fps = cap.get(cv2.CAP_PROP_FPS)
        width = int(cap.get(cv2.CAP_PROP_FRAME_WIDTH))
        height = int(cap.get(cv2.CAP_PROP_FRAME_HEIGHT))

        print(f"Analyzing {video_path}: {frame_count} frames, {fps} fps, {width}x{height}")

        black_frames = 0
        green_frames = 0
        magenta_frames = 0
        frozen_frames = 0
        freeze_streaks = []

        prev_frame = None
        current_freeze_streak = 0

        frame_idx = 0
        while True:
            ret, frame = cap.read()
            if not ret:
                break

            # Check for black frame
            if self._is_black_frame(frame):
                black_frames += 1

            # Check for green/magenta (swapped chroma)
            green, magenta = self._check_chroma_swap(frame)
            if green:
                green_frames += 1
            if magenta:
                magenta_frames += 1

            # Check for frozen frame
            if prev_frame is not None:
                if self._is_frozen_frame(frame, prev_frame):
                    current_freeze_streak += 1
                else:
                    if current_freeze_streak >= self.min_freeze_frames:
                        freeze_streaks.append(current_freeze_streak)
                        frozen_frames += current_freeze_streak
                    current_freeze_streak = 0
            prev_frame = frame.copy()

            frame_idx += 1
            if frame_idx % 300 == 0:
                print(f"  Processed {frame_idx}/{frame_count} frames...")

        # Handle final freeze streak
        if current_freeze_streak >= self.min_freeze_frames:
            freeze_streaks.append(current_freeze_streak)
            frozen_frames += current_freeze_streak

        cap.release()

        # Compute longest freeze in milliseconds
        longest_freeze_ms = 0
        if freeze_streaks and fps > 0:
            longest_freeze_ms = int(max(freeze_streaks) * 1000 / fps)

        total_corrupted = black_frames + green_frames + magenta_frames + frozen_frames
        corruption_detected = total_corrupted > 0

        result = {
            "video_path": video_path,
            "frame_count": frame_count,
            "fps": fps,
            "width": width,
            "height": height,
            "black_frames": black_frames,
            "green_frames": green_frames,
            "magenta_frames": magenta_frames,
            "frozen_frames": frozen_frames,
            "freeze_streaks": freeze_streaks,
            "longest_freeze_ms": longest_freeze_ms,
            "corruption_detected": corruption_detected,
            "total_corrupted_frames": total_corrupted,
            "corruption_rate": total_corrupted / frame_count if frame_count > 0 else 0,
        }

        return result

    def _is_black_frame(self, frame: np.ndarray) -> bool:
        """Check if frame is essentially black (all pixels near zero)."""
        mean_val = np.mean(frame)
        return mean_val < self.black_threshold

    def _check_chroma_swap(self, frame: np.ndarray) -> tuple[bool, bool]:
        """
        Check for green/magenta tint indicating swapped chroma (U/V planes).
        This happens when NV21 is incorrectly de-interleaved as I420.
        """
        # Convert to HSV for better color detection
        hsv = cv2.cvtColor(frame, cv2.COLOR_BGR2HSV)

        # Green hue range (around 60 in HSV)
        green_mask = cv2.inRange(hsv, (35, 50, 50), (85, 255, 255))
        green_ratio = np.sum(green_mask > 0) / (frame.shape[0] * frame.shape[1])

        # Magenta hue range (around 150 in HSV for OpenCV's 0-180 scale)
        magenta_mask1 = cv2.inRange(hsv, (130, 50, 50), (170, 255, 255))
        magenta_ratio = np.sum(magenta_mask1 > 0) / (frame.shape[0] * frame.shape[1])

        is_green = green_ratio > (self.green_magenta_threshold / 100.0)
        is_magenta = magenta_ratio > (self.green_magenta_threshold / 100.0)

        return is_green, is_magenta

    def _is_frozen_frame(self, frame1: np.ndarray, frame2: np.ndarray) -> bool:
        """Check if two consecutive frames are nearly identical."""
        # Compute structural similarity or simple difference
        diff = cv2.absdiff(frame1, frame2)
        mean_diff = np.mean(diff)
        max_possible_diff = 255.0
        similarity = 1.0 - (mean_diff / max_possible_diff)
        return similarity > self.freeze_threshold


def main():
    parser = argparse.ArgumentParser(description="Detect video corruption in interop captures")
    parser.add_argument("video_path", help="Path to video file to analyze")
    parser.add_argument("--output", "-o", help="Output JSON file for results")
    parser.add_argument("--observer", help="Named observer for the analysis")
    args = parser.parse_args()

    detector = CorruptionDetector()
    try:
        result = detector.analyze_video(args.video_path)
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)

    # Add observer if provided
    if args.observer:
        result["observer"] = args.observer

    # Print summary
    print("\n=== Corruption Analysis Results ===")
    print(f"Total frames: {result['frame_count']}")
    print(f"Black frames: {result['black_frames']}")
    print(f"Green frames (chroma swap): {result['green_frames']}")
    print(f"Magenta frames (chroma swap): {result['magenta_frames']}")
    print(f"Frozen frames: {result['frozen_frames']}")
    print(f"Freeze streaks: {result['freeze_streaks']}")
    print(f"Longest freeze: {result['longest_freeze_ms']} ms")
    print(f"Corruption detected: {result['corruption_detected']}")
    print(f"Corruption rate: {result['corruption_rate']:.4f}")

    # Write output if requested
    if args.output:
        with open(args.output, 'w') as f:
            json.dump(result, f, indent=2)
        print(f"\nResults written to {args.output}")

    # Exit with error code if corruption detected
    if result['corruption_detected']:
        print("\n❌ CORRUPTION DETECTED - Interop FAILED")
        sys.exit(1)
    else:
        print("\n✅ No corruption detected - Interop PASSED")
        sys.exit(0)


if __name__ == "__main__":
    main()
