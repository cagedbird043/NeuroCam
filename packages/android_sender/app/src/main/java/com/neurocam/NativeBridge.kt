// --- packages/android_sender/app/src/main/java/com/neurocam/NativeBridge.kt ---

package com.neurocam

import java.nio.ByteBuffer

object NativeBridge {

    init {
        System.loadLibrary("android_sender")
    }

    external fun init()

    // AI-MOD-START
    /**
     * 发送一个视频帧到 Rust 层进行处理。
     * @param frameBuffer 一个包含 H.264 编码数据的 Direct ByteBuffer。
     * @param size 缓冲区中有效数据的实际大小。
     */
    external fun sendVideoFrame(frameBuffer: ByteBuffer, size: Int)
    // AI-MOD-END

    external fun close()
}