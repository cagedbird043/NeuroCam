// --- packages/android_sender/app/src/main/java/com/neurocam/NativeBridge.kt ---

package com.neurocam

import android.util.Log
import kotlinx.coroutines.flow.asSharedFlow
import java.nio.ByteBuffer

object NativeBridge {

    // --- Rust to Kotlin Communication via SharedFlow ---
    private val _keyFrameRequestFlow = kotlinx.coroutines.flow.MutableSharedFlow<Unit>()
    val keyFrameRequestFlow = _keyFrameRequestFlow.asSharedFlow()
    var videoEncoder: VideoEncoder? = null
    /**
     * 这个函数由 Rust 层的 JNI 代码调用，作为一个回调。
     * 它向一个 SharedFlow 发射一个事件，通知应用层需要请求一个关键帧。
     */
    @JvmStatic
    fun requestKeyFrameFromNative() {
        Log.i("NativeBridge", "JNI回调: requestKeyFrameFromNative 被调用")
        onIFrameRequestFromRust()
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
     * @param timestampNs 帧的捕获时间戳（纳秒）。
     */
    external fun sendVideoFrame(frameBuffer: java.nio.ByteBuffer, size: Int, isKeyFrame: Boolean, timestampNs: Long)

    external fun sendSpsPps(buffer: ByteArray, size: Int)

    fun onIFrameRequestFromRust() {
        Log.i("NativeBridge", "收到I-Frame请求，videoEncoder=${videoEncoder != null}")
        videoEncoder?.shouldSendSpsPps = true
        videoEncoder?.requestKeyFrame()
    }
    
    external fun close()
}