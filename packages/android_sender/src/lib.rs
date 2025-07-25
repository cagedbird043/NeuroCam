// --- packages/android_sender/src/lib.rs ---

use jni::objects::JClass;
use jni::JNIEnv;

// 声明我们将要使用的 logger 模块
mod logger;

#[no_mangle]
pub extern "system" fn Java_com_neurocam_MainActivity_initRust(mut _env: JNIEnv, _class: JClass) {
    // 现在我们可以安全、简洁地调用日志函数了
    logger::info("Refactoring complete. Unsafe code is now encapsulated in the logger module.");
    logger::warn("This is a warning from the new logger module.");
    logger::error("This is an error from the new logger module.");
}
