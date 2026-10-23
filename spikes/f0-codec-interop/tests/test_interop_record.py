#!/usr/bin/env python3
"""
Tests for the codec interoperability evidence validation.

Validates that:
- Negotiated profile/level/format are present in evidence
- 10-minute duration is enforced
- Fallback record carries CPU, latency, quality, and hardware-acceleration boolean
"""

import json
import pytest
from pathlib import Path


class TestInteropEvidence:
    """Test the codec interoperability evidence format and constraints."""

    def test_negotiated_parameters_present(self):
        """Negotiated profile/level/format must be present in evidence."""
        evidence_path = Path("spikes/evidence/interop/interop_record.json")
        if not evidence_path.exists():
            pytest.skip("Evidence file not yet generated")

        with open(evidence_path) as f:
            evidence = json.load(f)

        # Required fields for negotiated codec
        assert "negotiated_codec" in evidence, "negotiated_codec field missing"
        negotiated = evidence["negotiated_codec"]

        assert "name" in negotiated, "codec name missing"
        assert negotiated["name"] in ["H264", "VP8"], f"Unexpected codec: {negotiated['name']}"

        if negotiated["name"] == "H264":
            assert "profile" in negotiated, "H.264 profile missing"
            assert "level" in negotiated, "H.264 level missing"
            assert "format" in negotiated, "H.264 format (e.g., NV12) missing"

    def test_ten_minute_duration_enforced(self):
        """Evidence must show at least 10 minutes (600 seconds) of continuous operation."""
        evidence_path = Path("spikes/evidence/interop/interop_record.json")
        if not evidence_path.exists():
            pytest.skip("Evidence file not yet generated")

        with open(evidence_path) as f:
            evidence = json.load(f)

        assert "duration_s" in evidence, "duration_s field missing"
        assert evidence["duration_s"] >= 600, f"Duration {evidence['duration_s']}s < 600s (10 minutes)"

    def test_fallback_record_has_required_fields(self):
        """If fallback was used, record must carry CPU, latency, quality, and hw-accel."""
        evidence_path = Path("spikes/evidence/interop/interop_record.json")
        if not evidence_path.exists():
            pytest.skip("Evidence file not yet generated")

        with open(evidence_path) as f:
            evidence = json.load(f)

        if evidence.get("used_fallback", False):
            assert "fallback_comparison" in evidence, "fallback_comparison missing when fallback used"

            fb = evidence["fallback_comparison"]
            assert "cpu_pct_machine_p95" in fb, "CPU p95 missing in fallback record"
            assert "latency_p95_ms" in fb, "Latency p95 missing in fallback record"
            assert "quality_metric" in fb, "Quality metric missing in fallback record"
            assert "hardware_accelerated" in fb, "Hardware acceleration boolean missing in fallback record"
            assert isinstance(fb["hardware_accelerated"], bool), "hardware_accelerated must be boolean"

    def test_no_corruption_for_ten_minutes(self):
        """Evidence must confirm no visible corruption for the full duration."""
        evidence_path = Path("spikes/evidence/interop/interop_record.json")
        if not evidence_path.exists():
            pytest.skip("Evidence file not yet generated")

        with open(evidence_path) as f:
            evidence = json.load(f)

        assert "corruption_detected" in evidence, "corruption_detected field missing"
        assert evidence["corruption_detected"] is False, "Corruption detected during interop run"

        # Also verify the corruption analysis was run
        assert "corruption_analysis" in evidence, "corruption_analysis missing"
        analysis = evidence["corruption_analysis"]
        assert "black_frames" in analysis
        assert "green_frames" in analysis
        assert "frozen_frames" in analysis
        assert "longest_freeze_ms" in analysis
        assert analysis["longest_freeze_ms"] < 2000, "Freeze longer than 2 seconds detected"

    def test_hardware_decode_status_recorded(self):
        """Hardware decode status must be recorded for the selected codec."""
        evidence_path = Path("spikes/evidence/interop/interop_record.json")
        if not evidence_path.exists():
            pytest.skip("Evidence file not yet generated")

        with open(evidence_path) as f:
            evidence = json.load(f)

        assert "hardware_accelerated" in evidence, "hardware_accelerated field missing"
        assert isinstance(evidence["hardware_accelerated"], bool), "hardware_accelerated must be boolean"

        # If H.264 was negotiated, hardware decode must be true (precondition PC-1)
        if evidence.get("negotiated_codec", {}).get("name") == "H264":
            assert evidence["hardware_accelerated"] is True, "H.264 requires hardware decode (PC-1)"

    def test_evidence_validates_against_schema(self):
        """Evidence must validate against the interop schema."""
        # This will be implemented when the schema exists
        evidence_path = Path("spikes/evidence/interop/interop_record.json")
        if not evidence_path.exists():
            pytest.skip("Evidence file not yet generated")

        with open(evidence_path) as f:
            evidence = json.load(f)

        # Basic structure validation
        required_fields = [
            "run_id",
            "environment_id",
            "started_at",
            "ended_at",
            "duration_s",
            "negotiated_codec",
            "corruption_detected",
            "hardware_accelerated",
            "corruption_analysis",
        ]
        for field in required_fields:
            assert field in evidence, f"Required field missing: {field}"

    def test_preferred_codec_tested_first(self):
        """The preferred codec (H.264) must be tested before fallback."""
        evidence_path = Path("spikes/evidence/interop/interop_record.json")
        if not evidence_path.exists():
            pytest.skip("Evidence file not yet generated")

        with open(evidence_path) as f:
            evidence = json.load(f)

        # The preferred codec should be H.264
        assert "preferred_codec" in evidence, "preferred_codec field missing"
        assert evidence["preferred_codec"] == "H264", "H.264 must be the preferred codec"

        # If fallback happened, it should be explicitly recorded
        if evidence.get("used_fallback", False):
            assert evidence["negotiated_codec"]["name"] == "VP8", "Fallback should be VP8"


if __name__ == "__main__":
    pytest.main([__file__, "-v"])