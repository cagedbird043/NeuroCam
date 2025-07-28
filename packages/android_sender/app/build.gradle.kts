// +++ 添加必要的 import 语句 +++
import java.util.Properties

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
}

// 读取 local.properties 文件
val localProperties = Properties()
val localPropertiesFile = rootDir.resolve("local.properties")
if (localPropertiesFile.exists()) {
    localProperties.load(localPropertiesFile.inputStream())
}


android {
    namespace = "com.neurocam"
    compileSdk = 36
    ndkVersion = "27.0.12077973" // <--- 请使用你警告中提示的版本号
    sourceSets {
        getByName("main") {
            // 将我们 cargoBuild 任务的输出目录指定为 JNI 库的源
            jniLibs.srcDirs("build/rustJniLibs/lib")
        }
    }

    defaultConfig {
        applicationId = "com.neurocam"
        minSdk = 30
        targetSdk = 36
        versionCode = 1
        versionName = "1.0"

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_11
        targetCompatibility = JavaVersion.VERSION_11
    }
    kotlinOptions {
        jvmTarget = "11"
    }
    buildFeatures {
        compose = true
    }
}
dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.lifecycle.runtime.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.ui)
    implementation(libs.androidx.ui.graphics)
    implementation(libs.androidx.ui.tooling.preview)
    implementation(libs.androidx.material3)

    
    // 添加 CameraX 依赖
    // camera-core: 核心 API
    // camera-camera2: Camera2 实现，CameraX 构建于其上
    // camera-lifecycle: 将相机生命周期与 LifecycleOwner 绑定
    // camera-view: 提供 PreviewView 控件
    implementation(libs.androidx.camera.core)
    implementation(libs.androidx.camera.camera2)
    implementation(libs.androidx.camera.lifecycle)
    implementation(libs.androidx.camera.view)
    

    testImplementation(libs.junit)
    androidTestImplementation(libs.androidx.junit)
    androidTestImplementation(libs.androidx.espresso.core)
    androidTestImplementation(platform(libs.androidx.compose.bom))
    androidTestImplementation(libs.androidx.ui.test.junit4)
    debugImplementation(libs.androidx.ui.tooling)
    debugImplementation(libs.androidx.ui.test.manifest)
}




// 注册一个名为 cargoBuild 的自定义任务，类型为 Exec (执行外部命令)
tasks.register<Exec>("cargoBuild") {
    // 在 Android Studio 的 Gradle 任务列表中，这个任务会出现在 'rust' 分组下
    group = "rust"
    description = "Build Rust code for all Android targets using cargo-ndk"

    // 设置命令的工作目录。rootDir 指向 Gradle 项目的根目录，
    // 在我们的例子中就是 packages/android_sender 目录。
    // 我们需要的是 Rust 工作区的根目录，即 NeuroCam 目录。
    // 因此，我们向上跳两级。
    workingDir = rootDir.parentFile.parentFile
    val sdkDir = localProperties.getProperty("sdk.dir") ?: error("SDK directory not found in local.properties. Please make sure sdk.dir is set.")
    val ndkDir = File(sdkDir, "ndk/${android.ndkVersion}")
    environment("ANDROID_NDK_HOME", ndkDir.absolutePath)
    // 在任务执行前，打印出计算出的工作目录的绝对路径
    doFirst {
        println("--- Cargo build working directory: ${workingDir.absolutePath} ---")
    }

    // 定义要执行的命令和参数
    commandLine("./build_android.sh")
}

// 将我们的 cargoBuild 任务挂载到标准的 preBuild 任务上。
// 这意味着每次 Android Studio 点击“运行”时，在编译 Kotlin 代码之前，
// Gradle 都会先执行我们的 cargoBuild 任务。
tasks.named("preBuild") {
    dependsOn(tasks.getByName("cargoBuild"))
}