// --- packages/standby_feeder/src/main.rs

use anyhow::{Context, Result};
use protocol::{DataHeader, PacketType, DATA_HEADER_SIZE};
use std::fs::File;
use std::time::{Duration, SystemTime};
use tokio::net::UdpSocket;

const LOOPBACK_ADDR: &str = "127.0.0.1:8080";
const HEARTBEAT_FILE: &str = "/tmp/neurocam.heartbeat";
const STANDBY_FRAME_PATH: &str = "packages/linux_receiver/standby_frame.h264"; // 和主进程用同一个
const NETWORK_TIMEOUT: Duration = Duration::from_secs(2);
const STANDBY_FEED_INTERVAL: Duration = Duration::from_millis(500);
const MAX_PAYLOAD_SIZE: usize = 1400;

#[tokio::main]
async fn main() -> Result<()> {
    println!("[FEEDER] Standby Feeder process started.");

    // 1. 加载待机帧
    let standby_frame_data = std::fs::read(STANDBY_FRAME_PATH)
        .with_context(|| format!("Failed to load standby frame: {}", STANDBY_FRAME_PATH))?;
    println!(
        "[FEEDER] Standby frame loaded ({} bytes).",
        standby_frame_data.len()
    );

    // 2. 准备 UDP socket
    let sender_socket = UdpSocket::bind("0.0.0.0:0").await?;
    println!(
        "[FEEDER] Ready to send standby frames to {}.",
        LOOPBACK_ADDR
    );

    loop {
        // 3. 检查心跳文件
        let is_timed_out = match File::open(HEARTBEAT_FILE) {
            Ok(file) => {
                let metadata = file.metadata()?;
                let modified_time = metadata.modified()?;
                // 如果文件最后修改时间距现在超过了阈值，则超时
                SystemTime::now().duration_since(modified_time)? > NETWORK_TIMEOUT
            }
            Err(_) => {
                // 如果文件不存在，也认为是超时
                true
            }
        };

        if is_timed_out {
            println!("[FEEDER] Timeout detected. Sending fragmented standby frame...");

            // 4. 对待机帧进行分片和发送
            let chunks: Vec<&[u8]> = standby_frame_data.chunks(MAX_PAYLOAD_SIZE).collect();
            let total_packets = chunks.len() as u16;

            for (i, chunk) in chunks.iter().enumerate() {
                let header = DataHeader {
                    frame_id: 0,
                    capture_timestamp_ns: 0,
                    packet_id: i as u16,
                    total_packets,
                    is_key_frame: 1,
                };
                let mut packet_data = Vec::with_capacity(1 + DATA_HEADER_SIZE + chunk.len());
                packet_data.push(PacketType::Data as u8);
                packet_data.extend_from_slice(&header.to_bytes());
                packet_data.extend_from_slice(chunk);

                if let Err(e) = sender_socket.send_to(&packet_data, LOOPBACK_ADDR).await {
                    eprintln!("[FEEDER] Error sending standby packet: {}", e);
                }
            }
        }

        tokio::time::sleep(STANDBY_FEED_INTERVAL).await;
    }
}
