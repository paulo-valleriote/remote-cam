package remotecam.core.capture

import java.nio.ByteBuffer
import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertFailsWith

/**
 * Tests for the YUV_420_888 plane copy used by the CameraX capture path.
 *
 * Validates that:
 * - Row padding is skipped when packing the destination
 * - Pixel stride 2 (NV21/NV12) never swaps the U and V planes
 * - Impossible strides are rejected instead of silently producing corruption
 */
class YuvConversionTest {

    @Test
    fun copiesPlanarPlaneSkippingRowPadding() {
        val width = 4
        val height = 2
        val srcStride = 6
        val src = ByteBuffer.allocateDirect(srcStride * height)
        val expected = ByteArray(width * height)

        for (row in 0 until height) {
            for (column in 0 until width) {
                val value = (row * width + column + 1).toByte()
                src.put(row * srcStride + column, value)
                expected[row * width + column] = value
            }
            // Padding bytes must never leak into the packed destination.
            src.put(row * srcStride + width, 0x7f)
            src.put(row * srcStride + width + 1, 0x7f)
        }

        val dst = ByteBuffer.allocateDirect(width * height)
        YuvPlaneCopier.copyPlane(
            src = src,
            srcRowStride = srcStride,
            srcPixelStride = 1,
            dst = dst,
            dstRowStride = width,
            width = width,
            height = height,
        )

        val actual = ByteArray(width * height)
        dst.get(0, actual)
        assertContentEquals(expected, actual)
    }

    @Test
    fun copiesSemiPlanarChromaWithoutSwappingUAndV() {
        val uvWidth = 2
        val uvHeight = 2
        val rowStride = 4
        val backing = ByteBuffer.allocateDirect(rowStride * uvHeight)
        val expectedU = ByteArray(uvWidth * uvHeight)
        val expectedV = ByteArray(uvWidth * uvHeight)

        // NV21 layout: V0 U0 V1 U1 per row.
        for (row in 0 until uvHeight) {
            for (column in 0 until uvWidth) {
                val index = row * uvWidth + column
                expectedV[index] = (200 + index).toByte()
                expectedU[index] = (10 + index).toByte()
                backing.put(row * rowStride + column * 2, expectedV[index])
                backing.put(row * rowStride + column * 2 + 1, expectedU[index])
            }
        }

        val vView = backing.duplicate().apply { position(0) }.slice()
        val uView = backing.duplicate().apply { position(1) }.slice()

        val uDst = ByteBuffer.allocateDirect(uvWidth * uvHeight)
        val vDst = ByteBuffer.allocateDirect(uvWidth * uvHeight)
        YuvPlaneCopier.copyPlane(uView, rowStride, 2, uDst, uvWidth, uvWidth, uvHeight)
        YuvPlaneCopier.copyPlane(vView, rowStride, 2, vDst, uvWidth, uvWidth, uvHeight)

        val actualU = ByteArray(uvWidth * uvHeight)
        val actualV = ByteArray(uvWidth * uvHeight)
        uDst.get(0, actualU)
        vDst.get(0, actualV)

        assertContentEquals(expectedU, actualU)
        assertContentEquals(expectedV, actualV)
    }

    @Test
    fun rejectsSourceStrideThatCannotHoldTheWidth() {
        val src = ByteBuffer.allocateDirect(8)
        val dst = ByteBuffer.allocateDirect(8)

        assertFailsWith<IllegalArgumentException> {
            YuvPlaneCopier.copyPlane(
                src = src,
                srcRowStride = 2,
                srcPixelStride = 1,
                dst = dst,
                dstRowStride = 4,
                width = 4,
                height = 1,
            )
        }
    }
}
