// --- packages/linux_receiver/src/main.rs ---

use anyhow::{anyhow, Result};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;

use protocol::{AckPacket, DataHeader, PacketType, ACK_PACKET_SIZE, DATA_HEADER_SIZE};
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::net::UdpSocket;
use tokio::time::sleep;

const LISTEN_ADDR: &str = "0.0.0.0:8080";
const MAX_DATAGRAM_SIZE: usize = 65_507;
const V4L2_DEVICE: &str = "/dev/video10";
const LATENCY_AVG_WINDOW: usize = 60;
const SIGNAL_TIMEOUT: Duration = Duration::from_secs(2);

// AI-MOD-START
// 关键修复 (结构错误): 将 FrameReassembler 的定义移到文件顶部，以便 main 和其他函数可以找到它。
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
// AI-MOD-END

struct VideoPipeline {
    pipeline: gst::Pipeline,
    appsrc: gst_app::AppSrc,
    start_time: Instant,
}

fn create_and_run_standby_pipeline() -> Result<gst::Pipeline> {
    let pipeline_str = format!(
        "videotestsrc is-live=true pattern=black ! videoconvert ! video/x-raw,format=YUY2 ! v4l2sink device={}",
        V4L2_DEVICE
    );
    let pipeline = gst::parse::launch(&pipeline_str)?
        .downcast::<gst::Pipeline>()
        .map_err(|_| anyhow!("Failed to create standby pipeline"))?;

    pipeline.set_state(gst::State::Playing)?;
    Ok(pipeline)
}

fn create_video_pipeline() -> Result<VideoPipeline> {
    let pipeline_str = format!(
        "appsrc name=src caps=\"video/x-h264,stream-format=byte-stream\" ! h264parse ! avdec_h264 ! videoconvert ! video/x-raw,format=YUY2 ! v4l2sink name=sink device={}",
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
    appsrc.set_property("do-timestamp", true);
    appsrc.set_format(gst::Format::Time);
    appsrc.set_latency(gst::ClockTime::ZERO, gst::ClockTime::ZERO);
    sink.set_property("sync", false);

    Ok(VideoPipeline {
        pipeline,
        appsrc,
        start_time: Instant::now(),
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    gst::init()?;
    println!("[NeuroCam Linux Receiver - FINAL ARCHITECTURE]");

    let socket = Arc::new(UdpSocket::bind(LISTEN_ADDR).await?);
    println!(
        "[OK] Listening on {}. Outputting to {}",
        LISTEN_ADDR, V4L2_DEVICE
    );

    let mut buf = vec![0u8; MAX_DATAGRAM_SIZE];
    let mut reassemblers: HashMap<u32, FrameReassembler> = HashMap::new();
    let mut latency_history: VecDeque<f64> = VecDeque::with_capacity(LATENCY_AVG_WINDOW);

    let mut last_packet_time = Instant::now();
    let mut standby_pipe = Some(create_and_run_standby_pipeline()?);
    let mut video_pipe: Option<VideoPipeline> = None;
    println!("[STATE] Standby pipeline running. Device is openable.");

    loop {
        tokio::select! {
            result = socket.recv_from(&mut buf) => {
                if let Ok((len, remote_addr)) = result {
                    if standby_pipe.is_some() {
                        println!("[STATE] Signal acquired! Switching to video pipeline...");
                        if let Some(p) = standby_pipe.take() {
                            p.set_state(gst::State::Null)?;
                        }
                        let new_video_pipe = create_video_pipeline()?;
                        new_video_pipe.pipeline.set_state(gst::State::Playing)?;
                        video_pipe = Some(new_video_pipe);

                        let request = [PacketType::IFrameRequest as u8];
                        let sock_clone = Arc::clone(&socket);
                        tokio::spawn(async move {
                            let _ = sock_clone.send_to(&request, remote_addr).await;
                        });
                        println!("[STATE] Video pipeline is now active.");
                    }

                    last_packet_time = Instant::now();

                    if let Some(vp) = &mut video_pipe {
                        handle_udp_packet(len, &buf, &remote_addr, &mut reassemblers, vp, &socket, &mut latency_history).await;
                    }
                }
            },
            _ = sleep(Duration::from_millis(500)) => {
                if video_pipe.is_some() && last_packet_time.elapsed() > SIGNAL_TIMEOUT {
                    println!("[STATE] Signal lost. Switching back to standby pipeline...");
                    if let Some(vp) = video_pipe.take() {
                        vp.pipeline.set_state(gst::State::Null)?;
                    }
                    standby_pipe = Some(create_and_run_standby_pipeline()?);
                    reassemblers.clear();
                    println!("[STATE] Standby pipeline is now active.");
                }
            }
        }
    }
}

async fn handle_udp_packet(
    len: usize,
    buf: &[u8],
    remote_addr: &SocketAddr,
    reassemblers: &mut HashMap<u32, FrameReassembler>,
    video_pipe: &mut VideoPipeline,
    socket: &Arc<UdpSocket>,
    latency_history: &mut VecDeque<f64>,
) {
    if len > 0 && PacketType::try_from(buf[0]) == Ok(PacketType::Data) {
        if let Some(header) = DataHeader::from_bytes(&buf[1..len]) {
            let reassembler = reassemblers
                .entry(header.frame_id)
                .or_insert_with(|| FrameReassembler::new(&header));
            let payload = buf[1 + DATA_HEADER_SIZE..len].to_vec();

            if let Some(complete_frame) = reassembler.add_packet(header.packet_id, payload) {
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

                println!(
                    "[FRAME] #{:<5} | Size: {:>5} KB | Clock Skew Latency: {:>7.2} ms",
                    header.frame_id,
                    complete_frame.len() / 1024,
                    log_latency_ms
                );

                let mut gst_buffer = gst::Buffer::with_size(complete_frame.len()).unwrap();
                {
                    let mut_buffer = gst_buffer.get_mut().unwrap();
                    let running_time = reassembler.last_seen.duration_since(video_pipe.start_time);
                    mut_buffer
                        .set_pts(gst::ClockTime::from_nseconds(running_time.as_nanos() as u64));
                    mut_buffer.copy_from_slice(0, &complete_frame).unwrap();
                }

                if let Err(e) = video_pipe.appsrc.push_buffer(gst_buffer) {
                    eprintln!("[GStreamer] Error pushing buffer: {:?}. This might indicate the pipe is shutting down.", e);
                }

                if reassembler.is_key_frame {
                    let ack = AckPacket {
                        frame_id: header.frame_id,
                    };
                    let mut ack_buf = [0u8; 1 + ACK_PACKET_SIZE];
                    ack_buf[0] = PacketType::Ack as u8;
                    ack_buf[1..].copy_from_slice(&ack.to_bytes());
                    let _ = socket.send_to(&ack_buf, remote_addr).await;
                }
                reassemblers.remove(&header.frame_id);
            }
        }
    }
}
