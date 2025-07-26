// --- packages/android_sender/app/src/main/java/com/neurocam/ImageProxyExt.kt ---

package com.neurocam

import androidx.camera.core.ImageProxy

/**
 * 将 YUV_420_888 格式的 ImageProxy 转换为 NV21 格式的 ByteArray。
 *
 * NV21 是一种标准的半平面格式，被许多硬件编码器所要求。
 * 它由一个全分辨率的 Y 平面和一个交错的 V/U 平面组成 (VUVUVU...)。
 *
 * 此函数正确处理了各种内存布局，包括行跨度 (row stride) 和像素跨度 (pixel stride)，
 * 以避免 BufferOverflowExceptions。
 *
 * @return 包含 NV21 格式图像数据的 ByteArray。
 */
fun ImageProxy.toNv21ByteArray(): ByteArray {
    val width = this.width
    val height = this.height

    // 1. 获取 Y, U, V 三个平面
    val yPlane = this.planes[0]
    val uPlane = this.planes[1]
    val vPlane = this.planes[2]

    val yBuffer = yPlane.buffer
    val uBuffer = uPlane.buffer
    val vBuffer = vPlane.buffer

    // AI-MOD-START
    // 核心修复：根据理论图像尺寸分配 ByteArray，而不是根据包含 padding 的 buffer.remaining()。
    // YUV420 格式的大小固定为 width * height * 1.5。
    val nv21 = ByteArray(width * height * 3 / 2)
    // AI-MOD-END

    // 3. 拷贝 Y 平面数据
    var yPos: Int
    if (yPlane.rowStride == width) {
        yBuffer.get(nv21, 0, width * height)
        yPos = width * height
    } else {
        yPos = 0
        for (row in 0 until height) {
            yBuffer.position(row * yPlane.rowStride)
            yBuffer.get(nv21, yPos, width)
            yPos += width
        }
    }

    // 4. 交错拷贝 U 和 V 平面数据，形成 V/U 格式 (NV21)
    val uvRowStride = vPlane.rowStride
    val uvPixelStride = vPlane.pixelStride

    var vuPos = yPos
    for (row in 0 until height / 2) {
        for (col in 0 until width / 2) {
            val vPos = row * uvRowStride + col * uvPixelStride
            // 修复 Gemini 的 bug: U 平面应该使用自己的 stride 和 pixel stride
            val uPos = row * uPlane.rowStride + col * uPlane.pixelStride

            if (vBuffer.capacity() > vPos && uBuffer.capacity() > uPos) {
                nv21[vuPos++] = vBuffer.get(vPos)
                nv21[vuPos++] = uBuffer.get(uPos)
            }
        }
    }

    return nv21
}