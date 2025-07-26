// --- packages/android_sender/app/src/main/java/com/neurocam/NativeBridge.kt ---

// AI-MOD-START
package com.neurocam

import java.nio.ByteBuffer

/**
 * NativeBridge 是一个单例对象，作为 Kotlin 与 Rust JNI 代码之间的唯一桥梁。
 * 所有的 native 方法都在这里声明。
 *
 * 使用 'object' 关键字可以确保这个类在整个应用中只有一个实例。
 */
object NativeBridge {

    init {
        // 在首次使用这个对象时，自动加载 Rust 库。
        // 这比在 Activity 中加载更健壮。
        System.loadLibrary("android_sender")
    }

    /**
     * 初始化 Rust 端的环境。
     * 应该在应用启动时调用一次。
     * 例如，可以在这里初始化网络连接、分配内存等。
     */
    external fun init()

    /**
     * 发送一个视频帧到 Rust 层进行处理。
     * @param frameBuffer 一个包含 H.264 编码数据的 Direct ByteBuffer。
     *                    使用 Direct ByteBuffer 是最高效的方式，因为它允许 Rust 直接访问底层内存，避免数据拷贝。
     */
    external fun sendVideoFrame(frameBuffer: ByteBuffer)

    /**
     * 清理和释放 Rust 端的资源。
     * 应该在应用关闭时调用。
     * 例如，关闭网络连接。
     */
    external fun close()
}
// AI-MOD-END