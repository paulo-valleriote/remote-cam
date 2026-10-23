package remotecam.core.capture

import android.content.Context
import android.graphics.ImageFormat
import android.util.Log
import android.util.Size
import android.view.Surface
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.core.ImageProxy.PlaneProxy
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.core.content.ContextCompat
import androidx.lifecycle.LifecycleOwner
import com.google.common.util.concurrent.ListenableFuture
import org.webrtc.CapturerObserver
import org.webrtc.JavaI420Buffer
import org.webrtc.VideoFrame
import java.nio.ByteBuffer
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

/**
 * CameraX capture source for the F0 spike.
 *
 * Binds ONLY ImageAnalysis (no Preview surface) to minimize ISP work, latency and heat.
 * Copies every plane honouring rowStride and pixelStride, which covers both planar
 * (pixelStride 1) and semi-planar (pixelStride 2) YUV_420_888 devices.
 *
 * This is a spike harness, not production code.
 */
class CameraXCaptureSource(
    private val context: Context,
    private val lifecycleOwner: LifecycleOwner
) {

    private val TAG = "CameraXCaptureSource"

    private var cameraProviderFuture: ListenableFuture<ProcessCameraProvider>? = null
    private var imageAnalysis: ImageAnalysis? = null
    private var captureExecutor: ExecutorService = Executors.newSingleThreadExecutor()
    private var capturerObserver: CapturerObserver? = null
    private var isActive = false

    companion object {
        const val TARGET_WIDTH = 1280
        const val TARGET_HEIGHT = 720
        const val TARGET_FPS = 30
    }

    /**
     * Start the capture source with the given observer.
     */
    fun start(observer: CapturerObserver) {
        if (isActive) {
            Log.w(TAG, "Capture source already active")
            return
        }

        this.capturerObserver = observer
        isActive = true

        cameraProviderFuture = ProcessCameraProvider.getInstance(context)
        cameraProviderFuture?.addListener({
            val cameraProvider = cameraProviderFuture?.get()
            bindImageAnalysis(cameraProvider!!)
        }, ContextCompat.getMainExecutor(context))
    }

    /**
     * Stop the capture source and release resources.
     */
    fun release() {
        isActive = false
        capturerObserver = null

        cameraProviderFuture?.cancel(true)
        cameraProviderFuture = null

        imageAnalysis?.clearAnalyzer()
        imageAnalysis = null

        captureExecutor.shutdown()
        captureExecutor = Executors.newSingleThreadExecutor()

        Log.d(TAG, "Capture source released")
    }

    private fun bindImageAnalysis(cameraProvider: ProcessCameraProvider) {
        // Select back camera
        val cameraSelector = CameraSelector.DEFAULT_BACK_CAMERA

        // Configure ImageAnalysis - NO Preview surface
        val imageAnalysis = ImageAnalysis.Builder()
            .setTargetResolution(Size(TARGET_WIDTH, TARGET_HEIGHT))
            .setTargetRotation(Surface.ROTATION_0)
            .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
            .setOutputImageFormat(ImageAnalysis.OUTPUT_IMAGE_FORMAT_YUV_420_888)
            .build()

        imageAnalysis.setAnalyzer(captureExecutor, ImageAnalyzer(this))

        // Bind only ImageAnalysis to lifecycle
        cameraProvider.bindToLifecycle(lifecycleOwner, cameraSelector, imageAnalysis)
        this.imageAnalysis = imageAnalysis

        Log.d(TAG, "ImageAnalysis bound to lifecycle")
    }

    /**
     * ImageAnalyzer that converts YUV_420_888 to I420 and feeds WebRTC.
     */
    private inner class ImageAnalyzer(
        private val captureSource: CameraXCaptureSource
    ) : ImageAnalysis.Analyzer {

        override fun analyze(imageProxy: ImageProxy) {
            if (!isActive || capturerObserver == null) {
                imageProxy.close()
                return
            }

            val format = imageProxy.format
            if (format != ImageFormat.YUV_420_888) {
                Log.w(TAG, "Unexpected image format: $format")
                imageProxy.close()
                return
            }

            val planes = imageProxy.planes
            if (planes.size != 3) {
                Log.w(TAG, "Expected 3 planes, got ${planes.size}")
                imageProxy.close()
                return
            }

            val width = imageProxy.width
            val height = imageProxy.height

            val i420Buffer = try {
                toI420(width, height, planes)
            } catch (failure: Exception) {
                Log.e(TAG, failure.message ?: "Unsupported image layout")
                imageProxy.close()
                return
            }

            // Create VideoFrame and deliver to observer
            val timestampNs = System.nanoTime()
            val rotation = 0 // We handle rotation via targetRotation above
            val frame = VideoFrame(i420Buffer, rotation, timestampNs)

            capturerObserver?.onFrameCaptured(frame)

            // Release our reference after delivery; WebRTC retained its own.
            frame.release()

            imageProxy.close()
        }

        /**
         * Copies the three YUV_420_888 planes into a WebRTC I420 buffer,
         * honouring rowStride/pixelStride. This covers both planar (pixelStride 1)
         * and semi-planar (pixelStride 2) layouts without swapping U and V.
         */
        private fun toI420(width: Int, height: Int, planes: Array<PlaneProxy>): JavaI420Buffer {
            require(width % 2 == 0 && height % 2 == 0) { "Odd frame size ${width}x$height" }
            val uvWidth = width / 2
            val uvHeight = height / 2

            val buffer = JavaI420Buffer.allocate(width, height)
            try {
                copyPlane(planes[0], buffer.dataY, buffer.strideY, width, height)
                copyPlane(planes[1], buffer.dataU, buffer.strideU, uvWidth, uvHeight)
                copyPlane(planes[2], buffer.dataV, buffer.strideV, uvWidth, uvHeight)
            } catch (failure: Exception) {
                buffer.release()
                throw failure
            }
            return buffer
        }

        /**
         * Copies one plane into a packed destination, skipping the source padding.
         */
        private fun copyPlane(
            plane: PlaneProxy,
            dst: ByteBuffer,
            dstStride: Int,
            width: Int,
            height: Int
        ) {
            YuvPlaneCopier.copyPlane(
                src = plane.buffer,
                srcRowStride = plane.rowStride,
                srcPixelStride = plane.pixelStride,
                dst = dst,
                dstRowStride = dstStride,
                width = width,
                height = height,
            )
        }
    }
}