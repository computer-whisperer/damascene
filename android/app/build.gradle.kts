plugins {
    id("com.android.application")
}

val rustTarget = "aarch64-linux-android"
val rustAbi = "arm64-v8a"
val generatedRustJniLibs = layout.buildDirectory.dir("generated/rustJniLibs")

fun androidHostTag(): String {
    val os = System.getProperty("os.name").lowercase()
    val arch = System.getProperty("os.arch").lowercase()
    return when {
        os.contains("linux") -> "linux-x86_64"
        os.contains("mac") && (arch == "aarch64" || arch == "arm64") -> "darwin-aarch64"
        os.contains("mac") -> "darwin-x86_64"
        os.contains("windows") -> "windows-x86_64"
        else -> error("Unsupported Android NDK host: os=$os arch=$arch")
    }
}

android {
    namespace = "com.cjbal.damascene.showcase"
    compileSdk = 35
    ndkVersion = "27.0.12077973"

    defaultConfig {
        applicationId = "com.cjbal.damascene.showcase"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0-dev"

        ndk {
            abiFilters += rustAbi
        }
    }

    sourceSets {
        named("main") {
            jniLibs.srcDir(generatedRustJniLibs)
        }
    }

    packaging {
        jniLibs {
            useLegacyPackaging = true
        }
    }
}

val cargoBuildArm64Debug = tasks.register<Exec>("cargoBuildArm64Debug") {
    val workspaceDir = rootProject.layout.projectDirectory.dir("..").asFile
    val hostTag = androidHostTag()
    val clang = android.ndkDirectory.resolve(
        "toolchains/llvm/prebuilt/$hostTag/bin/aarch64-linux-android26-clang"
    )
    val llvmAr = android.ndkDirectory.resolve(
        "toolchains/llvm/prebuilt/$hostTag/bin/llvm-ar"
    )

    workingDir = workspaceDir
    commandLine(
        "cargo",
        "build",
        "-p",
        "damascene-android-showcase",
        "--lib",
        "--release",
        "--target",
        rustTarget,
    )
    environment("ANDROID_NDK_HOME", android.ndkDirectory.absolutePath)
    environment("ANDROID_NDK_ROOT", android.ndkDirectory.absolutePath)
    environment("CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER", clang.absolutePath)
    environment("CC_aarch64_linux_android", clang.absolutePath)
    environment("AR_aarch64_linux_android", llvmAr.absolutePath)

    doFirst {
        check(clang.exists()) {
            "Android NDK clang not found at $clang. Install NDK ${android.ndkVersion} or run Gradle with network access so AGP can provision it."
        }
    }
}

val copyRustLibArm64Debug = tasks.register<Copy>("copyRustLibArm64Debug") {
    dependsOn(cargoBuildArm64Debug)
    from(rootProject.layout.projectDirectory.file("../target/$rustTarget/release/libmain.so"))
    into(generatedRustJniLibs.map { it.dir(rustAbi) })
}

tasks.named("preBuild") {
    dependsOn(copyRustLibArm64Debug)
}
