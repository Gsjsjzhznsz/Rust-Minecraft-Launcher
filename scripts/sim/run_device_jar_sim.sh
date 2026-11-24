#!/bin/bash
# 沙箱复现：设备 jar + 我们桥（host 构建）+ Mojang natives 跑真实 MC。
# 目标：复现「第一次触摸标题屏 -> 鼠标回调 -> libffi 闭包 -> CallVoidMethod -> pc=0」。
set -x
ROOT=/home/z/my-project
SIM=$ROOT/sim
MC=$SIM/minecraft
BRIDGE=$SIM/bridge-natives
INJ=$SIM/injector-classes

# 1) 桥落位（host 构建改名 libpojavexec.so，供 System.loadLibrary("pojavexec")）
mkdir -p "$BRIDGE"
cp -f "$ROOT/target/release/libquartz_bridge.so" "$BRIDGE/libpojavexec.so"

# 2) 注入器：无需预编译（Java 11+ 单文件源码模式，由 java 直接运行）
INJECTOR_SRC=$ROOT/scripts/sim/injector/sim/Injector.java

# 3) classpath：gladiolus 合并 jar 最前（org.lwjgl 类以设备版为准），
#    其后剔除 Mojang 自带 lwjgl 组（避免双实现混淆），其余照官方顺序。
GLADIOLUS=$ROOT/.qz-tmp/gladiolus/assets/components/lwjgl3/lwjgl-glfw-classes.jar
CP="$GLADIOLUS"
while IFS= read -r jar; do
  case "$jar" in
    *org/lwjgl/lwjgl-*|*org/lwjgl/lwjgl) ;;      # 跳过 Mojang 的 lwjgl 组
    *) CP="$CP:$jar" ;;
  esac
done < "$SIM/classpath.txt"

# 4) natives：桥目录最前（pojavexec 命中我们的），Mojang natives 兜底
NATIVES="$BRIDGE:$SIM/natives"

# 5) 环境（桥 / EGL）—— host-sim 内置 EGL-on-GLX 仿真 + Xvfb + llvmpipe
export DISPLAY=:99
export POJAVEXEC_EGL=sim                # 走 egl_sim（glX 上下文）
export POJAV_RENDERER=desktopgl_sim     # 桥绑定桌面 GL（llvmpipe 提供 4.5 core）
export QUARTZ_SIM_W=1280 QUARTZ_SIM_H=720
pgrep -f "Xvfb :99" >/dev/null || (Xvfb :99 -screen 0 1280x720x24 >/dev/null 2>&1 &) 
sleep 1

cd "$MC"

exec java -Xmx2048M \
  -Dorg.lwjgl.opengl.libname=libGL.so.1 \
  -Djava.library.path="$NATIVES" \
  -Dorg.lwjgl.system.SharedLibraryExtractPath=$SIM \
  -Djava.io.tmpdir=$SIM \
  -Djna.tmpdir=$SIM \
  -Djna.boot.library.path="$SIM/natives" \
  -Djna.nounpack=true \
  -Dminecraft.launcher.brand=QuartzSim \
  -Dminecraft.launcher.version=sim \
  -cp "$CP" \
  "$INJECTOR_SRC" \
  --username simuser --version 1.20.4 \
  --gameDir "$MC" --assetsDir "$MC/assets" --assetIndex 12 \
  --uuid 8953bc6e-f478-3814-995a-08dc673e7887 --accessToken 0 \
  --userType legacy --versionType QuartzSim
