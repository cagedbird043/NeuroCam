// --- packages/android_sender/app/src/main/kotlin/com/neurocam/MainActivity.kt ---

package com.neurocam

import android.app.Activity
import android.content.pm.PackageManager
import android.graphics.SurfaceTexture
import android.os.Bundle
import android.util.Log
import android.view.Surface
import android.widget.TextView
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat

class MainActivity : Activity() {

    private val CAMERA_REQUEST_CODE = 101

    // 修改 JNI 函数声明，使其可以接收一个 Surface 对象
    private external fun initRust(surface: Surface)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        Log.i("NeuroCamApp", "[Kotlin] MainActivity created.")
        
        if (ContextCompat.checkSelfPermission(this, android.Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED) {
            Log.i("NeuroCamApp", "[Kotlin] Camera permission already granted.")
            initializeNeuroCam()
        } else {
            Log.w("NeuroCamApp", "[Kotlin] Camera permission not granted. Requesting...")
            ActivityCompat.requestPermissions(this, arrayOf(android.Manifest.permission.CAMERA), CAMERA_REQUEST_CODE)
        }
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        when (requestCode) {
            CAMERA_REQUEST_CODE -> {
                if (grantResults.isNotEmpty() && grantResults[0] == PackageManager.PERMISSION_GRANTED) {
                    Log.i("NeuroCamApp", "[Kotlin] Camera permission granted by user.")
                    initializeNeuroCam()
                } else {
                    Log.e("NeuroCamApp", "[Kotlin] Camera permission denied by user.")
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
     * 封装了 NeuroCam 的核心初始化步骤。
     */
    private fun initializeNeuroCam() {
        // 1. 加载 native 库
        System.loadLibrary("android_sender")
        Log.i("NeuroCamApp", "[Kotlin] Library loaded.")

        // 2. 创建一个离屏的 SurfaceTexture 和 Surface
        // 参数 `1` 是一个任意的、非零的 OpenGL 纹理 ID。在这里它只是一个占位符。
        val surfaceTexture = SurfaceTexture(1)
        val surface = Surface(surfaceTexture)
        Log.i("NeuroCamApp", "[Kotlin] Off-screen Surface created.")

        // 3. 调用 Rust 函数，并将 Surface 对象传递过去
        initRust(surface)
        Log.i("NeuroCamApp", "[Kotlin] Called Rust init function with Surface.")

        // 4. 更新UI以提供反馈
        val textView = TextView(this).apply {
            text = "NeuroCam Active.\nSurface passed to Rust layer."
            textSize = 20f
            gravity = android.view.Gravity.CENTER
        }
        setContentView(textView)
    }
}