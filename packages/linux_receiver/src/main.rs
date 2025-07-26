// --- packages/linux_receiver/src/main.rs ---

// AI-MOD-START
use protocol::{AckPacket, DataHeader, PacketType, ACK_PACKET_SIZE, DATA_HEADER_SIZE};
use std::collections::{HashMap, VecDeque};
use std::io::{self};
use std::net::SocketAddr;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncWrite, AsyncWriteExt, BufWriter};
use tokio::net::UdpSocket;
use tokio::process::{Child, Command};

const LISTEN_ADDR: &str = "0.0.0.0:8080";
const MAX_DATAGRAM_SIZE: usize = 65_507;
const FRAME_TIMEOUT: Duration = Duration::from_secs(5);
const V4L2_DEVICE: &str = "/dev/video10";
const LATENCY_AVG_WINDOW: usize = 60; // 计算最近60帧的平均延迟

struct FrameReassembler {
    packets: Vec<Option<Vec<u8>>>,
    received_count: u16,
    total_packets: u16,
    last_seen: Instant,
    is_key_frame: bool,
    capture_timestamp_ns: u64, // 新增：存储帧的原始时间戳
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
    // ... add_packet 无变化
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

// ... spawn_ffmpeg 无变化
fn spawn_ffmpeg() -> io::Result<Child> {
    println!("[FFmpeg] Spawning ffmpeg to feed {}", V4L2_DEVICE);
    // AI-MOD-START
    Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            // --- 低延迟优化核心 ---
            "-fflags",
            "nobuffer", // 禁用 avformat 层的缓冲
            "-flags",
            "low_delay", // 提示 avcodec 层使用低延迟模式
            // --- 优化结束 ---
            "-probesize",
            "32",
            "-analyzeduration",
            "0",
            "-f",
            "h264",
            "-i",
            "pipe:0",
            "-f",
            "v4l2",
            "-pix_fmt",
            "yuv420p",
            V4L2_DEVICE,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    // AI-MOD-END
}

#[tokio::main]
async fn main() -> io::Result<()> {
    println!("[NeuroCam Linux Receiver - v8.1 LATENCY TEST]");
    println!("Starting UDP listener on {}...", LISTEN_ADDR);

    let mut ffmpeg_process = spawn_ffmpeg()?;
    let mut ffmpeg_stdin = BufWriter::new(
        ffmpeg_process
            .stdin
            .take()
            .expect("Failed to open ffmpeg stdin"),
    );
    if let Some(mut stderr) = ffmpeg_process.stderr.take() {
        tokio::spawn(async move {
            let mut reader = tokio::io::BufReader::new(&mut stderr);
            let mut line = String::new();
            while tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
                .await
                .is_ok()
            {
                if !line.is_empty() {
                    eprint!("[FFmpeg ERR] {}", line);
                    line.clear();
                } else {
                    break;
                }
            }
        });
    }

    let socket = Arc::new(UdpSocket::bind(LISTEN_ADDR).await?);
    println!(
        "[OK] Listening for NeuroCam stream. Outputting to {}",
        V4L2_DEVICE
    );

    let mut buf = vec![0u8; MAX_DATAGRAM_SIZE];
    let mut reassemblers: HashMap<u32, FrameReassembler> = HashMap::new();
    // 新增：用于计算移动平均延迟的队列
    let mut latency_history: VecDeque<f64> = VecDeque::with_capacity(LATENCY_AVG_WINDOW);

    loop {
        tokio::select! {
            result = socket.recv_from(&mut buf) => {
                match result {
                    Ok((len, remote_addr)) => {
                        handle_udp_packet(len, &buf, &remote_addr, &mut reassemblers, &mut ffmpeg_stdin, &socket, &mut latency_history).await;
                    }
                    Err(e) => { eprint!("[ERROR] Error receiving UDP packet: {}", e); break; }
                }
            },
            _ = ffmpeg_process.wait() => { eprint!("[ERROR] FFmpeg process exited unexpectedly. Shutting down."); break; }
        }
    }
    // ... shutdown 逻辑无变化
    println!("Shutting down... Closing ffmpeg pipe.");
    ffmpeg_stdin.shutdown().await?;
    println!("Waiting for ffmpeg process to terminate...");
    let _ = ffmpeg_process.wait().await;
    println!("NeuroCam Receiver has shut down cleanly.");
    Ok(())
}

async fn handle_udp_packet(
    len: usize,
    buf: &[u8],
    remote_addr: &SocketAddr,
    reassemblers: &mut HashMap<u32, FrameReassembler>,
    writer: &mut (impl AsyncWrite + Unpin),
    socket: &Arc<UdpSocket>,
    latency_history: &mut VecDeque<f64>, // 新增
) {
    if len > 0 {
        if let Ok(packet_type) = PacketType::try_from(buf[0]) {
            if packet_type == PacketType::Data {
                if let Some(header) = DataHeader::from_bytes(&buf[1..len]) {
                    let reassembler = reassemblers
                        .entry(header.frame_id)
                        .or_insert_with(|| FrameReassembler::new(&header));

                    let payload = buf[1 + DATA_HEADER_SIZE..len].to_vec();
                    if let Some(complete_frame) = reassembler.add_packet(header.packet_id, payload)
                    {
                        // --- 延迟计算核心逻辑 ---
                        let arrival_time_ns = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_nanos() as u64;
                        let latency_ns =
                            arrival_time_ns.saturating_sub(reassembler.capture_timestamp_ns);
                        let latency_ms = latency_ns as f64 / 1_000_000.0;

                        if latency_history.len() >= LATENCY_AVG_WINDOW {
                            latency_history.pop_front();
                        }
                        latency_history.push_back(latency_ms);
                        let avg_latency: f64 =
                            latency_history.iter().sum::<f64>() / latency_history.len() as f64;

                        println!(
                            "[LATENCY] Frame #{}: {:.2} ms (Avg over last {} frames: {:.2} ms)",
                            header.frame_id,
                            latency_ms,
                            latency_history.len(),
                            avg_latency
                        );
                        // --- 延迟计算结束 ---

                        if let Err(e) = writer.write_all(&complete_frame).await {
                            eprintln!("[ERROR] Failed to write to ffmpeg stdin: {}", e);
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
    }
    // ... 超时检查逻辑无变化
    reassemblers.retain(|_frame_id, reassembler| {
        if reassembler.last_seen.elapsed() > FRAME_TIMEOUT {
            if !reassembler.is_key_frame {
                let socket_for_task = Arc::clone(socket);
                let remote_addr_clone = *remote_addr;
                tokio::spawn(async move {
                    let request = [PacketType::IFrameRequest as u8];
                    let _ = socket_for_task.send_to(&request, remote_addr_clone).await;
                });
            }
            false
        } else {
            true
        }
    });
}
// AI-MOD-END
