// --- packages/linux_receiver/src/main.rs ---

// AI-MOD-START
use protocol::{AckPacket, DataHeader, PacketType, ACK_PACKET_SIZE, DATA_HEADER_SIZE};
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;

const LISTEN_ADDR: &str = "0.0.0.0:8080";
const MAX_DATAGRAM_SIZE: usize = 65_507;
const OUTPUT_FILENAME: &str = "output.h264";
const FRAME_TIMEOUT: Duration = Duration::from_secs(5);

struct FrameReassembler {
    packets: Vec<Option<Vec<u8>>>,
    received_count: u16,
    total_packets: u16,
    last_seen: Instant,
    is_key_frame: bool,
}

impl FrameReassembler {
    // ... (此部分无变化，保持原样)
    fn new(total_packets: u16, is_key_frame: bool) -> Self {
        FrameReassembler {
            packets: vec![None; total_packets as usize],
            received_count: 0,
            total_packets,
            last_seen: Instant::now(),
            is_key_frame,
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

#[tokio::main]
async fn main() -> io::Result<()> {
    // AI-MOD-START
    println!("[NeuroCam Linux Receiver - v7.0 with Active Error Recovery]");
    println!("Starting UDP listener on {}...", LISTEN_ADDR);
    println!(
        "Reassembled H.264 stream will be saved to '{}'",
        OUTPUT_FILENAME
    );

    // 将 socket 包装在 Arc 中，以便在异步任务间安全共享
    let socket = Arc::new(UdpSocket::bind(LISTEN_ADDR).await?);
    let output_file = File::create(OUTPUT_FILENAME)?;
    let mut writer = BufWriter::new(output_file);
    let mut buf = vec![0u8; MAX_DATAGRAM_SIZE];
    let mut reassemblers: HashMap<u32, FrameReassembler> = HashMap::new();

    loop {
        match socket.recv_from(&mut buf).await {
            Ok((len, remote_addr)) => {
                // 收到包之后，立即处理它
                if len > 0 {
                    if let Ok(packet_type) = PacketType::try_from(buf[0]) {
                        match packet_type {
                            PacketType::Data => {
                                handle_data_packet(
                                    &buf[1..len],
                                    &mut reassemblers,
                                    &mut writer,
                                    &socket,
                                    &remote_addr,
                                )
                                .await;
                            }
                            _ => { /* Receiver ignores other packet types */ }
                        }
                    } else {
                        eprintln!("Received packet with unknown type: {}", buf[0]);
                    }
                }

                // --- 超时检查逻辑 ---
                // 在处理完当前包后，顺便检查所有帧的超时情况
                let socket_for_timeout = Arc::clone(&socket);
                reassemblers.retain(|frame_id, reassembler| {
                    if reassembler.last_seen.elapsed() > FRAME_TIMEOUT {
                        let frame_type = if reassembler.is_key_frame {
                            "KEY FRAME"
                        } else {
                            "Frame"
                        };
                        println!(
                            "[TIMEOUT] {} #{} timed out. Discarding {} of {} received packets.",
                            frame_type,
                            frame_id,
                            reassembler.received_count,
                            reassembler.total_packets
                        );

                        if !reassembler.is_key_frame {
                            // 修复 'move' 错误：为异步任务克隆 Arc
                            let socket_for_task = Arc::clone(&socket_for_timeout);
                            println!("[RECOVERY] Requesting a new I-Frame due to lost P/B-frame.");
                            tokio::spawn(async move {
                                let request = [PacketType::IFrameRequest as u8];
                                if let Err(e) = socket_for_task.send_to(&request, remote_addr).await
                                {
                                    eprintln!("[ERROR] Failed to send I-Frame Request: {}", e);
                                }
                            });
                        }
                        false // 从 reassemblers 中移除超时的条目
                    } else {
                        true // 保留未超时的条目
                    }
                });
            }
            Err(e) => {
                eprintln!("Error receiving UDP packet: {}", e);
                break;
            }
        }
    }

    println!("Flushing buffer and shutting down...");
    writer.flush()?;
    Ok(())
    // AI-MOD-END
}

async fn handle_data_packet(
    data: &[u8],
    reassemblers: &mut HashMap<u32, FrameReassembler>,
    writer: &mut BufWriter<File>,
    socket: &UdpSocket,
    remote_addr: &SocketAddr,
) {
    let Some(header) = DataHeader::from_bytes(data) else {
        eprintln!("Received a malformed data packet (invalid header).");
        return;
    };

    let payload = data[DATA_HEADER_SIZE..].to_vec();
    let is_key_frame = header.is_key_frame != 0;

    let reassembler = reassemblers
        .entry(header.frame_id)
        .or_insert_with(|| FrameReassembler::new(header.total_packets, is_key_frame));

    if let Some(complete_frame) = reassembler.add_packet(header.packet_id, payload) {
        if reassembler.is_key_frame {
            println!(
                "[KEY FRAME] Frame #{} reassembled successfully. Sending ACK.",
                header.frame_id
            );
            let ack = AckPacket {
                frame_id: header.frame_id,
            };
            let mut ack_buf = [0u8; 1 + ACK_PACKET_SIZE];
            ack_buf[0] = PacketType::Ack as u8;
            ack_buf[1..].copy_from_slice(&ack.to_bytes());
            if let Err(e) = socket.send_to(&ack_buf, remote_addr).await {
                eprintln!(
                    "[ERROR] Failed to send ACK for frame #{}: {}",
                    header.frame_id, e
                );
            }
        }

        if let Err(e) = writer.write_all(&complete_frame) {
            eprintln!("Error writing to file: {}", e);
        }
        reassemblers.remove(&header.frame_id);
    }
}
// AI-MOD-END
