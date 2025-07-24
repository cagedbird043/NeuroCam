// --- packages/linux_receiver/src/main.rs

use std::fs::File;
use std::io::{self, BufWriter, Write};
use tokio::net::UdpSocket;

const LISTEN_ADDR: &str = "0.0.0.0:8080";
const MAX_DATAGRAM_SIZE: usize = 65_507;
const OUTPUT_FILENAME: &str = "output.mp4"; // 我们将接收到的码流保存到这个文件

#[tokio::main]
async fn main() -> io::Result<()> {
    println!("[NeuroCam Linux Receiver]");
    println!("Starting UDP listener on {}...", LISTEN_ADDR);
    println!(
        "Received H.264 stream will be saved to '{}'",
        OUTPUT_FILENAME
    );

    // 1. 绑定 UDP Socket
    let socket = UdpSocket::bind(LISTEN_ADDR).await?;
    println!("Successfully bound to {}", LISTEN_ADDR);

    // 2. 创建一个文件用于写入
    // File::create会创建一个新文件，如果文件已存在，则会清空其内容。
    let output_file = File::create(OUTPUT_FILENAME)?;

    // 3. 使用 BufWriter 提升写入性能
    // BufWriter 会在内存中创建一个缓冲区。数据先写入缓冲区，
    // 当缓冲区满或我们手动刷新时，才一次性写入磁盘。这比每次都直接写磁盘要快得多。
    let mut writer = BufWriter::new(output_file);

    // 4. 创建接收缓冲区
    let mut buf = vec![0u8; MAX_DATAGRAM_SIZE];

    // 5. 进入主接收循环
    loop {
        match socket.recv_from(&mut buf).await {
            Ok((len, remote_addr)) => {
                // 当成功接收到一个数据包
                println!("Received {} bytes from {}", len, remote_addr);

                // 将接收到的有效数据 (切片 buf[..len]) 写入 BufWriter
                if let Err(e) = writer.write_all(&buf[..len]) {
                    // 如果写入失败，打印错误并退出程序
                    eprintln!("Error writing to file: {}", e);
                    break;
                }
            }
            Err(e) => {
                // 当接收发生错误时
                eprintln!("Error receiving UDP packet: {}", e);
                break;
            }
        }
    }

    // 程序退出循环前（例如发生错误时），确保所有缓冲的数据都被写入磁盘。
    // writer.flush() 会执行这个操作。
    println!("Flushing buffer and shutting down...");
    writer.flush()?;

    Ok(())
}
