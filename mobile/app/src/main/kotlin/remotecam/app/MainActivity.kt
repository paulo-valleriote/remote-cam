package remotecam.app

import android.Manifest
import android.content.pm.PackageManager
import android.os.Bundle
import android.util.Log
import android.view.View
import android.widget.Button
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.launch
import remotecam.core.capture.CameraXCaptureSource
import remotecam.core.signaling.SignalingClient
import remotecam.core.transport.PeerConnectionFactoryProvider
import remotecam.protocol.QrPayload

/**
 * Main Activity for the F0 spike.
 * Minimal UI: shows QR code for pairing, starts streaming session.
 * No production camera controls - this is a spike harness.
 */
class MainActivity : AppCompatActivity() {

    private lateinit var statusText: TextView
    private lateinit var startButton: Button
    private lateinit var stopButton: Button
    private lateinit var qrTextView: TextView

    private val permissions = arrayOf(
        Manifest.permission.CAMERA,
        Manifest.permission.RECORD_AUDIO
    )

    private var signalingClient: SignalingClient? = null
    private var captureSource: CameraXCaptureSource? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        statusText = findViewById(R.id.statusText)
        startButton = findViewById(R.id.startButton)
        stopButton = findViewById(R.id.stopButton)
        qrTextView = findViewById(R.id.qrTextView)

        startButton.setOnClickListener { requestPermissionsAndStart() }
        stopButton.setOnClickListener { stopSession() }

        stopButton.isEnabled = false
    }

    private fun requestPermissionsAndStart() {
        val missingPermissions = permissions.filter {
            ContextCompat.checkSelfPermission(this, it) != PackageManager.PERMISSION_GRANTED
        }

        if (missingPermissions.isNotEmpty()) {
            ActivityCompat.requestPermissions(this, missingPermissions.toTypedArray(), 1)
        } else {
            startSession()
        }
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == 1 && grantResults.all { it == PackageManager.PERMISSION_GRANTED }) {
            startSession()
        } else {
            updateStatus("Permissions denied")
        }
    }

    private fun startSession() {
        lifecycleScope.launch {
            updateStatus("Initializing...")

            // Initialize WebRTC factory
            val factoryProvider = PeerConnectionFactoryProvider(this@MainActivity)
            factoryProvider.initialize()

            // Create capture source
            captureSource = CameraXCaptureSource(this@MainActivity)

            // Create signaling client
            signalingClient = SignalingClient(this@MainActivity, factoryProvider)

            // Start the pairing handshake - this will show a QR code
            signalingClient?.startPairing { qrPayload ->
                runOnUiThread {
                    displayQrPayload(qrPayload)
                    updateStatus("Waiting for pairing...")
                }
            } ?: runOnUiThread {
                updateStatus("Failed to start pairing")
            }
        }

        runOnUiThread {
            startButton.isEnabled = false
            stopButton.isEnabled = true
        }
    }

    private fun stopSession() {
        signalingClient?.stop()
        signalingClient = null

        captureSource?.release()
        captureSource = null

        runOnUiThread {
            startButton.isEnabled = true
            stopButton.isEnabled = false
            qrTextView.text = ""
            updateStatus("Session stopped")
        }
    }

    private fun displayQrPayload(payload: QrPayload) {
        // Display QR as base64 or text representation for debugging
        // In a real app this would render an actual QR code image
        val text = "RCM1 QR Payload:\n" +
            "Session: ${payload.sessionId}\n" +
            "Endpoints: ${payload.endpoints.size}\n" +
            "Expires: ${payload.expiresAt}\n" +
            "Token: ${payload.pairingToken.take(16)}...\n" +
            "TLS FP: ${payload.tlsFingerprint.take(16)}..."
        qrTextView.text = text
    }

    private fun updateStatus(message: String) {
        runOnUiThread {
            statusText.text = message
            Log.d("RemoteCam", message)
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        stopSession()
    }
}