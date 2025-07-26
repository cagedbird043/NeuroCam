// --- packages/android_sender/src/lib.rs ---

use jni::objects::{JByteBuffer, JClass};
use jni::JNIEnv;
use std::net::UdpSocket;
// AI-MOD-START
// 引入 OnceLock 和 Mutex
use std::sync::{Mutex, OnceLock};
// AI-MOD-END

mod logger;

const TARGET_ADDR: &str = "192.168.1.3:8080"; // 请替换为您的 Linux 接收端 IP 地址

// AI-MOD-START
// 使用 OnceLock 来安全地、一次性地初始化 Socket。
// OnceLock<T> 保证其内部值最多被初始化一次。
// Mutex<UdpSocket> 保证对 UdpSocket 的访问是线程安全的。
static UDP_SOCKET: OnceLock<Mutex<UdpSocket>> = OnceLock::new();

/// 初始化 Rust 环境，创建 UDP Socket。
/// 这个函数现在可以被安全地从任何线程调用。
#[no_mangle]
pub extern "system" fn Java_com_neurocam_NativeBridge_init(_env: JNIEnv, _class: JClass) {
    // get_or_init 会在 UDP_SOCKET 未初始化时，执行闭包来创建值。
    // 如果已经初始化，则直接返回现有值，闭包不会被执行。
    // 整个过程是线程安全的。
    UDP_SOCKET.get_or_init(|| {
        logger::info("Initializing UDP_SOCKET for the first time...");
        let socket = UdpSocket::bind("0.0.0.0:0").expect("Failed to bind UDP socket");
        logger::info(&format!(
            "Successfully created and bound UDP socket. Target: {}",
            TARGET_ADDR
        ));
        Mutex::new(socket)
    });
    logger::info("Rust NativeBridge_init call completed.");
}

/// 从 Kotlin 接收一个 ByteBuffer 并通过 UDP 发送其内容。
#[no_mangle]
pub extern "system" fn Java_com_neurocam_NativeBridge_sendVideoFrame(
    env: JNIEnv,
    _class: JClass,
    frame_buffer: JByteBuffer,
    // AI-MOD-START
    // 新增的参数，表示有效数据的大小
    size: jni::sys::jint,
    // AI-MOD-END
) {
    if let Some(socket_mutex) = UDP_SOCKET.get() {
        let Ok(data_ptr) = env.get_direct_buffer_address(&frame_buffer) else {
            logger::error("[Rust] Failed to get direct buffer address.");
            return;
        };

        // AI-MOD-START
        // 核心修复：不再使用 get_direct_buffer_capacity()，而是使用从 Kotlin 传来的精确 size。
        // 我们需要将 jint 转换为 Rust 的 usize 类型。
        let len = size as usize;

        logger::info(&format!(
            "[Rust] Preparing to send a frame. Actual Size: {} bytes. Target: {}",
            len, TARGET_ADDR
        ));

        // 使用精确的 len 来创建切片
        let data_slice = unsafe { std::slice::from_raw_parts(data_ptr, len) };
        // AI-MOD-END

        let socket = socket_mutex.lock().unwrap();

        match socket.send_to(data_slice, TARGET_ADDR) {
            Ok(bytes_sent) => {
                if bytes_sent != len {
                    logger::warn(&format!(
                        "[Rust] UDP send warning: tried to send {} bytes, but OS only sent {}.",
                        len, bytes_sent
                    ));
                }
                // 为了避免日志刷屏，成功发送的消息就不打印了
            }
            Err(e) => {
                logger::error(&format!("[Rust] Failed to send UDP packet. Error: {}", e));
            }
        }
    } else {
        logger::error("[Rust] UDP socket is not initialized. Cannot send frame.");
    }
}

/// 清理资源。对于 OnceLock，没有内置的“销毁”方法，资源会随程序终止而释放。
/// 保留此函数以符合接口设计。
#[no_mangle]
pub extern "system" fn Java_com_neurocam_NativeBridge_close(_env: JNIEnv, _class: JClass) {
    logger::info("Rust NativeBridge_close called. Socket will be closed on process exit.");
}
// AI-MOD-END
