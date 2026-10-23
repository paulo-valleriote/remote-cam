package remotecam.core.transport

import android.content.Context
import android.net.NetworkCapabilities
import android.util.Log
import org.webrtc.AudioSource
import org.webrtc.AudioTrack
import org.webrtc.CameraVideoCapturer
import org.webrtc.DefaultVideoEncoderFactory
import org.webrtc.DefaultVideoDecoderFactory
import org.webrtc.EglBase
import org.webrtc.MediaConstraints
import org.webrtc.MediaStream
import org.webrtc.PeerConnection
import org.webrtc.PeerConnectionFactory
import org.webrtc.RtpTransceiver
import org.webrtc.SdpSemantics
import org.webrtc.VideoCapturer
import org.webrtc.VideoEncoderFactory
import org.webrtc.VideoTrack
import org.webrtc.audio.JavaAudioDeviceModule
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

/**
 * PeerConnectionFactoryProvider for the F0 spike.
 *
 * Creates a WebRTC PeerConnectionFactory configured for:
 * - SEND_ONLY transceivers for audio and video with "remotecam" stream id
 * - Empty iceServers (host candidates only, no STUN/TURN)
 * - networkIgnoreMask for cellular/VPN
 * - H.264 preferred / VP8 fallback negotiation
 *
 * This is a spike harness, not production code.
 */
class PeerConnectionFactoryProvider(
    private val context: Context
) {

    private val TAG = "PeerConnectionFactoryProvider"

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

        // Initialize WebRTC with custom audio device module
        PeerConnectionFactory.initialize(
            PeerConnectionFactory.InitializationOptions.builder(context)
                .setEnableInternalTracer(true)
                .setFieldTrials("WebRTC-H264HighProfile/Enabled/")
                .createInitializationOptions()
        )

        // Create EGL base for video encoding
        eglBase = EglBase.create()

        // Create encoder/decoder factories with H.264 support
        videoEncoderFactory = DefaultVideoEncoderFactory(
            eglBase!!.eglBaseContext,
            true,  // enable H.264 high profile
            true   // enable VP8
        )
        videoDecoderFactory = DefaultVideoDecoderFactory(eglBase!!.eglBaseContext)

        // Create audio device module
        val audioDeviceModule = JavaAudioDeviceModule.builder(context).createAudioDeviceModule()

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
        val config = PeerConnection.RTCConfiguration().apply {
            iceServers = emptyList() // No STUN/TURN - host candidates only
            iceTransportPolicy = PeerConnection.IceTransportPolicy.ALL
            bundlePolicy = PeerConnection.BundlePolicy.MAX_BUNDLE
            rtcpMuxPolicy = PeerConnection.RtcpMuxPolicy.REQUIRE
            // Ignore cellular and VPN interfaces
            networkIgnoreMask = NetworkCapabilities.TRANSPORT_CELLULAR or NetworkCapabilities.TRANSPORT_VPN
            // Disable ICE candidate pooling to reduce memory
            iceCandidatePoolSize = 0
            // Disable continual gathering
            continualGatheringPolicy = PeerConnection.ContinualGatheringPolicy.GATHER_ONCE
            // SDP semantics
            sdpSemantics = SdpSemantics.UNIFIED_PLAN
        }

        // Media constraints
        val constraints = MediaConstraints()
        constraints.mandatory.add(MediaConstraints.KeyValuePair("OfferToReceiveAudio", "false"))
        constraints.mandatory.add(MediaConstraints.KeyValuePair("OfferToReceiveVideo", "false"))

        peerConnection = factory.createPeerConnection(config, constraints, observer)

        // Set codec preferences: H.264 preferred over VP8
        setCodecPreferences()

        Log.d(TAG, "PeerConnection created with host-only ICE")
        return peerConnection!!
    }

    /**
     * Configure codec preferences: H.264 before VP8.
     * This must be called after creating the PeerConnection.
     */
    private fun setCodecPreferences() {
        val pc = peerConnection ?: return

        // Get the video transceiver
        val transceivers = pc.transceivers
        for (transceiver in transceivers) {
            if (transceiver.receiver.track() is VideoTrack) {
                // Set H.264 as preferred codec
                val capabilities = transceiver.receiver.getCapabilities(org.webrtc.RtpTransceiver.MediaType.VIDEO)
                val h264Codecs = capabilities.codecs.filter { it.name.equals("H264", ignoreCase = true) }
                val vp8Codecs = capabilities.codecs.filter { it.name.equals("VP8", ignoreCase = true) }

                // Reorder: H.264 first, then VP8
                val preferredCodecs = mutableListOf<org.webrtc.RtpCodecCapability>()
                preferredCodecs.addAll(h264Codecs)
                preferredCodecs.addAll(vp8Codecs)
                preferredCodecs.addAll(capabilities.codecs.filter { !it.name.equals("H264", ignoreCase = true) && !it.name.equals("VP8", ignoreCase = true) })

                transceiver.setCodecPreferences(preferredCodecs)
                Log.d(TAG, "Codec preferences set: H.264 preferred over VP8")
                break
            }
        }
    }

    /**
     * Create and add audio track to the local stream and peer connection.
     */
    fun addAudioTrack(): AudioTrack {
        val factory = peerConnectionFactory ?: throw IllegalStateException("Not initialized")

        val audioConstraints = MediaConstraints()
        audioSource = factory.createAudioSource(audioConstraints)
        audioTrack = factory.createAudioTrack("audio", audioSource!!)

        localStream!!.addTrack(audioTrack!!)

        // Add transceiver with SEND_ONLY direction
        val init = RtpTransceiver.RtpTransceiverInit(
            RtpTransceiverDirection.SEND_ONLY,
            listOf("remotecam")
        )
        peerConnection!!.addTransceiver(audioTrack!!, init)

        Log.d(TAG, "Audio track added with SEND_ONLY direction")
        return audioTrack!!
    }

    /**
     * Create and add video track to the local stream and peer connection.
     * The video capturer is provided by the capture source.
     */
    fun addVideoTrack(capturer: VideoCapturer): VideoTrack {
        val factory = peerConnectionFactory ?: throw IllegalStateException("Not initialized")

        videoTrack = factory.createVideoTrack("video", capturer)

        localStream!!.addTrack(videoTrack!!)

        // Add transceiver with SEND_ONLY direction and "remotecam" stream id
        val init = RtpTransceiver.RtpTransceiverInit(
            RtpTransceiverDirection.SEND_ONLY,
            listOf("remotecam")
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
    fun createOffer(): org.webrtc.SessionDescription {
        val pc = peerConnection ?: throw IllegalStateException("No PeerConnection")
        val constraints = MediaConstraints()
        constraints.mandatory.add(MediaConstraints.KeyValuePair("OfferToReceiveAudio", "false"))
        constraints.mandatory.add(MediaConstraints.KeyValuePair("OfferToReceiveVideo", "false"))
        return pc.createOffer(constraints).get()
    }

    /**
     * Set the local description (offer or answer).
     */
    fun setLocalDescription(description: org.webrtc.SessionDescription) {
        val pc = peerConnection ?: throw IllegalStateException("No PeerConnection")
        pc.setLocalDescription(object : PeerConnection.SdpObserver {
            override fun onCreateSuccess(sessionDescription: org.webrtc.SessionDescription) {}
            override fun onSetSuccess() { Log.d(TAG, "Local description set: ${sessionDescription.type}") }
            override fun onCreateFailure(error: String) { Log.e(TAG, "Create local description failed: $error") }
            override fun onSetFailure(error: String) { Log.e(TAG, "Set local description failed: $error") }
        }, description)
    }

    /**
     * Set the remote description (answer or offer).
     */
    fun setRemoteDescription(description: org.webrtc.SessionDescription) {
        val pc = peerConnection ?: throw IllegalStateException("No PeerConnection")
        pc.setRemoteDescription(object : PeerConnection.SdpObserver {
            override fun onCreateSuccess(sessionDescription: org.webrtc.SessionDescription) {}
            override fun onSetSuccess() { Log.d(TAG, "Remote description set: ${sessionDescription.type}") }
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