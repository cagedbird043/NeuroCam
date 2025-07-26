// --- packages/android_sender/app/src/main/kotlin/com/neurocam/CameraHelper.kt ---

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
import android.view.Surface

class CameraHelper(private val context: Context, private val listener: Listener? = null) {

    /**
     * 一个简单的回调接口，用于通知调用者摄像头状态。
     */
    interface Listener {
        fun onCameraStarted()
        fun onCameraError(error: String)
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
            cameraManager.openCamera(cameraId, deviceStateCallback, cameraHandler)
        } catch (e: Exception) {
            val errorMsg = "Failed to initiate camera opening: ${e.message}"
            Log.e(TAG, errorMsg, e)
            listener?.onCameraError(errorMsg)
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