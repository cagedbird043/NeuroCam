// --- packages/linux_receiver/src/main.rs (THE FINAL, STABLE, LIVE-SOURCE ARCHITECTURE) ---

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

/// 最终的管线结构体
struct FinalPipeline {
    pipeline: gst::Pipeline,
    appsrc: gst_app::AppSrc,
    selector: gst::Element,
}

/// 创建最终的、使用实时待机源的稳定管线
fn create_final_pipeline() -> Result<FinalPipeline, Box<dyn Error>> {
    gst::init()?;
    let pipeline = gst::Pipeline::new();

    // 1. 创建所有元素
    // ================== 核心修正：使用 videotestsrc 作为永不终结的实时待机源 ==================
    let standby_src = gst::ElementFactory::make("videotestsrc")
        .name("standby_src")
        .build()?;
    let standby_convert = gst::ElementFactory::make("videoconvert")
        .name("standby_convert")
        .build()?;
    let standby_enc = gst::ElementFactory::make("x264enc")
        .name("standby_encoder")
        .build()?;
    // ========================================================================================

    // -- 网络分支 --
    let appsrc_element = gst::ElementFactory::make("appsrc").name("appsrc").build()?;

    // -- 切换器 --
    let selector = gst::ElementFactory::make("input-selector")
        .name("selector")
        .build()?;

    // -- 公共处理路径 --
    let parse = gst::ElementFactory::make("h264parse")
        .name("parser")
        .build()?;
    let decode = gst::ElementFactory::make("avdec_h264")
        .name("decoder")
        .build()?;
    let common_convert = gst::ElementFactory::make("videoconvert")
        .name("common_convert")
        .build()?;
    let sink = gst::ElementFactory::make("v4l2sink").name("sink").build()?;

    // 2. 配置元素
    standby_src.set_property("is-live", true);
    // 设置你想要的待机画面模式: 0=smpte, 1=snow, 2=black, 3=white, 18=ball, 24=smpte100
    standby_src.set_property_from_str("pattern", "smpte"); // 也可以 "snow"、"black"、"white"、"ball"、"smpte100"
    standby_enc.set_property_from_str("tune", "zerolatency");
    let appsrc = appsrc_element.downcast_ref::<gst_app::AppSrc>().unwrap();
    appsrc.set_property_from_str(
        "caps",
        "video/x-h264, stream-format=byte-stream, alignment=au",
    );
    appsrc.set_property("is-live", true);
    appsrc.set_property("do-timestamp", true);
    appsrc.set_format(gst::Format::Time);

    sink.set_property("device", V4L2_DEVICE);
    sink.set_property("sync", false);

    // 3. 添加所有元素到管线
    pipeline.add_many(&[
        &standby_src,
        &standby_convert,
        &standby_enc,
        &appsrc_element,
        &selector,
        &parse,
        &decode,
        &common_convert,
        &sink,
    ])?;

    // 4. 链接管线
    // -- 链接待机分支到 selector --
    gst::Element::link_many(&[&standby_src, &standby_convert, &standby_enc])?;
    let standby_enc_pad = standby_enc.static_pad("src").unwrap();
    let selector_sink_0 = selector.request_pad_simple("sink_0").unwrap();
    standby_enc_pad.link(&selector_sink_0)?;

    // -- 链接网络分支到 selector --
    let appsrc_pad = appsrc_element.static_pad("src").unwrap();
    let selector_sink_1 = selector.request_pad_simple("sink_1").unwrap();
    appsrc_pad.link(&selector_sink_1)?;

    // -- 链接公共处理路径 --
    selector.link(&parse)?;
    gst::Element::link_many(&[&parse, &decode, &common_convert, &sink])?;

    // 5. 设置初始状态：激活待机分支
    selector.set_property("active-pad", &selector_sink_0);

    println!("[GStreamer] Final, robust pipeline created with LIVE standby source.");
    Ok(FinalPipeline {
        pipeline,
        appsrc: appsrc.clone(),
        selector,
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let p = create_final_pipeline()?;
    p.pipeline.set_state(gst::State::Playing)?;

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
                if !signal_active {
                    println!("[STATE] Signal Acquired! Switching to Network Stream...");
                    selector.set_property("active-pad", &network_pad);
                    signal_active = true;
                    stream_start_time = Some(std::time::Instant::now());
                }
                last_packet_time = std::time::Instant::now();
                handle_udp_packet(
                    len,
                    &buf,
                    &remote_addr,
                    &appsrc,
                    &socket,
                    &mut reassemblers,
                    stream_start_time,
                )
                .await;
            }
            Err(_) => {
                // Timeout
                if signal_active && last_packet_time.elapsed() > SIGNAL_TIMEOUT {
                    println!("[STATE] Signal Lost! Switching back to Standby Stream...");
                    selector.set_property("active-pad", &standby_pad);
                    signal_active = false;
                    reassemblers.clear();
                    stream_start_time = None;
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
) {
    if len > DATA_HEADER_SIZE && PacketType::try_from(buf[0]) == Ok(PacketType::Data) {
        if let Some(header) = DataHeader::from_bytes(&buf[1..]) {
            let reassembler = reassemblers
                .entry(header.frame_id)
                .or_insert_with(|| FrameReassembler::new(&header));
            let payload = buf[1 + DATA_HEADER_SIZE..len].to_vec();
            if let Some(complete_frame) = reassembler.add_packet(header.packet_id, payload) {
                let mut gst_buffer = gst::Buffer::with_size(complete_frame.len()).unwrap();
                let buffer_ref = gst_buffer.get_mut().unwrap();
                {
                    let mut map = buffer_ref.map_writable().unwrap();
                    map.copy_from_slice(&complete_frame);
                }
                if let Some(start_time) = stream_start_time {
                    let running_time =
                        std::time::Instant::now().saturating_duration_since(start_time);
                    buffer_ref.set_pts(Some(gst::ClockTime::from_nseconds(
                        running_time.as_nanos() as u64,
                    )));
                }
                let _ = appsrc.push_buffer(gst_buffer);
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
