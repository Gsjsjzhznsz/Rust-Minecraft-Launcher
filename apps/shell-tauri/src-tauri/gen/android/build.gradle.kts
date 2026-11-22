buildscript {
    repositories {
        google()
        mavenCentral()
    }
    dependencies {
        classpath("com.android.tools.build:gradle:9.3.1")
        classpath("org.jetbrains.kotlin:kotlin-gradle-plugin:2.2.10")
    }
}

allprojects {
    repositories {
        google()
        mavenCentral()
    }
    // 根卷（9.9G thin 卷）配额已满（df: 0 available）：构建中间产物全部
    // 重定向到 PolarFS 持久盘（/tmp/my-project，容量 64P、实测写 ~120MB/s）。
    // 源码与依赖缓存仍在根卷读取，仅输出落点变更，不影响构建正确性。
    layout.buildDirectory.set(file("/tmp/my-project/.qz-gradle-build/${project.name}"))
}

tasks.register("clean").configure {
    delete("build")
}

