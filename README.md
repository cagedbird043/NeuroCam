# NeuroCam

**Transform your phone into a high-performance AI vision sensor for Linux.**

<!-- [//]: # "这里可以放一个GIF动图，演示手机画面在Linux上被YOLO识别的效果" -->

coming soon...

NeuroCam is a high-performance, low-latency solution that streams your Android device's camera to a Linux machine, presenting it as a standard V4L2 video device (`/dev/videoX`). This allows any standard video application on Linux (like OpenCV, GStreamer, FFmpeg, or your AI models) to use your phone's camera as a direct input, without any code modification.

## Features

- **High Performance**: Utilizes native code (Rust) on both Android and Linux for maximum throughput and minimal overhead.
- **Low Latency**: Optimized for real-time applications like AI inference and remote operation over local networks (UDP).
- **Hardware Acceleration**: Leverages hardware encoding on Android and hardware decoding on Linux.
- **Seamless Integration**: Creates a virtual V4L2 device on Linux, making your phone camera appear as a standard webcam.
- **Cross-Platform**: Rust core logic with minimal platform-specific wrappers.

## Architecture

coming soon...

## Getting Started

### Prerequisites

- **Linux**: Kernel with `v4l2loopback` module, Rust toolchain, FFmpeg libraries.
- **Android**: Android device with developer mode enabled.

### Installation & Usage

coming soon...

1. **Linux Receiver Setup**
   ```bash
   # ...
   ```
2. **Android Sender Setup**
   ```bash
   # ...
   ```

## License

This project is licensed under the **MIT License**.
