// --- packages/linux_receiver/src/main.rs ---

// AI-MOD-START
use protocol::{AckPacket, DataHeader, PacketType, ACK_PACKET_SIZE, DATA_HEADER_SIZE};
use std::collections::HashMap;
use std::io::{self};
use std::net::SocketAddr;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncWrite, AsyncWriteExt, BufWriter};
use tokio::net::UdpSocket;
use tokio::process::{Child, Command};

const LISTEN_ADDR: &str = "0.0.0.0:8080";
const MAX_DATAGRAM_SIZE: usize = 65_507;
const FRAME_TIMEOUT: Duration = Duration::from_secs(5);
const V4L2_DEVICE: &str = "/dev/video10"; // V4L2 虚拟设备路径

struct FrameReassembler {
    packets: Vec<Option<Vec<u8>>>,
    received_count: u16,
    total_packets: u16,
    last_seen: Instant,
    is_key_frame: bool,
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

fn spawn_ffmpeg() -> io::Result<Child> {
    println!("[FFmpeg] Spawning ffmpeg to feed {}", V4L2_DEVICE);
    Command::new("ffmpeg")
        .args([
            "-hide_banner", // 隐藏冗余的启动信息
            "-loglevel",
            "error", // 只输出错误日志
            "-probesize",
            "32", // 快速启动
            "-analyzeduration",
            "0",
            "-f",
            "h264", // 输入格式为H.264码流
            "-i",
            "pipe:0", // 从标准输入读取数据
            "-f",
            "v4l2", // 输出格式为V4L2
            "-pix_fmt",
            "yuv420p", // V4L2设备常用的像素格式
            V4L2_DEVICE,
        ])
        .stdin(Stdio::piped()) // 创建一个可以写入的stdin管道
        .stdout(Stdio::null()) // 忽略ffmpeg的正常输出
        .stderr(Stdio::piped()) // 捕获ffmpeg的错误输出以便调试
        .spawn()
}

#[tokio::main]
async fn main() -> io::Result<()> {
    println!("[NeuroCam Linux Receiver - v8.0 FINAL]");
    println!("Starting UDP listener on {}...", LISTEN_ADDR);

    let mut ffmpeg_process = spawn_ffmpeg()?;
    let mut ffmpeg_stdin = BufWriter::new(
        ffmpeg_process
            .stdin
            .take()
            .expect("Failed to open ffmpeg stdin"),
    );
    // 捕获 ffmpeg 的错误输出并异步打印
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

    loop {
        // 使用 tokio::select 来同时监听 UDP 包和 ffmpeg 进程退出
        tokio::select! {
            result = socket.recv_from(&mut buf) => {
                match result {
                    Ok((len, remote_addr)) => {
                        handle_udp_packet(len, &buf, &remote_addr, &mut reassemblers, &mut ffmpeg_stdin, &socket).await;
                    }
                    Err(e) => {
                        eprintln!("[ERROR] Error receiving UDP packet: {}", e);
                        break;
                    }
                }
            },
            _ = ffmpeg_process.wait() => {
                eprintln!("[ERROR] FFmpeg process exited unexpectedly. Shutting down.");
                break;
            }
        }
    }

    println!("Shutting down... Closing ffmpeg pipe.");
    ffmpeg_stdin.shutdown().await?; // 确保所有缓冲数据都写入管道，并关闭它
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
    // AI-MOD-START
    // 核心修复 1: 函数签名现在正确地期望一个对 Arc<UdpSocket> 的引用。
    socket: &Arc<UdpSocket>,
    // AI-MOD-END
) {
    if len > 0 {
        if let Ok(packet_type) = PacketType::try_from(buf[0]) {
            if packet_type == PacketType::Data {
                if let Some(header) = DataHeader::from_bytes(&buf[1..len]) {
                    let payload = buf[1 + DATA_HEADER_SIZE..len].to_vec();
                    let is_key_frame = header.is_key_frame != 0;
                    let reassembler = reassemblers.entry(header.frame_id).or_insert_with(|| {
                        FrameReassembler::new(header.total_packets, is_key_frame)
                    });

                    if let Some(complete_frame) = reassembler.add_packet(header.packet_id, payload)
                    {
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

    // 超时检查逻辑
    reassemblers.retain(|_frame_id, reassembler| {
        if reassembler.last_seen.elapsed() > FRAME_TIMEOUT {
            if !reassembler.is_key_frame {
                // AI-MOD-START
                // 核心修复 2: 在需要创建异步任务时，才从外部的 Arc<UdpSocket> 克隆一个新的 Arc。
                // 这样每次 spawn 都会得到一个新的、自己的 Arc 副本，解决了所有权和 move 的问题。
                let socket_for_task = Arc::clone(socket);
                // AI-MOD-END
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
