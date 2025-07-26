// --- packages/android_sender/src/lib.rs ---

// AI-MOD-START
use jni::objects::{JByteBuffer, JClass};
use jni::JNIEnv;
use protocol::{PacketHeader, HEADER_SIZE, MAX_PAYLOAD_SIZE};
use std::net::UdpSocket;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

mod logger;

const TARGET_ADDR: &str = "192.168.1.3:8080"; // 请替换为您的 Linux 接收端 IP 地址

static UDP_SOCKET: OnceLock<Mutex<UdpSocket>> = OnceLock::new();
// 使用原子计数器为每一帧生成唯一的ID
static FRAME_COUNTER: AtomicU32 = AtomicU32::new(0);

#[no_mangle]
pub extern "system" fn Java_com_neurocam_NativeBridge_init(_env: JNIEnv, _class: JClass) {
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

// --- packages/android_sender/src/lib.rs ---

// AI-MOD-START
#[no_mangle]
pub extern "system" fn Java_com_neurocam_NativeBridge_sendVideoFrame(
    env: JNIEnv,
    _class: JClass,
    frame_buffer: JByteBuffer,
    size: jni::sys::jint,
    is_key_frame: jni::sys::jboolean, // 新增参数
) {
    let Some(socket_mutex) = UDP_SOCKET.get() else {
        logger::error("[Rust] UDP socket is not initialized. Cannot send frame.");
        return;
    };

    let Ok(data_ptr) = env.get_direct_buffer_address(&frame_buffer) else {
        logger::error("[Rust] Failed to get direct buffer address.");
        return;
    };

    let data_slice = unsafe { std::slice::from_raw_parts(data_ptr, size as usize) };
    let frame_id = FRAME_COUNTER.fetch_add(1, Ordering::Relaxed);

    // 使用 .chunks() 方法优雅地进行分片
    let chunks: Vec<&[u8]> = data_slice.chunks(MAX_PAYLOAD_SIZE).collect();
    let total_packets = chunks.len() as u16;

    if is_key_frame != 0 {
        logger::info(&format!(
            "[Rust] Sending Key Frame #{} in {} packets.",
            frame_id, total_packets
        ));
    }

    let socket = socket_mutex.lock().unwrap();
    let mut packet_buffer = [0u8; HEADER_SIZE + MAX_PAYLOAD_SIZE];

    for (i, chunk) in chunks.iter().enumerate() {
        let packet_id = i as u16;
        let header = PacketHeader {
            frame_id,
            packet_id,
            total_packets,
            is_key_frame: is_key_frame as u8, // 将 jboolean (u8) 直接转换为 u8
        };

        // 填充包头
        packet_buffer[..HEADER_SIZE].copy_from_slice(&header.to_bytes());
        // 填充负载
        packet_buffer[HEADER_SIZE..HEADER_SIZE + chunk.len()].copy_from_slice(chunk);

        // 发送整个包（头 + 负载）
        let packet_size = HEADER_SIZE + chunk.len();
        if let Err(e) = socket.send_to(&packet_buffer[..packet_size], TARGET_ADDR) {
            logger::error(&format!("[Rust] Failed to send UDP packet. Error: {}", e));
            // 如果一个分片发送失败，后续的分片也失去了意义，直接中断本次发送。
            break;
        }
    }
}
// AI-MOD-END

#[no_mangle]
pub extern "system" fn Java_com_neurocam_NativeBridge_close(_env: JNIEnv, _class: JClass) {
    logger::info("Rust NativeBridge_close called. Socket will be closed on process exit.");
}
// AI-MOD-END
