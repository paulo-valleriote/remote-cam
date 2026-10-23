package remotecam.core.capture

import java.nio.ByteBuffer

/**
 * Copies one YUV plane into a packed destination buffer.
 *
 * Kept free of Android and WebRTC types so the stride handling can be unit
 * tested on the JVM. A source pixel is read from
 * `src[row * srcRowStride + column * srcPixelStride]`, which covers both planar
 * (pixel stride 1) and semi-planar (pixel stride 2) YUV_420_888 layouts.
 */
object YuvPlaneCopier {

    fun copyPlane(
        src: ByteBuffer,
        srcRowStride: Int,
        srcPixelStride: Int,
        dst: ByteBuffer,
        dstRowStride: Int,
        width: Int,
        height: Int,
    ) {
        require(width >= 0 && height >= 0) { "Negative plane dimensions" }
        require(srcRowStride >= width * srcPixelStride) {
            "Source row stride $srcRowStride cannot hold $width pixels at stride $srcPixelStride"
        }
        require(dstRowStride >= width) { "Destination row stride $dstRowStride is smaller than $width" }

        for (row in 0 until height) {
            val srcRow = row * srcRowStride
            val dstRow = row * dstRowStride
            for (column in 0 until width) {
                dst.put(dstRow + column, src.get(srcRow + column * srcPixelStride))
            }
        }
    }
}
