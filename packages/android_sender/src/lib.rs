// --- packages/android_sender/src/lib.rs ---

// AI-MOD-START
use jni::objects::{JByteBuffer, JClass};
use jni::sys::jboolean;
use jni::JNIEnv;
use lazy_static::lazy_static;
use protocol::{
    AckPacket, DataHeader, PacketType, ACK_PACKET_SIZE, DATA_HEADER_SIZE, MAX_PAYLOAD_SIZE,
};
use std::collections::{HashMap, HashSet};
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

mod logger;

const TARGET_ADDR: &str = "192.168.1.3:8080";
const RETRANSMISSION_TIMEOUT: Duration = Duration::from_millis(500); // I-frame ACK 超时时间
const MAX_RETRIES: u8 = 5; // 最大重试次数

// --- 全局状态与缓存 ---
lazy_static! {
    // UDP Socket 是线程安全的，可以被多个线程共享
    static ref UDP_SOCKET: Arc<UdpSocket> = {
        let socket = UdpSocket::bind("0.0.0.0:0").expect("Failed to bind UDP socket");
        socket.set_nonblocking(true).expect("Failed to set socket to non-blocking");
        Arc::new(socket)
    };
    // 存储已收到ACK的I-frame ID
    static ref ACKED_FRAMES: Arc<Mutex<HashSet<u32>>> = Arc::new(Mutex::new(HashSet::new()));
    // 存储待确认的I-frame数据 (frame_id -> (分片数据, 发送时间, 重试次数))
    static ref UNACKED_IFRAMES: Arc<Mutex<HashMap<u32, (Vec<Vec<u8>>, Instant, u8)>>> = Arc::new(Mutex::new(HashMap::new()));
    // 后台线程的 JoinHandle，用于确保在 close 时能等待线程结束
    static ref THREAD_HANDLES: Mutex<Vec<JoinHandle<()>>> = Mutex::new(Vec::new());
}
static FRAME_COUNTER: AtomicU32 = AtomicU32::new(0);
static SHUTDOWN_FLAG: AtomicBool = AtomicBool::new(false);
static ONCE_INIT: std::sync::Once = std::sync::Once::new();

#[no_mangle]
pub extern "system" fn Java_com_neurocam_NativeBridge_init(_env: JNIEnv, _class: JClass) {
    // ONCE_INIT 确保后台线程只被启动一次
    ONCE_INIT.call_once(|| {
        logger::info("Performing first-time initialization of NativeBridge...");

        // 克隆 Arc 引用以在线程中使用
        let socket_clone = Arc::clone(&UDP_SOCKET);
        let acked_frames_clone = Arc::clone(&ACKED_FRAMES);
        let _unacked_iframes_clone = Arc::clone(&UNACKED_IFRAMES);

        // 1. ACK 监听线程
        let ack_listener = thread::spawn(move || {
            let mut buf = [0u8; 1 + ACK_PACKET_SIZE];
            while !SHUTDOWN_FLAG.load(Ordering::Relaxed) {
                if let Ok((len, _)) = socket_clone.recv_from(&mut buf) {
                    if len > 0
                        && PacketType::try_from(buf[0]).unwrap_or(PacketType::Data)
                            == PacketType::Ack
                    {
                        if let Some(ack) = AckPacket::from_bytes(&buf[1..len]) {
                            acked_frames_clone.lock().unwrap().insert(ack.frame_id);
                            // logger::info(&format!("[ACK] Received for frame #{}", ack.frame_id));
                        }
                    }
                }
                thread::sleep(Duration::from_millis(10)); // 避免CPU空转
            }
            logger::info("ACK listener thread shutting down.");
        });

        // 2. I-frame 重传线程
        let retransmitter = thread::spawn(move || {
            while !SHUTDOWN_FLAG.load(Ordering::Relaxed) {
                let mut acked_ids = Vec::new();
                let mut retransmit_ids = Vec::new();

                // 检查已收到的ACK
                if let Ok(mut acked_frames) = ACKED_FRAMES.lock() {
                    if !acked_frames.is_empty() {
                        if let Ok(mut unacked_frames) = UNACKED_IFRAMES.lock() {
                            for ack_id in acked_frames.iter() {
                                if unacked_frames.remove(ack_id).is_some() {
                                    logger::info(&format!(
                                        "[ACK OK] Frame #{} confirmed and removed from cache.",
                                        ack_id
                                    ));
                                }
                            }
                        }
                        acked_frames.clear();
                    }
                }

                // 检查超时的帧
                if let Ok(mut unacked_frames) = UNACKED_IFRAMES.lock() {
                    for (frame_id, (_, sent_at, retries)) in unacked_frames.iter_mut() {
                        if sent_at.elapsed() > RETRANSMISSION_TIMEOUT {
                            if *retries < MAX_RETRIES {
                                logger::warn(&format!(
                                    "[RETRY] Frame #{} timed out. Retrying (attempt {})...",
                                    frame_id,
                                    *retries + 1
                                ));
                                *sent_at = Instant::now();
                                *retries += 1;
                                retransmit_ids.push(*frame_id);
                            } else {
                                logger::error(&format!(
                                    "[GIVE UP] Frame #{} exceeded max retries. Dropping.",
                                    frame_id
                                ));
                                acked_ids.push(*frame_id); // "假装"它被ACK了，以便从缓存中移除
                            }
                        }
                    }
                    // 执行重传
                    for frame_id in retransmit_ids {
                        if let Some((packets, _, _)) = unacked_frames.get(&frame_id) {
                            for packet_data in packets {
                                let _ = UDP_SOCKET.send_to(packet_data, TARGET_ADDR);
                            }
                        }
                    }
                    // 移除放弃的帧
                    for frame_id in acked_ids {
                        unacked_frames.remove(&frame_id);
                    }
                }

                thread::sleep(Duration::from_millis(200)); // 重传检查周期
            }
            logger::info("Retransmission thread shutting down.");
        });

        let mut handles = THREAD_HANDLES.lock().unwrap();
        handles.push(ack_listener);
        handles.push(retransmitter);
        logger::info("Background threads for ACK and retransmission have been started.");
    });
    logger::info("Rust NativeBridge_init call completed.");
}

#[no_mangle]
pub extern "system" fn Java_com_neurocam_NativeBridge_sendVideoFrame(
    _env: JNIEnv,
    _class: JClass,
    frame_buffer: JByteBuffer,
    size: jni::sys::jint,
    is_key_frame: jboolean,
) {
    let Ok(data_ptr) = _env.get_direct_buffer_address(&frame_buffer) else {
        logger::error("[Rust] Failed to get direct buffer address.");
        return;
    };
    let data_slice = unsafe { std::slice::from_raw_parts(data_ptr, size as usize) };
    let frame_id = FRAME_COUNTER.fetch_add(1, Ordering::Relaxed);
    let chunks: Vec<&[u8]> = data_slice.chunks(MAX_PAYLOAD_SIZE).collect();
    let total_packets = chunks.len() as u16;

    let mut packets_to_cache = if is_key_frame != 0 {
        Vec::with_capacity(chunks.len())
    } else {
        Vec::new()
    };

    for (i, chunk) in chunks.iter().enumerate() {
        let header = DataHeader {
            frame_id,
            packet_id: i as u16,
            total_packets,
            is_key_frame: is_key_frame as u8,
        };

        let mut packet_data = Vec::with_capacity(1 + DATA_HEADER_SIZE + chunk.len());
        packet_data.push(PacketType::Data as u8);
        packet_data.extend_from_slice(&header.to_bytes());
        packet_data.extend_from_slice(chunk);

        if let Err(e) = UDP_SOCKET.send_to(&packet_data, TARGET_ADDR) {
            logger::error(&format!("[Rust] Failed to send UDP packet. Error: {}", e));
            return; // 发送失败则中止当前帧
        }

        if is_key_frame != 0 {
            packets_to_cache.push(packet_data);
        }
    }

    if is_key_frame != 0 {
        if let Ok(mut unacked_frames) = UNACKED_IFRAMES.lock() {
            logger::info(&format!("[CACHE] Caching I-Frame #{} for ACK.", frame_id));
            unacked_frames.insert(frame_id, (packets_to_cache, Instant::now(), 0));
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_com_neurocam_NativeBridge_close(_env: JNIEnv, _class: JClass) {
    logger::info("NativeBridge_close called. Signaling threads to shut down...");
    SHUTDOWN_FLAG.store(true, Ordering::Relaxed);

    // 等待后台线程结束
    let mut handles = THREAD_HANDLES.lock().unwrap();
    while let Some(handle) = handles.pop() {
        let _ = handle.join();
    }
    logger::info("All background threads have been shut down.");
}
// AI-MOD-END
