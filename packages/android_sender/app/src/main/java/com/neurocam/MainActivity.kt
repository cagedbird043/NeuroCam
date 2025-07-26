package com.neurocam

import android.Manifest
import android.content.pm.PackageManager
import android.os.Bundle
import android.util.Log
import android.util.Size
import android.view.SurfaceHolder
import android.view.SurfaceView
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import android.graphics.Matrix
import android.graphics.SurfaceTexture
import android.view.TextureView
import androidx.compose.foundation.layout.fillMaxWidth
import com.neurocam.ui.theme.NeuroCamSenderTheme

class MainActivity : ComponentActivity() {

    // private external fun initRust() // 我们稍后会用到它

    companion object {
        private const val TAG = "NeuroCam/MainActivity"
        init {
            // 确保在任何 JNI 调用之前加载 Rust 库
            System.loadLibrary("android_sender")
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            NeuroCamSenderTheme {
                MainScreen()
            }
        }
    }
}

@Composable
fun MainScreen(modifier: Modifier = Modifier) {
    val context = LocalContext.current
    var hasPermission by remember {
        mutableStateOf(
            ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED
        )
    }

    val permissionLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.RequestPermission(),
        onResult = { isGranted ->
            hasPermission = isGranted
            if (isGranted) {
                Log.i("NeuroCam/MainScreen", "摄像头权限已被用户授予。")
            } else {
                Log.w("NeuroCam/MainScreen", "摄像头权限被用户拒绝。")
            }
        }
    )

    LaunchedEffect(key1 = Unit) {
        if (!hasPermission) {
            permissionLauncher.launch(Manifest.permission.CAMERA)
        }
    }

    Scaffold(modifier = modifier.fillMaxSize()) { innerPadding ->
        Column(modifier = Modifier.padding(innerPadding)) {
            if (hasPermission) {
                CameraPreview()
            } else {
                PermissionDeniedScreen(
                    onRequestPermission = {
                        permissionLauncher.launch(Manifest.permission.CAMERA)
                    }
                )
            }
        }
    }
}

@Composable
fun CameraPreview(modifier: Modifier = Modifier) {
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current
    var previewSize by remember { mutableStateOf<Size?>(null) }

    val cameraHelper = remember {
        CameraHelper(context, object : CameraHelper.Listener {
            override fun onCameraStarted() {
                Log.i("NeuroCam/CameraPreview", "Listener: Camera has started successfully.")
            }
            override fun onCameraError(error: String) {
                Log.e("NeuroCam/CameraPreview", "Listener: Camera error: $error")
            }
            override fun onPreviewSizeSelected(size: Size) {
                Log.d("NeuroCam/CameraPreview", "PreviewSizeSelected: ${size.width}x${size.height}")
                previewSize = size
            }
        })
    }

    DisposableEffect(lifecycleOwner) {
        onDispose {
            Log.i("NeuroCam/CameraPreview", "DisposableEffect: Closing camera.")
            cameraHelper.closeCamera()
        }
    }

    Box(
        modifier = modifier.fillMaxSize(),
        contentAlignment = Alignment.Center
    ) {
        AndroidView(
            factory = { ctx ->
                TextureView(ctx).apply {
                    surfaceTextureListener = object : TextureView.SurfaceTextureListener {
                        override fun onSurfaceTextureAvailable(surface: SurfaceTexture, width: Int, height: Int) {
                            Log.i("NeuroCam/TextureView", "SurfaceTexture available. Starting camera.")
                            cameraHelper.startCamera(android.view.Surface(surface))
                        }
                        override fun onSurfaceTextureSizeChanged(surface: SurfaceTexture, width: Int, height: Int) {}
                        override fun onSurfaceTextureDestroyed(surface: SurfaceTexture): Boolean = true
                        override fun onSurfaceTextureUpdated(surface: SurfaceTexture) {}
                    }
                }
            },
            modifier = Modifier
                .let {
                    if (previewSize != null) {
                        // AI-MOD-START
                        // 修正：我们必须反转宽高比以适应竖屏显示
//                        val aspectRatio = previewSize!!.height.toFloat() / previewSize!!.width.toFloat()
                        val aspectRatio = 0.75f // 经典的 4:3 比例 (3 / 4 = 0.75)
                        // AI-MOD-END
                        Log.d("NeuroCam/CameraPreview", "Applying INVERTED aspect ratio: $aspectRatio")
                        it
                            .fillMaxWidth()
                            .aspectRatio(aspectRatio)
                    } else {
                        it
                    }
                }
        )

        if (previewSize == null) {
            CircularProgressIndicator()
            Text(
                "Initializing camera...",
                modifier = Modifier
                    .align(Alignment.Center)
                    .padding(top = 64.dp)
            )
        }
    }
}


@Composable
fun PermissionDeniedScreen(
    modifier: Modifier = Modifier,
    onRequestPermission: () -> Unit
) {
    Column(
        modifier = modifier
            .fillMaxSize()
            .padding(16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center
    ) {
        Text(
            textAlign = TextAlign.Center,
            text = "此应用需要摄像头权限才能工作。\n请授予权限以继续。"
        )
        Button(
            onClick = onRequestPermission,
            modifier = Modifier.padding(top = 16.dp)
        ) {
            Text("请求权限")
        }
    }
}


@Preview(showBackground = true)
@Composable
fun MainScreenPreview_PermissionGranted() {
    NeuroCamSenderTheme {
        Box(
            modifier = Modifier.fillMaxSize(),
            contentAlignment = Alignment.Center
        ) {
            Text("摄像头预览区域")
        }
    }
}

@Preview(showBackground = true)
@Composable
fun MainScreenPreview_PermissionDenied() {
    NeuroCamSenderTheme {
        PermissionDeniedScreen(onRequestPermission = {})
    }
}