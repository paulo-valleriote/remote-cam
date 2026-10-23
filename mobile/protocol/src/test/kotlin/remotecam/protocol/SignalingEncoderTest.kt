package remotecam.protocol

import kotlin.test.Test
import kotlin.test.assertIs

class SignalingEncoderTest {

    @Test
    fun everyEncodedFrameIsAcceptedByTheParser() {
        val messages = listOf(
            SignalingMessage.Pair(
                token = ByteArray(32) { 0x11 },
                dtlsFingerprint = ByteArray(32) { 0x22 },
                limits = Limits(maxWidth = 1280, maxHeight = 720, maxFrameRate = 30),
            ),
            SignalingMessage.Paired(
                sessionId = ByteArray(16) { 0x33 },
                epoch = 1,
                dtlsFingerprint = ByteArray(32) { 0x44 },
                acceptedLimits = Limits(maxWidth = 1280),
            ),
            SignalingMessage.Offer(epoch = 1, sdp = "v=0\r\n"),
            SignalingMessage.Answer(epoch = 1, sdp = "v=0\r\n"),
            SignalingMessage.Ice(
                epoch = 1,
                candidates = listOf(Candidate("192.168.1.10", 5000, 1, "udp")),
            ),
            SignalingMessage.StatsMessage(
                Stats(
                    fps = 30.0,
                    videoBitrateKbps = 2000.0,
                    rttMs = 20.0,
                    lossPct = 0.1,
                    thermalStatus = "light",
                ),
            ),
            SignalingMessage.Close(ReasonCode.ENDED_BY_MOBILE),
            SignalingMessage.Error(ReasonCode.INVALID_MESSAGE, "malformed frame"),
            SignalingMessage.Ping,
            SignalingMessage.Pong,
        )

        for (message in messages) {
            val encoded = SignalingEncoder.encode(message)
            val result = SignalingParser.parse(encoded)
            assertIs<SignalingParseResult.Accepted>(
                result,
                "encoder produced a frame the parser rejects: $encoded",
            )
        }
    }
}
