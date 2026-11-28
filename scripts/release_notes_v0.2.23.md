# v0.2.23 —— zink 死因终局：无 SONAME 依赖必须住 nativeLibraryDir（ZL2 APK 同款）

## 根因（v0.2.22 实机日志 launch-1795856232 复盘）

`libcutils.so 预载完成` 但 `libEGL_mesa/libzink_dri 的 DT_NEEDED "libcutils.so" not found in namespace clns-7`。
机制：**:game 是 Zygote 派生的 Android 子进程**，Rust 壳层跑在 APK PathClassLoader 的
classloader namespace（clns）里，其搜索路径 = nativeLibraryDir，**进程创建时冻结**。
v0.2.22 在进程内 set_var 改 LD_LIBRARY_PATH，只影响「自己 getenv 再拼绝对路径」的读者
（Mesa loader 搜 libzink_dri.so，这条成立），对 clns 按名解析**无效** —— 而无 SONAME 的
libcutils.so 恰恰只能走按名解析：

- 六件库 ELF 实测（readelf -d，v0.2.22 制品）：唯一 **无 DT_SONAME** 的被按名依赖 =
  `libcutils.so`（libEGL_mesa.so / libzink_dri.so 的 DT_NEEDED）；
- 其余全部可解：libglapi/libglxshim/libc++_shared 有 SONAME（预载按名命中）；
  liblog/libnativewindow/libsync/libz/libm/libdl/libc 是公共库（clns public-lib 链接可达）。

## ZL2 蓝本怎么解的（1:1 机制，非变通）

ZL2 2.8.2 把 **libcutils.so 原版直接打进自家 APK lib/arm64-v8a**（我们 v0.2.21 逐字节
比对的六件库之一正是它）。安装后落 nativeLibraryDir → clns 搜索路径天然覆盖 →
DT_NEEDED "libcutils.so" 按**文件名**解析命中，不需要 SONAME、不需要 shim、不需要
namespace 旁路。ZL2 的裸名 `dlopen("libEGL_mesa.so")`（POJAVEXEC_EGL）同理成立。

## v0.2.22 → v0.2.23 改动（三处）

1. **APK 注入 ZL2 2.8.2 原版 libcutils.so**（bridge/libcutils.so，21320 B，
   sha256 `04258bb1…e347` 锁定 = 制品 zip 内逐字节同源）：repack_apk.py 注入
   `lib/arm64-v8a/libcutils.so`（STORED，zipalign -P 16 页对齐），安装器落
   nativeLibraryDir → 按名解析通。
2. **POJAVEXEC_EGL 回滚为绝对路径**（v0.2.22 的 ZL2 裸名在 Quartz 架构必败：我们的
   libEGL_mesa 在数据目录，不在 nativeLibraryDir，clns 按名找不到 —— bridge.rs
   load_api() dlopen 会直接失败）。ZL2 裸名成立的前提就是它的 Mesa 住在 APK 里。
3. **保留**：LD_LIBRARY_PATH（ZL2 getLibraryPath 同款顺序，Mesa loader 按
   getenv 搜 libzink_dri.so）、预载两遍法（诊断）、fork 隔离 EGL 预检安全网、
   load_vulkan 1:1（v0.2.22 已与 ZL2 编译产物对齐）。

## 契约（三层防线）

- quartz-core `zink_libcutils_jni_libs_contract`：仓库内契约件 sha256/大小钉死（74 测试绿）；
- finalize_apk.sh：注入前 `sha256sum -c` 硬校验；
- 收尾验证：最终 APK 必须含 `lib/arm64-v8a/libcutils.so` 且 STORED + sha256 相符，
  否则构建失败。

## 验收判据（不变）

日志两行齐 = 天玑通路通了：
- `QUARTZ_VK_PROBE:OK:system ... devices=N:gpu0=Mali-...`
- `渲染层 EGL 预检通过：zink`

老 APK 注意：本版必须**重装 APK**（libcutils.so 在 APK 内，重下 zink 包无法补装）。
