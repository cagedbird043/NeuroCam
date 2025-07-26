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
) {
    // 检查 socket 是否已经被初始化
    if let Some(socket_mutex) = UDP_SOCKET.get() {
        let Ok(data_ptr) = env.get_direct_buffer_address(&frame_buffer) else {
            logger::error("Failed to get direct buffer address.");
            return;
        };
        let Ok(len) = env.get_direct_buffer_capacity(&frame_buffer) else {
            logger::error("Failed to get direct buffer capacity.");
            return;
        };

        let data_slice = unsafe { std::slice::from_raw_parts(data_ptr, len) };
        let socket = socket_mutex.lock().unwrap();

        match socket.send_to(data_slice, TARGET_ADDR) {
            Ok(bytes_sent) => {
                if bytes_sent != len {
                    logger::warn(&format!(
                        "UDP send warning: tried to send {} bytes, but only sent {}.",
                        len, bytes_sent
                    ));
                }
            }
            Err(e) => {
                logger::error(&format!("Failed to send UDP packet: {}", e));
            }
        }
    } else {
        logger::error("UDP socket is not initialized. Call init() first.");
    }
}

/// 清理资源。对于 OnceLock，没有内置的“销毁”方法，资源会随程序终止而释放。
/// 保留此函数以符合接口设计。
#[no_mangle]
pub extern "system" fn Java_com_neurocam_NativeBridge_close(_env: JNIEnv, _class: JClass) {
    logger::info("Rust NativeBridge_close called. Socket will be closed on process exit.");
}
// AI-MOD-END
