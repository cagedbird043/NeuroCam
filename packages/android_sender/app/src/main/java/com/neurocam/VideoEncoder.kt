// --- packages/android_sender/app/src/main/java/com/neurocam/VideoEncoder.kt ---
package com.neurocam

import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.util.Log
import androidx.camera.core.ImageProxy

/**
 * 封装了 MediaCodec API，用于将来自 CameraX 的 ImageProxy (YUV_420_888) 编码为 H.264 视频流。
 */
class VideoEncoder(
    private val width: Int,
    private val height: Int,
    private val bitrate: Int = 2_000_000 // 2 Mbps
) {
    companion object {
        private const val TAG = "NeuroCam/VideoEncoder"
        private const val MIME_TYPE = "video/avc" // H.264
        private const val FRAME_RATE = 30
        private const val I_FRAME_INTERVAL = 1 // 1 秒一个 I 帧
    }

    private var mediaCodec: MediaCodec? = null
    private var isRunning = false

    fun start() {
        if (isRunning) {
            Log.w(TAG, "Encoder is already running.")
            return
        }
        try {
            val format = MediaFormat.createVideoFormat(MIME_TYPE, width, height).apply {
                setInteger(MediaFormat.KEY_COLOR_FORMAT, MediaCodecInfo.CodecCapabilities.COLOR_FormatYUV420Flexible)
                setInteger(MediaFormat.KEY_BIT_RATE, bitrate)
                setInteger(MediaFormat.KEY_FRAME_RATE, FRAME_RATE)
                setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, I_FRAME_INTERVAL)
            }
            mediaCodec = MediaCodec.createEncoderByType(MIME_TYPE)
            mediaCodec?.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
            mediaCodec?.start()
            isRunning = true
            Log.i(TAG, "VideoEncoder started successfully.")
        } catch (e: Exception) {
            Log.e(TAG, "Failed to start VideoEncoder", e)
            stop()
        }
    }

    fun stop() {
        if (!isRunning) return
        try {
            mediaCodec?.stop()
            mediaCodec?.release()
        } catch (e: Exception) {
            Log.e(TAG, "Error stopping VideoEncoder", e)
        } finally {
            mediaCodec = null
            isRunning = false
            Log.i(TAG, "VideoEncoder stopped.")
        }
    }

    /**
     * 对单帧图像进行编码 (添加了诊断日志)。
     * @param imageProxy 来自 CameraX 的图像帧。
     */
    fun encodeFrame(imageProxy: ImageProxy) {
        if (!isRunning) return
        val codec = mediaCodec ?: return

        try {
            val nv21Data = imageProxy.toNv21ByteArray()

            val inputBufferIndex = codec.dequeueInputBuffer(10000)
            if (inputBufferIndex >= 0) {
                val inputBuffer = codec.getInputBuffer(inputBufferIndex)!!
                inputBuffer.clear()
                inputBuffer.put(nv21Data)
                codec.queueInputBuffer(
                    inputBufferIndex,
                    0,
                    nv21Data.size,
                    imageProxy.imageInfo.timestamp,
                    0
                )
            }

            val bufferInfo = MediaCodec.BufferInfo()
            while (true) {
                val outputBufferIndex = codec.dequeueOutputBuffer(bufferInfo, 0)
                when {
                    outputBufferIndex >= 0 -> {
                        val outputBuffer = codec.getOutputBuffer(outputBufferIndex)
                        if (outputBuffer != null && bufferInfo.size > 0) {
                            Log.d(TAG, "Encoded frame ready. Size: ${bufferInfo.size}. Calling NativeBridge.")

                            // AI-MOD-START
                            // 核心修复：调用新的 JNI 函数，将 bufferInfo.size 作为第二个参数传递
                            NativeBridge.sendVideoFrame(outputBuffer, bufferInfo.size)
                            // AI-MOD-END
                        }
                        codec.releaseOutputBuffer(outputBufferIndex, false)
                    }
                    outputBufferIndex == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED -> Log.i(TAG, "Output format changed: ${codec.outputFormat}")
                    outputBufferIndex == MediaCodec.INFO_TRY_AGAIN_LATER -> break
                    else -> break
                }
            }
        } catch (e: Exception) {
            Log.e(TAG, "Encoding error (Final Fix)", e)
        }
    }
}