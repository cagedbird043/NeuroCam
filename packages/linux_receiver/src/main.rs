// --- packages/linux_receiver/src/main.rs ---

use anyhow::{anyhow, Result};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;

use protocol::{AckPacket, DataHeader, PacketType, ACK_PACKET_SIZE, DATA_HEADER_SIZE};
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::net::UdpSocket;
// 删除了 tokio::time::sleep

const LISTEN_ADDR: &str = "0.0.0.0:8080";
const MAX_DATAGRAM_SIZE: usize = 65_507;
const V4L2_DEVICE: &str = "/dev/video10";
const LATENCY_AVG_WINDOW: usize = 60;
// 删除了 SIGNAL_TIMEOUT

struct FrameReassembler {
    packets: Vec<Option<Vec<u8>>>,
    received_count: u16,
    total_packets: u16,
    last_seen: Instant,
    is_key_frame: bool,
    capture_timestamp_ns: u64,
}

impl FrameReassembler {
    fn new(header: &DataHeader) -> Self {
        FrameReassembler {
            packets: vec![None; header.total_packets as usize],
            received_count: 0,
            total_packets: header.total_packets,
            last_seen: Instant::now(),
            is_key_frame: header.is_key_frame != 0,
            capture_timestamp_ns: header.capture_timestamp_ns,
        }
    }

    fn add_packet(&mut self, packet_id: u16, data: Vec<u8>) -> Option<Vec<u8>> {
        let id = packet_id as usize;
        if id < self.packets.len() && self.packets[id].is_none() {
            self.packets[id] = Some(data);
            self.received_count += 1;
        }
        self.last_seen = Instant::now();
        if self.received_count == self.total_packets {
            let total_size = self.packets.iter().map(|p| p.as_ref().unwrap().len()).sum();
            let mut frame_data = Vec::with_capacity(total_size);
            for packet in self.packets.iter_mut() {
                frame_data.extend_from_slice(packet.take().unwrap().as_slice());
            }
            Some(frame_data)
        } else {
            None
        }
    }
}

// --- 核心修改 START ---

// 我们不再需要 VideoPipeline 结构体，因为 pipeline 和 appsrc 在 main 函数中创建后会一直存在。

fn create_video_pipeline() -> Result<(gst::Pipeline, gst_app::AppSrc)> {
    let pipeline_str = format!(
        "appsrc name=src caps=\"video/x-h264,stream-format=byte-stream\" ! queue ! h264parse ! avdec_h264 ! videoconvert ! videoflip method=1 ! video/x-raw,format=YUY2 ! v4l2sink name=sink device={}",
        V4L2_DEVICE
    );

    let pipeline = gst::parse::launch(&pipeline_str)?
        .downcast::<gst::Pipeline>()
        .map_err(|_| anyhow!("Failed to create video pipeline"))?;

    let appsrc = pipeline
        .by_name("src")
        .unwrap()
        .downcast::<gst_app::AppSrc>()
        .unwrap();
    let sink = pipeline.by_name("sink").unwrap();

    appsrc.set_property("is-live", true);
    appsrc.set_property("do-timestamp", true); // 让 appsrc 根据 buffer 的 PTS/DTS 来同步
    appsrc.set_format(gst::Format::Time);
    // 设置一个合理的延迟，但在这里我们主要依赖buffer时间戳
    appsrc.set_latency(
        gst::ClockTime::from_mseconds(100),
        gst::ClockTime::from_mseconds(100),
    );
    sink.set_property("sync", false); // v4l2sink 通常不需要同步，它会尽快渲染

    Ok((pipeline, appsrc))
}

#[tokio::main]
async fn main() -> Result<()> {
    gst::init()?;
    println!("[NeuroCam Linux Receiver - STABLE ARCHITECTURE]");

    let socket = Arc::new(UdpSocket::bind(LISTEN_ADDR).await?);
    println!(
        "[OK] Listening on {}. Outputting to {}",
        LISTEN_ADDR, V4L2_DEVICE
    );

    // 1. 创建唯一的、持久的 GStreamer 管线
    let (pipeline, appsrc) = create_video_pipeline()?;

    // 2. 立即启动管线，让它进入播放状态并永远保持
    pipeline.set_state(gst::State::Playing)?;
    println!("[STATE] Video pipeline is now running and waiting for data.");

    // 第一次收到 I-frame 之前，我们需要主动请求一次，确保画面能尽快出来
    let mut requested_initial_iframe = false;

    // 这些状态仍然需要
    let mut buf = vec![0u8; MAX_DATAGRAM_SIZE];
    let mut reassemblers: HashMap<u32, FrameReassembler> = HashMap::new();
    let mut latency_history: VecDeque<f64> = VecDeque::with_capacity(LATENCY_AVG_WINDOW);
    let pipeline_start_time = Instant::now(); // 我们需要一个固定的时间起点来计算buffer的PTS

    // 3. 进入主循环，只做一件事：接收UDP包
    let mut last_remote_ip: Option<std::net::IpAddr> = None;
    let mut sps_pps_inject_count = 0usize;
    loop {
        match socket.recv_from(&mut buf).await {
            Ok((len, remote_addr)) => {
                let current_ip = remote_addr.ip();
                let is_new_source = match last_remote_ip {
                    Some(ip) => current_ip != ip,
                    None => true,
                };
                if is_new_source {
                    println!(
                    "[SWITCH] Source IP changed from {:?} to {}. Resetting pipeline, clearing reassemblers, and requesting I-Frame.",
                    last_remote_ip,
                    current_ip
                );
                    // 只在真正切换时赋值
                    last_remote_ip = Some(current_ip);
                    pipeline.set_state(gst::State::Null)?;
                    pipeline.set_state(gst::State::Playing)?;
                    reassemblers.clear();
                    requested_initial_iframe = false;
                }

                // 如果这是我们收到的第一个包，立即向发送端请求一个I-frame
                if !requested_initial_iframe {
                    println!(
                        "[STATE] First packet received. Requesting I-Frame from {}...",
                        remote_addr
                    );
                    let request = [PacketType::IFrameRequest as u8];
                    if let Err(e) = socket.send_to(&request, remote_addr).await {
                        eprintln!("[ERROR] Failed to send I-Frame request: {}", e);
                    }
                    requested_initial_iframe = true;
                }

                // 处理包的逻辑保持不变
                handle_udp_packet(
                    len,
                    &buf,
                    &remote_addr,
                    &mut reassemblers,
                    &appsrc,
                    &socket,
                    &mut latency_history,
                    pipeline_start_time,
                    &mut sps_pps_inject_count,
                    &pipeline,
                )
                .await;
            }
            Err(e) => {
                eprintln!("[ERROR] UDP recv_from failed: {}", e);
            }
        }
    }
}

async fn handle_udp_packet(
    len: usize,
    buf: &[u8],
    remote_addr: &SocketAddr,
    reassemblers: &mut HashMap<u32, FrameReassembler>,
    appsrc: &gst_app::AppSrc,
    socket: &Arc<UdpSocket>,
    latency_history: &mut VecDeque<f64>,
    pipeline_start_time: Instant,
    sps_pps_inject_count: &mut usize,
    pipeline: &gst::Pipeline, // 新增
) {
    // --- 新增：SPS/PPS缓存 ---
    use std::sync::OnceLock;
    static SPS_PPS_CACHE: OnceLock<std::sync::Mutex<Option<Vec<u8>>>> = OnceLock::new();
    let sps_pps_cache = SPS_PPS_CACHE.get_or_init(|| std::sync::Mutex::new(None));
    // --- 新增结束 ---

    use std::sync::Mutex;
    static LAST_SPS_PPS: OnceLock<Mutex<Option<Vec<u8>>>> = OnceLock::new();

    if len > 0 && PacketType::try_from(buf[0]) == Ok(PacketType::SpsPps) {
        let new_sps_pps = buf[1..len].to_vec();
        let last_sps_pps = LAST_SPS_PPS.get_or_init(|| Mutex::new(None));
        let mut last_guard = last_sps_pps.lock().unwrap();
        let changed = match &*last_guard {
            Some(old) => *old != new_sps_pps,
            None => true,
        };
        if changed {
            println!("[INFO] SPS/PPS changed, restarting pipeline!");
            *last_guard = Some(new_sps_pps.clone());
            *sps_pps_cache.lock().unwrap() = Some(new_sps_pps);
            *sps_pps_inject_count = 0;
            if let Err(e) = pipeline.set_state(gst::State::Null) {
                eprintln!("[ERROR] Failed to set pipeline to Null: {:?}", e);
            }
            if let Err(e) = pipeline.set_state(gst::State::Playing) {
                eprintln!("[ERROR] Failed to set pipeline to Playing: {:?}", e);
            }
            // 只在变化时请求I-Frame
            let request = [PacketType::IFrameRequest as u8];
            if let Err(e) = socket.send_to(&request, remote_addr).await {
                eprintln!("[ERROR] Failed to send I-Frame request: {}", e);
            }
        } else {
            // 仅更新缓存，不重启pipeline
            *sps_pps_cache.lock().unwrap() = Some(new_sps_pps);
        }
        return;
    }

    if len > 0 && PacketType::try_from(buf[0]) == Ok(PacketType::Data) {
        if let Some(header) = DataHeader::from_bytes(&buf[1..len]) {
            let reassembler = reassemblers
                .entry(header.frame_id)
                .or_insert_with(|| FrameReassembler::new(&header));
            let payload = buf[1 + DATA_HEADER_SIZE..len].to_vec();

            if let Some(complete_frame) = reassembler.add_packet(header.packet_id, payload) {
                // 丢弃空帧
                if complete_frame.is_empty() {
                    eprintln!("[WARN] Dropped empty frame (size=0), skipping push to appsrc.");
                    reassemblers.remove(&header.frame_id);
                    return;
                }
                if !reassembler.is_key_frame {
                    // println!(
                    //     "[DEBUG] Non-key frame: len={}, head={:02x?}",
                    //     complete_frame.len(),
                    //     &complete_frame[..std::cmp::min(32, complete_frame.len())]
                    // );
                }
                if !reassembler.is_key_frame
                    && complete_frame.len() < 8192
                    && (complete_frame.windows(5).any(|w| w == [0, 0, 0, 1, 0x67])
                        || complete_frame.windows(5).any(|w| w == [0, 0, 0, 1, 0x68]))
                {
                    *sps_pps_cache.lock().unwrap() = Some(complete_frame.clone());
                    reassemblers.remove(&header.frame_id);
                    println!(
                        "[INFO] SPS/PPS cached. len={}, head={:02x?}",
                        complete_frame.len(),
                        &complete_frame[..std::cmp::min(16, complete_frame.len())]
                    );
                    return;
                }

                // I帧前拼接缓存的SPS/PPS
                let final_frame = if reassembler.is_key_frame {
                    if *sps_pps_inject_count < 3 {
                        if let Some(ref sps_pps) = *sps_pps_cache.lock().unwrap() {
                            let mut v = sps_pps.clone();
                            v.extend_from_slice(&complete_frame);
                            *sps_pps_inject_count += 1;
                            v
                        } else {
                            complete_frame.clone()
                        }
                    } else {
                        complete_frame.clone()
                    }
                } else {
                    complete_frame.clone()
                };
                // --- 新增结束 ---

                let arrival_time_ns = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos() as u64;
                let log_latency_ns =
                    arrival_time_ns.saturating_sub(reassembler.capture_timestamp_ns);
                let log_latency_ms = log_latency_ns as f64 / 1_000_000.0;

                if latency_history.len() >= LATENCY_AVG_WINDOW {
                    latency_history.pop_front();
                }
                latency_history.push_back(log_latency_ms);

                let avg_latency: f64 =
                    latency_history.iter().sum::<f64>() / latency_history.len() as f64;

                // println!(
                //     "[FRAME] #{:<5} | Size: {:>5} bytes | Latency (now): {:>6.2} ms | Latency (avg): {:>6.2} ms",
                //     header.frame_id,
                //     final_frame.len(),
                //     log_latency_ms,
                //     avg_latency,
                // );

                let mut gst_buffer = gst::Buffer::with_size(final_frame.len()).unwrap();
                {
                    let mut_buffer = gst_buffer.get_mut().unwrap();
                    let running_time = Instant::now().duration_since(pipeline_start_time);
                    mut_buffer
                        .set_pts(gst::ClockTime::from_nseconds(running_time.as_nanos() as u64));
                    mut_buffer.copy_from_slice(0, &final_frame).unwrap();
                }

                if let Err(e) = appsrc.push_buffer(gst_buffer) {
                    eprintln!(
                        "[GStreamer] Error pushing buffer: {:?}. The pipeline might be broken.",
                        e
                    );
                }

                if reassembler.is_key_frame {
                    let ack = AckPacket {
                        frame_id: header.frame_id,
                    };
                    let mut ack_buf = [0u8; 1 + ACK_PACKET_SIZE];
                    ack_buf[0] = PacketType::Ack as u8;
                    ack_buf[1..].copy_from_slice(&ack.to_bytes());
                    let sock_clone = Arc::clone(socket);
                    let remote_addr_clone = *remote_addr;
                    tokio::spawn(async move {
                        if let Err(e) = sock_clone.send_to(&ack_buf, remote_addr_clone).await {
                            eprintln!(
                                "[ERROR] Failed to send ACK for frame #{}: {}",
                                header.frame_id, e
                            );
                        }
                    });
                }
                reassemblers.remove(&header.frame_id);
            }
        }
    }
}
