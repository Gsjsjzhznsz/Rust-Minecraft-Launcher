//! ZL2 移植（v0.2.18）：linker namespace 旁路 —— zink 的 Vulkan 通道。
//!
//! 源码对照：ZalithLauncher2 `ZalithLauncher/src/main/jni/driver_helper/nsbypass.c`
//! + `driver_helper.c`（loadTurnipVulkan），FCLauncher 同源。这是 Pojav 系
//! zink 在骁龙上真正跑通的根本机制，Quartz v0.2.15~17 的三条死路恰好全部
//! 被它绕开：
//!
//! 1. **clns 公共库白名单**：turnip 的 DT_NEEDED `libhardware.so` 在
//!    classloader namespace 里「not found」（非 public 库，libnativeloader
//!    拦截）—— driver_namespace 是 `__loader_android_create_namespace` 建的
//!    裸命名空间，不经 libnativeloader 过滤，搜索路径含 `/system/lib64`，
//!    按名解析直接命中 libhardware.so 等全部系统库。
//! 2. **sphal permitted 限制**：`android_load_sphal_library` 的 sphal
//!    命名空间 permitted 路径不含应用数据目录，/data 下的 turnip 永远
//!    「not accessible」—— driver_namespace 的 permitted 明确含 `/data/`，
//!    turnip 与驱动包按绝对路径/相对 SONAME 都可载入。
//! 3. **系统 loader SIGSEGV**：v0.2.13~17 的 loader 探针用的是进程内既有
//!    `libvulkan.so` 实例（clns），ICD 发现落空后 vkCreateInstance 崩溃；
//!    ZL2 的做法是把 **DT_SONAME 改写成唯一值的全新 libvulkan.so 实例**
//!    装进 driver_namespace（绕过 bionic 按 SONAME 去重，强制新 soinfo），
//!    该实例在自己的命名空间内完成 ICD 发现，与既有实例零冲突。
//!
//! 链路（与 ZL2 loadTurnipVulkan 同构；linkerhook 环节以「ICD 先行落位 +
//! VK_ICD_FILENAMES 绝对路径 JSON」等效替代）：
//!
//! ```text
//! ① 裸 __loader_dlopen（dlsym 优先，ZL2 PLT 走位兑底，caller=libdl 内地址）
//! ② raw loader 载 ld-android.so → dlsym __loader_android_create_namespace /
//!    __loader_android_link_namespaces
//! ③ 建 driver_namespace（type=ISOLATED|SHARED=3，搜索 /system/lib64:驱动目录，
//!    permitted /system/:/data/:/vendor/:/apex/:/system_ext —— ZL2 原值）
//! ④ link ld-android/libnativeloader(_lazy)（ZL2 原值，逐项 best-effort）
//! ⑤ android_dlopen_ext + USE_NAMESPACE：turnip libvulkan_freedreno.so 先进命名空间
//! ⑥ 读 /system/lib64/libvulkan.so → DT_SONAME 打唯一前缀 → 落盘 runtime_dir →
//!    同通道载入（ZL2 linker_ns_dlopen_unique 的 fd 通道以绝对路径等效）
//! ⑦ 返回该实例句柄 → VULKAN_PTR —— zink（IronizedZink 消费端）dlsym
//!    vkGetInstanceProcAddr，vkCreateInstance 时该实例在命名空间内命中已
//!    就位的 turnip ICD（freedreno KGSL 免 root 直通 /dev/kgsl-3d0）
//! ```
//!
//! v0.2.19（MediaTek 小米系统 libvulkan 包装层兼容 + RimDroid vkshim 救场
//! 链全量移植，源码对照 `udarmolota/RimDroid` unity/src/main/cpp/vkshim/
//! driver.c + app/src/main/cpp/liblinkernsbypass/android_linker_ns.cpp）：
//! 部分 ROM（实测小米天玑机型）的 /system/lib64/libvulkan.so 是包装层 ——
//! DT_NEEDED 直接挂着 libgpud_sys.so（本体在 /system_ext/lib64，普通 app
//! 经系统默认命名空间天然可见；裸命名空间首载即报
//! `library "libgpud_sys.so" not found`，RimDroid 注释原文："The loader
//! too can need vendor libraries (libgpud_sys.so on a MediaTek Xiaomi)"）。
//! 三层对策：
//! 1. 命名空间搜索路径补全：/system_ext/lib64(+ndk)、system/vendor/odm
//!    的 ndk 子目录 —— permitted 本就含 /system_ext，只差搜索；
//! 2. 救场第一级（RimDroid 同款）：建 default 命名空间的 SHARED 副本
//!    （parent=null + caller=libdl 实体地址），`__loader_android_link_
//!    namespaces_all_libs` 全量连回（符号缺失时按 dlerror 解析缺失
//!    SONAME 定向 `__loader_android_link_namespaces` 补位）后重试；
//! 3. 救场第二级：`__loader_android_get_exported_namespace("sphal")`
//!    同法连回再重试 —— 与 RimDroid vkshim_open_custom_loader 的三级
//!    ladder 同构。

#![allow(dead_code)]

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicU16, Ordering};

/// caller 地址锚点：必须落在本 so 的地址空间内（ZL2 用
/// `__builtin_return_address(0)` 的本库地址作等效物 —— linker 以 caller
/// 地址判定归属命名空间，driver_namespace 创建依赖它）。
static ZL2_CALLER_MARKER: u8 = 0;

/// bionic android/dlext.h：android_dlextinfo（64 位布局 flags/reserved_addr/
/// reserved_size/library_fd(+pad)/library_fd_offset/library_namespace）。
#[repr(C)]
struct AndroidDlextinfo {
    flags: u64,
    reserved_addr: *mut c_void,
    reserved_size: usize,
    library_fd: c_int,
    library_fd_offset: i64,
    library_namespace: *mut c_void,
}

/// ANDROID_DLEXT_USE_NAMESPACE（bionic dlext.h 固定值 0x200）。
const ANDROID_DLEXT_USE_NAMESPACE: u64 = 0x200;
const RTLD_LAZY: c_int = 1;
const RTLD_NOW: c_int = 2;
const RTLD_LOCAL: c_int = 0;

/// `__loader_dlopen(filename, flags, caller_addr)`（ld-android.so 导出的裸
/// loader 入口；caller 地址显式传入，是整个旁路的地基）。
type RawDlopenFn = unsafe extern "C" fn(*const c_char, c_int, *const c_void) -> *mut c_void;
/// `__loader_android_create_namespace(name, ld_library_path, default_library_path,
/// type, permitted_when_isolated_path, parent, caller_addr)`。
type CreateNamespaceFn = unsafe extern "C" fn(
    *const c_char,
    *const c_char,
    *const c_char,
    u64,
    *const c_char,
    *mut c_void,
    *const c_void,
) -> *mut c_void;
/// `__loader_android_link_namespaces(from, to, shared_libs_sonames)`。
type LinkNamespacesFn = unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_char) -> c_int;
/// `android_dlopen_ext(filename, flags, extinfo)`（libdl 公共 API）。
type AndroidDlopenExtFn =
    unsafe extern "C" fn(*const c_char, c_int, *const AndroidDlextinfo) -> *mut c_void;

#[cfg(target_os = "android")]
unsafe extern "C" {
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlerror() -> *const c_char;
}

#[cfg(target_os = "android")]
mod ffi {
    use super::*;

    unsafe fn dlerr_text() -> String {
        let p = dlerror();
        if p.is_null() {
            "（无 dlerror 详情）".to_string()
        } else {
            CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    }

    /// dlopen 符号实体的真实地址（Rust 取 extern fn 地址经 GOT 解析 =
    /// libdl.so 内函数体；ZL2 传 `&dlopen` 的等效物）。该地址同时用作裸
    /// loader 调用的 caller_addr —— linker 把它归到系统默认命名空间，
    /// 于是 `ld-android.so` 可按名解析（应用 clns 里它不可见）。
    fn dlopen_body_addr() -> Result<usize, String> {
        type DlopenTwo = unsafe extern "C" fn(*const c_char, c_int) -> *mut c_void;
        let f: DlopenTwo = dlopen;
        Ok(unsafe { std::mem::transmute::<DlopenTwo, usize>(f) })
    }

    /// AArch64：从 start 起向后扫第一条 BL，返回符号扩展后的目标地址。
    /// ZL2 find_branch_label 原版用无符号算术（默认目标在前方）；跨 DSO 的
    /// BL 目标可能在后方（PLT 通常位于段首），这里按 imm26 符号扩展处理，
    /// 正负两向都正确。只读扫描（.text 页本就 R+X，不做 mprotect）。
    #[cfg(target_arch = "aarch64")]
    fn find_first_bl_target(start: usize) -> Option<usize> {
        const OP_MASK: u32 = 0xFC00_0000;
        const BL_OP: u32 = 0x9400_0000;
        let mut p = start as *const u32;
        for _ in 0..8192 {
            let ins = unsafe { p.read() };
            if ins & OP_MASK == BL_OP {
                // imm26 符号扩展（<<6 到位 31 再算术右移 6），×4 为字节距离
                let delta = ((((ins as i32) << 6) >> 6) as isize) * 4;
                let tgt = (p as usize).wrapping_add(delta as usize);
                // 目标须在 ±256MB 内（同名库内/相邻系统库），否则视为误扫
                if tgt.abs_diff(start) <= 256 * 1024 * 1024 {
                    return Some(tgt);
                }
            }
            p = unsafe { p.add(1) };
        }
        None
    }

    /// 非 aarch64 编译兜底（模块实际只在 Android arm64 使用）。
    #[cfg(not(target_arch = "aarch64"))]
    fn find_first_bl_target(_start: usize) -> Option<usize> {
        None
    }

    /// 裸 `__loader_dlopen` 解析：优先 dlsym（:game 是 exec 出的进程，应用库
    /// 落在默认命名空间，ld-android.so 在全局组内可见）；解析不到走 ZL2 的
    /// PLT 走位。两种路径下 caller_addr 都是 libdl.so 内的 dlopen 实体地址。
    unsafe fn resolve_raw_dlopen() -> Result<(RawDlopenFn, usize), String> {
        let dl_body = dlopen_body_addr()?;
        let sym = CString::new("__loader_dlopen").unwrap();
        let p = dlsym(ptr::null_mut(), sym.as_ptr());
        if !p.is_null() {
            let f: RawDlopenFn = std::mem::transmute(p);
            return Ok((f, dl_body));
        }
        let target = find_first_bl_target(dl_body).ok_or_else(|| {
            "stage=zl2-rawdl:dlsym 失败且 PLT 走位未找到 BL（非 AArch64？）".to_string()
        })?;
        let f: RawDlopenFn = std::mem::transmute(target);
        Ok((f, dl_body))
    }

    /// namespace 上下文：句柄 + linker API 指针缓存（救场链复用）。
    struct NsCtx {
        ns: *mut c_void,
        dlopen_ext: AndroidDlopenExtFn,
        create: CreateNamespaceFn,
        link: LinkNamespacesFn,
        /// driver_namespace 的 caller（本 so 地址 → 归属 clns，ZL2 语义）
        ns_caller: *const c_void,
        /// libdl.so 内 dlopen 实体地址 —— 建「default 命名空间可用副本」的
        /// caller（RimDroid android_create_namespace_escape 的 &dlopen 同款：
        /// parent=null 时新建 ns 以 caller 归属的命名空间为父，libdl 落在
        /// default → 得到 default 的 SHARED 副本）。
        default_caller: usize,
    }

    /// __loader_ 系列 linker API 宽容解析：全局组直查 → libdl_android.so →
    /// ld-android.so（三者都是公共 LLNDK，clns 内可直接 dlopen）。
    unsafe fn dlsym_any(name: &str) -> *mut c_void {
        let c = CString::new(name).unwrap();
        let p = dlsym(ptr::null_mut(), c.as_ptr());
        if !p.is_null() {
            return p;
        }
        for lib in ["libdl_android.so", "ld-android.so"] {
            let lc = CString::new(lib).unwrap();
            let h = dlopen(lc.as_ptr(), RTLD_LAZY | RTLD_LOCAL);
            if !h.is_null() {
                let p = dlsym(h, c.as_ptr());
                if !p.is_null() {
                    return p;
                }
            }
        }
        ptr::null_mut()
    }

    /// 建 driver_namespace 并接好 link（ZL2 nsbypass.c linker_ns_load 全量移植）。
    unsafe fn ns_load(search: &str) -> Result<NsCtx, String> {
        let (raw_dlopen, caller_addr) = resolve_raw_dlopen()?;

        let ld_c = CString::new("ld-android.so").unwrap();
        let ld_android = raw_dlopen(ld_c.as_ptr(), RTLD_LAZY, caller_addr as *const c_void);
        if ld_android.is_null() {
            return Err(format!("stage=zl2-ld-android:{}", dlerr_text()));
        }
        let create_c = CString::new("__loader_android_create_namespace").unwrap();
        let create_p = dlsym(ld_android, create_c.as_ptr());
        let link_c = CString::new("__loader_android_link_namespaces").unwrap();
        let link_p = dlsym(ld_android, link_c.as_ptr());
        if create_p.is_null() || link_p.is_null() {
            return Err(
                "stage=zl2-ns-api:__loader_android_create_namespace/link_namespaces 符号缺失"
                    .to_string(),
            );
        }
        let create: CreateNamespaceFn = std::mem::transmute(create_p);
        let link: LinkNamespacesFn = std::mem::transmute(link_p);

        // ZL2 原值：type=3（ISOLATED|SHARED），permitted 五前缀
        let name_c = CString::new("qz-driver-ns").unwrap();
        let search_c = CString::new(search.to_string()).unwrap();
        let permitted_c = CString::new("/system/:/data/:/vendor/:/apex/:/system_ext").unwrap();
        let caller = &ZL2_CALLER_MARKER as *const u8 as *const c_void;
        let ns = create(
            name_c.as_ptr(),
            search_c.as_ptr(),
            search_c.as_ptr(),
            3,
            permitted_c.as_ptr(),
            ptr::null_mut(),
            caller,
        );
        if ns.is_null() {
            return Err(format!("stage=zl2-create-ns:{}", dlerr_text()));
        }
        // ZL2 原值三项，逐项 best-effort（失败不阻断 —— 符号解析兜底走搜索路径）
        for soname in [
            "ld-android.so",
            "libnativeloader.so",
            "libnativeloader_lazy.so",
        ] {
            let c = CString::new(soname).unwrap();
            let _ = link(ns, ptr::null_mut(), c.as_ptr());
        }
        // android_dlopen_ext（公共 API；RTLD_DEFAULT 解析不到时 dlopen libdl 兜底）
        let ext_c = CString::new("android_dlopen_ext").unwrap();
        let mut ext_p = dlsym(ptr::null_mut(), ext_c.as_ptr());
        if ext_p.is_null() {
            let c = CString::new("libdl.so").unwrap();
            let h = dlopen(c.as_ptr(), RTLD_LAZY | RTLD_LOCAL);
            if !h.is_null() {
                ext_p = dlsym(h, ext_c.as_ptr());
            }
        }
        if ext_p.is_null() {
            return Err("stage=zl2-dlopen-ext:android_dlopen_ext 符号缺失".to_string());
        }
        let dlopen_ext: AndroidDlopenExtFn = std::mem::transmute(ext_p);
        Ok(NsCtx {
            ns,
            dlopen_ext,
            create,
            link,
            ns_caller: caller,
            default_caller: caller_addr,
        })
    }

    /// `__loader_android_link_namespaces_all_libs(from, to)`：把 to 的全部库
    /// 接进 from（bionic API；旧 ROM 可能没有 —— 调用方须判空）。
    type LinkAllLibsFn = unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int;
    unsafe fn link_all_libs_fn() -> Option<LinkAllLibsFn> {
        let p = dlsym_any("__loader_android_link_namespaces_all_libs");
        if p.is_null() {
            None
        } else {
            Some(std::mem::transmute::<*mut c_void, LinkAllLibsFn>(p))
        }
    }

    /// `__loader_android_get_exported_namespace(name)`：取 sphal 等导出
    /// 命名空间（厂商库提供方）。
    type GetExportedNsFn = unsafe extern "C" fn(*const c_char) -> *mut c_void;
    unsafe fn exported_ns(name: &str) -> *mut c_void {
        let p = dlsym_any("__loader_android_get_exported_namespace");
        if p.is_null() {
            return ptr::null_mut();
        }
        let f: GetExportedNsFn = std::mem::transmute(p);
        let c = CString::new(name).unwrap();
        f(c.as_ptr())
    }

    /// 从 bionic dlerror 文案解析缺失依赖的 SONAME：
    /// `dlopen failed: library "libgpud_sys.so" not found: needed by …`。
    fn parse_missing_soname(err: &str) -> Option<String> {
        let start = err.find("library \"")? + "library \"".len();
        let rest = &err[start..];
        let end = rest.find('"')?;
        let s = rest[..end].to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }

    /// RimDroid liblinkernsbypass linkernsbypass_link_namespace_to_default_all_libs
    /// 移植：建 default 命名空间的 SHARED 副本（parent=null + caller=libdl
    /// 实体地址 —— linker 以 caller 归属命名空间，libdl 落在 default）。
    unsafe fn default_copy_ns(ctx: &NsCtx) -> *mut c_void {
        let name_c = CString::new("qz-default-copy").unwrap();
        (ctx.create)(
            name_c.as_ptr(),
            ptr::null(),
            ptr::null(),
            2, // ANDROID_NAMESPACE_TYPE_SHARED
            ptr::null(),
            ptr::null_mut(),
            ctx.default_caller as *const c_void,
        )
    }

    /// 把 provider 命名空间的库接进 driver_namespace（from=我们的 ns）：
    /// 优先 all_libs 全量连回；符号缺失或失败时对缺失 SONAME 定向
    /// __loader_android_link_namespaces 补位（该 API 全版本必有）。
    unsafe fn link_provider_into_ns(
        ctx: &NsCtx,
        provider: *mut c_void,
        missing: Option<&str>,
    ) -> bool {
        if let Some(f) = link_all_libs_fn() {
            if f(ctx.ns, provider) != 0 {
                return true;
            }
        }
        if let Some(s) = missing {
            let c = CString::new(s).unwrap();
            return (ctx.link)(ctx.ns, provider, c.as_ptr()) != 0;
        }
        false
    }

    /// android_dlopen_ext + USE_NAMESPACE：把 path 装入指定命名空间。
    unsafe fn ns_dlopen(
        ns: *mut c_void,
        path: &CStr,
        flag: c_int,
        dlopen_ext: AndroidDlopenExtFn,
    ) -> *mut c_void {
        let info = AndroidDlextinfo {
            flags: ANDROID_DLEXT_USE_NAMESPACE,
            reserved_addr: ptr::null_mut(),
            reserved_size: 0,
            library_fd: 0,
            library_fd_offset: 0,
            library_namespace: ns,
        };
        dlopen_ext(path.as_ptr(), flag, &info)
    }

    /// 全新 SONAME 的 libvulkan.so 实例（ZL2 linker_ns_dlopen_unique 移植；
    /// fd 通道以「读整文件 + 内存补丁 + 落盘 runtime_dir + 绝对路径载入」等效
    /// —— runtime_dir 在 /data 下，driver_namespace permitted 已覆盖）。
    unsafe fn ns_dlopen_unique_ctx(ctx: &NsCtx) -> Result<*mut c_void, String> {
        let real = std::fs::read("/system/lib64/libvulkan.so").map_err(|e| {
            format!("stage=zl2-read-loader:读取 /system/lib64/libvulkan.so 失败：{e}")
        })?;
        let id = PATCH_ID.fetch_add(1, Ordering::Relaxed) & 0x0FFF;
        let mut buf = real;
        if !patch_soname_in_bytes(&mut buf, id) {
            return Err("stage=zl2-soname:libvulkan.so 未找到 DT_SONAME（结构异常）".to_string());
        }
        let tmp = crate::paths::runtime_dir().join(format!("qzvulkan_{id:03x}.so"));
        std::fs::write(&tmp, &buf)
            .map_err(|e| format!("stage=zl2-write-loader:{} 写入失败：{e}", tmp.display()))?;
        let cs = CString::new(tmp.to_string_lossy().into_owned()).unwrap();
        let h = ns_dlopen(ctx.ns, &cs, RTLD_NOW | RTLD_LOCAL, ctx.dlopen_ext);
        let _ = std::fs::remove_file(&tmp); // 映射建立后临时文件可删（不阻断）
        if h.is_null() {
            return Err(format!("stage=zl2-loader-load:{}", dlerr_text()));
        }
        Ok(h)
    }

    /// 全新 loader 实例装载 + RimDroid vkshim 三级救场（v0.2.19）：
    /// 首载（依赖搜索路径）失败 → default_copy 全量/定向连回重试 →
    /// sphal 全量/定向连回重试 → 全败。返回 (句柄, 胜出救场档位)。
    unsafe fn loader_unique_with_rescue(
        ctx: &NsCtx,
    ) -> Result<(*mut c_void, &'static str), String> {
        let mut last_err: String;
        match ns_dlopen_unique_ctx(ctx) {
            Ok(h) => return Ok((h, "search-path")),
            Err(e) => last_err = e,
        }
        // 第一级：default 命名空间副本全量/定向连回（libgpud_sys.so 场景的
        // 主救场 —— RimDroid vkshim_open_custom_loader 同款）
        let missing = parse_missing_soname(&last_err);
        let copy = default_copy_ns(ctx);
        if !copy.is_null() && link_provider_into_ns(ctx, copy, missing.as_deref()) {
            match ns_dlopen_unique_ctx(ctx) {
                Ok(h) => return Ok((h, "default-copy")),
                Err(e) => last_err = format!("{last_err}；{e}"),
            }
        }
        // 第二级：sphal 导出命名空间（厂商库提供方 —— RimDroid 第三级同源）
        let sphal = exported_ns("sphal");
        if !sphal.is_null() && link_provider_into_ns(ctx, sphal, missing.as_deref()) {
            match ns_dlopen_unique_ctx(ctx) {
                Ok(h) => return Ok((h, "sphal")),
                Err(e) => last_err = format!("{last_err}；{e}"),
            }
        }
        Err(format!("stage=zl2-loader-load:救场链全败（{last_err}）"))
    }

    /// ZL2 loadTurnipVulkan 等效链（v0.2.18 移植主入口；v0.2.19 补救场链）。
    ///
    /// # Safety
    /// 仅在 :game 进程（或 fork 隔离子进程）调用；操作进程级 linker
    /// namespace，失败路径全部返回 Err 不 panic。命名空间/句柄不跨 fork
    /// 存活 —— 探针子进程验证后父进程须重放同一链。
    pub unsafe fn zl2_driver_loader(
        driver_dir: Option<&Path>,
    ) -> Result<(*mut c_void, String), String> {
        // 命名空间搜索路径（ZL2 SEARCH_PATH="/system/lib64" 原值基础上补全）：
        // v0.2.19 —— 部分 ROM（实测小米天玑机型）的系统 libvulkan 是包装层，
        // DT_NEEDED libgpud_sys.so 本体在 /system_ext/lib64；ndk 子目录是
        // 其他 ROM 库的常见落点。permitted 本就含 /system_ext，只差搜索。
        // 驱动目录兜住 turnip + libc++_shared；vendor hw 目录服务厂商 blob
        // 自动发现变体（turnip 变体有 VK_ICD_FILENAMES 门控，厂商 JSON 不
        // 参与发现，搜索路径多几项无副作用）。
        let sys_paths = "/system_ext/lib64:/system_ext/lib64/ndk:/system/lib64/ndk";
        let vendor_paths =
            "/vendor/lib64:/vendor/lib64/hw:/vendor/lib64/ndk:/odm/lib64:/odm/lib64/hw:/odm/lib64/ndk";
        let search = match driver_dir {
            Some(d) => format!("/system/lib64:{sys_paths}:{vendor_paths}:{}", d.display()),
            None => format!("/system/lib64:{sys_paths}:{vendor_paths}"),
        };
        let ctx = ns_load(&search)?;

        // ZL2 顺序：ICD 先进命名空间（后续 loader 按 SONAME/绝对路径直接命中；
        // libc++_shared 等 DT_NEEDED 由搜索路径里的驱动目录按名解析 ——
        // v0.2.17 在 clns 里「libhardware.so not found」的死因在裸命名空间
        // 不存在：/system/lib64 直搜，无公共库白名单）
        if let Some(d) = driver_dir {
            let icd = d.join("libvulkan_freedreno.so");
            if !icd.exists() {
                return Err(format!("stage=zl2-icd-missing:{} 不存在", icd.display()));
            }
            let cs = CString::new(icd.to_string_lossy().into_owned()).unwrap();
            let h = ns_dlopen(ctx.ns, &cs, RTLD_NOW | RTLD_LOCAL, ctx.dlopen_ext);
            if h.is_null() {
                return Err(format!("stage=zl2-icd-load:{}", dlerr_text()));
            }
        }

        let (h, tier) = loader_unique_with_rescue(&ctx)?;
        Ok((h, tier.to_string()))
    }
}

/// Android 上转发 FFI 实现（保持调用点签名稳定）。
#[cfg(target_os = "android")]
pub unsafe fn zl2_driver_loader(
    driver_dir: Option<&Path>,
) -> Result<(*mut c_void, String), String> {
    ffi::zl2_driver_loader(driver_dir)
}

/// 非 Android 目标占位（模块仅 Android 运行时有效；占位保证全平台可编译）。
#[cfg(not(target_os = "android"))]
pub unsafe fn zl2_driver_loader(
    _driver_dir: Option<&Path>,
) -> Result<(*mut c_void, String), String> {
    Err("stage=zl2-unavailable:仅 Android 运行时可用".to_string())
}

/// ZL2 patch_elf_soname 移植（纯字节操作版）：把 DT_SONAME 字符串前 3 字节
/// 覆写为 3 位十六进制 id —— bionic 按 SONAME 去重，必须让 libvulkan.so
/// 成为从未装载过的新库名，才能在 driver_namespace 里拿到全新实例。
/// 只动前 3 字节：`libvulkan.so`（12 字符）→ `xxXvulkan.so`，长度与 NUL 不变。
fn patch_soname_in_bytes(buf: &mut [u8], id: u16) -> bool {
    const DT_SONAME: u64 = 14;
    const SHT_DYNAMIC: u32 = 6;
    if buf.len() < 64 {
        return false;
    }
    // Elf64_Ehdr：e_shoff@0x28(u64) e_shentsize@0x3A(u16) e_shnum@0x3C(u16)
    let e_shoff = u64::from_le_bytes(buf[0x28..0x30].try_into().unwrap_or([0; 8])) as usize;
    let e_shentsize = u16::from_le_bytes(buf[0x3A..0x3C].try_into().unwrap_or([0; 2])) as usize;
    let e_shnum = u16::from_le_bytes(buf[0x3C..0x3E].try_into().unwrap_or([0; 2])) as usize;
    if e_shoff == 0 || e_shentsize < 64 || e_shnum == 0 || e_shoff >= buf.len() {
        return false;
    }
    for i in 0..e_shnum {
        let sh = e_shoff + i * e_shentsize;
        if sh + 64 > buf.len() {
            return false;
        }
        // Elf64_Shdr：sh_type@0x04(u32) sh_offset@0x18 sh_size@0x20 sh_link@0x28
        let sh_type = u32::from_le_bytes(buf[sh + 0x04..sh + 0x08].try_into().unwrap_or([0; 4]));
        if sh_type != SHT_DYNAMIC {
            continue;
        }
        let sh_offset =
            u64::from_le_bytes(buf[sh + 0x18..sh + 0x20].try_into().unwrap_or([0; 8])) as usize;
        let sh_size =
            u64::from_le_bytes(buf[sh + 0x20..sh + 0x28].try_into().unwrap_or([0; 8])) as usize;
        let sh_link =
            u32::from_le_bytes(buf[sh + 0x28..sh + 0x2C].try_into().unwrap_or([0; 4])) as usize;
        if sh_link >= e_shnum {
            return false;
        }
        let st = e_shoff + sh_link * e_shentsize;
        if st + 64 > buf.len() {
            return false;
        }
        let st_offset =
            u64::from_le_bytes(buf[st + 0x18..st + 0x20].try_into().unwrap_or([0; 8])) as usize;
        // Elf64_Dyn：d_tag(u64) + d_un(u64)，逐条找 DT_SONAME
        let entries = sh_size / 16;
        for k in 0..entries {
            let e = sh_offset + k * 16;
            if e + 16 > buf.len() {
                return false;
            }
            let tag = u64::from_le_bytes(buf[e..e + 8].try_into().unwrap_or([0; 8]));
            if tag == DT_SONAME {
                let off =
                    u64::from_le_bytes(buf[e + 8..e + 16].try_into().unwrap_or([0; 8])) as usize;
                let name_pos = st_offset + off;
                if name_pos + 3 > buf.len() {
                    return false;
                }
                let prefix = format!("{id:03x}");
                buf[name_pos..name_pos + 3].copy_from_slice(prefix.as_bytes());
                return true;
            }
        }
        return false;
    }
    false
}

/// SONAME 补丁序号（进程内自增；探针子进程与父进程各自独立计数）。
static PATCH_ID: AtomicU16 = AtomicU16::new(0);

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造最小 Elf64 骨架：ehdr + 一个 SHT_DYNAMIC 节 + 关联 strtab，
    /// DT_SONAME 指向 "libvulkan.so"。验证补丁确实改写前 3 字节。
    #[test]
    fn soname_patch_rewrites_prefix() {
        let soname_str = b"libvulkan.so\0";
        let strtab_off = 64usize; // 紧跟 ehdr
        let dyn_off = strtab_off + soname_str.len(); // 64+13=77 → 8 对齐到 80
        let dyn_off = (dyn_off + 7) & !7;
        let dyn_bytes: usize = 16 * 3; // DT_SONAME + DT_NULL + 余量
        let shstr_off = dyn_off + dyn_bytes;
        let shstr = b".shstrtab\0.dynamic\0.dynstr\0\0";
        let shoff = shstr_off + shstr.len();
        let shoff = (shoff + 7) & !7;

        let total = shoff + 64 * 3;
        let mut buf = vec![0u8; total];
        // ehdr
        buf[0x28..0x30].copy_from_slice(&(shoff as u64).to_le_bytes());
        buf[0x3A..0x3C].copy_from_slice(&64u16.to_le_bytes());
        buf[0x3C..0x3E].copy_from_slice(&3u16.to_le_bytes());
        // strtab 内容
        buf[strtab_off..strtab_off + soname_str.len()].copy_from_slice(soname_str);
        // .dynamic 节：DT_SONAME(14) + 偏移 0 + DT_NULL
        buf[dyn_off..dyn_off + 8].copy_from_slice(&14u64.to_le_bytes());
        buf[dyn_off + 8..dyn_off + 16].copy_from_slice(&0u64.to_le_bytes());
        // shstrtab
        buf[shstr_off..shstr_off + shstr.len()].copy_from_slice(shstr);
        // 节表：[0]=.dynamic(SHT_DYNAMIC=6, sh_link=1) [1]=.dynstr [2]=.shstrtab
        let put = |buf: &mut [u8], i: usize, ty: u32, off: u64, size: u64, link: u32| {
            let s = shoff + i * 64;
            buf[s + 0x04..s + 0x08].copy_from_slice(&ty.to_le_bytes());
            buf[s + 0x18..s + 0x20].copy_from_slice(&off.to_le_bytes());
            buf[s + 0x20..s + 0x28].copy_from_slice(&size.to_le_bytes());
            buf[s + 0x28..s + 0x2C].copy_from_slice(&link.to_le_bytes());
        };
        put(&mut buf, 0, 6, dyn_off as u64, 32, 1);
        put(
            &mut buf,
            1,
            3,
            strtab_off as u64,
            soname_str.len() as u64,
            0,
        );
        put(&mut buf, 2, 3, shstr_off as u64, shstr.len() as u64, 0);

        assert!(patch_soname_in_bytes(&mut buf, 0xABC));
        let patched = &buf[strtab_off..strtab_off + 12];
        assert_eq!(std::str::from_utf8(patched).unwrap(), "abcvulkan.so");
    }

    /// 畸形输入（空/截断/无节表）必须返回 false，绝不越界。
    #[test]
    fn soname_patch_rejects_malformed() {
        assert!(!patch_soname_in_bytes(&mut [], 1));
        assert!(!patch_soname_in_bytes(&mut vec![0u8; 32], 1));
        let mut no_sh = vec![0u8; 64]; // e_shoff=0
        assert!(!patch_soname_in_bytes(&mut no_sh, 1));
    }
}
