fn main() {
    // Android cdylib：链接系统 libandroid（ANativeWindow）。libEGL 由运行期
    // dlopen（渲染层自带 EGL），不在此链接。
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("android") {
        println!("cargo:rustc-link-lib=dylib=android");
        println!("cargo:rustc-link-lib=dylib=log");
        println!("cargo:rustc-link-lib=dylib=dl");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
