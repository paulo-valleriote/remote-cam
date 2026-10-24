package remotecam.core.transport

import android.content.Context
import android.util.Log
import org.webrtc.AudioSource
import org.webrtc.AudioTrack
import org.webrtc.DefaultVideoEncoderFactory
import org.webrtc.DefaultVideoDecoderFactory
import org.webrtc.EglBase
import org.webrtc.MediaConstraints
import org.webrtc.MediaStream
import org.webrtc.PeerConnection
import org.webrtc.PeerConnectionFactory
import org.webrtc.RtpTransceiver
import org.webrtc.SdpObserver
import org.webrtc.VideoDecoderFactory
import org.webrtc.VideoEncoderFactory
import org.webrtc.VideoSource
import org.webrtc.VideoTrack
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

/**
 * PeerConnectionFactoryProvider for the F0 spike.
 *
 * Creates a WebRTC PeerConnectionFactory configured for:
 * - SEND_ONLY transceivers for audio and video with "remotecam" stream id
 * - Empty iceServers (host candidates only, no STUN/TURN)
 * - H.264 preferred / VP8 fallback negotiation
 *
 * This is a spike harness, not production code.
 */
class PeerConnectionFactoryProvider(
    private val context: Context
) {

    private val TAG = "PeerConnectionFactoryProvider"

    companion object {
        const val STREAM_ID = "remotecam"
        const val AUDIO_TRACK_ID = "audio"
        const val VIDEO_TRACK_ID = "video"

        /** Host-candidates-only configuration (no STUN/TURN). */
        internal fun rtcConfiguration(): PeerConnection.RTCConfiguration =
            PeerConnection.RTCConfiguration(emptyList())
    }

    private var peerConnectionFactory: PeerConnectionFactory? = null
    private var eglBase: EglBase? = null
    private var executor: ExecutorService = Executors.newSingleThreadExecutor()

    // The single PeerConnection for this session
    private var peerConnection: PeerConnection? = null

    // Media stream for the session
    private var localStream: MediaStream? = null

    // Audio/video tracks
    private var audioTrack: AudioTrack? = null
    private var videoTrack: VideoTrack? = null

    // Audio source
    private var audioSource: AudioSource? = null

    // Video source fed by the capture source
    private var videoSource: VideoSource? = null

    // Encoder/Decoder factories
    private var videoEncoderFactory: VideoEncoderFactory? = null
    private var videoDecoderFactory: VideoDecoderFactory? = null

    /**
     * Initialize WebRTC and create the factory.
     * Must be called before createPeerConnection().
     */
    fun initialize() {
        if (peerConnectionFactory != null) {
            Log.w(TAG, "Already initialized")
            return
        }

        // Initialize WebRTC
        PeerConnectionFactory.initialize(
            PeerConnectionFactory.InitializationOptions.builder(context)
                .setEnableInternalTracer(true)
                .createInitializationOptions()
        )

        // Create EGL base for video encoding
        eglBase = EglBase.create()

        // Create encoder/decoder factories
        videoEncoderFactory = DefaultVideoEncoderFactory(
            eglBase!!.eglBaseContext,
            true,  // enable H.264 high profile
            true   // enable VP8
        )
        videoDecoderFactory = DefaultVideoDecoderFactory(eglBase!!.eglBaseContext)

        // Create audio device module
        val audioDeviceModule = org.webrtc.audio.JavaAudioDeviceModule.builder(context).createAudioDeviceModule()

        // Create PeerConnectionFactory
        peerConnectionFactory = PeerConnectionFactory.builder()
            .setVideoEncoderFactory(videoEncoderFactory)
            .setVideoDecoderFactory(videoDecoderFactory)
            .setAudioDeviceModule(audioDeviceModule)
            .createPeerConnectionFactory()

        // Create local media stream
        localStream = peerConnectionFactory!!.createLocalMediaStream("remotecam")

        Log.d(TAG, "PeerConnectionFactory initialized")
    }

    /**
     * Create a PeerConnection configured for the spike.
     * Returns the created PeerConnection.
     */
    fun createPeerConnection(observer: PeerConnection.Observer): PeerConnection {
        val factory = peerConnectionFactory ?: throw IllegalStateException("Not initialized. Call initialize() first.")

        // Configuration: host candidates only, no STUN/TURN
        val config = rtcConfiguration()

        // Media constraints
        val constraints = MediaConstraints()

        peerConnection = factory.createPeerConnection(config, constraints, observer)

        Log.d(TAG, "PeerConnection created with host-only ICE")
        return peerConnection!!
    }

    /**
     * Create and add audio track to the local stream and peer connection.
     */
    fun addAudioTrack(): AudioTrack {
        val factory = peerConnectionFactory ?: throw IllegalStateException("Not initialized")

        val audioConstraints = MediaConstraints()
        audioSource = factory.createAudioSource(audioConstraints)
        audioTrack = factory.createAudioTrack(AUDIO_TRACK_ID, audioSource!!)

        localStream!!.addTrack(audioTrack!!)

        // Add transceiver with SEND_ONLY direction
        val init = RtpTransceiver.RtpTransceiverInit(
            RtpTransceiver.RtpTransceiverDirection.SEND_ONLY,
            listOf(STREAM_ID)
        )
        peerConnection!!.addTransceiver(audioTrack!!, init)

        Log.d(TAG, "Audio track added with SEND_ONLY direction")
        return audioTrack!!
    }

    /**
     * Create the video source that the capture source feeds frames into.
     * Call [addVideoTrack] with it once the PeerConnection exists.
     */
    fun createVideoSource(): VideoSource {
        val factory = peerConnectionFactory ?: throw IllegalStateException("Not initialized")
        val source = factory.createVideoSource(false)
        videoSource = source
        return source
    }

    /**
     * Create and add a video track to the local stream and peer connection.
     */
    fun addVideoTrack(source: VideoSource): VideoTrack {
        val factory = peerConnectionFactory ?: throw IllegalStateException("Not initialized")

        videoTrack = factory.createVideoTrack(VIDEO_TRACK_ID, source)

        localStream!!.addTrack(videoTrack!!)

        // Add transceiver with SEND_ONLY direction and "remotecam" stream id
        val init = RtpTransceiver.RtpTransceiverInit(
            RtpTransceiver.RtpTransceiverDirection.SEND_ONLY,
            listOf(STREAM_ID)
        )
        peerConnection!!.addTransceiver(videoTrack!!, init)

        Log.d(TAG, "Video track added with SEND_ONLY direction")
        return videoTrack!!
    }

    /**
     * Get the current PeerConnection.
     */
    fun getPeerConnection(): PeerConnection? {
        return peerConnection
    }

    /**
     * Get the local media stream.
     */
    fun getLocalStream(): MediaStream? {
        return localStream
    }

    /**
     * Create an offer for the signaling handshake.
     */
    fun createOffer(observer: SdpObserver) {
        val pc = peerConnection ?: throw IllegalStateException("No PeerConnection")
        pc.createOffer(observer, MediaConstraints())
    }

    /**
     * Set the local description (offer or answer).
     */
    fun setLocalDescription(description: org.webrtc.SessionDescription) {
        val pc = peerConnection ?: throw IllegalStateException("No PeerConnection")
        pc.setLocalDescription(object : SdpObserver {
            override fun onCreateSuccess(sessionDescription: org.webrtc.SessionDescription) {}
            override fun onSetSuccess() { Log.d(TAG, "Local description set: ${description.type}") }
            override fun onCreateFailure(error: String) { Log.e(TAG, "Create local description failed: $error") }
            override fun onSetFailure(error: String) { Log.e(TAG, "Set local description failed: $error") }
        }, description)
    }

    /**
     * Set the remote description (answer or offer).
     */
    fun setRemoteDescription(description: org.webrtc.SessionDescription) {
        val pc = peerConnection ?: throw IllegalStateException("No PeerConnection")
        pc.setRemoteDescription(object : SdpObserver {
            override fun onCreateSuccess(sessionDescription: org.webrtc.SessionDescription) {}
            override fun onSetSuccess() { Log.d(TAG, "Remote description set: ${description.type}") }
            override fun onCreateFailure(error: String) { Log.e(TAG, "Create remote description failed: $error") }
            override fun onSetFailure(error: String) { Log.e(TAG, "Set remote description failed: $error") }
        }, description)
    }

    /**
     * Add an ICE candidate.
     */
    fun addIceCandidate(candidate: org.webrtc.IceCandidate) {
        val pc = peerConnection ?: return
        pc.addIceCandidate(candidate)
    }

    /**
     * Release all resources.
     */
    fun release() {
        peerConnection?.close()
        peerConnection = null

        audioTrack?.dispose()
        audioTrack = null

        videoTrack?.dispose()
        videoTrack = null

        audioSource?.dispose()
        audioSource = null

        videoSource?.dispose()
        videoSource = null

        localStream?.dispose()
        localStream = null

        videoEncoderFactory = null
        videoDecoderFactory = null

        eglBase?.release()
        eglBase = null

        peerConnectionFactory?.dispose()
        peerConnectionFactory = null

        executor.shutdown()

        Log.d(TAG, "PeerConnectionFactoryProvider released")
    }
}