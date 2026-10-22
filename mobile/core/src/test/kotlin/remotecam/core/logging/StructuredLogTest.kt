package remotecam.core.logging

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertTrue
import remotecam.protocol.ReasonCode

class StructuredLogTest {
    private fun record() = StructuredLog.at(
        "2026-11-03T09:14:22Z",
        Severity.INFO,
        "pairing.attempt",
        "00112233445566778899aabbccddeeff",
        Outcome.REJECTED,
    )

    @Test
    fun emitsTheContractFieldsAndOmitsAbsentOnes() {
        val plain = StructuredLog.toJson(record())
        assertTrue(plain.contains("\"severity\":\"info\""))
        assertTrue(plain.contains("\"operation\":\"pairing.attempt\""))
        assertTrue(plain.contains("\"outcome\":\"rejected\""))
        assertFalse(plain.contains("reason_code"))
        assertFalse(plain.contains("detail"))

        val withReason = StructuredLog.toJson(
            StructuredLog.withDetail(record().copy(reasonCode = ReasonCode.CERTIFICATE_PIN_MISMATCH), "mismatch"),
        )
        assertTrue(withReason.contains("\"reason_code\":\"certificate_pin_mismatch\""))
        assertTrue(withReason.contains("\"detail\":\"mismatch\""))
    }

    @Test
    fun rejectsInvalidOperationsAndCorrelationIds() {
        assertFailsWith<IllegalArgumentException> {
            StructuredLog.at(
                "2026-11-03T09:14:22Z",
                Severity.INFO,
                "Pairing attempt",
                "00112233445566778899aabbccddeeff",
                Outcome.STARTED,
            )
        }
        assertFailsWith<IllegalArgumentException> {
            StructuredLog.at("2026-11-03T09:14:22Z", Severity.INFO, "pairing.attempt", "XYZ", Outcome.STARTED)
        }
    }

    @Test
    fun redactsTokensFingerprintsAndPaths() {
        val token = "0123456789abcdef".repeat(4)
        assertEquals("token=[redacted]", StructuredLog.redact("token=$token"))
        assertEquals("read <path>", StructuredLog.redact("read /home/alice/secret.wav"))
        assertEquals("read <path>", StructuredLog.redact("read C:\\Users\\alice\\secret.wav"))
        assertEquals("path=<path>", StructuredLog.redact("path=/home/alice/x.wav"))
        assertEquals("line break", StructuredLog.redact("line\nbreak"))
        assertEquals("keep 0123456789abcdef", StructuredLog.redact("keep 0123456789abcdef"))
    }

    @Test
    fun refusesAnUnsafeDetailAfterRedaction() {
        val safe = StructuredLog.withDetail(record(), "presented ${"ab".repeat(32)}")
        assertTrue(safe.detail!!.contains("[redacted]"))
    }

    @Test
    fun truncatesLongDetails() {
        val record = StructuredLog.withDetail(record(), "x".repeat(500))
        assertEquals(200, record.detail!!.length)
    }
}
