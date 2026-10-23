package remotecam.core.signaling

import android.content.Context
import android.util.Log
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import okio.ByteString
import org.webrtc.DataChannel
import org.webrtc.IceCandidate
import org.webrtc.MediaConstraints
import org.webrtc.MediaStream
import org.webrtc.PeerConnection
import org.webrtc.RtpReceiver
import org.webrtc.RtpTransceiver
import org.webrtc.SdpObserver
import org.webrtc.SessionDescription
import remotecam.core.transport.PeerConnectionFactoryProvider
import remotecam.protocol.Candidate
import remotecam.protocol.Limits
import remotecam.protocol.QrPayload
import remotecam.protocol.SignalingEncoder
import remotecam.protocol.SignalingMessage
import remotecam.protocol.SignalingParseResult
import remotecam.protocol.SignalingParser
import java.security.MessageDigest
import java.util.concurrent.TimeUnit
import javax.net.ssl.SSLContext
import javax.net.ssl.X509TrustManager

/**
 * Mobile signaling client for the F0 spike.
 *
 * Implements the WebSocket over TLS 1.3 signaling channel with:
 * - Fingerprint pinning (SHA-256 of leaf certificate DER)
 * - Raw-byte QR decode using Result.getBytes() (never getText())
 * - Validation before any socket opens
 * - Token authentication
 *
 * This is a spike harness, not production code.
 */
class SignalingClient(
    private val context: Context,
    private val factoryProvider: PeerConnectionFactoryProvider
) {

    private val TAG = "SignalingClient"

    private var webSocket: WebSocket? = null
    private var okHttpClient: OkHttpClient? = null
    private val scope = CoroutineScope(Dispatchers.IO + Job())
    private var isConnected = false
    private var pairingCallback: ((QrPayload) -> Unit)? = null
    private var sessionId: ByteArray? = null
    private var pinnedFingerprint: String? = null
    private var pairingToken: ByteArray? = null
    private var currentEpoch: Long = 0

    // TLS 1.3 with custom trust manager for pinning
    private val trustManager = object : X509TrustManager {
        override fun checkClientTrusted(chain: Array<java.security.cert.X509Certificate>?, authType: String?) {}
        override fun checkServerTrusted(chain: Array<java.security.cert.X509Certificate>?, authType: String?) {
            // Verify the pinned fingerprint
            chain?.firstOrNull()?.let { cert ->
                val der = cert.encoded
                val digest = MessageDigest.getInstance("SHA-256").digest(der)
                val fingerprint = digest.joinToString("") { "%02x".format(it) }
                if (fingerprint != pinnedFingerprint) {
                    throw java.security.cert.CertificateException("Certificate pin mismatch: expected $pinnedFingerprint, got $fingerprint")
                }
            } ?: throw java.security.cert.CertificateException("No certificate in chain")
        }
        override fun getAcceptedIssuers(): Array<java.security.cert.X509Certificate> = emptyArray()
    }

    /**
     * Start the pairing handshake.
     * Generates a QR payload and displays it for the desktop to scan.
     */
    fun startPairing(onQrGenerated: (QrPayload) -> Unit) {
        this.pairingCallback = onQrGenerated

        // In the F0 spike, the desktop generates the QR and the mobile scans it.
        // Here we simulate receiving a QR code for testing.
        // In production, this would use CameraX ImageAnalysis to scan QR codes.
        scope.launch {
            // For the spike, we'll wait for a QR to be provided via scanQrCode()
            // This is a placeholder - the actual QR scanning is done separately
        }
    }

    /**
     * Process a scanned QR code payload (raw bytes from Result.getBytes()).
     * Validates the payload before opening any WebSocket connection.
     */
    fun scanQrCode(rawBytes: ByteArray): Boolean {
        return try {
            // Parse QR payload using the protocol library
            val result = QrPayload.decode(rawBytes)

            when (result) {
                is remotecam.protocol.QrDecodeResult.Accepted -> {
                    val payload = result.payload

                    // Validate the payload
                    if (!payload.isValid()) {
                        Log.e(TAG, "QR payload validation failed")
                        return false
                    }

                    // Check expiry
                    if (payload.isExpired()) {
                        Log.e(TAG, "QR code expired")
                        return false
                    }

                    // Check private endpoints
                    if (!payload.hasPrivateEndpoints()) {
                        Log.e(TAG, "No private endpoints in QR payload")
                        return false
                    }

                    // Extract fingerprint and token
                    this.pinnedFingerprint = payload.tlsFingerprintHex
                    this.pairingToken = payload.pairingToken
                    this.sessionId = payload.sessionId

                    Log.d(TAG, "QR validated: session=${payload.sessionIdHex}, endpoints=${payload.endpoints.size}")

                    // Connect to the first private endpoint
                    val endpoint = payload.endpoints.first()
                    connectToEndpoint(endpoint.addressString() ?: return false)

                    true
                }
                is remotecam.protocol.QrDecodeResult.Rejected -> {
                    Log.e(TAG, "QR decode rejected: ${result.reason}")
                    false
                }
            }
        } catch (e: Exception) {
            Log.e(TAG, "QR decode failed", e)
            false
        }
    }

    /**
     * Connect to the desktop signaling endpoint over WSS with TLS 1.3 pinning.
     */
    private fun connectToEndpoint(endpoint: String) {
        scope.launch {
            try {
                val sslContext = SSLContext.getInstance("TLS")
                sslContext.init(null, arrayOf(trustManager), null)

                okHttpClient = OkHttpClient.Builder()
                    .sslSocketFactory(sslContext.socketFactory, trustManager)
                    .hostnameVerifier { _, _ -> true } // We verify via pinning, not hostname
                    .pingInterval(15, TimeUnit.SECONDS)
                    .build()

                val url = "wss://$endpoint/signaling"
                val request = Request.Builder().url(url).build()

                webSocket = okHttpClient!!.newWebSocket(request, object : WebSocketListener() {
                    override fun onOpen(webSocket: WebSocket, response: okhttp3.Response) {
                        Log.d(TAG, "WebSocket connected to $endpoint")
                        isConnected = true
                        sendPairingToken()
                    }

                    override fun onMessage(webSocket: WebSocket, text: String) {
                        handleMessage(text)
                    }

                    override fun onMessage(webSocket: WebSocket, bytes: ByteString) {
                        handleMessage(bytes.toByteArray())
                    }

                    override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
                        Log.d(TAG, "WebSocket closed: $code $reason")
                        isConnected = false
                    }

                    override fun onFailure(webSocket: WebSocket, t: Throwable, response: okhttp3.Response?) {
                        Log.e(TAG, "WebSocket failure", t)
                        isConnected = false
                    }
                })
            } catch (e: Exception) {
                Log.e(TAG, "Connection failed", e)
            }
        }
    }

    private fun sendPairingToken() {
        pairingToken?.let { token ->
            val message = SignalingMessage.Pair(
                token = token,
                dtlsFingerprint = ByteArray(32), // Mobile doesn't have a certificate in F0
                limits = Limits()
            )
            sendMessage(message)
        }
    }

    private fun sendMessage(message: SignalingMessage) {
        webSocket?.send(SignalingEncoder.encode(message))
    }

    private fun handleMessage(text: String) {
        try {
            when (val parsed = SignalingParser.parse(text)) {
                is SignalingParseResult.Rejected -> {
                    Log.w(TAG, "Rejected signaling frame: ${parsed.reason.wireName}")
                }
                is SignalingParseResult.Accepted -> when (val message = parsed.message) {
                    is SignalingMessage.Paired -> {
                        Log.d(TAG, "Pairing successful: ${message.sessionId.toHex()}")
                        sessionId = message.sessionId
                        currentEpoch = message.epoch
                        // Start WebRTC negotiation
                        startWebRtcNegotiation()
                    }
                    is SignalingMessage.Offer -> {
                        Log.d(TAG, "Received offer")
                        currentEpoch = message.epoch
                        handleOffer(message)
                    }
                    is SignalingMessage.Answer -> {
                        Log.d(TAG, "Received answer")
                        handleAnswer(message)
                    }
                    is SignalingMessage.Ice -> {
                        currentEpoch = message.epoch
                        handleIceCandidate(message)
                    }
                    is SignalingMessage.Close -> {
                        Log.d(TAG, "Received close: ${message.code}")
                        stop()
                    }
                    is SignalingMessage.Error -> {
                        Log.e(TAG, "Received error: ${message.code}")
                        stop()
                    }
                    else -> {
                        Log.w(TAG, "Unhandled message: ${message::class.simpleName}")
                    }
                }
            }
        } catch (e: Exception) {
            Log.e(TAG, "Message handling failed", e)
        }
    }

    private fun handleMessage(bytes: ByteArray) {
        // Binary messages not expected in F0 signaling
        Log.w(TAG, "Received binary message, ignoring")
    }

    private fun startWebRtcNegotiation() {
        // Create PeerConnection and start capture
        val pc = factoryProvider.createPeerConnection(object : PeerConnection.Observer {
            override fun onIceCandidate(candidate: IceCandidate) {
                val parsed = parseCandidate(candidate.sdp) ?: return
                sendMessage(SignalingMessage.Ice(currentEpoch, listOf(parsed)))
            }

            override fun onIceConnectionChange(state: PeerConnection.IceConnectionState) {
                Log.d(TAG, "ICE connection state: $state")
            }

            override fun onIceConnectionReceivingChange(receiving: Boolean) {}

            override fun onIceGatheringChange(newState: PeerConnection.IceGatheringState) {}

            override fun onSignalingChange(state: PeerConnection.SignalingState) {}

            override fun onIceCandidatesRemoved(candidates: Array<IceCandidate>) {}

            override fun onAddStream(stream: MediaStream) {}

            override fun onRemoveStream(stream: MediaStream) {}

            override fun onDataChannel(channel: DataChannel) {}

            override fun onRenegotiationNeeded() {}

            override fun onAddTrack(receiver: RtpReceiver, streams: Array<MediaStream>) {}

            override fun onTrack(transceiver: RtpTransceiver) {}
        })

        // Add audio and video tracks
        factoryProvider.addAudioTrack()
        // Video track will be added when capture source is ready
    }

    private fun handleOffer(message: SignalingMessage.Offer) {
        val pc = factoryProvider.getPeerConnection() ?: return

        val offer = SessionDescription(SessionDescription.Type.OFFER, message.sdp)
        pc.setRemoteDescription(object : SdpObserver {
            override fun onCreateSuccess(sessionDescription: SessionDescription) {}
            override fun onSetSuccess() {
                // Create answer
                pc.createAnswer(object : SdpObserver {
                    override fun onCreateSuccess(answer: SessionDescription) {
                        pc.setLocalDescription(object : SdpObserver {
                            override fun onCreateSuccess(sessionDescription: SessionDescription) {}
                            override fun onSetSuccess() {
                                sendMessage(SignalingMessage.Answer(message.epoch, answer.description))
                            }
                            override fun onCreateFailure(error: String) {
                                Log.e(TAG, "Create answer failed: $error")
                            }
                            override fun onSetFailure(error: String) {
                                Log.e(TAG, "Set local answer failed: $error")
                            }
                        }, answer)
                    }

                    override fun onSetSuccess() {}
                    override fun onCreateFailure(error: String) { Log.e(TAG, "Create answer failed: $error") }
                    override fun onSetFailure(error: String) {}
                }, MediaConstraints())
            }
            override fun onCreateFailure(error: String) { Log.e(TAG, "Create offer failed: $error") }
            override fun onSetFailure(error: String) { Log.e(TAG, "Set remote offer failed: $error") }
        }, offer)
    }

    private fun handleAnswer(message: SignalingMessage.Answer) {
        val pc = factoryProvider.getPeerConnection() ?: return

        val answer = SessionDescription(SessionDescription.Type.ANSWER, message.sdp)
        pc.setRemoteDescription(object : SdpObserver {
            override fun onCreateSuccess(sessionDescription: SessionDescription) {}
            override fun onSetSuccess() { Log.d(TAG, "Remote answer set") }
            override fun onCreateFailure(error: String) {}
            override fun onSetFailure(error: String) { Log.e(TAG, "Set remote answer failed: $error") }
        }, answer)
    }

    private fun handleIceCandidate(message: SignalingMessage.Ice) {
        val pc = factoryProvider.getPeerConnection() ?: return
        for (candidate in message.candidates) {
            val sdp = "candidate:${candidate.component} ${candidate.component} " +
                "${candidate.protocol ?: "udp"} 2130706431 ${candidate.address} ${candidate.port} typ host"
            pc.addIceCandidate(IceCandidate("0", 0, sdp))
        }
    }

    /**
     * Parses a WebRTC candidate SDP line into the contract candidate shape.
     * Returns null for candidates the contract cannot represent (non-UDP or
     * components outside 1..2).
     */
    private fun parseCandidate(sdp: String): Candidate? {
        val fields = sdp.removePrefix("candidate:").split(' ')
        if (fields.size < 6) return null
        val component = fields[1].toIntOrNull() ?: return null
        val protocol = fields[2].lowercase()
        val address = fields[4]
        val port = fields[5].toIntOrNull() ?: return null
        if (component !in 1..2 || protocol != "udp") return null
        return Candidate(address, port, component, protocol)
    }

    private fun ByteArray.toHex(): String = joinToString("") { "%02x".format(it) }

    /**
     * Close the signaling connection and clean up.
     */
    fun stop() {
        scope.cancel()
        webSocket?.close(1000, "Client closing")
        webSocket = null
        okHttpClient?.dispatcher?.executorService?.shutdown()
        okHttpClient = null
        isConnected = false
        pairingCallback = null
        sessionId = null
        pinnedFingerprint = null
        pairingToken = null
        factoryProvider.release()
    }
}
