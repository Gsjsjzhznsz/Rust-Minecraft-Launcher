fn main() {
    // Android cdylib：链接系统 libandroid（ANativeWindow）。libEGL 由运行期
    // dlopen（渲染层自带 EGL），不在此链接。
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("android") {
        println!("cargo:rustc-link-lib=dylib=android");
        println!("cargo:rustc-link-lib=dylib=log");
        println!("cargo:rustc-link-lib=dylib=dl");
    }
    // host-sim：链接 X11 + GL（EGL-on-GLX 仿真与 ANativeWindow 桩用）
    if !target.contains("android") && std::env::var("CARGO_FEATURE_HOST_SIM").is_ok() {
        // 沙箱无 -dev 软链，用本地 shim 目录指向 .so.N
        println!(
            "cargo:rustc-link-search=native={}",
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../sim/link-shims")
        );
        println!("cargo:rustc-link-lib=dylib=X11");
        println!("cargo:rustc-link-lib=dylib=GL");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
