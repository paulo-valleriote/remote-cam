package remotecam.protocol

import java.nio.file.Files
import java.nio.file.Path
import kotlin.test.Test
import kotlin.test.assertContains
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

class ContractTest {
    @Test
    fun everyPinnedVectorProducesItsExpectedOutcome() {
        val report = Conformance.run(Conformance.repositoryRoot().resolve("protocol/vectors"))
        assertTrue(report.passed(), "conformance failures: ${report.failures()}")
        assertEquals(11, report.qr.size)
        assertEquals(43, report.signaling.size)
        assertEquals(26, report.reasonCodes.count)
        assertTrue(report.reasonCodes.ok)

        System.getenv("CONFORMANCE_OUTPUT")?.let { output ->
            Files.write(Path.of(output), (report.toJson() + "\n").toByteArray())
        }
    }

    @Test
    fun reasonCodesAreAClosedSet() {
        assertEquals(26, ReasonCode.entries.size)
        assertNull(ReasonCode.fromWire("not_a_code"))
        for (code in ReasonCode.entries) {
            assertEquals(code, ReasonCode.fromWire(code.wireName))
        }
        assertTrue(ReasonCode.CERTIFICATE_PIN_MISMATCH.securityEvent)
        assertFalse(ReasonCode.RATE_LIMITED.securityEvent)
        assertEquals(
            listOf(ReasonCategory.SIGNALING_ERROR),
            ReasonCode.INVALID_MESSAGE.categories,
        )
    }

    @Test
    fun parsesEveryMessageType() {
        val messages = listOf(
            """{"version":1,"type":"pair","token":"${"00".repeat(32)}","dtls_fingerprint":"${"aa".repeat(32)}","limits":{}}""",
            """{"version":1,"type":"paired","session_id":"${"00".repeat(16)}","epoch":1,"dtls_fingerprint":"${"aa".repeat(32)}","accepted_limits":{}}""",
            """{"version":1,"type":"offer","epoch":1,"sdp":"v=0"}""",
            """{"version":1,"type":"answer","epoch":1,"sdp":"v=0"}""",
            """{"version":1,"type":"ice","epoch":1,"candidates":[]}""",
            """{"version":1,"type":"stats"}""",
            """{"version":1,"type":"close","code":"timeout"}""",
            """{"version":1,"type":"error","code":"internal_error"}""",
            """{"version":1,"type":"ping"}""",
            """{"version":1,"type":"pong"}""",
        )
        for (message in messages) {
            assertIs<SignalingParseResult.Accepted>(SignalingParser.parse(message), message)
        }
    }

    @Test
    fun rejectsUnknownFieldsAndNewerVersions() {
        assertEquals(
            ReasonCode.UNSUPPORTED_VERSION,
            (SignalingParser.parse("""{"version":2,"type":"ping"}""") as SignalingParseResult.Rejected).reason,
        )
        assertEquals(
            ReasonCode.INVALID_MESSAGE,
            (SignalingParser.parse("""{"version":1,"type":"ping","x":1}""") as SignalingParseResult.Rejected).reason,
        )
        assertEquals(
            ReasonCode.INVALID_MESSAGE,
            (SignalingParser.parse("""{"version":1,"type":"nope"}""") as SignalingParseResult.Rejected).reason,
        )
    }

    @Test
    fun validatesEpochsAndLimits() {
        val negativeEpoch = """{"version":1,"type":"offer","epoch":-1,"sdp":"v=0"}"""
        assertIs<SignalingParseResult.Rejected>(SignalingParser.parse(negativeEpoch))
        val outOfRange = """{"version":1,"type":"pair","token":"${"00".repeat(32)}","dtls_fingerprint":"${"aa".repeat(32)}","limits":{"max_width":9000}}"""
        assertIs<SignalingParseResult.Rejected>(SignalingParser.parse(outOfRange))
    }

    @Test
    fun roundTripsAQrPayload() {
        val endpoint = assertNotNull(QrEndpoint.fromAddress(4, "192.168.1.42", 51820))
        val payload = QrPayload(
            sessionId = ByteArray(16) { 1 },
            epoch = 7,
            expiresAt = 2_000_000_000,
            pairingToken = ByteArray(32) { 2 },
            tlsFingerprint = ByteArray(32) { 3 },
            capabilities = 0x0011,
            endpoints = listOf(endpoint),
        )
        val decoded = QrPayload.decode(payload.encode(), 1_900_000_000)
        assertIs<QrDecodeResult.Accepted>(decoded)
        assertEquals(payload, decoded.payload)
        assertEquals("192.168.1.42", decoded.payload.endpoints.single().addressString())
    }

    @Test
    fun rejectsExpiredAndPublicQrPayloads() {
        val endpoint = assertNotNull(QrEndpoint.fromAddress(4, "192.168.1.42", 51820))
        val payload = QrPayload(
            sessionId = ByteArray(16),
            epoch = 1,
            expiresAt = 2_000_000_000,
            pairingToken = ByteArray(32),
            tlsFingerprint = ByteArray(32),
            capabilities = 0,
            endpoints = listOf(endpoint),
        )
        val expired = QrPayload.decode(payload.encode(), 2_000_000_000)
        assertEquals(ReasonCode.QR_EXPIRED, (expired as QrDecodeResult.Rejected).reason)

        val truncated = QrPayload.decode(byteArrayOf(1, 2, 3), 0)
        assertEquals(ReasonCode.INVALID_QR_PAYLOAD, (truncated as QrDecodeResult.Rejected).reason)

        assertNull(QrEndpoint.fromAddress(4, "8.8.8.8", 1))
        assertNull(QrEndpoint.fromAddress(5, "192.168.1.1", 1))
    }

    @Test
    fun pinsTheExpiryBoundaryToTheGivenInstant() {
        val bytes = qrPayload(expiresAt = 2_000_000_000).encode()

        assertIs<QrDecodeResult.Accepted>(QrPayload.decode(bytes, 1_999_999_999))
        assertEquals(ReasonCode.QR_EXPIRED, rejectionReason(bytes, 2_000_000_000))
        assertEquals(ReasonCode.QR_EXPIRED, rejectionReason(bytes, 2_000_000_001))
    }

    @Test
    fun structuralValidityIgnoresTheClock() {
        val expiredLongAgo = qrPayload(expiresAt = 1)

        assertTrue(expiredLongAgo.isStructurallyValid())
        assertFalse(expiredLongAgo.isExpiredAt(0))
        assertTrue(expiredLongAgo.isExpiredAt(1))
        assertTrue(expiredLongAgo.isExpiredAt(1_700_000_000))
    }

    @Test
    fun structuralValidityRejectsPayloadsThatCannotBeEncoded() {
        val publicEndpoint = QrEndpoint(4, ByteArray(12) + byteArrayOf(8, 8, 8, 8), 51820)
        val payload = qrPayload(expiresAt = 2_000_000_000, endpoints = listOf(publicEndpoint))

        assertFalse(payload.isStructurallyValid())
    }

    private fun qrPayload(
        expiresAt: Long,
        endpoints: List<QrEndpoint> = listOf(assertNotNull(QrEndpoint.fromAddress(4, "192.168.1.42", 51820))),
    ): QrPayload = QrPayload(
        sessionId = ByteArray(16) { 1 },
        epoch = 7,
        expiresAt = expiresAt,
        pairingToken = ByteArray(32) { 2 },
        tlsFingerprint = ByteArray(32) { 3 },
        capabilities = 0,
        endpoints = endpoints,
    )

    private fun rejectionReason(input: ByteArray, nowUnix: Long): ReasonCode =
        (QrPayload.decode(input, nowUnix) as QrDecodeResult.Rejected).reason

    @Test
    fun endpointOrderIsStable() {
        val first = QrPayload(
            sessionId = ByteArray(16),
            epoch = 1,
            expiresAt = 2_000_000_000,
            pairingToken = ByteArray(32),
            tlsFingerprint = ByteArray(32),
            capabilities = 0,
            endpoints = listOf(
                assertNotNull(QrEndpoint.fromAddress(6, "fe80::1", 3)),
                assertNotNull(QrEndpoint.fromAddress(4, "10.0.0.2", 2)),
                assertNotNull(QrEndpoint.fromAddress(4, "10.0.0.1", 1)),
            ),
        )
        val reversed = QrPayload(
            sessionId = first.sessionId,
            epoch = first.epoch,
            expiresAt = first.expiresAt,
            pairingToken = first.pairingToken,
            tlsFingerprint = first.tlsFingerprint,
            capabilities = first.capabilities,
            endpoints = first.endpoints.reversed(),
        )
        assertTrue(first.encode().contentEquals(reversed.encode()))
    }

    @Test
    fun hexIsStrictlyLowercase() {
        assertEquals("000fa5ff", Hex.encode(byteArrayOf(0x00, 0x0f, 0xa5.toByte(), 0xff.toByte())))
        assertNull(Hex.decodeExact("AA", 1))
        assertNull(Hex.decode("abc"))
        assertNull(Hex.decode("zz"))
    }

    @Test
    fun conformanceReportSerializesItscanonicalFields() {
        val report = ConformanceReport(
            qr = listOf(QrResult("minimal-v1", true, null, "00", null)),
            signaling = listOf(SignalingResult("msg-ping-valid", "message", true, null)),
            reasonCodes = ReasonCodeResult(26, true),
        )
        assertTrue(report.passed())
        val json = report.toJson()
        assertContains(json, "\"reason_code\":null")
        assertFalse(json.contains("encoded_payload_hex"))
    }
}
