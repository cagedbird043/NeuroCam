// --- packages/linux_receiver/src/main.rs ---

// AI-MOD-START
use protocol::{PacketHeader, HEADER_SIZE};
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;

const LISTEN_ADDR: &str = "0.0.0.0:8080";
const MAX_DATAGRAM_SIZE: usize = 65_507;
const OUTPUT_FILENAME: &str = "output.h264";
const FRAME_TIMEOUT: Duration = Duration::from_secs(5);

/// 用于重组单个视频帧的结构体
struct FrameReassembler {
    packets: Vec<Option<Vec<u8>>>,
    received_count: u16,
    total_packets: u16,
    last_seen: Instant,
    is_key_frame: bool, // 新增：存储关键帧标记
}

impl FrameReassembler {
    fn new(total_packets: u16, is_key_frame: bool) -> Self {
        FrameReassembler {
            packets: vec![None; total_packets as usize],
            received_count: 0,
            total_packets,
            last_seen: Instant::now(),
            is_key_frame,
        }
    }

    /// 添加一个分片，如果帧已完成则返回完整的帧数据
    fn add_packet(&mut self, packet_id: u16, data: Vec<u8>) -> Option<Vec<u8>> {
        let id = packet_id as usize;
        if id < self.packets.len() && self.packets[id].is_none() {
            self.packets[id] = Some(data);
            self.received_count += 1;
        }
        self.last_seen = Instant::now();

        if self.received_count == self.total_packets {
            // 所有分片已集齐，拼接它们
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

#[tokio::main]
async fn main() -> io::Result<()> {
    println!("[NeuroCam Linux Receiver - v6.0 with I-Frame Detection]");
    println!("Starting UDP listener on {}...", LISTEN_ADDR);
    println!(
        "Reassembled H.264 stream will be saved to '{}'",
        OUTPUT_FILENAME
    );

    let socket = UdpSocket::bind(LISTEN_ADDR).await?;
    let output_file = File::create(OUTPUT_FILENAME)?;
    let mut writer = BufWriter::new(output_file);
    let mut buf = vec![0u8; MAX_DATAGRAM_SIZE];

    let mut reassemblers: HashMap<u32, FrameReassembler> = HashMap::new();

    loop {
        match socket.recv_from(&mut buf).await {
            Ok((len, _)) => {
                let Some(header) = PacketHeader::from_bytes(&buf[..len]) else {
                    eprintln!("Received a malformed packet (invalid header).");
                    continue;
                };

                let payload = buf[HEADER_SIZE..len].to_vec();
                let is_key_frame = header.is_key_frame != 0;

                let reassembler = reassemblers
                    .entry(header.frame_id)
                    .or_insert_with(|| FrameReassembler::new(header.total_packets, is_key_frame));

                if let Some(complete_frame) = reassembler.add_packet(header.packet_id, payload) {
                    // 核心诊断逻辑
                    if reassembler.is_key_frame {
                        println!("[KEY FRAME] Frame #{} reassembled successfully ({} bytes). Writing to file.", header.frame_id, complete_frame.len());
                    } else {
                        // 为了保持控制台清爽，可以注释掉非关键帧的日志
                        // println!("Frame #{} reassembled ({} bytes).", header.frame_id, complete_frame.len());
                    }

                    if let Err(e) = writer.write_all(&complete_frame) {
                        eprintln!("Error writing to file: {}", e);
                        break;
                    }
                    reassemblers.remove(&header.frame_id);
                }
            }
            Err(e) => {
                eprintln!("Error receiving UDP packet: {}", e);
                break;
            }
        }

        reassemblers.retain(|frame_id, reassembler| {
            if reassembler.last_seen.elapsed() > FRAME_TIMEOUT {
                let frame_type = if reassembler.is_key_frame {
                    "KEY FRAME"
                } else {
                    "Frame"
                };
                println!(
                    "[TIMEOUT] {} #{} timed out. Discarding {} of {} received packets.",
                    frame_type, frame_id, reassembler.received_count, reassembler.total_packets
                );
                false
            } else {
                true
            }
        });
    }

    println!("Flushing buffer and shutting down...");
    writer.flush()?;
    Ok(())
}
// AI-MOD-END
