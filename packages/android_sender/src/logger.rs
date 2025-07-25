// --- packages/android_sender/src/logger.rs ---

use std::ffi::CString;
use std::os::raw::{c_char, c_int};

// 定义Android日志优先级的枚举
#[repr(i32)]
enum AndroidLogPriority {
    Info = 4,
    Warn = 5,
    Error = 6,
}

// 链接 liblog.so
#[link(name = "log")]
extern "C" {
    // 声明我们将调用的C函数
    fn __android_log_write(prio: c_int, tag: *const c_char, text: *const c_char) -> c_int;
}

// 这是一个私有的、不安全的辅助函数
unsafe fn log_write(prio: AndroidLogPriority, tag: &str, message: &str) {
    let tag = CString::new(tag).unwrap();
    let message = CString::new(message).unwrap();
    __android_log_write(prio as c_int, tag.as_ptr(), message.as_ptr());
}

// --- 以下是我们将暴露给其他模块的、安全的公共函数 ---

const LOG_TAG: &str = "rust";

pub fn info(message: &str) {
    unsafe { log_write(AndroidLogPriority::Info, LOG_TAG, message) }
}

pub fn warn(message: &str) {
    unsafe { log_write(AndroidLogPriority::Warn, LOG_TAG, message) }
}

pub fn error(message: &str) {
    unsafe { log_write(AndroidLogPriority::Error, LOG_TAG, message) }
}
