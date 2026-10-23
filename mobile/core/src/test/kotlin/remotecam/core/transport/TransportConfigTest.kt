package remotecam.core.transport

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * Tests for the WebRTC transport configuration.
 *
 * The native PeerConnection cannot be instantiated in a JVM unit test, so these
 * tests pin the pure configuration and contract identifiers instead.
 */
class TransportConfigTest {

    @Test
    fun iceServersAreEmptyForHostCandidatesOnly() {
        val config = PeerConnectionFactoryProvider.rtcConfiguration()

        assertTrue(
            config.iceServers.isEmpty(),
            "iceServers must be empty for host-candidate-only ICE",
        )
    }

    @Test
    fun streamIdIsRemotecam() {
        assertEquals("remotecam", PeerConnectionFactoryProvider.STREAM_ID)
    }

    @Test
    fun trackIdsArePinned() {
        assertEquals("audio", PeerConnectionFactoryProvider.AUDIO_TRACK_ID)
        assertEquals("video", PeerConnectionFactoryProvider.VIDEO_TRACK_ID)
    }
}
