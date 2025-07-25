// --- packages/android_sender/build.gradle.kts ---

// 在这里，我们为所有子项目定义插件和它们的版本
// `apply false` 的意思是“不要将这个插件应用到根项目自身，
// 只是让这个版本对所有子项目可用”。
plugins {
    id("com.android.application") version "8.2.2" apply false
    id("org.jetbrains.kotlin.android") version "1.9.22" apply false
}