/* Quartz libcutils shim（bionic/aarch64）。
 *
 * 根因（实机 2026-11）：IronizedZink v1.1.0 pack 的 libcutils.so 无 DT_SONAME，
 * bionic dlopen 按完整路径注册 soinfo；之后 libgallium_dri.so / libEGL_mesa.so
 * 的 DT_NEEDED "libcutils.so" 按名解析在 clns 命名空间必败
 * （dlopen failed: library "libcutils.so" not found ... in namespace clns-N）。
 *
 * 本 shim 以正确 SONAME=libcutils.so 编译：
 *  - property_* 转发系统 bionic（__system_property_get/set，系统 libc 可解析）；
 *  - atrace_* 全 no-op（Mesa 仅用它做 systrace 打点）；
 *  - 无 DT_NEEDED、无 CRT（-nostdlib），未定义符号由加载期 bionic libc 解析。
 *
 * 构建：aarch64-linux-gnu-gcc -nostdlib -shared -fPIC -O2 -fno-builtin
 *       -Wl,-soname,libcutils.so -o libqzcutils.so libcutils_shim.c
 */

/* —— bionic 系统接口（手工声明，规避 glibc sysroot 头文件）—— */
#define PROP_VALUE_MAX 92

/* 弱引用（weak）：Android 15+ 的 bionic libc 可能将 __system_property_* 归入
 * LIBC_PRIVATE，无版本 UND 引用按名解析失败会让 dlopen 整体失败（实机
 * 2026-11 v0.2.1 实证）。弱引用未解析时值为 NULL，运行时判空走默认值路径
 * —— dlopen 必成功，属性有则读、无则缺省（Mesa 的 zink 驱动选择由环境
 * 变量 GALLIUM_DRIVER/MESA_LOADER_DRIVER_OVERRIDE 锁定，不依赖属性）。 */
extern int __system_property_get(const char* name, char* value) __attribute__((weak));
extern int __system_property_set(const char* key, const char* value) __attribute__((weak));

/* —— nostdlib 自实现工具 —— */
static unsigned long qz_len(const char* s) {
    unsigned long n = 0;
    if (!s) return 0;
    while (s[n]) n++;
    return n;
}
static int qz_eq_any(const char* v, const char* const* list) {
    for (unsigned i = 0; list[i]; i++) {
        const char* a = v;
        const char* b = list[i];
        while (*a && *b && *a == *b) { a++; b++; }
        if (*a == *b) return 1;
    }
    return 0;
}
static void qz_copyz(char* d, const char* s) {
    while ((*d++ = *s++)) {}
}

/* —— property API（AOSP 语义）—— */
int property_get(const char* key, char* value, const char* default_value) {
    if (!value) return 0;
    if (key && __system_property_get && __system_property_get(key, value) > 0) {
        return (int)qz_len(value);
    }
    if (default_value) {
        qz_copyz(value, default_value);
        return (int)qz_len(default_value);
    }
    value[0] = '\0';
    return 0;
}

int property_set(const char* key, const char* value) {
    if (__system_property_set) {
        return __system_property_set(key, value ? value : "");
    }
    return 0;
}

int property_get_bool(const char* key, char default_value) {
    char v[PROP_VALUE_MAX];
    int n = property_get(key, v, (const char*)0);
    if (n <= 0) return default_value;
    static const char* const truthy[] = {"1", "true", "y", "yes", "on", (const char*)0};
    static const char* const falsy[]  = {"0", "false", "n", "no", "off", (const char*)0};
    if (qz_eq_any(v, truthy)) return 1;
    if (qz_eq_any(v, falsy)) return 0;
    return default_value;
}

/* 十进制（可带 0x）整型解析，无 libc 依赖 */
static long long qz_parse_ll(const char* v, int* ok) {
    long long acc = 0;
    int neg = 0, any = 0;
    const char* p = v;
    if (*p == '+' || *p == '-') { neg = (*p == '-'); p++; }
    if (p[0] == '0' && (p[1] == 'x' || p[1] == 'X')) {
        p += 2;
        for (; (*p >= '0' && *p <= '9') || (*p >= 'a' && *p <= 'f') || (*p >= 'A' && *p <= 'F'); p++) {
            int c = (*p <= '9') ? *p - '0' : ((*p | 32) - 'a' + 10);
            acc = acc * 16 + c; any = 1;
        }
    } else {
        for (; *p >= '0' && *p <= '9'; p++) { acc = acc * 10 + (*p - '0'); any = 1; }
    }
    *ok = any;
    return neg ? -acc : acc;
}

int property_get_int32(const char* key, int default_value) {
    char v[PROP_VALUE_MAX];
    int ok = 0;
    long long r;
    if (property_get(key, v, (const char*)0) <= 0) return default_value;
    r = qz_parse_ll(v, &ok);
    return ok ? (int)r : default_value;
}

long long property_get_int64(const char* key, long long default_value) {
    char v[PROP_VALUE_MAX];
    int ok = 0;
    long long r;
    if (property_get(key, v, (const char*)0) <= 0) return default_value;
    r = qz_parse_ll(v, &ok);
    return ok ? r : default_value;
}

int property_list(int (*propfn)(const char* name, const char* value, void* cookie), void* cookie) {
    (void)propfn;
    (void)cookie;
    return 0; /* 空列表：遍历实现无必要（Mesa 不依赖） */
}

/* —— atrace（systrace 打点，全部 no-op）—— */
unsigned long long atrace_enabled_tags = 0;
int atrace_marker_fd = -1;
char atrace_is_ready = 1;

unsigned long long atrace_get_enabled_tags(void) { return atrace_enabled_tags; }
unsigned long long atrace_update_tags(void) { return atrace_enabled_tags; }
void atrace_setup(void) {}
void atrace_init(void) {}
void atrace_set_tracing_enabled(int enabled) { (void)enabled; }
void atrace_begin_body(const char* name) { (void)name; }
void atrace_end_body(void) {}
void atrace_int_body(const char* name, int value) { (void)name; (void)value; }
void atrace_int64_body(const char* name, long long value) { (void)name; (void)value; }
void atrace_instant_body(const char* name) { (void)name; }
void atrace_instant_for_track_body(const char* track, const char* name) { (void)track; (void)name; }
void atrace_async_begin_body(const char* name, int cookie) { (void)name; (void)cookie; }
void atrace_async_end_body(const char* name, int cookie) { (void)name; (void)cookie; }
void atrace_async_for_track_begin_body(const char* track, const char* name, int cookie) { (void)track; (void)name; (void)cookie; }
void atrace_async_for_track_end_body(const char* track, const char* name, int cookie) { (void)track; (void)name; (void)cookie; }
