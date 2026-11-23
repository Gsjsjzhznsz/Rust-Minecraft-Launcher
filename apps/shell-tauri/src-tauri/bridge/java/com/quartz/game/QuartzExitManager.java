package com.quartz.game;

import java.security.Permission;

/**
 * Quartz 游戏退出捕获管理器（v0.1.6，自研，不含任何 Pojav 代码）。
 *
 * 背景：进程内 JVM 架构下，Minecraft 崩溃路径（写完崩溃报告后调用
 * System.exit）会经 OpenJDK os::exit 直接终止整个应用进程 —— 用户看到的
 * 是「整个应用闪退」而不是「回到启动器」。安装本管理器后：
 *
 *   System.exit(N) / Runtime.exit(N) / Runtime.halt(N)
 *      -> SecurityManager.checkExit(N)
 *      -> 抛 ExitSignal(N)
 *      -> 沿调用栈向上传播（正常情况下穿透 Main.main）
 *      -> 壳层 Rust 侧 CallStaticVoidMethod 返回挂起异常
 *      -> 解析状态码 -> 回收 Surface / 恢复竖屏 / 上报 launch://exit
 *
 * 其余权限检查全部放行（无操作），因此行为上等价于无管理器，仅
 * checkExit 生效。需要在 CreateVM 参数里加 -Djava.security.manager=allow
 * （JDK 17 起 setSecurityManager 默认被 JEP 411 禁用）。
 */
public final class QuartzExitManager extends SecurityManager {
    /** 由壳层 Rust 侧经 JNI 调用（CallStaticVoidMethod）。 */
    public static void install() {
        try {
            if (System.getSecurityManager() == null) {
                System.setSecurityManager(new QuartzExitManager());
            }
        } catch (Throwable t) {
            // 安装失败仅意味着退出不再被捕获（进程级退出，旧行为），不打断启动
            t.printStackTrace();
        }
    }

    /** 唯一生效点：否决一切退出请求，转为可传播的退出信号。 */
    @Override
    public void checkExit(int status) {
        throw new ExitSignal(status);
    }

    @Override
    public void checkPermission(Permission perm) {
        // 全放行
    }

    @Override
    public void checkPermission(Permission perm, Object context) {
        // 全放行
    }
}
