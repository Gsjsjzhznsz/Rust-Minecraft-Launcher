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
