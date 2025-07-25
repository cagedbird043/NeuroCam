// --- packages/android_sender/app/build.gradle.kts ---

import org.gradle.process.ExecSpec
import java.io.File

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.neurocam"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.neurocam"
        minSdk = 26
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_11
        targetCompatibility = JavaVersion.VERSION_11
    }
    kotlinOptions {
        jvmTarget = "11"
    }

    sourceSets {
        getByName("main") {
            jniLibs.srcDirs("build/rustJniLibs/lib")
        }
    }
}

dependencies {
    // +++ 添加此行 +++
    // 这个库提供了兼容旧版安卓系统的辅助功能，包括权限请求。
    implementation("androidx.core:core-ktx:1.13.1")
}

tasks.register<Exec>("cargoBuild") {
    group = "rust"
    description = "Build Rust code for all Android targets"
    workingDir = rootDir
    commandLine(
        "cargo", "ndk",
        "-o", "app/build/rustJniLibs/lib",
        "build", "--release"
    )
}

tasks.named("preBuild") {
    dependsOn(tasks.getByName("cargoBuild"))
}