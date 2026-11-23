package com.quartz.game;

/**
 * Quartz 游戏退出信号（v0.1.6）：checkExit 否决 System.exit/halt 时抛出，
 * 携带请求的退出码沿调用栈向上传播，最终由壳层 Rust 侧（quartz-jvm 线程）
 * 捕获并解析状态码 —— 游戏崩溃/退出不再终止整个应用进程，而是回到启动器。
 *
 * 继承 SecurityException（checked）：MC 的 ReportedException 处理链不会
 * 特判它；消息格式 "quartz-exit:<status>" 供 Rust 侧按字符串解析。
 */
public final class ExitSignal extends SecurityException {
    /** 本次 System.exit/Runtime.halt 请求的退出码。 */
    public final int status;

    public ExitSignal(int status) {
        super("quartz-exit:" + status);
        this.status = status;
    }
}
