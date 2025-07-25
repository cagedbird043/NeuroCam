// --- packages/android_sender/src/lib.rs ---

use jni::objects::{GlobalRef, JClass, JObject};
use jni::JNIEnv;
use std::sync::Once;

mod logger;

// 使用 Once 来确保我们的初始化代码只运行一次。
static INIT: Once = Once::new();
// 定义一个 static mut 变量来存储 Surface 的全局引用。
// Option<GlobalRef> 表示它可能为空，直到被初始化。
static mut SURFACE_REF: Option<GlobalRef> = None;

#[no_mangle]
pub extern "system" fn Java_com_neurocam_MainActivity_initRust(
    env: JNIEnv,
    _class: JClass,
    // 新增参数：从 Kotlin 传来的 Surface 对象
    surface: JObject,
) {
    // 使用 call_once 来执行一次性初始化
    INIT.call_once(|| {
        logger::info("Rust init function called for the first time.");

        // 将传入的 JObject (局部引用) 转换为 GlobalRef
        let global_surface = env
            .new_global_ref(surface)
            .expect("Failed to create a global reference from the Surface object.");

        logger::info("Successfully created a GlobalRef for the Surface object.");

        // unsafe 块是必需的，因为我们在修改一个 static mut 变量。
        // 由于 Once 的保护，这里的写操作是线程安全的。
        unsafe {
            SURFACE_REF = Some(global_surface);
        }

        logger::info("Surface object has been stored globally in Rust.");
    });

    // 即使 initRust 被多次调用，这条日志也会显示，但上面的初始化块只会执行一次。
    logger::info("initRust call finished. Initialization state is now set.");
}
