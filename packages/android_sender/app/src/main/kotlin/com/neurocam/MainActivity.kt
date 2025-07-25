// --- packages/android_sender/app/src/main/kotlin/com/neurocam/MainActivity.kt ---

package com.neurocam

import android.app.Activity
import android.content.pm.PackageManager
import android.os.Bundle
import android.util.Log
import android.widget.TextView
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat

class MainActivity : Activity() {

    // 定义一个请求码，用于在回调中识别我们的摄像头权限请求
    private val CAMERA_REQUEST_CODE = 101

    private external fun initRust()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        Log.i("NeuroCamApp", "[Kotlin] MainActivity created.")
        
        // 检查摄像头权限
        if (ContextCompat.checkSelfPermission(this, android.Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED) {
            // 如果已经有权限，直接初始化
            Log.i("NeuroCamApp", "[Kotlin] Camera permission already granted.")
            initRustAndShowMessage()
        } else {
            // 如果没有权限，发起请求
            Log.w("NeuroCamApp", "[Kotlin] Camera permission not granted. Requesting...")
            ActivityCompat.requestPermissions(this, arrayOf(android.Manifest.permission.CAMERA), CAMERA_REQUEST_CODE)
        }
    }

    /**
     * 这是处理权限请求结果的回调函数。
     */
    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        when (requestCode) {
            CAMERA_REQUEST_CODE -> {
                if (grantResults.isNotEmpty() && grantResults[0] == PackageManager.PERMISSION_GRANTED) {
                    // 用户授予了权限
                    Log.i("NeuroCamApp", "[Kotlin] Camera permission granted by user.")
                    initRustAndShowMessage()
                } else {
                    // 用户拒绝了权限
                    Log.e("NeuroCamApp", "[Kotlin] Camera permission denied by user.")
                    // 显示一条错误消息，告知用户应用无法工作
                    val textView = TextView(this).apply {
                        text = "Error: Camera permission is required for NeuroCam to work."
                        textSize = 20f
                        gravity = android.view.Gravity.CENTER
                    }
                    setContentView(textView)
                }
            }
        }
    }

    /**
     * 将Rust初始化和UI设置封装到一个函数中，以便在获得权限后调用。
     */
    private fun initRustAndShowMessage() {
        // 加载 native 库
        System.loadLibrary("android_sender")
        Log.i("NeuroCamApp", "[Kotlin] Library loaded. Calling Rust init function...")

        // 调用 Rust 函数
        initRust()

        // 设置UI
        val textView = TextView(this).apply {
            text = "NeuroCam Sender Initialized.\nCheck logcat for the Rust message."
            textSize = 20f
            gravity = android.view.Gravity.CENTER
        }
        setContentView(textView)
    }
}