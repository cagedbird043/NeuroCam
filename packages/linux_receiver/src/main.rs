// --- packages/linux_receiver/src/main.rs (THE ABSOLUTE FORMAT LOCK-IN FIX) ---

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
// ================== THE MOST CRITICAL CHANGE IN THE ENTIRE PROJECT ==================
// Define ONE SINGLE, UNAMBIGUOUS, rock-solid format that BOTH pipelines will be forced to use.
// YUY2 is a very common and well-supported V4L2 format.
const V4L2_FORMAT: &str = "YUY2";
// ====================================================================================

/// Creates the caps that will be enforced right before the v4l2sink.
fn create_final_caps() -> gst::Caps {
    gst::Caps::builder("video/x-raw")
        .field("width", VIDEO_WIDTH)
        .field("height", VIDEO_HEIGHT)
        .field("format", V4L2_FORMAT)
        .build()
}

/// 创建待机管线，强制输出最终格式。
fn create_standby_pipeline() -> Result<gst::Pipeline, Box<dyn Error>> {
    let pipeline = gst::Pipeline::new();
    let src = gst::ElementFactory::make("videotestsrc")
        .name("src")
        .build()?;
    let convert = gst::ElementFactory::make("videoconvert")
        .name("convert")
        .build()?;
    let capsfilter = gst::ElementFactory::make("capsfilter")
        .name("capsfilter")
        .build()?;
    let sink = gst::ElementFactory::make("v4l2sink").name("sink").build()?;

    capsfilter.set_property("caps", &create_final_caps());

    src.set_property_from_str("is-live", "true");
    src.set_property_from_str("pattern", "smpte");
    sink.set_property("device", V4L2_DEVICE);
    sink.set_property_from_str("sync", "false");

    pipeline.add_many(&[&src, &convert, &capsfilter, &sink])?;
    // The order is crucial: src -> convert (does the work) -> capsfilter (enforces) -> sink
    gst::Element::link_many(&[&src, &convert, &capsfilter, &sink])?;

    println!(
        "[GStreamer] Standby pipeline created, enforcing {} format.",
        V4L2_FORMAT
    );
    Ok(pipeline)
}

/// 创建网络管线，同样强制输出最终格式。
fn create_network_pipeline() -> Result<(gst::Pipeline, gst_app::AppSrc), Box<dyn Error>> {
    let pipeline = gst::Pipeline::new();
    let appsrc_element = gst::ElementFactory::make("appsrc").name("appsrc").build()?;
    let parse = gst::ElementFactory::make("h264parse")
        .name("parse")
        .build()?;
    let decode = gst::ElementFactory::make("avdec_h264")
        .name("decode")
        .build()?;
    let convert = gst::ElementFactory::make("videoconvert")
        .name("convert")
        .build()?;
    let capsfilter = gst::ElementFactory::make("capsfilter")
        .name("capsfilter")
        .build()?;
    let sink = gst::ElementFactory::make("v4l2sink").name("sink").build()?;

    capsfilter.set_property("caps", &create_final_caps());

    let appsrc = appsrc_element.downcast_ref::<gst_app::AppSrc>().unwrap();
    appsrc.set_property_from_str(
        "caps",
        "video/x-h264, stream-format=byte-stream, alignment=au, profile=baseline",
    );
    appsrc.set_property_from_str("is-live", "true");
    appsrc.set_property_from_str("do-timestamp", "false");
    appsrc.set_format(gst::Format::Time);

    parse.set_property_from_str("config-interval", "-1");
    sink.set_property("device", V4L2_DEVICE);
    sink.set_property_from_str("sync", "false");

    pipeline.add_many(&[
        &appsrc_element,
        &parse,
        &decode,
        &convert,
        &capsfilter,
        &sink,
    ])?;
    // The order is crucial and IDENTICAL to the standby pipeline's tail.
    gst::Element::link_many(&[
        &appsrc_element,
        &parse,
        &decode,
        &convert,
        &capsfilter,
        &sink,
    ])?;

    println!(
        "[GStreamer] Network pipeline created, enforcing {} format.",
        V4L2_FORMAT
    );
    Ok((pipeline, appsrc.clone()))
}

// The AppState and main loop logic is correct. No changes needed below.
enum AppState {
    Standby {
        pipeline: gst::Pipeline,
    },
    Active {
        pipeline: gst::Pipeline,
        appsrc: gst_app::AppSrc,
        stream_start_time: std::time::Instant,
    },
}

impl AppState {
    fn stop(&self) {
        let pipeline = match self {
            AppState::Standby { pipeline } => pipeline,
            AppState::Active { pipeline, .. } => pipeline,
        };
        pipeline
            .set_state(gst::State::Null)
            .expect("Failed to set pipeline to Null");
        println!("[State] Pipeline stopped and destroyed.");
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    gst::init()?;

    let socket = Arc::new(UdpSocket::bind(LISTEN_ADDR).await?);
    let mut buf = vec![0u8; MAX_DATAGRAM_SIZE];
    let mut reassemblers: HashMap<u32, FrameReassembler> = HashMap::new();

    let mut state = {
        let standby_pipeline = create_standby_pipeline()?;
        standby_pipeline.set_state(gst::State::Playing)?;
        AppState::Standby {
            pipeline: standby_pipeline,
        }
    };

    loop {
        match &mut state {
            AppState::Standby { .. } => {
                if let Ok((len, remote_addr)) = socket.recv_from(&mut buf).await {
                    if let Some(PacketOutcome::IFrameReceived(frame_data)) =
                        handle_udp_packet(len, &buf, &remote_addr, &mut reassemblers, &socket).await
                    {
                        println!("[State] First I-Frame received. Switching to Active state...");
                        state.stop();
                        let (pipeline, appsrc) = create_network_pipeline()?;
                        pipeline.set_state(gst::State::Playing)?;
                        push_frame_to_appsrc(
                            &appsrc,
                            frame_data,
                            std::time::Duration::from_secs(0),
                        );
                        state = AppState::Active {
                            pipeline,
                            appsrc,
                            stream_start_time: std::time::Instant::now(),
                        };
                    }
                }
            }
            AppState::Active {
                pipeline: _,
                appsrc,
                stream_start_time,
            } => {
                match tokio::time::timeout(SIGNAL_TIMEOUT, socket.recv_from(&mut buf)).await {
                    Ok(Ok((len, remote_addr))) => {
                        if let Some(
                            PacketOutcome::FrameProcessed(frame_data)
                            | PacketOutcome::IFrameReceived(frame_data),
                        ) =
                            handle_udp_packet(len, &buf, &remote_addr, &mut reassemblers, &socket)
                                .await
                        {
                            let running_time = std::time::Instant::now()
                                .saturating_duration_since(*stream_start_time);
                            push_frame_to_appsrc(appsrc, frame_data, running_time);
                        }
                    }
                    Err(_) => {
                        println!("[State] Signal lost. Switching back to Standby state...");
                        state.stop();
                        let standby_pipeline = create_standby_pipeline()?;
                        standby_pipeline.set_state(gst::State::Playing)?;
                        state = AppState::Standby {
                            pipeline: standby_pipeline,
                        };
                        // Clear the reassembler state to be ready for the next connection
                        reassemblers.clear();
                    }
                    _ => {}
                }
            }
        }
    }
}

struct FrameReassembler {
    packets: Vec<Option<Vec<u8>>>,
    received_count: u16,
    total_packets: u16,
    frame_id: u32,
}
impl FrameReassembler {
    fn new(header: &DataHeader) -> Self {
        Self {
            packets: vec![None; header.total_packets as usize],
            received_count: 0,
            total_packets: header.total_packets,
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

enum PacketOutcome {
    FrameProcessed(Vec<u8>),
    IFrameReceived(Vec<u8>),
}

async fn handle_udp_packet(
    len: usize,
    buf: &[u8],
    remote_addr: &SocketAddr,
    reassemblers: &mut HashMap<u32, FrameReassembler>,
    socket: &Arc<UdpSocket>,
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
            let is_key_frame = header.is_key_frame != 0;
            reassemblers.remove(&header.frame_id);

            if is_key_frame {
                let ack = AckPacket {
                    frame_id: header.frame_id,
                };
                let mut ack_buf = [0u8; 1 + ACK_PACKET_SIZE];
                ack_buf[0] = PacketType::Ack as u8;
                ack_buf[1..].copy_from_slice(&ack.to_bytes());
                let _ = socket.send_to(&ack_buf, remote_addr).await;
                return Some(PacketOutcome::IFrameReceived(complete_frame));
            } else {
                return Some(PacketOutcome::FrameProcessed(complete_frame));
            }
        }
    }
    None
}

fn push_frame_to_appsrc(
    appsrc: &gst_app::AppSrc,
    frame_data: Vec<u8>,
    running_time: std::time::Duration,
) {
    if frame_data.is_empty() {
        eprintln!("[ERROR] Trying to push a zero-length frame, skipping.");
        return;
    }
    let mut gst_buffer = gst::Buffer::with_size(frame_data.len()).unwrap();
    {
        let buffer_ref = gst_buffer.get_mut().unwrap();
        {
            let mut map = buffer_ref.map_writable().unwrap();
            map.copy_from_slice(&frame_data);
        }
        buffer_ref.set_pts(Some(gst::ClockTime::from_nseconds(
            running_time.as_nanos() as u64
        )));
    }
    if let Err(err) = appsrc.push_buffer(gst_buffer) {
        eprintln!("[ERROR] Failed to push buffer to appsrc: {}", err);
    }
}
