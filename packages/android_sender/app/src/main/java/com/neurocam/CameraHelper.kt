package com.neurocam

import android.annotation.SuppressLint
import android.content.Context
import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CameraManager
import android.os.Handler
import android.os.HandlerThread
import android.util.Log
import android.util.Size
import android.view.Display // 确保这个 import 存在
import android.view.Surface
import android.view.SurfaceHolder
import android.graphics.SurfaceTexture
import java.util.Collections
import kotlin.math.abs

class CameraHelper(private val context: Context, private val listener: Listener? = null) {

    /**
     * 一个简单的回调接口，用于通知调用者摄像头状态。
     */
    interface Listener {
        fun onCameraStarted()
        fun onCameraError(error: String)
        // AI-MOD-START
        /**
         * 当选择了最佳预览尺寸后回调
         * @param size 选定的预览尺寸
         */
        fun onPreviewSizeSelected(size: Size)
        // AI-MOD-END
    }

    companion object {
        private const val TAG = "NeuroCam/CameraHelper"
    }

    private val cameraManager: CameraManager by lazy {
        context.getSystemService(Context.CAMERA_SERVICE) as CameraManager
    }

    private var cameraThread: HandlerThread? = null
    private var cameraHandler: Handler? = null

    private var cameraDevice: CameraDevice? = null
    private var captureSession: CameraCaptureSession? = null

    private var targetSurface: Surface? = null

    @SuppressLint("MissingPermission")
    fun startCamera(surface: Surface) {
        Log.i(TAG, "Starting camera setup...")
        if (!surface.isValid) {
            val errorMsg = "Provided Surface is invalid."
            Log.e(TAG, errorMsg)
            listener?.onCameraError(errorMsg)
            return
        }
        this.targetSurface = surface
        startCameraThread()

        try {
            val cameraId = findBackFacingCamera()
                ?: throw IllegalStateException("No back-facing camera found.")
            Log.i(TAG, "Found back-facing camera with ID: $cameraId")

            val characteristics = cameraManager.getCameraCharacteristics(cameraId)
            val streamConfigurationMap = characteristics.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP)
                ?: throw IllegalStateException("Cannot get stream configuration map.")

            // AI-MOD-START
            // 修正：现在我们使用 TextureView，查询尺寸时应该针对 SurfaceTexture
            val supportedSizes = streamConfigurationMap.getOutputSizes(SurfaceTexture::class.java)
            if (supportedSizes.isNullOrEmpty()) {
                throw IllegalStateException("No supported preview sizes found for SurfaceTexture.")
            }

            // 从支持的尺寸中选择一个最佳尺寸
            val previewSize = chooseOptimalSize(supportedSizes)
            // AI-MOD-END
            Log.i(TAG, "Optimal preview size selected: ${previewSize.width}x${previewSize.height}")

            // 通过回调通知UI
            listener?.onPreviewSizeSelected(previewSize)

            cameraManager.openCamera(cameraId, deviceStateCallback, cameraHandler)
        } catch (e: Exception) {
            val errorMsg = "Failed to initiate camera opening: ${e.message}"
            Log.e(TAG, errorMsg, e)
            listener?.onCameraError(errorMsg)
        }
    }


    /**
     * 【修正版 V3】选择一个合适的视频预览尺寸。
     * 策略：在所有不超过 1080p 的尺寸中，选择面积最大的那个。
     */
    private fun chooseOptimalSize(supportedSizes: Array<Size>): Size {
        val maxPreviewWidth = 1920
        val maxPreviewHeight = 1080

        val suitableSizes = supportedSizes.filter {
            it.width <= maxPreviewWidth && it.height <= maxPreviewHeight
        }

        return if (suitableSizes.isNotEmpty()) {
            Collections.max(suitableSizes) { a, b ->
                (a.width.toLong() * a.height).compareTo(b.width.toLong() * b.height)
            }
        } else {
            // 如果没有小于1080p的，就退回到选择面积最小的
            supportedSizes.minByOrNull { it.width.toLong() * it.height } ?: supportedSizes.first()
        }
    }

    fun closeCamera() {
        Log.i(TAG, "Closing camera and releasing resources...")
        try {
            captureSession?.close()
            captureSession = null
            cameraDevice?.close()
            cameraDevice = null
            targetSurface = null
        } catch (e: Exception) {
            Log.e(TAG, "Error closing camera resources", e)
        } finally {
            stopCameraThread()
        }
    }

    private val deviceStateCallback = object : CameraDevice.StateCallback() {
        override fun onOpened(camera: CameraDevice) {
            Log.i(TAG, "CameraDevice.StateCallback: onOpened")
            cameraDevice = camera
            createCaptureSession()
        }

        override fun onDisconnected(camera: CameraDevice) {
            val errorMsg = "Camera was disconnected."
            Log.w(TAG, errorMsg)
            listener?.onCameraError(errorMsg)
            camera.close()
            cameraDevice = null
        }

        override fun onError(camera: CameraDevice, error: Int) {
            val errorMsg = "CameraDevice.StateCallback: onError - Code $error"
            Log.e(TAG, errorMsg)
            listener?.onCameraError(errorMsg)
            camera.close()
            cameraDevice = null
        }
    }

    private fun createCaptureSession() {
        val device = cameraDevice
        val surface = targetSurface
        if (device == null || surface == null) {
            val errorMsg = "Cannot create capture session, cameraDevice or surface is null."
            Log.e(TAG, errorMsg)
            listener?.onCameraError(errorMsg)
            return
        }

        try {
            Log.i(TAG, "Creating capture session...")
            device.createCaptureSession(listOf(surface), sessionStateCallback, cameraHandler)
        } catch (e: Exception) {
            val errorMsg = "Failed to create capture session: ${e.message}"
            Log.e(TAG, errorMsg, e)
            listener?.onCameraError(errorMsg)
        }
    }

    private val sessionStateCallback = object : CameraCaptureSession.StateCallback() {
        override fun onConfigured(session: CameraCaptureSession) {
            Log.i(TAG, "CaptureSession.StateCallback: onConfigured")
            captureSession = session
            sendRepeatingPreviewRequest()
        }

        override fun onConfigureFailed(session: CameraCaptureSession) {
            val errorMsg = "CaptureSession.StateCallback: onConfigureFailed"
            Log.e(TAG, errorMsg)
            listener?.onCameraError(errorMsg)
        }
    }

    private fun sendRepeatingPreviewRequest() {
        val session = captureSession
        val surface = targetSurface
        if (session == null || surface == null) {
            val errorMsg = "Cannot send repeating request, session or surface is null."
            Log.e(TAG, errorMsg)
            listener?.onCameraError(errorMsg)
            return
        }
        try {
            Log.i(TAG, "Sending repeating preview request...")
            val builder = session.device.createCaptureRequest(CameraDevice.TEMPLATE_PREVIEW)
            builder.addTarget(surface)
            session.setRepeatingRequest(builder.build(), null, cameraHandler)
            Log.i(TAG, "Camera is now streaming to the Surface.")
            listener?.onCameraStarted()
        } catch (e: Exception) {
            val errorMsg = "Failed to send repeating preview request: ${e.message}"
            Log.e(TAG, errorMsg, e)
            listener?.onCameraError(errorMsg)
        }
    }

    private fun findBackFacingCamera(): String? {
        for (cameraId in cameraManager.cameraIdList) {
            val characteristics = cameraManager.getCameraCharacteristics(cameraId)
            val facing = characteristics.get(CameraCharacteristics.LENS_FACING)
            if (facing == CameraCharacteristics.LENS_FACING_BACK) {
                return cameraId
            }
        }
        return null
    }

    private fun startCameraThread() {
        if (cameraThread == null) {
            cameraThread = HandlerThread("CameraThread").apply { start() }
            cameraHandler = Handler(cameraThread!!.looper)
        }
    }

    private fun stopCameraThread() {
        cameraThread?.quitSafely()
        try {
            cameraThread?.join(500)
            cameraThread = null
            cameraHandler = null
        } catch (e: InterruptedException) {
            Log.e(TAG, "Error joining camera thread", e)
        }
    }
}