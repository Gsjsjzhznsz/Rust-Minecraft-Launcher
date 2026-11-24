package sim;

import java.lang.reflect.Method;

/**
 * 沙箱注入器：先起后台线程模拟「用户触摸标题屏」，再启动 MC 主类。
 * 事件走 jar 自己的 org.lwjgl.glfw.CallbackBridge.nativeSendData ——
 * 与设备 GameService->QuartzBridge->桥 C ABI 完全同构的派发路径：
 * bridge dispatch_event -> 事件队列 -> pojavPumpEvents -> libffi 闭包 -> CallVoidMethod。
 */
public class Injector {
    public static void main(String[] args) throws Exception {
        Thread t = new Thread(() -> {
            for (int i = 0; i < 80; i++) { // 最多 20 分钟，每 15s 一轮
                try {
                    Thread.sleep(15_000);
                    Class<?> cb = Class.forName("org.lwjgl.glfw.CallbackBridge");
                    Method send = cb.getMethod("nativeSendData", boolean.class, int.class, String.class);
                    // 光标移动（触发 upd_mouse -> cursor_pos 回调）
                    send.invoke(null, true, 1003, "500.0 300.0");
                    // 鼠标左键按下/抬起（设备崩溃时的参数：window, 0, 1, 0）
                    send.invoke(null, true, 1006, "0 1 0");
                    send.invoke(null, true, 1006, "0 0 0");
                    // 滚轮（第三类回调）
                    send.invoke(null, true, 1007, "0.0 -1.0");
                    System.out.println("[injector] 第 " + (i + 1) + " 轮事件已注入");
                } catch (Throwable e) {
                    // 类未加载/桥未就绪：继续重试
                    if (i % 4 == 0) System.out.println("[injector] 等待桥就绪: " + e);
                }
            }
            System.out.println("[injector] 完成");
        }, "QuartzSimInjector");
        t.setDaemon(true);
        t.start();

        Method main = Class.forName("net.minecraft.client.main.Main")
                .getMethod("main", String[].class);
        main.invoke(null, (Object) args);
    }
}
