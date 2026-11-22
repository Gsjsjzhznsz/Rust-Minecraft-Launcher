/*
 * Quartz：Android 12+ tagged-pointer 兼容层（LD_PRELOAD 注入 java 进程）。
 *
 * 根因
 * ----
 * Android 12（API 31）起，AArch64 上系统保留用户态指针的整个最高字节
 * （top byte，见 android/ndk#1653）。OpenJDK HotSpot 会在 top byte 上做
 * 自己的标记用途，与系统的堆指针标记检查冲突：JVM 把带 top-byte 标记的
 * 指针传给 malloc/free 时，bionic/scudo 记录
 *   "Pointer tag for 0x... was truncated, see
 *    https://source.android.com/devices/tech/debug/tagged-pointers"
 * 随后 chunk 校验失败 -> CHECK -> abort()（SIGABRT，退出码 134），典型崩在
 * GC 工作线程（GC Thread#N）。
 *
 * 修复
 * ----
 * 进程早期调用 mallopt(M_BIONIC_SET_HEAP_TAGGING_LEVEL,
 * M_HEAP_TAGGING_LEVEL_NONE) 关闭堆标记（bionic 文档明确允许任意时刻调用，
 * 关闭后不可再开启）。与 Termux openjdk-17 修复（termux-packages PR#10347，
 * patch 0021：java.c android_disable_tags()）同一方案；本文件以 LD_PRELOAD
 * 构造函数实现，时机比 JLI_Launch 更早，且无需改动 JRE 包内容。
 *
 * API < 31（Android 11-）无堆指针标记机制，mallopt 对未知选项返回 0，
 * 本调用为无害 no-op，因此无需按 API 分支。
 */

#include <malloc.h>

/* bionic malloc.h（API 31+）：
 * #define M_BIONIC_SET_HEAP_TAGGING_LEVEL (-204)
 * enum { M_HEAP_TAGGING_LEVEL_NONE = 0, ... }; */
#ifndef M_BIONIC_SET_HEAP_TAGGING_LEVEL
#define M_BIONIC_SET_HEAP_TAGGING_LEVEL (-204)
#endif
#ifndef M_HEAP_TAGGING_LEVEL_NONE
#define M_HEAP_TAGGING_LEVEL_NONE 0
#endif

/* mallopt 的声明随 API 27+ 的头文件提供；这里用弱引用保证 shim 在
 * API 24-26 上也能正常加载（符号缺失时跳过修复——那些版本本无此问题），
 * 避免 LD_PRELOAD 因未解析符号导致 java 进程启动失败。 */
extern int mallopt(int __option, int __value) __attribute__((weak));

__attribute__((constructor)) static void
quartz_disable_heap_tagging(void) {
    if (mallopt) {
        mallopt(M_BIONIC_SET_HEAP_TAGGING_LEVEL, M_HEAP_TAGGING_LEVEL_NONE);
    }
}
