// --- packages/android_sender/app/src/main/java/com/neurocam/VideoEncoder.kt ---
package com.neurocam

import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.os.Bundle
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
        private const val I_FRAME_INTERVAL = 1 // 1 秒一个 I-帧
    }

    private var mediaCodec: MediaCodec? = null
    private var isRunning = false

    // AI-MOD-START
    /**
     *  请求编码器立即生成一个关键帧 (I-frame)。
     *  这是一个异步请求，编码器将在下一个可用的时机生成I-frame。
     */
    fun requestKeyFrame() {
        if (!isRunning || mediaCodec == null) {
            Log.w(TAG, "Cannot request key frame, encoder is not running.")
            return
        }
        try {
            Log.i(TAG, "Actively requesting a new I-Frame...")
            val params = Bundle()
            params.putInt(MediaCodec.PARAMETER_KEY_REQUEST_SYNC_FRAME, 0)
            mediaCodec?.setParameters(params)
        } catch (e: Exception) {
            Log.e(TAG, "Failed to request key frame", e)
        }
    }
    // AI-MOD-END

    fun nv21ToNv12(nv21: ByteArray, width: Int, height: Int): ByteArray {
        val nv12 = ByteArray(nv21.size)
        val frameSize = width * height
        // 拷贝Y分量
        System.arraycopy(nv21, 0, nv12, 0, frameSize)
        // 交换UV分量
        var i = 0
        while (i < frameSize / 2) {
            nv12[frameSize + i] = nv21[frameSize + i + 1] // U
            nv12[frameSize + i + 1] = nv21[frameSize + i] // V
            i += 2
        }
        return nv12
    }

    fun start() {
        if (isRunning) {
            Log.w(TAG, "Encoder is already running.")
            return
        }
        try {
            val format = MediaFormat.createVideoFormat(MIME_TYPE, width, height).apply {
                setInteger(MediaFormat.KEY_COLOR_FORMAT, MediaCodecInfo.CodecCapabilities.COLOR_FormatYUV420SemiPlanar) // NV12
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

    fun encodeFrame(imageProxy: ImageProxy) {
        if (!isRunning) return
        val codec = mediaCodec ?: return

        try {
            val nv21Data = imageProxy.toNv21ByteArray()
            val nv12Data = nv21ToNv12(nv21Data, width, height) // 新增：转换为NV12
            val inputBufferIndex = codec.dequeueInputBuffer(10000)
            if (inputBufferIndex >= 0) {
                val inputBuffer = codec.getInputBuffer(inputBufferIndex)!!
                inputBuffer.clear()
                inputBuffer.put(nv12Data)
                // 这里的 presentationTimeUs 仍然使用 ImageProxy 的时间戳，以保证帧的顺序
                codec.queueInputBuffer(
                    inputBufferIndex,
                    0,
                    nv12Data.size,
                    imageProxy.imageInfo.timestamp / 1000, // 将纳秒转换为微秒给 MediaCodec
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
                            val isKeyFrame = (bufferInfo.flags and MediaCodec.BUFFER_FLAG_KEY_FRAME) != 0

                            // AI-MOD-START
                            // 核心修复：不再使用 bufferInfo 的时间戳，因为它基于单调时钟。
                            // 我们在即将发送数据时，获取当前的“墙上时钟”时间（基于Unix纪元），
                            // 这样就能与 Linux 端的时钟进行有意义的比较。
                            val timestampNs = System.currentTimeMillis() * 1_000_000
                            NativeBridge.sendVideoFrame(outputBuffer, bufferInfo.size, isKeyFrame, timestampNs)
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