// --- packages/android_sender/app/src/main/java/com/neurocam/VideoEncoder.kt (REVISED AND ROBUST FIX) ---
package com.neurocam

import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.os.Bundle
import android.util.Log
import androidx.camera.core.ImageProxy
import java.nio.ByteBuffer

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

    // AI-MOD-START: 新增一个变量来缓存 SPS/PPS 配置数据
    private var csdBuffer: ByteArray? = null
    // AI-MOD-END

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
                setInteger(MediaFormat.KEY_PROFILE, MediaCodecInfo.CodecProfileLevel.AVCProfileBaseline)

                // 我们不再完全依赖这个标志，但保留它也无妨
                if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.Q) {
                    setInteger(MediaFormat.KEY_PREPEND_HEADER_TO_SYNC_FRAMES, 1)
                }
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
            csdBuffer = null // 清理缓存
            Log.i(TAG, "VideoEncoder stopped.")
        }
    }

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
                    imageProxy.imageInfo.timestamp * 1000, // 注意这里是微秒
                    0
                )
            }

            val bufferInfo = MediaCodec.BufferInfo()
            while (true) {
                val outputBufferIndex = codec.dequeueOutputBuffer(bufferInfo, 0)
                when {
                    // AI-MOD-START: 捕获 CODEC_CONFIG buffer
                    outputBufferIndex >= 0 && (bufferInfo.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG) != 0 -> {
                        val outputBuffer = codec.getOutputBuffer(outputBufferIndex)!!
                        val csd = ByteArray(bufferInfo.size)
                        outputBuffer.get(csd)
                        csdBuffer = csd // 缓存 SPS/PPS
                        Log.i(TAG, "Captured SPS/PPS (Codec-Config) data. Size: ${csd.size}")
                        codec.releaseOutputBuffer(outputBufferIndex, false)
                    }
                    // AI-MOD-END

                    outputBufferIndex >= 0 -> {
                        val outputBuffer = codec.getOutputBuffer(outputBufferIndex)
                        if (outputBuffer != null && bufferInfo.size > 0) {
                            val isKeyFrame = (bufferInfo.flags and MediaCodec.BUFFER_FLAG_KEY_FRAME) != 0
                            val timestampNs = System.currentTimeMillis() * 1_000_000

                            // AI-MOD-START: 核心修复逻辑
                            // 如果是关键帧，并且我们已经缓存了SPS/PPS，则将它们拼接在一起发送
                            if (isKeyFrame && csdBuffer != null) {
                                Log.d(TAG, "Prepending SPS/PPS to a keyframe.")
                                val frameData = ByteArray(bufferInfo.size)
                                outputBuffer.get(frameData)

                                val combinedBuffer = csdBuffer!! + frameData
                                val directBuffer = ByteBuffer.allocateDirect(combinedBuffer.size)
                                directBuffer.put(combinedBuffer)
                                directBuffer.flip()

                                NativeBridge.sendVideoFrame(directBuffer, directBuffer.remaining(), isKeyFrame, timestampNs)
                            } else {
                                // 对于非关键帧，或者在SPS/PPS还没收到时的第一个关键帧，直接发送
                                NativeBridge.sendVideoFrame(outputBuffer, bufferInfo.size, isKeyFrame, timestampNs)
                            }
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
            Log.e(TAG, "Encoding error", e)
        }
    }
}