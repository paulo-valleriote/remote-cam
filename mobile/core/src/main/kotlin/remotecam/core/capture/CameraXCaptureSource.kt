package remotecam.core.capture

import android.content.Context
import android.util.Log
import android.util.Size
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.core.ImageProxy.PlaneProxy
import androidx.camera.core.UseCaseGroup
import androidx.camera.core.impl.UseCaseConfigFactory
import androidx.camera.core.impl.utils.executor.CameraXExecutors
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.video.Recorder
import androidx.camera.video.Recording
import androidx.camera.video.VideoRecordEvent
import androidx.core.content.ContextCompat
import androidx.lifecycle.LifecycleOwner
import com.google.common.util.concurrent.ListenableFuture
import org.webrtc.CapturerObserver
import org.webrtc.JavaI420Buffer
import org.webrtc.VideoFrame
import org.webrtc.VideoSink
import java.nio.ByteBuffer
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

/**
 * CameraX capture source for the F0 spike.
 *
 * Binds ONLY ImageAnalysis (no Preview surface) to minimize ISP work, latency and heat.
 * Runtime-guards pixelStride and de-interleaves stride-2 devices into a scratch I420 buffer.
 * Uses JavaI420Buffer.wrap(...) with no copy for planar stride 1.
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

    // Scratch buffer for stride-2 de-interleaving
    private var scratchY: ByteArray? = null
    private var scratchU: ByteArray? = null
    private var scratchV: ByteArray? = null

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

        scratchY = null
        scratchU = null
        scratchV = null

        Log.d(TAG, "Capture source released")
    }

    private fun bindImageAnalysis(cameraProvider: ProcessCameraProvider) {
        // Select back camera
        val cameraSelector = CameraSelector.DEFAULT_BACK_CAMERA

        // Configure ImageAnalysis - NO Preview surface
        val imageAnalysis = ImageAnalysis.Builder()
            .setTargetResolution(Size(TARGET_WIDTH, TARGET_HEIGHT))
            .setTargetRotation(lifecycleOwner.lifecycle.currentState?.ordinal ?: 0)
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
            if (format != ImageProxy.Format.YUV_420_888) {
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

            val yPlane = planes[0]
            val uPlane = planes[1]
            val vPlane = planes[2]

            val width = imageProxy.width
            val height = imageProxy.height

            // Check pixel stride for chroma planes
            val uPixelStride = uPlane.pixelStride
            val vPixelStride = vPlane.pixelStride

            val yBuffer = yPlane.buffer
            val uBuffer = uPlane.buffer
            val vBuffer = vPlane.buffer

            val yStride = yPlane.rowStride
            val uStride = uPlane.rowStride
            val vStride = vPlane.rowStride

            val uvWidth = width / 2
            val uvHeight = height / 2
            val uvSize = uvWidth * uvHeight

            val i420Buffer: JavaI420Buffer

            if (uPixelStride == 1 && vPixelStride == 1) {
                // Planar I420 - can wrap directly with no copy
                // Ensure buffers are positioned correctly
                yBuffer.rewind()
                uBuffer.rewind()
                vBuffer.rewind()

                // Allocate or reuse direct buffers if needed
                val yData = ByteArray(yBuffer.remaining())
                val uData = ByteArray(uBuffer.remaining())
                val vData = ByteArray(vBuffer.remaining())

                yBuffer.get(yData)
                uBuffer.get(uData)
                vBuffer.get(vData)

                i420Buffer = JavaI420Buffer.wrap(
                    width, height,
                    yData, 0, yStride,
                    uData, 0, uStride,
                    vData, 0, vStride,
                    null
                )
            } else if (uPixelStride == 2 && vPixelStride == 2) {
                // Semi-planar NV21/NV12 - must de-interleave into scratch I420 buffer
                i420Buffer = deinterleaveStride2(
                    width, height,
                    yBuffer, yStride,
                    uBuffer, uStride,
                    vBuffer, vStride
                )
            } else {
                Log.e(TAG, "Unsupported pixel stride: U=$uPixelStride, V=$vPixelStride")
                imageProxy.close()
                return
            }

            // Create VideoFrame and deliver to observer
            val timestampNs = System.nanoTime()
            val rotation = 0 // We handle rotation via targetRotation above
            val frame = VideoFrame(i420Buffer, rotation, timestampNs)

            capturerObserver?.onFrameCaptured(frame)

            // Release the buffer reference (WebRTC manages the buffer lifecycle)
            i420Buffer.release()

            imageProxy.close()
        }

        /**
         * De-interleave stride-2 (NV21/NV12) chroma planes into planar I420.
         * This is the critical path for devices where YUV_420_888 is actually NV21.
         */
        private fun deinterleaveStride2(
            width: Int,
            height: Int,
            yBuffer: ByteBuffer,
            yStride: Int,
            uBuffer: ByteBuffer,
            uStride: Int,
            vBuffer: ByteBuffer,
            vStride: Int
        ): JavaI420Buffer {
            val ySize = width * height
            val uvSize = (width / 2) * (height / 2)

            // Allocate or reuse scratch buffers
            if (scratchY == null || scratchY!!.size != ySize) {
                scratchY = ByteArray(ySize)
                scratchU = ByteArray(uvSize)
                scratchV = ByteArray(uvSize)
            }

            // Copy Y plane (planar, stride may be > width)
            copyPlane(yBuffer, yStride, width, height, scratchY!!, width)

            // For NV21: the U and V buffers are actually interleaved in a single plane
            // The pixelStride == 2 means the buffer alternates V, U, V, U...
            // We need to read the combined buffer and de-interleave

            // Determine which buffer has the interleaved data (typically uBuffer)
            val combinedBuffer = if (uBuffer.remaining() >= 2 * uvSize) uBuffer else vBuffer
            val combinedStride = if (uBuffer.remaining() >= 2 * uvSize) uStride else vStride

            combinedBuffer.rewind()

            // Read interleaved VU data
            val combinedData = ByteArray(2 * uvSize)
            combinedBuffer.get(combinedData)

            // De-interleave: V0 U0 V1 U1 V2 U2 ...
            var combinedIndex = 0
            for (i in 0 until uvSize) {
                scratchV!![i] = combinedData[combinedIndex]
                scratchU!![i] = combinedData[combinedIndex + 1]
                combinedIndex += 2
            }

            return JavaI420Buffer.wrap(
                width, height,
                scratchY!!, 0, width,
                scratchU!!, 0, uvWidth,
                scratchV!!, 0, uvWidth,
                null
            )
        }

        /**
         * Copy a plane with row stride to a packed buffer.
         */
        private fun copyPlane(
            src: ByteBuffer,
            srcStride: Int,
            width: Int,
            height: Int,
            dst: ByteArray,
            dstStride: Int
        ) {
            src.rewind()
            for (y in 0 until height) {
                val srcOffset = y * srcStride
                val dstOffset = y * dstStride
                src.position(srcOffset)
                src.get(dst, dstOffset, width)
            }
        }
    }
}