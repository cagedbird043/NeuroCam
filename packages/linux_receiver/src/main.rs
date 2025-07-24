// --- packages/linux_receiver/src/main.rs

use tokio::net::UdpSocket;

const LISTEN_ADDR: &str = "0.0.0.0:8080"; // 监听所有网络接口的 8080 端口
const MAX_DATAGRAM_SIZE: usize = 65_507; // UDP 数据报的最大理论大小

#[tokio::main]
async fn main() -> std::io::Result<()> {
    println!("[NeuroCam Linux Receiver]");
    println!("Starting UDP listener on {}...", LISTEN_ADDR);

    // 1. 绑定 UDP Socket
    // 我们使用 Tokio 提供的异步 UdpSocket
    let socket = UdpSocket::bind(LISTEN_ADDR).await?;
    println!("Successfully bound to {}", LISTEN_ADDR);

    // 2. 创建一个缓冲区来接收数据
    // 为了防止任何可能的溢出，我们使用理论上的最大值
    let mut buf = vec![0u8; MAX_DATAGRAM_SIZE];

    // 3. 进入主循环，等待接收数据
    loop {
        // `recv_from` 是一个异步方法。它会暂停（而不是阻塞线程）直到接收到数据。
        // 当数据到达时，`tokio` 运行时会唤醒这个任务。
        let (len, remote_addr) = socket.recv_from(&mut buf).await?;

        // 打印接收到的数据信息
        println!("Received {} bytes from {}", len, remote_addr);

        // 为了演示，我们也可以打印前16个字节的内容（如果存在）
        // 在实际视频流中，这里将是解码和写入 v4l2loopback 的逻辑
        let data_preview = &buf[..len.min(16)];
        println!("  Data preview: {:x?}", data_preview);
    }
}
