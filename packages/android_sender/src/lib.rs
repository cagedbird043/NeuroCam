// --- packages/android_sender/src/lib.rs ---

use jni::objects::{GlobalRef, JByteBuffer, JClass};
use jni::sys::jboolean;
use jni::{JNIEnv, JavaVM};
use lazy_static::lazy_static;
use protocol::{AckPacket, DataHeader, PacketType, DATA_HEADER_SIZE, MAX_PAYLOAD_SIZE};
use std::collections::{HashMap, HashSet};
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

mod logger;

const TARGET_ADDR: &str = "192.168.1.3:8080";
const RETRANSMISSION_TIMEOUT: Duration = Duration::from_millis(500);
const MAX_RETRIES: u8 = 5;
const CONTROL_MSG_BUFFER_SIZE: usize = 128;

// --- 全局状态与缓存 ---
lazy_static! {
    static ref UDP_SOCKET: Arc<UdpSocket> = {
        let socket = UdpSocket::bind("0.0.0.0:0").expect("Failed to bind UDP socket");
        socket
            .set_nonblocking(true)
            .expect("Failed to set socket to non-blocking");
        Arc::new(socket)
    };
    static ref ACKED_FRAMES: Arc<Mutex<HashSet<u32>>> = Arc::new(Mutex::new(HashSet::new()));
    static ref UNACKED_IFRAMES: Arc<Mutex<HashMap<u32, (Vec<Vec<u8>>, Instant, u8)>>> =
        Arc::new(Mutex::new(HashMap::new()));
    static ref THREAD_HANDLES: Mutex<Vec<JoinHandle<()>>> = Mutex::new(Vec::new());
}
static JAVA_VM: OnceLock<JavaVM> = OnceLock::new();
static NATIVE_BRIDGE_CLASS: OnceLock<GlobalRef> = OnceLock::new(); // 新增：存储 NativeBridge 类的全局引用
static FRAME_COUNTER: AtomicU32 = AtomicU32::new(0);
static SHUTDOWN_FLAG: AtomicBool = AtomicBool::new(false);
static ONCE_INIT: std::sync::Once = std::sync::Once::new();

fn call_request_key_frame_from_native() {
    if let (Some(vm), Some(class_ref)) = (JAVA_VM.get(), NATIVE_BRIDGE_CLASS.get()) {
        match vm.attach_current_thread() {
            Ok(mut env) => {
                // 核心修复：直接使用全局类引用 (class_ref) 进行调用，而不是字符串
                match env.call_static_method(class_ref, "requestKeyFrameFromNative", "()V", &[]) {
                    Ok(_) => logger::info("[JNI] Successfully called requestKeyFrameFromNative."),
                    Err(e) => {
                        logger::error(&format!("[JNI] Failed to call static method: {:?}", e))
                    }
                }
            }
            Err(e) => logger::error(&format!("[JNI] Failed to attach current thread: {:?}", e)),
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_com_neurocam_NativeBridge_init(mut env: JNIEnv, _class: JClass) {
    if JAVA_VM.get().is_none() {
        if let Ok(vm) = env.get_java_vm() {
            let _ = JAVA_VM.set(vm);
        } else {
            logger::error("Could not get JavaVM. Callbacks will be disabled.");
            return;
        }
    }
    if NATIVE_BRIDGE_CLASS.get().is_none() {
        // 核心修复：查找 NativeBridge 类并创建全局引用
        match env.find_class("com/neurocam/NativeBridge") {
            Ok(class) => match env.new_global_ref(class) {
                Ok(global_ref) => {
                    let _ = NATIVE_BRIDGE_CLASS.set(global_ref);
                }
                Err(e) => logger::error(&format!("Failed to create global ref: {:?}", e)),
            },
            Err(e) => logger::error(&format!("Failed to find NativeBridge class: {:?}", e)),
        }
    }

    ONCE_INIT.call_once(|| {
        logger::info("Performing first-time initialization of NativeBridge...");

        let socket_for_control = Arc::clone(&UDP_SOCKET);
        let acked_frames_for_control = Arc::clone(&ACKED_FRAMES);
        let control_listener = thread::spawn(move || {
            let mut buf = [0u8; CONTROL_MSG_BUFFER_SIZE];
            while !SHUTDOWN_FLAG.load(Ordering::Relaxed) {
                if let Ok((len, _)) = socket_for_control.recv_from(&mut buf) {
                    if len == 0 {
                        continue;
                    }
                    match PacketType::try_from(buf[0]) {
                        Ok(PacketType::Ack) => {
                            if let Some(ack) = AckPacket::from_bytes(&buf[1..len]) {
                                acked_frames_for_control
                                    .lock()
                                    .unwrap()
                                    .insert(ack.frame_id);
                            }
                        }
                        Ok(PacketType::IFrameRequest) => {
                            logger::info("[CONTROL] Received I-Frame Request from receiver.");
                            call_request_key_frame_from_native();
                        }
                        _ => { /* Ignore Data packets or unknown types */ }
                    }
                }
                thread::sleep(Duration::from_millis(10));
            }
            logger::info("Control listener thread shutting down.");
        });

        let socket_for_retry = Arc::clone(&UDP_SOCKET);
        let acked_frames_for_retry = Arc::clone(&ACKED_FRAMES);
        let unacked_iframes_for_retry = Arc::clone(&UNACKED_IFRAMES);
        let retransmitter = thread::spawn(move || {
            while !SHUTDOWN_FLAG.load(Ordering::Relaxed) {
                let ids_to_process: Vec<u32> = {
                    let mut acked_set = acked_frames_for_retry.lock().unwrap();
                    acked_set.drain().collect()
                };

                if !ids_to_process.is_empty() {
                    let mut unacked_cache = unacked_iframes_for_retry.lock().unwrap();
                    for id in ids_to_process {
                        if unacked_cache.remove(&id).is_some() {
                            logger::info(&format!("[ACK OK] Frame #{} confirmed.", id));
                        }
                    }
                }

                let mut retransmit_ids = Vec::new();
                let mut drop_ids = Vec::new();
                if let Ok(mut unacked_cache) = unacked_iframes_for_retry.lock() {
                    for (frame_id, (_, sent_at, retries)) in unacked_cache.iter_mut() {
                        if sent_at.elapsed() > RETRANSMISSION_TIMEOUT {
                            if *retries < MAX_RETRIES {
                                logger::warn(&format!(
                                    "[RETRY] Frame #{} timed out (attempt {}).",
                                    frame_id,
                                    *retries + 1
                                ));
                                *sent_at = Instant::now();
                                *retries += 1;
                                retransmit_ids.push(*frame_id);
                            } else {
                                logger::error(&format!(
                                    "[GIVE UP] Frame #{} exceeded max retries.",
                                    frame_id
                                ));
                                drop_ids.push(*frame_id);
                            }
                        }
                    }

                    for id in &retransmit_ids {
                        if let Some((packets, _, _)) = unacked_cache.get(id) {
                            for packet_data in packets {
                                let _ = socket_for_retry.send_to(packet_data, TARGET_ADDR);
                            }
                        }
                    }

                    for id in drop_ids {
                        unacked_cache.remove(&id);
                    }
                }
                thread::sleep(Duration::from_millis(200));
            }
            logger::info("Retransmission thread shutting down.");
        });

        let mut handles = THREAD_HANDLES.lock().unwrap();
        handles.push(control_listener);
        handles.push(retransmitter);
        logger::info("Background threads for control and retransmission have been started.");
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

    capture_timestamp_ns: jni::sys::jlong, // 新增时间戳参数
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
            capture_timestamp_ns: capture_timestamp_ns as u64,
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
            return;
        }

        if is_key_frame != 0 {
            packets_to_cache.push(packet_data);
        }
    }

    if is_key_frame != 0 {
        if let Ok(mut unacked_frames) = UNACKED_IFRAMES.lock() {
            unacked_frames.insert(frame_id, (packets_to_cache, Instant::now(), 0));
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_com_neurocam_NativeBridge_close(_env: JNIEnv, _class: JClass) {
    logger::info("NativeBridge_close called. Signaling threads to shut down...");
    SHUTDOWN_FLAG.store(true, Ordering::Relaxed);

    logger::info("Waiting for a graceful shutdown...");
    thread::sleep(RETRANSMISSION_TIMEOUT + Duration::from_millis(100));

    let mut handles = THREAD_HANDLES.lock().unwrap();
    while let Some(handle) = handles.pop() {
        if let Err(e) = handle.join() {
            logger::error(&format!("Failed to join thread: {:?}", e));
        }
    }
    logger::info("All background threads have been shut down cleanly.");

    // 全局引用 (GlobalRef) 存储在 static 变量中，其生命周期与应用进程一致。
    // 当进程终止时，它所占用的资源会被操作系统回收。
    // 在 `jni-rs` 中，GlobalRef 的 Drop trait 会自动处理 JNI 的删除逻辑。
    // 因此，此处无需也无法手动删除。
}

use jni::objects::JByteArray;

#[no_mangle]
pub extern "system" fn Java_com_neurocam_NativeBridge_sendSpsPps(
    env: JNIEnv,
    _class: JClass,
    buffer: jni::sys::jbyteArray,
    size: jni::sys::jint,
) {
    let size = size as usize;
    let jarray = unsafe { JByteArray::from_raw(buffer) }; // 关键：用 from_raw
    let spspps = env.convert_byte_array(jarray).unwrap();
    let mut packet = Vec::with_capacity(1 + size);
    packet.push(PacketType::SpsPps as u8);
    packet.extend_from_slice(&spspps[..size]);
    let _ = UDP_SOCKET.send_to(&packet, TARGET_ADDR);
}
