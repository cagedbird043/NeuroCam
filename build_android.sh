#!/bin/bash

# --- build_android.sh (Corrected Version) ---

# 如果任何命令执行失败，脚本会立即退出
set -e

echo "--- [build_android.sh] Starting Rust build for Android ---"

# 这是正确的 cargo-ndk 命令，--package 参数在 build 子命令之后
cargo ndk \
    -o packages/android_sender/app/build/rustJniLibs/lib \
    build --package android_sender --release

echo "--- [build_android.sh] Rust build finished successfully ---"