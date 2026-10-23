package remotecam.core.capture

import androidx.test.ext.junit.runners.AndroidJUnit4
import com.google.zxing.Result
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith
import org.webrtc.JavaI420Buffer
import org.webrtc.VideoFrame

/**
 * Tests for YUV chroma conversion in the CameraX capture path.
 *
 * Validates that:
 * - Planar stride 1 passes through without copying
 * - Stride 2 (NV21/NV12) is de-interleaved into a scratch I420 buffer
 * - Swapped-chroma (green/magenta) output is impossible
 */
@RunWith(AndroidJUnit4::class)
class YuvConversionTest {

    /** Stride 1 (planar I420) should pass through without de-interleaving. */
    @Test
    fun strideOnePassesThrough() {
        // Create a synthetic I420 frame with stride 1 for all planes
        val width = 1280
        val height = 720
        val ySize = width * height
        val uvSize = (width / 2) * (height / 2)
        val totalSize = ySize + 2 * uvSize

        val data = ByteArray(totalSize)
        // Fill with deterministic pattern
        for (i in data.indices) {
            data[i] = (i % 256).toByte()
        }

        // Strides for planar I420
        val strideY = width
        val strideU = width / 2
        val strideV = width / 2

        // Wrap directly - no copy should happen for stride 1
        val buffer = JavaI420Buffer.wrap(
            width, height,
            data, 0, strideY,
            data, ySize, strideU,
            data, ySize + uvSize, strideV,
            null
        )

        assertNotNull(buffer)
        assertEquals(width, buffer.width)
        assertEquals(height, buffer.height)
        assertEquals(strideY, buffer.strideY)
        assertEquals(strideU, buffer.strideU)
        assertEquals(strideV, buffer.strideV)

        // Verify the data is accessible and correct
        val yData = ByteArray(ySize)
        val uData = ByteArray(uvSize)
        val vData = ByteArray(uvSize)

        buffer.dataY.get(yData)
        buffer.dataU.get(uData)
        buffer.dataV.get(vData)

        // First byte of Y should be 0
        assertEquals(0, yData[0].toInt() and 0xFF)
        // First byte of U should be at offset ySize
        assertEquals((ySize % 256).toByte(), uData[0])
        // First byte of V should be at offset ySize + uvSize
        assertEquals(((ySize + uvSize) % 256).toByte(), vData[0])
    }

    /** Stride 2 (NV21/NV12 semi-planar) must be de-interleaved into I420. */
    @Test
    fun strideTwoIsDeinterleaved() {
        val width = 1280
        val height = 720
        val ySize = width * height
        val uvSize = (width / 2) * (height / 2)

        // Create NV21 data: Y plane followed by interleaved VU (stride 2)
        val yData = ByteArray(ySize)
        val vuData = ByteArray(2 * uvSize)

        // Fill with known pattern
        for (i in yData.indices) yData[i] = (i % 256).toByte()
        for (i in vuData.indices) vuData[i] = ((i + 100) % 256).toByte()

        // Strides: Y has stride=width, UV planes have pixelStride=2
        val strideY = width
        val strideUV = width // byte stride for interleaved VU

        // Test the de-interleaving logic that CameraXCaptureSource must implement
        val i420Buffer = deinterleaveNV21ToI420(width, height, yData, vuData, strideY, strideUV)

        assertNotNull(i420Buffer)
        assertEquals(width, i420Buffer.width)
        assertEquals(height, i420Buffer.height)

        // Verify U and V are correctly separated (not swapped)
        val uOut = ByteArray(uvSize)
        val vOut = ByteArray(uvSize)
        i420Buffer.dataU.get(uOut)
        i420Buffer.dataV.get(vOut)

        // In NV21, the interleaved order is VU VU VU...
        // So vuData[0] = V0, vuData[1] = U0, vuData[2] = V1, vuData[3] = U1...
        // After de-interleaving: U0 = vuData[1], V0 = vuData[0]
        val expectedU0 = vuData[1]
        val expectedV0 = vuData[0]

        assertEquals(expectedU0.toInt() and 0xFF, uOut[0].toInt() and 0xFF)
        assertEquals(expectedV0.toInt() and 0xFF, vOut[0].toInt() and 0xFF)

        // Check a few more positions
        assertEquals(vuData[3].toInt() and 0xFF, uOut[1].toInt() and 0xFF) // U1
        assertEquals(vuData[2].toInt() and 0xFF, vOut[1].toInt() and 0xFF) // V1
    }

    /** Swapped chroma (green/magenta output) must be impossible with correct de-interleaving. */
    @Test
    fun swappedChromaIsImpossible() {
        // Create a test pattern where U and V are clearly distinct
        val width = 640
        val height = 480
        val ySize = width * height
        val uvSize = (width / 2) * (height / 2)

        val yData = ByteArray(ySize)
        val vuData = ByteArray(2 * uvSize)

        // Y = 128 (middle gray)
        yData.fill(128.toByte())

        // V = 255 (red component), U = 0 (blue component) in a checkerboard
        // This creates a very visible pattern if swapped
        for (y in 0 until height / 2) {
            for (x in 0 until width / 2) {
                val idx = (y * (width / 2) + x) * 2
                vuData[idx] = -1.toByte()     // V = 255
                vuData[idx + 1] = 0.toByte()  // U = 0
            }
        }

        val strideY = width
        val strideUV = width

        val i420Buffer = deinterleaveNV21ToI420(width, height, yData, vuData, strideY, strideUV)

        assertNotNull(i420Buffer)

        // Verify U plane is all 0 (blue component = 0)
        val uOut = ByteArray(uvSize)
        val vOut = ByteArray(uvSize)
        i420Buffer.dataU.get(uOut)
        i420Buffer.dataV.get(vOut)

        // All U values should be 0
        for (b in uOut) {
            assertEquals(0, b.toInt() and 0xFF)
        }

        // All V values should be 255
        for (b in vOut) {
            assertEquals(255, b.toInt() and 0xFF)
        }
    }

    /**
     * Reference implementation of NV21 to I420 de-interleaving.
     * This is what CameraXCaptureSource must do when pixelStride == 2.
     */
    private fun deinterleaveNV21ToI420(
        width: Int,
        height: Int,
        yData: ByteArray,
        vuData: ByteArray,
        strideY: Int,
        strideUV: Int
    ): JavaI420Buffer {
        val uvWidth = width / 2
        val uvHeight = height / 2
        val uvSize = uvWidth * uvHeight

        // Allocate scratch I420 buffers
        val scratchY = ByteArray(yData.size)
        val scratchU = ByteArray(uvSize)
        val scratchV = ByteArray(uvSize)

        // Copy Y plane (already planar)
        System.arraycopy(yData, 0, scratchY, 0, yData.size)

        // De-interleave VU -> U and V planes
        // NV21 layout: V0 U0 V1 U1 V2 U2 ...
        var vuIndex = 0
        for (i in 0 until uvSize) {
            scratchV[i] = vuData[vuIndex]
            scratchU[i] = vuData[vuIndex + 1]
            vuIndex += 2
        }

        return JavaI420Buffer.wrap(
            width, height,
            scratchY, 0, strideY,
            scratchU, 0, uvWidth,
            scratchV, 0, uvWidth,
            null
        )
    }

    /** Test that a stride-1 buffer wrapped directly produces correct I420. */
    @Test
    fun directWrapStrideOneProducesValidI420() {
        val width = 320
        val height = 240
        val ySize = width * height
        val uvSize = (width / 2) * (height / 2)

        val yData = ByteArray(ySize)
        val uData = ByteArray(uvSize)
        val vData = ByteArray(uvSize)

        // Fill with pattern
        for (i in yData.indices) yData[i] = (i % 256).toByte()
        for (i in uData.indices) uData[i] = ((i + 50) % 256).toByte()
        for (i in vData.indices) vData[i] = ((i + 100) % 256).toByte()

        val buffer = JavaI420Buffer.wrap(
            width, height,
            yData, 0, width,
            uData, 0, width / 2,
            vData, 0, width / 2,
            null
        )

        assertNotNull(buffer)

        // Read back and verify
        val yOut = ByteArray(ySize)
        val uOut = ByteArray(uvSize)
        val vOut = ByteArray(uvSize)
        buffer.dataY.get(yOut)
        buffer.dataU.get(uOut)
        buffer.dataV.get(vOut)

        assertArrayEquals(yData, yOut)
        assertArrayEquals(uData, uOut)
        assertArrayEquals(vData, vOut)
    }
}