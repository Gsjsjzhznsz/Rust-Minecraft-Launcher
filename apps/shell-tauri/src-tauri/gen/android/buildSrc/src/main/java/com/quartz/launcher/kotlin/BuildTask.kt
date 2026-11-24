import java.io.File
import org.apache.tools.ant.taskdefs.condition.Os
import org.gradle.api.DefaultTask
import org.gradle.api.GradleException
import org.gradle.api.logging.LogLevel
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.TaskAction
import javax.inject.Inject
import org.gradle.process.ExecOperations

abstract class BuildTask : DefaultTask() {
    @get:Inject
    abstract val execOperations: ExecOperations

    @Input
    var rootDirRel: String? = null
    @Input
    var projectDir: String? = null
    @Input
    var target: String? = null
    @Input
    var release: Boolean? = null

    @TaskAction
    fun assemble() {
        // 空间受限环境：已通过 tauri CLI 预编译好 .so 时，设置此变量跳过 cargo
        //（复用 target/<triple>/<profile>/libquartz_shell_lib.so）
        if (System.getenv("QUARTZ_SKIP_CARGO_BUILD") == "1") {
            logger.lifecycle("[quartz] QUARTZ_SKIP_CARGO_BUILD=1 —— 跳过 cargo，复用已编译的动态库")
            return
        }
        // 修复：全局 npm CLI 入口（原模板 `node tauri ...` 无法解析 main.js）
        val executable = """node""";
        try {
            runTauriCli(executable)
        } catch (e: Exception) {
            if (Os.isFamily(Os.FAMILY_WINDOWS)) {
                // Try different Windows-specific extensions
                val fallbacks = listOf(
                    "$executable.exe",
                    "$executable.cmd",
                    "$executable.bat",
                )

                var lastException: Exception = e
                for (fallback in fallbacks) {
                    try {
                        runTauriCli(fallback)
                        return
                    } catch (fallbackException: Exception) {
                        lastException = fallbackException
                    }
                }
                throw lastException
            } else {
                throw e;
            }
        }
    }

    fun runTauriCli(executable: String) {
        val rootDirRel = rootDirRel ?: throw GradleException("rootDirRel cannot be null")
        val target = target ?: throw GradleException("target cannot be null")
        val release = release ?: throw GradleException("release cannot be null")
        // tauri-cli JS 入口动态解析（本地与 CI 通用）：
        // 1) QUARTZ_TAURI_CLI_JS 显式指定  2) NPM_CONFIG_PREFIX 派生（setup-env.sh 已导出）
        // 3) 旧默认前缀兜底
        val cliJs = System.getenv("QUARTZ_TAURI_CLI_JS")
            ?: System.getenv("NPM_CONFIG_PREFIX")?.let { "$it/lib/node_modules/@tauri-apps/cli/tauri.js" }
            ?: "/home/z/.npm-global/lib/node_modules/@tauri-apps/cli/tauri.js"
        val args = listOf(cliJs, "android", "android-studio-script");

        execOperations.exec {
            workingDir(File(projectDir, rootDirRel))
            executable(executable)
            args(args)
            if (logger.isEnabled(LogLevel.DEBUG)) {
                args("-vv")
            } else if (logger.isEnabled(LogLevel.INFO)) {
                args("-v")
            }
            if (release) {
                args("--release")
            }
            args(listOf("--target", target))
        }.assertNormalExitValue()
    }
}