// --- packages/linux_receiver/src/main.rs ---

// AI-MOD-START
// 关键修复 (清理): 移除未使用的 use 语句，消除编译器警告。
use anyhow::Result;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;

use protocol::{AckPacket, DataHeader, PacketType, ACK_PACKET_SIZE, DATA_HEADER_SIZE};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::net::UdpSocket;
use tokio::time::sleep;

const LISTEN_ADDR: &str = "0.0.0.0:8080";
const MAX_DATAGRAM_SIZE: usize = 65_507;
const V4L2_DEVICE: &str = "/dev/video10";
const SIGNAL_TIMEOUT: Duration = Duration::from_secs(2);
const HANDSHAKE_INTERVAL: Duration = Duration::from_millis(500);

// 关键修复 (结构错误): 将 FrameReassembler 的定义移到文件顶部。
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

struct GstreamerPipeline {
    pipeline: gst::Pipeline,
    appsrc: gst_app::AppSrc,
    input_selector: gst::Element,
    standby_pad: gst::Pad,
    network_pad: gst::Pad,
}

fn create_final_pipeline() -> Result<GstreamerPipeline> {
    gst::init()?;

    let pipeline = gst::Pipeline::new();
    let appsrc_elem = gst::ElementFactory::make("appsrc").name("netsrc").build()?;
    let videotestsrc = gst::ElementFactory::make("videotestsrc")
        .name("standbysrc")
        .build()?;
    let h264parse = gst::ElementFactory::make("h264parse").build()?;
    let avdec_h264 = gst::ElementFactory::make("avdec_h264").build()?;
    let input_selector = gst::ElementFactory::make("input-selector")
        .name("selector")
        .build()?;
    let videoconvert = gst::ElementFactory::make("videoconvert").build()?;
    let v4l2sink = gst::ElementFactory::make("v4l2sink").build()?;

    let appsrc = appsrc_elem.downcast_ref::<gst_app::AppSrc>().unwrap();
    appsrc.set_property_from_str("caps", "video/x-h264,stream-format=byte-stream");
    appsrc.set_property("is-live", true);
    appsrc.set_property("do-timestamp", true);
    appsrc.set_format(gst::Format::Time);
    appsrc.set_latency(gst::ClockTime::ZERO, gst::ClockTime::ZERO);

    videotestsrc.set_property("is-live", true);
    videotestsrc.set_property_from_str("pattern", "black");

    v4l2sink.set_property("device", V4L2_DEVICE);
    v4l2sink.set_property("sync", false);

    pipeline.add_many(&[
        &appsrc_elem,
        &h264parse,
        &avdec_h264,
        &videotestsrc,
        &input_selector,
        &videoconvert,
        &v4l2sink,
    ])?;

    let standby_pad = input_selector.request_pad_simple("sink_%u").unwrap();
    gst::Element::link_many(&[&videotestsrc, &input_selector])?;

    let network_pad = input_selector.request_pad_simple("sink_%u").unwrap();
    gst::Element::link_many(&[&appsrc_elem, &h264parse, &avdec_h264, &input_selector])?;

    gst::Element::link_many(&[&input_selector, &videoconvert, &v4l2sink])?;

    println!("[GStreamer] Final unified pipeline created successfully.");

    Ok(GstreamerPipeline {
        pipeline,
        appsrc: appsrc.clone(),
        input_selector,
        standby_pad,
        network_pad,
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    let gst_stuff = create_final_pipeline()?;

    gst_stuff
        .input_selector
        .set_property("active-pad", &gst_stuff.standby_pad);
    gst_stuff.pipeline.set_state(gst::State::Playing)?;
    println!("[STATE] Standby. Pipeline running, device is openable.");

    let socket = Arc::new(UdpSocket::bind(LISTEN_ADDR).await?);
    println!(
        "[OK] Listening on {}. Outputting to {}",
        LISTEN_ADDR, V4L2_DEVICE
    );

    let mut buf = vec![0u8; MAX_DATAGRAM_SIZE];
    let mut reassemblers: HashMap<u32, FrameReassembler> = HashMap::new();

    let mut last_packet_time = Instant::now();
    let mut last_handshake_time = Instant::now();
    let mut current_remote_addr: Option<SocketAddr> = None;
    let mut is_streaming = false;

    loop {
        tokio::select! {
            result = socket.recv_from(&mut buf) => {
                if let Ok((len, remote_addr)) = result {
                    last_packet_time = Instant::now();
                    current_remote_addr = Some(remote_addr);

                    if !is_streaming {
                        println!("[STATE] Signal acquired! Switching to network stream.");
                        gst_stuff.input_selector.set_property("active-pad", &gst_stuff.network_pad);
                        is_streaming = true;
                    }

                    handle_udp_packet(len, &buf, &remote_addr, &mut reassemblers, &gst_stuff.appsrc, &socket).await;
                }
            },
            _ = sleep(HANDSHAKE_INTERVAL) => {
                if is_streaming && last_packet_time.elapsed() > SIGNAL_TIMEOUT {
                    println!("[STATE] Signal lost. Switching back to standby.");
                    gst_stuff.input_selector.set_property("active-pad", &gst_stuff.standby_pad);
                    reassemblers.clear();
                    is_streaming = false;
                }

                if is_streaming && reassemblers.is_empty() && last_handshake_time.elapsed() > HANDSHAKE_INTERVAL {
                     if let Some(addr) = current_remote_addr {
                        println!("[STATE] Handshaking... Requesting I-Frame to recover stream.");
                        let request = [PacketType::IFrameRequest as u8];
                        let sock_clone = Arc::clone(&socket);
                        tokio::spawn(async move {
                            let _ = sock_clone.send_to(&request, addr).await;
                        });
                        last_handshake_time = Instant::now();
                     }
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
    appsrc: &gst_app::AppSrc,
    socket: &Arc<UdpSocket>,
) {
    if len > 0 && PacketType::try_from(buf[0]) == Ok(PacketType::Data) {
        if let Some(header) = DataHeader::from_bytes(&buf[1..len]) {
            let reassembler = reassemblers
                .entry(header.frame_id)
                .or_insert_with(|| FrameReassembler::new(&header));
            let payload = buf[1 + DATA_HEADER_SIZE..len].to_vec();

            if let Some(complete_frame) = reassembler.add_packet(header.packet_id, payload) {
                // AI-MOD-START
                // 恢复有用的延迟日志打印，这会使用 reassembler.capture_timestamp_ns，从而解决 dead_code 警告。
                let arrival_time_ns = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos() as u64;
                let clock_skew_latency_ns =
                    arrival_time_ns.saturating_sub(reassembler.capture_timestamp_ns);
                let clock_skew_latency_ms = clock_skew_latency_ns as f64 / 1_000_000.0;
                println!(
                    "[FRAME] #{:<5} | Size: {:>5} KB | Clock Skew Latency: {:>7.2} ms",
                    header.frame_id,
                    complete_frame.len() / 1024,
                    clock_skew_latency_ms
                );
                // AI-MOD-END

                let mut gst_buffer = gst::Buffer::with_size(complete_frame.len()).unwrap();
                {
                    let mut_buffer = gst_buffer.get_mut().unwrap();
                    mut_buffer.set_pts(gst::ClockTime::from_nseconds(arrival_time_ns));
                    mut_buffer.copy_from_slice(0, &complete_frame).unwrap();
                }

                if let Err(e) = appsrc.push_buffer(gst_buffer) {
                    eprintln!("[GStreamer] Error pushing buffer: {:?}.", e);
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
