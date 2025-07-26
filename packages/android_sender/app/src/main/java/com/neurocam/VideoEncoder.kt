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
            // 1. 使用我们的扩展函数将 ImageProxy 转换为 NV21 格式的字节数组。
            val nv21Data = imageProxy.toNv21ByteArray()

            // --- 输入阶段 ---
            val inputBufferIndex = codec.dequeueInputBuffer(10000)
            if (inputBufferIndex >= 0) {
                val inputBuffer = codec.getInputBuffer(inputBufferIndex)!!

                // AI-MOD-START
                // --- 诊断步骤 ---
                // 打印出缓冲区和我们数据的大小，以找出不匹配的原因。
                // 这是解决问题的关键。
                val inputCapacity = inputBuffer.capacity()
                val dataSize = nv21Data.size

                if (inputCapacity < dataSize) {
                    Log.e(TAG, "CRITICAL ERROR: BufferOverflow about to happen. " +
                            "InputBuffer Capacity: $inputCapacity, " +
                            "Data Size: $dataSize")
                    // 如果容量不足，我们就不执行 put 操作，直接返回，避免崩溃。
                    // 这样我们可以安全地看到日志。
                    return
                }

                Log.d(TAG, "Buffer Info: inputBuffer capacity=${inputCapacity}, nv21Data size=${dataSize}")
                // --- 诊断结束 ---
                // AI-MOD-END

                inputBuffer.clear()
                // 2. 将准备好的字节数组放入编码器的输入缓冲区。
                inputBuffer.put(nv21Data)

                codec.queueInputBuffer(
                    inputBufferIndex,
                    0,
                    nv21Data.size, // 使用字节数组的实际大小
                    imageProxy.imageInfo.timestamp,
                    0
                )
            }

            // --- 输出阶段 ---
            val bufferInfo = MediaCodec.BufferInfo()
            while (true) {
                val outputBufferIndex = codec.dequeueOutputBuffer(bufferInfo, 0)
                when {
                    outputBufferIndex >= 0 -> {
                        val outputBuffer = codec.getOutputBuffer(outputBufferIndex)
                        if (outputBuffer != null && bufferInfo.size > 0) {
                            Log.d(TAG, "Encoded frame ready. Size: ${bufferInfo.size}. Calling NativeBridge.")
                            outputBuffer.position(bufferInfo.offset)
                            outputBuffer.limit(bufferInfo.offset + bufferInfo.size)
                            NativeBridge.sendVideoFrame(outputBuffer)
                        }
                        codec.releaseOutputBuffer(outputBufferIndex, false)
                    }
                    outputBufferIndex == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED -> Log.i(TAG, "Output format changed: ${codec.outputFormat}")
                    outputBufferIndex == MediaCodec.INFO_TRY_AGAIN_LATER -> break
                    else -> break
                }
            }
        } catch (e: Exception) {
            Log.e(TAG, "Encoding error (Diagnostic Step)", e)
        }
    }
}