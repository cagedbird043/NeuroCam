# NeuroCam

**让你的手机秒变高性能 AI 视觉传感器，极致低延迟、全链路自愈、工业级协议健壮性！**  
**Turn your phone into a high-performance AI vision sensor: ultra-low latency, full-link self-healing, industrial-grade protocol robustness!**

> 手机摄像头 → 安卓硬编 → Rust UDP 协议 → Linux 虚拟摄像头（/dev/videoX）→ 任意 AI/视频/机器人应用  
> Android camera → hardware H.264 → Rust UDP protocol → Linux virtual camera (/dev/videoX) → Any AI/video/robotics app

---

## 最新特性 / Latest Features (2025.07)

- **全链路防御性编程**：无论安卓端、Linux 端谁先启动、何时重启、转屏、断网，系统都能自动恢复、秒级自愈。  
  **Full-link defensive programming:** Auto-recovery and self-healing no matter which side restarts, rotates, or disconnects.
- **极低延迟**：端到端延迟可低至 50ms，内网 4K/8K 流畅推送。  
  **Ultra-low latency:** End-to-end latency as low as 50ms, smooth 4K/8K streaming in LAN.
- **协议健壮性**：SPS/PPS 专用通道、I 帧握手、心跳兜底、只在参数集变化时重启解码器，彻底消除花屏/死锁。  
  **Protocol robustness:** Dedicated SPS/PPS channel, I-frame handshake, heartbeat fallback, pipeline reset only on parameter change.
- **自动适配 AI/嵌入式/机器人**：虚拟摄像头即插即用，OpenCV、YOLO、ffplay、浏览器、VLC 等全兼容。  
  **Plug-and-play for AI/embedded/robotics:** Virtual camera works with OpenCV, YOLO, ffplay, browsers, VLC, etc.
- **高性能 Rust 核心**：安卓/服务端全用 Rust 实现，极致性能、极低资源占用。  
  **High-performance Rust core:** Rust on both Android and Linux for maximum efficiency.
- **可扩展协议**：支持 FEC、NACK、ACK 窗口、丢包统计、加密等高级特性。  
  **Extensible protocol:** FEC, NACK, ACK window, loss statistics, encryption, and more.

---

## 主要特性 / Key Features

- **高性能 / High Performance**：Rust 原生代码，安卓与 Linux 双端极致吞吐。
- **极低延迟 / Ultra Low Latency**：实时流传输，局域网下 <50ms。
- **自愈能力 / Self-Healing**：任意端重启、网络抖动、参数变化均可自动恢复。
- **硬件加速 / Hardware Acceleration**：安卓硬编，Linux 软/硬解码。
- **无缝集成 / Seamless Integration**：标准 V4L2 设备，任意 Linux 应用即插即用。
- **协议健壮 / Protocol Robustness**：SPS/PPS 握手、I 帧请求、心跳兜底、防御性 pipeline 重启。
- **跨平台 / Cross-Platform**：Rust 核心，极少平台相关代码。

---

## 架构图 / Architecture

```
┌────────────┐   H.264+SPS/PPS+协议   ┌──────────────┐   虚拟摄像头   ┌──────────────┐
│ Android Cam│ ────────────────▶ │ Linux Receiver│ ─────────────▶ │ AI/Video App │
└────────────┘                   └──────────────┘                └──────────────┘
```

1. **Android 端 / Android Side**：采集摄像头，硬编 H.264，Rust 协议推流，SPS/PPS/I 帧专用包，心跳兜底。  
   Capture camera, hardware H.264 encode, Rust protocol streaming, dedicated SPS/PPS/I-frame packets, heartbeat fallback.
2. **Linux 端 / Linux Side**：Rust 协议收流，参数集变化智能重启 GStreamer pipeline，推送到 v4l2loopback 虚拟摄像头。  
   Rust protocol receiver, smart pipeline reset on parameter change, push to v4l2loopback virtual camera.
3. **下游应用 / Downstream Apps**：OpenCV、YOLO、ffplay、浏览器、VLC 等即插即用。  
   Plug-and-play for OpenCV, YOLO, ffplay, browsers, VLC, etc.

---

## 快速上手 / Getting Started

### 依赖 / Prerequisites

- **Linux**: 内核支持 `v4l2loopback`，Rust 工具链，FFmpeg，GStreamer。
- **Android**: 支持 Camera2 API 的安卓设备，已开启开发者模式。

---

### 安装与运行 / Installation & Usage

#### 1. Linux 端 / Linux Receiver

```bash
# 安装依赖
sudo apt install v4l2loopback-dkms ffmpeg gstreamer1.0-tools
# 加载虚拟摄像头
sudo modprobe v4l2loopback devices=1 video_nr=10 card_label="NeuroCam"
# 编译并运行
cd packages/linux_receiver
cargo run --release
```

#### 2. 安卓端 / Android Sender

- 用 Android Studio 编译并安装 `packages/android_sender` 到手机。
- 打开 App，授权摄像头和网络权限。
- 手机和 Linux 在同一局域网即可自动发现。

---

## 常见问题 / FAQ

- **Q: 支持哪些下游应用？ / What apps are supported?**  
  A: 只要能用 V4L2 摄像头的都支持，如 OpenCV、YOLO、ffplay、浏览器、VLC 等。  
  Any app supporting V4L2 cameras: OpenCV, YOLO, ffplay, browsers, VLC, etc.

- **Q: 支持多路流/多分辨率/参数热切换吗？ / Multi-stream, multi-res, hot parameter change?**  
  A: 支持，参数变化自动重启解码器，协议自愈。  
  Yes, auto pipeline reset and protocol self-healing on parameter change.

- **Q: 延迟能做到多低？ / How low is the latency?**  
  A: 内网下端到端延迟可低至 50ms，取决于编码器和解码器性能。  
  End-to-end latency as low as 50ms in LAN, depends on codec performance.

- **Q: 丢包/断网/重启会花屏吗？ / Will packet loss/disconnect cause artifacts?**  
  A: 不会，协议层全链路自愈，自动恢复。  
  No, protocol self-heals and auto-recovers from loss/disconnect.

---

## 许可证 / License

MIT License

---

**欢迎反馈与贡献！如需协议扩展、FEC、NACK、加密等高级特性，欢迎 issue 或 PR！**  
**Feedback and contributions welcome! For protocol extensions, FEC, NACK, encryption, etc., feel free to open issues or PRs!**
