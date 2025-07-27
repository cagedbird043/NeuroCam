// --- packages/linux_receiver/src/main.rs (WITH THE FINAL COMPILER ERROR FIXED) ---

use gstreamer as gst;
use gstreamer::glib;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use protocol::{AckPacket, DataHeader, PacketType, ACK_PACKET_SIZE, DATA_HEADER_SIZE};
use std::collections::HashMap;
use std::error::Error;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket; // GLib is needed for the MainContext and ControlFlow

const LISTEN_ADDR: &str = "0.0.0.0:8080";
const V4L2_DEVICE: &str = "/dev/video10";
const MAX_DATAGRAM_SIZE: usize = 65_507;
const SIGNAL_TIMEOUT: Duration = Duration::from_secs(3);
const STANDBY_FILE: &str = "packages/linux_receiver/standby.h264";

/// Creates the single, robust, persistent pipeline.
fn create_pipeline() -> Result<
    (
        gst::Pipeline,
        gst_app::AppSrc,
        gst::Element,
        gst::Pad,
        gst::Pad,
    ),
    Box<dyn Error>,
> {
    let pipeline = gst::Pipeline::new();

    let filesrc = gst::ElementFactory::make("filesrc")
        .name("filesrc")
        .build()?;
    let standby_parse = gst::ElementFactory::make("h264parse")
        .name("standby_parse")
        .build()?;
    let standby_queue = gst::ElementFactory::make("queue")
        .name("standby_queue")
        .build()?;

    let appsrc_element = gst::ElementFactory::make("appsrc").name("appsrc").build()?;
    let net_queue = gst::ElementFactory::make("queue")
        .name("net_queue")
        .build()?;

    let selector = gst::ElementFactory::make("input-selector")
        .name("selector")
        .build()?;
    let common_parse = gst::ElementFactory::make("h264parse")
        .name("common_parse")
        .build()?;
    let decode = gst::ElementFactory::make("avdec_h264")
        .name("decode")
        .build()?;
    let convert = gst::ElementFactory::make("videoconvert")
        .name("convert")
        .build()?;
    let sink = gst::ElementFactory::make("v4l2sink").name("sink").build()?;

    filesrc.set_property("location", STANDBY_FILE);

    let bus = pipeline.bus().unwrap();
    let pipeline_weak = pipeline.downgrade();
    bus.add_watch_local(move |_, msg| {
        if let gst::MessageView::Eos(_) = msg.view() {
            if let Some(pipeline) = pipeline_weak.upgrade() {
                pipeline
                    .seek_simple(gst::SeekFlags::FLUSH, gst::ClockTime::ZERO)
                    .expect("Failed to seek pipeline");
            }
        }
        // === FIX: The one and only correct way to return from the bus watch ===
        glib::ControlFlow::Continue
    })?;

    let appsrc = appsrc_element.downcast_ref::<gst_app::AppSrc>().unwrap();
    appsrc.set_property_from_str(
        "caps",
        "video/x-h264, stream-format=byte-stream, alignment=au, profile=baseline",
    );
    appsrc.set_property_from_str("is-live", "true");
    appsrc.set_property_from_str("format", "time");

    sink.set_property("device", V4L2_DEVICE);
    sink.set_property_from_str("sync", "false");

    pipeline.add_many(&[
        &filesrc,
        &standby_parse,
        &standby_queue,
        &appsrc_element,
        &net_queue,
        &selector,
        &common_parse,
        &decode,
        &convert,
        &sink,
    ])?;

    gst::Element::link_many(&[&filesrc, &standby_parse, &standby_queue])?;
    let selector_sink_0 = selector.request_pad_simple("sink_0").unwrap();
    standby_queue
        .static_pad("src")
        .unwrap()
        .link(&selector_sink_0)?;

    gst::Element::link_many(&[&appsrc_element, &net_queue])?;
    let selector_sink_1 = selector.request_pad_simple("sink_1").unwrap();
    net_queue
        .static_pad("src")
        .unwrap()
        .link(&selector_sink_1)?;

    gst::Element::link_many(&[&selector, &common_parse, &decode, &convert, &sink])?;

    selector.set_property("active-pad", &selector_sink_0);

    println!("[GStreamer] Final, correct pipeline created. Standby stream is handled by filesrc.");

    Ok((
        pipeline,
        appsrc.clone(),
        selector,
        selector_sink_0,
        selector_sink_1,
    ))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    gst::init()?;

    let (pipeline, appsrc, selector, standby_pad, network_pad) = create_pipeline()?;
    pipeline.set_state(gst::State::Playing)?;

    let socket = Arc::new(UdpSocket::bind(LISTEN_ADDR).await?);
    let mut buf = vec![0u8; MAX_DATAGRAM_SIZE];
    let mut reassemblers: HashMap<u32, FrameReassembler> = HashMap::new();

    let mut is_active = false;
    let mut last_packet_time = Instant::now();

    let main_context = glib::MainContext::default();

    loop {
        match tokio::time::timeout(Duration::from_millis(10), socket.recv_from(&mut buf)).await {
            Ok(Ok((len, remote_addr))) => {
                last_packet_time = Instant::now();

                if let Some(frame_data) =
                    handle_network_packet(len, &buf, &remote_addr, &mut reassemblers, &socket).await
                {
                    let buffer = gst::Buffer::from_slice(frame_data);
                    if appsrc.push_buffer(buffer).is_err() {
                        eprintln!("[AppSrc] Failed to push network buffer. Pipeline might be shutting down.");
                    }

                    if !is_active {
                        println!("[State] First complete network frame received. Switching to Active mode.");
                        is_active = true;
                        selector.set_property("active-pad", &network_pad);
                    }
                }
            }
            Err(_) => (),
            Ok(Err(e)) => {
                eprintln!("[Socket] Error receiving packet: {}", e);
            }
        }

        if is_active && last_packet_time.elapsed() > SIGNAL_TIMEOUT {
            println!("[State] Signal lost (timeout). Switching back to Standby mode.");
            is_active = false;
            selector.set_property("active-pad", &standby_pad);
            reassemblers.clear();
        }

        while main_context.pending() {
            main_context.iteration(false);
        }
    }
}

// --- Helper Structs and Functions (Unchanged, they were correct) ---
struct FrameReassembler {
    packets: Vec<Option<Vec<u8>>>,
    received_count: u16,
    total_packets: u16,
}
impl FrameReassembler {
    fn new(header: &DataHeader) -> Self {
        Self {
            packets: vec![None; header.total_packets as usize],
            received_count: 0,
            total_packets: header.total_packets,
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

async fn handle_network_packet(
    len: usize,
    buf: &[u8],
    remote_addr: &SocketAddr,
    reassemblers: &mut HashMap<u32, FrameReassembler>,
    socket: &Arc<UdpSocket>,
) -> Option<Vec<u8>> {
    if len <= DATA_HEADER_SIZE || PacketType::try_from(buf[0]).is_err() {
        return None;
    }
    if let Some(header) = DataHeader::from_bytes(&buf[1..]) {
        let reassembler = reassemblers
            .entry(header.frame_id)
            .or_insert_with(|| FrameReassembler::new(&header));
        let payload = buf[1 + DATA_HEADER_SIZE..len].to_vec();
        if let Some(complete_frame) = reassembler.add_packet(header.packet_id, payload) {
            reassemblers.remove(&header.frame_id);
            if header.is_key_frame != 0 {
                let ack = AckPacket {
                    frame_id: header.frame_id,
                };
                let mut ack_buf = [0u8; 1 + ACK_PACKET_SIZE];
                ack_buf[0] = PacketType::Ack as u8;
                ack_buf[1..].copy_from_slice(&ack.to_bytes());
                let socket_clone = socket.clone();
                let remote_addr_owned = *remote_addr;
                tokio::spawn(async move {
                    let _ = socket_clone.send_to(&ack_buf, remote_addr_owned).await;
                });
            }
            return Some(complete_frame);
        }
    }
    None
}
