# Quartz APK 签名密钥说明

- 密钥文件：`keystore/quartz-debug.keystore`（同时放了一份在 `~/.android/debug.keystore`，
  让以后 `cargo tauri android build --apk` 的 debug 构建自动用同一把签名）。
- 别名：`androiddebugkey`
- storepass / keypass：`android`（标准 debug 凭据）
- SHA-256 证书指纹：`63860a42899137e861cfa9c7a46e7f975d29dc8197de84a0b19cbe14813b0783`

## 为什么有这个文件

旧构建的 debug 密钥随沙箱回收丢失，2026-11-22 起改用这把新密钥。
`libquartz_shell_lib.so` 在发布时用交叉 binutils 的 `aarch64-linux-gnu-strip
--strip-unneeded` 剥离调试/静态符号（183MB → 19.9MB），APK 体积 190MB → 26.7MB，
解决网页通道下载失败问题。运行时只需 .dynsym（动态链接符号），功能零损失。

## 手动重打包流程（工具链丢失时的应急路径）

1. 剥离 so：优先用交叉 binutils（`apt-get download binutils-aarch64-linux-gnu
   libbinutils binutils-common` 后 dpkg -x 解包，`aarch64-linux-gnu-strip
   --strip-unneeded`）。`scripts/sstrip_elf.py`（截断 ELF）是备选，勿删 META-INF。
2. `python3 scripts/repack_apk.py <输出.apk>`（源 APK 路径在脚本开头常量里；
   只能剔除签名文件，META-INF/services 下的 ServiceLoader 注册必须保留——
   2026-11-22 曾因全删 META-INF 导致 coroutines 主调度器初始化失败、启动即闪退）。
3. `zipalign -f -p 4 <repack> <aligned>`
4. `apksigner sign --ks keystore/quartz-debug.keystore --ks-pass pass:android
   --ks-key-alias androiddebugkey --out Quartz.apk <aligned>`

若设备上装过旧签名版本，安装新包会报「签名不一致」，需先卸载旧版。

## JNI 教训（v5 修复，2026-11-22）

- Redmi/xaga Android 15 崩溃：`JNI NewStringUTF called with pending exception
  NoSuchMethodError: Application.isStorageAccessGranted()Z`。
- 根因 1：ndk-context 给 Rust 的 context 是 **Application**（不是 MainActivity），
  应用自定义实例方法在其上不存在，GetMethodID 必抛 NoSuchMethodError。
  权限判定改为纯系统 API：静态 `Environment.isExternalStorageManager()`（API 30+）
  + `Context.checkSelfPermission`（API <30）；权限申请走 MainActivity 伴生对象
  静态方法 `requestStorageAccessStatic()`（companion 持有 instance）。
- 根因 2：JNI 报错后挂起异常未清理，同线程下一个 JNI 调用（NewStringUTF）在
  CheckJNI 下直接 abort。`with_jni` 进出闭包均 `exception_check/exception_clear` 兜底。
- 陷阱 3：`FindClass` 在 AttachCurrentThread 的原生线程走系统类加载器，
  解析不到 APK 内的应用类；应用类必须经 `context.getClassLoader().loadClass(...)`。
- 工具链注意：zipalign/apksigner 在 `.toolchain/android-sdk/build-tools/36.0.0/`，
  /tmp/apkwork/sdk 的副本已丢失；交叉 strip 在 /tmp/apkwork/btbin（需 LD_LIBRARY_PATH）。

## 游戏进程 SIGABRT 教训（v6 修复，2026-11-22）

- v5 现象：游戏进程启动（exec 成功）即退出码 134（SIGABRT），stderr 只有
  「Pointer tag ... truncated」警告（无害），无 hs_err —— JVM 侧静默，纯原生 abort。
- 根因（jar 反汇编实证）：Pojav `lwjgl-glfw-classes.jar` 的
  `org.lwjgl.glfw.GLFW` 静态初始化 `System.loadLibrary("pojavexec")`，
  GLFW Functions 表（pojavInit/pojavCreateContext/pojavMakeCurrent/...）
  全部由 **libpojavexec.so** 导出 —— 而 v5 的 LWJGL pack 只下了
  liblwjgl/liblwjgl_opengl/liblwjgl_tinyfd/libopenal/libfreetype 五个 so，
  缺 `libpojavexec.so`、`libjnidispatch.so`（JNA/oshi 必须，jar 自带的是
  glibc 版）、`liblwjgl_stb.so`（1.19+ 图像处理）。
- 修复：lwjgl.rs 改为从 Pojav **gladiolus 官方 Release APK**（约 154MB）
  提取 jar + 8 个 bionic so（单一来源保证 jar 与 natives 版本配套）。
  libpojavexec.so 只随 release APK 分发（仓库 jniLibs 无此文件，FCL 版
  依赖 libfcl.so/libbytehook.so 不可用）。
- LWJGL 模块默认库名（v6 已按此接线，勿再猜测）：
  core=lwjgl / glfw=**libpojavexec.so** / opengl=lwjgl_opengl /
  stb=lwjgl_stb / tinyfd=lwjgl_tinyfd / openal=openal / freetype=freetype。
  `org.lwjgl.opengl.libname` 必须指向渲染器 GL 库（liblwjgl_opengl 按它
  dlopen；PojavRendererInit 也据它识别渲染层并挂内部初始化）。
- java.library.path 顺序：LWJGL natives（私有目录，含 pojavexec/jnidispatch）
  -> 渲染层 -> 游戏 natives（FUSE 仅兜底 —— FUSE noexec，不能 dlopen）。
- 诊断体系（v6 新增，SIGABRT 的证据链在 logd 不在 stderr/hs_err）：
  1. JVM 二分自检：完整 JVM 参数（无 classpath/主类）先跑 `-version`，
     把「参数/环境崩」与「游戏类/native 崩」当场分开；
  2. 退出非 0 时 `logcat -d -b crash/-b main` 按 pid 摘录（bionic abort
     的 Fatal signal/Abort message 只在 logcat）；
  3. 组件清单（lwjgl natives/渲染器/jre lib/server）落盘启动日志。
- 已知边界（M2）：pojavexec 的渲染窗口由应用侧 `JREUtils.setupBridgeWindow`
  注入 ANativeWindow；子进程 JVM 尚无窗口桥 —— 走到 glfwCreateWindow 时
  可能报错/退出（日志会给出确切诉求），渲染窗口桥是下一里程碑。
- gradle 输出确认在 `/tmp/my-project/.qz-gradle-build/app/outputs/apk/universal/debug/`
  （gen/…/build 下无产物）；tauri 完成提示打印的 gen 路径不可信。

## JRE GC 线程 SIGABRT 教训（v7 修复，2026-11-22）

- 现象（13:13 v6 构建，用户日志 launch-1795355649.log）：LWJGL/渲染器/JVM 组件
  齐全后游戏进程依旧退出码 134。logcat：`Fatal signal 6 (SIGABRT), code -1
  (SI_QUEUE) in tid (GC Thread#5)`，同线程先出现「Pointer tag for 0x... was
  truncated」；stderr 无 JVM「# A fatal error」横幅 —— 说明 abort 来自
  bionic/scudo 而非 JVM fatal()。JVM 二分自检通过（无 classpath 不初始化堆），
  真实运行 1.5s 后首个 GC 周期崩溃。
- 根因：Android 12（API 31）起 AArch64 用户态指针的整个最高字节被系统保留
  （android/ndk#1653）；OpenJDK HotSpot 在 top byte 上有自己的标记用途，
  JVM 把带 top-byte 标记的指针传给 malloc/free -> scudo 记录截断警告 ->
  chunk 校验失败 -> abort（SIGABRT，SI_QUEUE）。2021-08 的 Pojav JRE17 构建
  无此防护（Termux issue #7332 同源问题）。
- 修复（Termux openjdk-17 PR#10347 同款方案）：java 进程早期
  `mallopt(M_BIONIC_SET_HEAP_TAGGING_LEVEL(-204), M_HEAP_TAGGING_LEVEL_NONE(0))`。
  实现走 LD_PRELOAD 注入而非改 JRE 包：
  `scripts/tagfix/quartz_tagfix.c`（3.8KB，弱引用 mallopt——API<27 设备符号
  缺失时优雅跳过，避免 LD_PRELOAD 未解析符号导致 java 无法启动）→
  `scripts/build-tagfix.sh`（NDK aarch64-linux-android24-clang）→ shim 经
  `include_bytes!` 编入壳层 → `spawn_game` 前落盘
  `runtime_dir/libqztagfix.so` → LD_PRELOAD 注入（smoke test / 二分自检 /
  真实启动三处一致，日志头记录 LD_PRELOAD 路径）。
- 为什么不换 JRE 包：老仓库 android-openjdk-build-multiarch 2022 后的 release
  是 iOS 构建（jre17-ca01427 明言 iOS/iPadOS，Mach-O 不可用）；修复后的
  Android 构建只随 Pojav APK 内置分发，无公开 tarball 可下载 —— shim 注入
  是唯一不动 JRE 内容的路径。
- strip 工具已改用 NDK 自带 `llvm-strip`（无需交叉 binutils + LD_LIBRARY_PATH）：
  `.toolchain/android-sdk/ndk/27.0.12077973/.../bin/llvm-strip`。
- 预期边界：GC 层 abort 修复后，JVM 可稳定进入 MC 主流程；pojavexec 在
  glfwCreateWindow 仍需应用侧窗口桥（见 v6 节「已知边界」）——若下一轮
  日志显示 GLFW/窗口错误，属已记录的 M2 里程碑，不再是 SIGABRT。

## v8 修复：闪退自愈 + 下载空间防线（2026-11-23）

用户报告「一打开就闪退」（此前「又下载失败」）。Rust 启动路径全面审查未发现主线程
panic 点（setup 钩子 / 首轮 invoke 命令均不 panic），判定最可能根因：
① 失败下载残留（.qz-part/.part/.staging，Pojav APK 154MB / JRE pack 38MB）反复重试
   塞满 userdata 分区（FUSE 与应用私有目录同分区）→ WebView 无法初始化，「一打开
   就闪退」；② WebView 数据损坏（进程被杀时写坏 IndexedDB/缓存）导致崩溃循环。

- 崩溃证据链（闪退不再黑盒）：
  - Rust：`install_panic_hook()`（run() 最早调用）→ 任意线程 panic 追加到
    `<files>/crash/quartz-crash.log`，含线程/位置/消息/backtrace 40 行；
  - Java：新增 `QuartzApp`（Application 子类，Manifest android:name 注册）未捕获
    异常处理器 → 同一文件同一标记格式 `==== CRASH @<epoch秒> KIND ====`；
  - Android 原生 abort（bionic）不可捕获，但 logcat 摘录体系（v6/v7）继续覆盖游戏进程。
- WebView 崩溃循环自愈：QuartzApp 检测 2 分钟内 ≥2 次崩溃 → 在 Activity/WebView
  创建前删除 `app_webview`（Chromium profile）。只清 WebView 数据，不碰
  app_data（Tauri 数据）与任何游戏文件。
- 存储防线（断「下载失败 → 塞满 → 闪退」循环）：
  - quartz-core 新增 `space` 模块（statvfs f_bavail×f_frsize）；
  - 启动时 `cleanup_stale_partials()` 清理 `*.qz-part`/`*.part`/`*.tmp` 与
    `.*.staging`（只动顶层明确临时名）；
  - 下载前置空间检查：游戏=缺失任务字节+64MB（instance）、runtime=size×2+100MB、
    LWJGL 400MB、渲染层 200MB、壳层四命令统一 200MB 底线（不可探测则放行）；
  - os error 28/13 → 中文可行动文案（`error::io_context`），写盘点全部接入；
  - `meta::expand_asset_index` 短哈希切片 panic 加固（损坏索引报错而非 abort）。
- 诊断上屏：新增 `get_diagnostics` 命令（lastCrash / lastLaunchTail / freeBytes /
  crashLogPath）→ 前端开屏横幅（登录屏也可见，可关闭）；设置页新增「可用空间」行；
  `storage_json` 带 freeBytes。游戏 SIGABRT 的证据（launch-*.log 尾部 60 行）
  经同一横幅直达 UI。
- 版本：Cargo/tauri.conf 0.1.3，versionCode 1003。
- 交付：Quartz.apk 27,804,767 字节；签名 SHA-256 63860a42…783 不变（可覆盖安装）；
  930 条目、so STORED、5 个 META-INF/services 保留、QuartzApp 编入 classes6.dex、
  targetSdk 28 不变。

## v6 修复：下载管线镜像回退（2026-11-23）

用户反馈「下载失败」：v5 全部下载源为 GitHub 直连（JRE pack、154MB Pojav APK、
MobileGlues APK、raw.githubusercontent jniLibs）+ Mojang 官方源，国内网络下
GitHub 直连极易握手超时/传输重置。v6 新增 `crates/quartz-core/src/mirror.rs`：

- 候选源策略（镜像优先、直连兜底，2026-11 实测逐 URL 验证）：
  - Mojang 四类源（piston-meta / piston-data / libraries.minecraft.net /
    resources.download.minecraft.net）→ BMCLAPI 宿主替换
    （bmclapi2.bangbang93.com，清单/版本 JSON/maven/assets 全端点 200/206）
    → 官方直连兜底；
  - GitHub（github.com release / raw.githubusercontent.com）→
    ghfast.top → gh-proxy.com → ghproxy.net → 直连
    （mirror.ghproxy.com、github.moeyy.xyz 已失效不收录）；
  - 其余源（maven.fabricmc.net 等）直连。
- 执行器：任务按候选逐个尝试 × 2 轮；流停滞 30s 超时（大文件不设整体超时）；
  修复 TaskFailed 事件丢真实任务标签的 bug（v5 UI 只显示 "task"）。
- 端到端冒烟（examples/live_mirror.rs）：BMCLAPI 清单 925 版本、
  libvgpu.so 经镜像 1,288,192 字节、JRE pack content_length 38,065,004
  与 runtime.rs spec 精确一致。
- 交付：Quartz.apk 27,763,807 字节，签名证书 SHA-256 与 v5 相同
  （63860a42…783，覆盖安装免卸载）；930 条目、so STORED、5 个
  META-INF/services 条目保留。
- 环境备注：构建 JDK 已落 .toolchain/jdk-21.0.13.1+2（Temurin 完整版，
  含 javac）；setup-env.sh 自动在 .toolchain 与 /usr/lib/jvm 间回退。
  UI 依赖位置 apps/shell-tauri/ui（src-tauri/ui 为符号链接，回收后需重建）。
