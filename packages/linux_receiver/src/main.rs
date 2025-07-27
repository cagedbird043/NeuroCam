// --- packages/linux_receiver/src/main.rs (THE ULTIMATE TEE/FAKESINK PATTERN) ---

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use protocol::{AckPacket, DataHeader, PacketType, ACK_PACKET_SIZE, DATA_HEADER_SIZE};
use std::collections::HashMap;
use std::error::Error;
use std::net::SocketAddr;
use std::result::Result;
use std::sync::Arc;
use tokio::net::UdpSocket;

const LISTEN_ADDR: &str = "0.0.0.0:8080";
const V4L2_DEVICE: &str = "/dev/video10";
const MAX_DATAGRAM_SIZE: usize = 65_507;
const SIGNAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

const VIDEO_WIDTH: i32 = 640;
const VIDEO_HEIGHT: i32 = 480;

struct FinalPipeline {
    pipeline: gst::Pipeline,
    appsrc: gst_app::AppSrc,
    selector: gst::Element,
}

/// 创建最终的、解耦的、工业级稳定的管线
/// 终极模式: 使用 tee 和 fakesink 创建一个“泄压阀”，确保管线永不因 Sink 阻塞而冻结。
fn create_final_pipeline() -> Result<FinalPipeline, Box<dyn Error>> {
    gst::init()?;
    let pipeline = gst::Pipeline::new();

    // --- 1. 创建所有 GStreamer 元素 ---

    let raw_video_caps = gst::Caps::builder("video/x-raw")
        .field("width", VIDEO_WIDTH)
        .field("height", VIDEO_HEIGHT)
        .field("format", "I420")
        .build();

    // 输入分支
    let standby_src = gst::ElementFactory::make("videotestsrc")
        .name("standby_src")
        .build()?;
    let standby_caps = gst::ElementFactory::make("capsfilter")
        .name("standby_caps")
        .build()?;
    let standby_queue = gst::ElementFactory::make("queue")
        .name("standby_queue")
        .build()?;

    let appsrc_element = gst::ElementFactory::make("appsrc").name("appsrc").build()?;
    let net_parse = gst::ElementFactory::make("h264parse")
        .name("net_parser")
        .build()?;
    let net_decode = gst::ElementFactory::make("avdec_h264")
        .name("net_decoder")
        .build()?;
    let net_convert = gst::ElementFactory::make("videoconvert")
        .name("net_convert")
        .build()?;
    let net_caps = gst::ElementFactory::make("capsfilter")
        .name("net_caps")
        .build()?;
    let net_queue = gst::ElementFactory::make("queue")
        .name("net_queue")
        .build()?;

    // 切换与分流
    let selector = gst::ElementFactory::make("input-selector")
        .name("selector")
        .build()?;
    let tee = gst::ElementFactory::make("tee").name("tee").build()?;

    // 输出分支 1: v4l2loopback (可能会阻塞)
    let v4l2_queue = gst::ElementFactory::make("queue")
        .name("v4l2_queue")
        .build()?;
    let v4l2_convert = gst::ElementFactory::make("videoconvert")
        .name("v4l2_convert")
        .build()?;
    let v4l2_sink = gst::ElementFactory::make("v4l2sink")
        .name("v4l2_sink")
        .build()?;

    // 输出分支 2: 泄压阀 (永远畅通)
    let fake_queue = gst::ElementFactory::make("queue")
        .name("fake_queue")
        .build()?;
    let fake_sink = gst::ElementFactory::make("fakesink")
        .name("fakesink")
        .build()?;

    // --- 2. 配置元素属性 ---

    standby_src.set_property_from_str("is-live", "true");
    standby_src.set_property_from_str("pattern", "smpte");
    standby_caps.set_property("caps", &raw_video_caps);

    let appsrc = appsrc_element.downcast_ref::<gst_app::AppSrc>().unwrap();
    appsrc.set_property_from_str(
        "caps",
        "video/x-h264, stream-format=byte-stream, alignment=au, profile=baseline",
    );
    appsrc.set_property_from_str("is-live", "true");
    appsrc.set_property_from_str("do-timestamp", "false");
    appsrc.set_format(gst::Format::Time);
    net_parse.set_property_from_str("config-interval", "-1");
    net_caps.set_property("caps", &raw_video_caps);

    v4l2_sink.set_property("device", V4L2_DEVICE);
    v4l2_sink.set_property_from_str("sync", "false");

    // 配置 fakesink，让它不影响管线时钟，并尽快消费数据
    fake_sink.set_property_from_str("sync", "false");
    fake_sink.set_property_from_str("async", "false");

    // --- 3. 添加到管线 ---
    pipeline.add_many(&[
        &standby_src,
        &standby_caps,
        &standby_queue,
        &appsrc_element,
        &net_parse,
        &net_decode,
        &net_convert,
        &net_caps,
        &net_queue,
        &selector,
        &tee,
        &v4l2_queue,
        &v4l2_convert,
        &v4l2_sink,
        &fake_queue,
        &fake_sink,
    ])?;

    // --- 4. 链接元素 ---

    // 链接输入分支到 selector
    gst::Element::link_many(&[&standby_src, &standby_caps, &standby_queue, &selector])?;
    gst::Element::link_many(&[
        &appsrc_element,
        &net_parse,
        &net_decode,
        &net_convert,
        &net_caps,
        &net_queue,
    ])?;
    let net_pad = net_queue.static_pad("src").unwrap();
    let selector_sink_1 = selector.request_pad_simple("sink_1").unwrap();
    net_pad.link(&selector_sink_1)?;

    // 链接 selector 到 tee
    selector.link(&tee)?;

    // 链接 tee 的两个输出分支
    let tee_v4l2_pad = tee.request_pad_simple("src_%u").unwrap();
    gst::Element::link_many(&[&v4l2_queue, &v4l2_convert, &v4l2_sink])?;
    let v4l2_queue_pad = v4l2_queue.static_pad("sink").unwrap();
    tee_v4l2_pad.link(&v4l2_queue_pad)?;

    let tee_fake_pad = tee.request_pad_simple("src_%u").unwrap();
    gst::Element::link_many(&[&fake_queue, &fake_sink])?;
    let fake_queue_pad = fake_queue.static_pad("sink").unwrap();
    tee_fake_pad.link(&fake_queue_pad)?;

    // --- 5. 设置初始状态 ---
    selector.set_property("active-pad", &selector.static_pad("sink_0").unwrap());

    println!("[GStreamer] The ultimate pipeline with Tee/Fakesink pressure relief is created.");
    Ok(FinalPipeline {
        pipeline,
        appsrc: appsrc.clone(),
        selector,
    })
}

// =========================================================================================
//  main() 和其他辅助函数无需任何修改。
// =========================================================================================

#[derive(Debug, PartialEq, Eq)]
enum PacketOutcome {
    FrameProcessed,
    IFrameReceived,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let p = create_final_pipeline()?;
    p.pipeline.set_state(gst::State::Playing)?;

    let pipeline = p.pipeline;
    let appsrc = p.appsrc;
    let selector = p.selector;
    let standby_pad = selector.static_pad("sink_0").unwrap();
    let network_pad = selector.static_pad("sink_1").unwrap();

    let socket = Arc::new(UdpSocket::bind(LISTEN_ADDR).await?);
    let mut buf = vec![0u8; MAX_DATAGRAM_SIZE];
    let mut reassemblers: HashMap<u32, FrameReassembler> = HashMap::new();

    let mut signal_active = false;
    let mut last_packet_time = std::time::Instant::now();
    let mut stream_start_time: Option<std::time::Instant> = None;

    loop {
        let recv_result = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            socket.recv_from(&mut buf),
        )
        .await;
        match recv_result {
            Ok(Ok((len, remote_addr))) => {
                last_packet_time = std::time::Instant::now();
                let outcome = handle_udp_packet(
                    len,
                    &buf,
                    &remote_addr,
                    &appsrc,
                    &socket,
                    &mut reassemblers,
                    stream_start_time,
                )
                .await;

                if !signal_active && matches!(outcome, Some(PacketOutcome::IFrameReceived)) {
                    println!("[STATE] First I-Frame Received! Switching to Network Stream...");
                    selector.set_property("active-pad", &network_pad);
                    signal_active = true;
                    stream_start_time = Some(std::time::Instant::now());
                }
            }
            Err(_) => {
                // Timeout
                if signal_active && last_packet_time.elapsed() > SIGNAL_TIMEOUT {
                    println!("[STATE] Signal Lost! Switching back to Standby Stream...");

                    pipeline.set_state(gst::State::Paused)?;
                    selector.set_property("active-pad", &standby_pad);
                    pipeline.set_state(gst::State::Playing)?;

                    signal_active = false;
                    reassemblers.clear();
                    stream_start_time = None;
                    println!("[STATE] Switched back to standby and pipeline restarted.");
                }
            }
            Ok(Err(e)) => eprintln!("[ERROR] UDP recv error: {}", e),
        }
    }
}

struct FrameReassembler {
    packets: Vec<Option<Vec<u8>>>,
    received_count: u16,
    total_packets: u16,
    is_key_frame: bool,
    frame_id: u32,
}
impl FrameReassembler {
    fn new(header: &DataHeader) -> Self {
        Self {
            packets: vec![None; header.total_packets as usize],
            received_count: 0,
            total_packets: header.total_packets,
            is_key_frame: header.is_key_frame != 0,
            frame_id: header.frame_id,
        }
    }
    fn add_packet(&mut self, packet_id: u16, data: Vec<u8>) -> Option<Vec<u8>> {
        let id = packet_id as usize;
        if id < self.packets.len() && self.packets[id].is_none() {
            self.packets[id] = Some(data);
            self.received_count += 1;
        }
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

async fn handle_udp_packet(
    len: usize,
    buf: &[u8],
    remote_addr: &SocketAddr,
    appsrc: &gst_app::AppSrc,
    socket: &Arc<UdpSocket>,
    reassemblers: &mut HashMap<u32, FrameReassembler>,
    stream_start_time: Option<std::time::Instant>,
) -> Option<PacketOutcome> {
    if len <= DATA_HEADER_SIZE || PacketType::try_from(buf[0]).is_err() {
        return None;
    }

    if let Some(header) = DataHeader::from_bytes(&buf[1..]) {
        let reassembler = reassemblers
            .entry(header.frame_id)
            .or_insert_with(|| FrameReassembler::new(&header));
        let payload = buf[1 + DATA_HEADER_SIZE..len].to_vec();

        if let Some(complete_frame) = reassembler.add_packet(header.packet_id, payload) {
            if complete_frame.is_empty() {
                eprintln!(
                    "[ERROR] Reassembled a zero-length frame for ID {}, skipping push.",
                    header.frame_id
                );
                reassemblers.remove(&header.frame_id);
                return None;
            }

            let mut gst_buffer = gst::Buffer::with_size(complete_frame.len()).unwrap();
            let buffer_ref = gst_buffer.get_mut().unwrap();
            {
                let mut map = buffer_ref.map_writable().unwrap();
                map.copy_from_slice(&complete_frame);
            }
            if let Some(start_time) = stream_start_time {
                let running_time = std::time::Instant::now().saturating_duration_since(start_time);
                buffer_ref.set_pts(Some(gst::ClockTime::from_nseconds(
                    running_time.as_nanos() as u64
                )));
            }
            let _ = appsrc.push_buffer(gst_buffer);

            let is_key_frame = reassembler.is_key_frame;
            reassemblers.remove(&header.frame_id);

            if is_key_frame {
                println!("[DEBUG] I-Frame received, frame_id={}", header.frame_id);
                let ack = AckPacket {
                    frame_id: header.frame_id,
                };
                let mut ack_buf = [0u8; 1 + ACK_PACKET_SIZE];
                ack_buf[0] = PacketType::Ack as u8;
                ack_buf[1..].copy_from_slice(&ack.to_bytes());
                let _ = socket.send_to(&ack_buf, remote_addr).await;
                return Some(PacketOutcome::IFrameReceived);
            } else {
                return Some(PacketOutcome::FrameProcessed);
            }
        }
    }
    None
}
