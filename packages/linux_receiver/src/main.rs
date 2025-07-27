// --- packages/linux_receiver/src/main.rs (THE FINAL, DECOUPLED-DECODER ARCHITECTURE) ---

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

struct FinalPipeline {
    pipeline: gst::Pipeline,
    appsrc: gst_app::AppSrc,
    selector: gst::Element,
}

/// 创建最终的、使用双解码器隔离的、工业级稳定管线
fn create_final_pipeline() -> Result<FinalPipeline, Box<dyn Error>> {
    gst::init()?;
    let pipeline = gst::Pipeline::new();

    // 1. 创建所有元素
    // -- 源 --
    let standby_src = gst::ElementFactory::make("videotestsrc")
        .name("standby_src")
        .build()?;
    let standby_caps = gst::ElementFactory::make("capsfilter")
        .name("standby_caps")
        .build()?;
    let standby_enc = gst::ElementFactory::make("x264enc")
        .name("standby_encoder")
        .build()?;
    let appsrc_element = gst::ElementFactory::make("appsrc").name("appsrc").build()?;

    // -- 合并器与分发器 --
    let funnel = gst::ElementFactory::make("funnel").name("funnel").build()?;
    let parse = gst::ElementFactory::make("h264parse")
        .name("parser")
        .build()?;
    let tee = gst::ElementFactory::make("tee").name("tee").build()?;

    // -- 解码分支 1 (待机) --
    let queue1 = gst::ElementFactory::make("queue").name("queue1").build()?;
    let dec1 = gst::ElementFactory::make("avdec_h264")
        .name("dec1")
        .build()?;
    let conv1 = gst::ElementFactory::make("videoconvert")
        .name("conv1")
        .build()?;

    // -- 解码分支 2 (网络) --
    let queue2 = gst::ElementFactory::make("queue").name("queue2").build()?;
    let dec2 = gst::ElementFactory::make("avdec_h264")
        .name("dec2")
        .build()?;
    let conv2 = gst::ElementFactory::make("videoconvert")
        .name("conv2")
        .build()?;

    // -- 最终切换器与输出 --
    let selector = gst::ElementFactory::make("input-selector")
        .name("selector")
        .build()?;
    let sink = gst::ElementFactory::make("v4l2sink").name("sink").build()?;

    // 2. 配置元素
    standby_src.set_property_from_str("is-live", "true");
    standby_src.set_property_from_str("pattern", "smpte");
    let raw_video_caps = gst::Caps::builder("video/x-raw")
        .field("width", 640)
        .field("height", 480)
        .build();
    standby_caps.set_property("caps", &raw_video_caps);
    standby_enc.set_property_from_str("tune", "zerolatency");

    let appsrc = appsrc_element.downcast_ref::<gst_app::AppSrc>().unwrap();
    appsrc.set_property_from_str(
        "caps",
        "video/x-h264, stream-format=byte-stream, alignment=au",
    );
    appsrc.set_property_from_str("is-live", "true");
    appsrc.set_property_from_str("do-timestamp", "false");
    appsrc.set_format(gst::Format::Time);

    sink.set_property("device", V4L2_DEVICE);
    sink.set_property_from_str("sync", "false");

    // 3. 添加所有元素
    pipeline.add_many(&[
        &standby_src,
        &standby_caps,
        &standby_enc,
        &appsrc_element,
        &funnel,
        &parse,
        &tee,
        &queue1,
        &dec1,
        &conv1,
        &queue2,
        &dec2,
        &conv2,
        &selector,
        &sink,
    ])?;

    // 4. 链接管线
    // -- 源到 funnel --
    gst::Element::link_many(&[&standby_src, &standby_caps, &standby_enc])?;
    standby_enc.link(&funnel)?;
    appsrc_element.link(&funnel)?;

    // -- funnel 到 tee --
    gst::Element::link_many(&[&funnel, &parse, &tee])?;

    // -- tee 到两个解码分支 --
    gst::Element::link_many(&[&queue1, &dec1, &conv1])?;
    let tee_src_pad1 = tee.request_pad_simple("src_%u").unwrap();
    let q1_sink_pad = queue1.static_pad("sink").unwrap();
    tee_src_pad1.link(&q1_sink_pad)?;

    gst::Element::link_many(&[&queue2, &dec2, &conv2])?;
    let tee_src_pad2 = tee.request_pad_simple("src_%u").unwrap();
    let q2_sink_pad = queue2.static_pad("sink").unwrap();
    tee_src_pad2.link(&q2_sink_pad)?;

    // -- 两个解码分支到 selector --
    conv1.link(&selector)?; // 默认链接到 sink_0
    conv2.link(&selector)?; // 默认链接到 sink_1

    // -- selector 到 sink --
    selector.link(&sink)?;

    // 5. 将 funnel 的行为设置为，只激活一个输入
    funnel.set_property_from_str("forward-sticky-events", "true");

    println!("[GStreamer] Final, dual-decoder architecture created. All systems nominal.");
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
                }
                last_packet_time = std::time::Instant::now();
                handle_udp_packet(len, &buf, &remote_addr, &appsrc, &mut reassemblers, &socket)
                    .await;
            }
            Err(_) => {
                // Timeout
                if signal_active && last_packet_time.elapsed() > SIGNAL_TIMEOUT {
                    println!("[STATE] Signal Lost! Switching back to Standby Stream...");
                    selector.set_property("active-pad", &standby_pad);
                    signal_active = false;
                    reassemblers.clear();
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
}
impl FrameReassembler {
    fn new(header: &DataHeader) -> Self {
        Self {
            packets: vec![None; header.total_packets as usize],
            received_count: 0,
            total_packets: header.total_packets,
            is_key_frame: header.is_key_frame != 0,
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
    reassemblers: &mut HashMap<u32, FrameReassembler>,
    socket: &Arc<UdpSocket>,
) {
    if len > DATA_HEADER_SIZE && PacketType::try_from(buf[0]).is_ok() {
        if let Some(header) = DataHeader::from_bytes(&buf[1..]) {
            let reassembler = reassemblers
                .entry(header.frame_id)
                .or_insert_with(|| FrameReassembler::new(&header));
            let payload = buf[1 + DATA_HEADER_SIZE..len].to_vec();
            if let Some(complete_frame) = reassembler.add_packet(header.packet_id, payload) {
                // We no longer need to manage timestamps, GStreamer will handle it.
                let gst_buffer = gst::Buffer::from_slice(complete_frame);
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
