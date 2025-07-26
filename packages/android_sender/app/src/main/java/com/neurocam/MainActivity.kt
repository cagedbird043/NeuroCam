package com.neurocam
import android.Manifest
import android.content.pm.PackageManager
import android.os.Bundle
import android.util.Log
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
// 为 CameraX 的 Preview 类指定一个别名 CameraXPreview，以避免与 Compose 的 Preview 注解冲突
import androidx.camera.core.Preview as CameraXPreview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
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
import androidx.compose.ui.tooling.preview.Preview // Compose 的 Preview 注解保持原名
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import com.neurocam.ui.theme.NeuroCamSenderTheme
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        // AI-MOD-START
        // 核心修复：将 NativeBridge.init() 移到后台线程。
        // lifecycleScope 会将协程的生命周期与 Activity 绑定，当 Activity 销毁时自动取消。
        // Dispatchers.IO 是专门为网络和磁盘 I/O 操作优化的线程池。
        lifecycleScope.launch(Dispatchers.IO) {
            NativeBridge.init()
            Log.i("NeuroCam/MainActivity", "NativeBridge initialized on a background thread.")
        }
        // AI-MOD-END

        enableEdgeToEdge()
        setContent {
            NeuroCamSenderTheme {
                MainScreen()
            }
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        // close() 通常不涉及网络，可以保持原样，但为了对称性，也可以移到后台线程。
        // 这里我们暂时保持不动，因为它目前是空的。
        NativeBridge.close()
        Log.i("NeuroCam/MainActivity", "NativeBridge closed.")
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


// --- packages/android_sender/app/src/main/java/com/neurocam/MainActivity.kt ---

@Composable
fun CameraPreview(modifier: Modifier = Modifier) {
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current
    val cameraProviderFuture = remember { ProcessCameraProvider.getInstance(context) }
    val cameraExecutor = remember { java.util.concurrent.Executors.newSingleThreadExecutor() }

    var videoEncoder: VideoEncoder? by remember { mutableStateOf(null) }

    // --- 监听来自 Rust 的 I-Frame 请求 ---
    LaunchedEffect(videoEncoder) {
        // 只有当 videoEncoder 被创建后才开始监听
        videoEncoder?.let { encoder ->
            NativeBridge.keyFrameRequestFlow.collect {
                // 当我们从 Flow 收到一个事件时
                Log.d("NeuroCam/CameraPreview", "Received key frame request from native layer.")
                encoder.requestKeyFrame()
            }
        }
    }
    // --- 监听结束 ---

    AndroidView(
        factory = { ctx ->
            val previewView = PreviewView(ctx).apply {
                this.scaleType = PreviewView.ScaleType.FILL_CENTER
            }

            cameraProviderFuture.addListener({
                val cameraProvider = cameraProviderFuture.get()

                val preview = CameraXPreview.Builder()
                    .build()
                    .also {
                        it.setSurfaceProvider(previewView.surfaceProvider)
                    }

                val imageAnalyzer = androidx.camera.core.ImageAnalysis.Builder()
                    .setBackpressureStrategy(androidx.camera.core.ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                    .setTargetResolution(android.util.Size(640, 480))
                    .build()
                    .also {
                        it.setAnalyzer(cameraExecutor, androidx.camera.core.ImageAnalysis.Analyzer { imageProxy ->
                            if (videoEncoder == null) {
                                val actualWidth = imageProxy.width
                                val actualHeight = imageProxy.height
                                Log.i("NeuroCam/CameraPreview", "First frame received. " +
                                        "Actual resolution: ${actualWidth}x${actualHeight}. Initializing encoder.")
                                videoEncoder = VideoEncoder(width = actualWidth, height = actualHeight).apply {
                                    start()
                                }
                            }
                            videoEncoder?.encodeFrame(imageProxy)
                            imageProxy.close()
                        })
                    }

                val cameraSelector = CameraSelector.DEFAULT_BACK_CAMERA

                try {
                    cameraProvider.unbindAll()
                    cameraProvider.bindToLifecycle(
                        lifecycleOwner,
                        cameraSelector,
                        preview,
                        imageAnalyzer
                    )
                } catch (exc: Exception) {
                    Log.e("NeuroCam/CameraPreview", "用例绑定失败", exc)
                }
            }, ContextCompat.getMainExecutor(ctx))

            previewView
        },
        modifier = modifier.fillMaxSize()
    )

    DisposableEffect(Unit) {
        onDispose {
            Log.d("NeuroCam/MainScreen", "Disposing resources...")
            cameraExecutor.shutdown()
            videoEncoder?.stop()
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