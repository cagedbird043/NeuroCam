// --- packages/android_sender/app/src/main/java/com/neurocam/NativeBridge.kt ---

package com.neurocam

import kotlinx.coroutines.flow.asSharedFlow
import java.nio.ByteBuffer

object NativeBridge {

    // --- Rust to Kotlin Communication via SharedFlow ---
    private val _keyFrameRequestFlow = kotlinx.coroutines.flow.MutableSharedFlow<Unit>()
    val keyFrameRequestFlow = _keyFrameRequestFlow.asSharedFlow()

    /**
     * 这个函数由 Rust 层的 JNI 代码调用，作为一个回调。
     * 它向一个 SharedFlow 发射一个事件，通知应用层需要请求一个关键帧。
     */
    @JvmStatic // 确保 Rust 可以像调用静态方法一样调用它
    private fun requestKeyFrameFromNative() {
        // tryEmit 是非阻塞的，适合从外部线程调用
        _keyFrameRequestFlow.tryEmit(Unit)
    }
    // --- End of Communication Channel ---

    init {
        System.loadLibrary("android_sender")
    }

    external fun init()

    // AI-MOD-START
    /**
     * 发送一个视频帧到 Rust 层进行处理。
     * @param frameBuffer 一个包含 H.264 编码数据的 Direct ByteBuffer。
     * @param size 缓冲区中有效数据的实际大小。
     * @param isKeyFrame 标记此帧是否为关键帧 (I-frame)。
     */
    external fun sendVideoFrame(frameBuffer: ByteBuffer, size: Int, isKeyFrame: Boolean)
    // AI-MOD-END
    external fun close()
}