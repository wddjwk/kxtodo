import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

android {
    compileSdk = 36
    namespace = "com.wddjwk.kxtodo"
    signingConfigs {
        create("release") {
            val envPath = System.getenv("TAURI_ANDROID_KEYSTORE_PATH")
            if (envPath != null) {
                // 从环境变量读取自定义 keystore（CI/CD 或生产签名）
                storeFile = file(envPath)
                storePassword = System.getenv("TAURI_ANDROID_KEYSTORE_PASSWORD") ?: ""
                keyAlias = System.getenv("TAURI_ANDROID_KEY_ALIAS") ?: ""
                keyPassword = System.getenv("TAURI_ANDROID_KEY_PASSWORD") ?: ""
            } else {
                // 回退到项目本地 keystore（由 package.ps1 自动生成）
                storeFile = file("../keystore/release.jks")
                storePassword = "kxtodo"
                keyAlias = "kxtodo"
                keyPassword = "kxtodo"
            }
        }
    }
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "com.wddjwk.kxtodo"
        minSdk = 24
        targetSdk = 36
        // 版本号唯一来源是 git：package.ps1 构建时注入环境变量 KXTODO_VERSION（X.Y.Z）。
        // versionCode 以 900000000 为基线：旧构建曾因过期 versionName 8.2.1 产生
        // versionCode 8002001，基线保证升级安装永不因版本号回退而降级；
        // 每段三位（*1000000/*1000）避免 0.2.100 与 0.3.0 这类进位碰撞。
        val envVersion = System.getenv("KXTODO_VERSION")
            ?.let { Regex("""^(\d+)\.(\d+)\.(\d+)$""").matchEntire(it) }
        if (envVersion != null) {
            val (major, minor, patch) = envVersion.destructured
            versionName = "$major.$minor.$patch"
            versionCode = 900000000 + major.toInt() * 1000000 + minor.toInt() * 1000 + patch.toInt()
        } else {
            versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
            versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
        }
    }
    buildTypes {
        getByName("debug") {
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            isMinifyEnabled = true
            signingConfig = signingConfigs.getByName("release")
            proguardFiles(
                *fileTree(".") { include("**/*.pro") }
                    .plus(getDefaultProguardFile("proguard-android-optimize.txt"))
                    .toList().toTypedArray()
            )
        }
    }
    kotlinOptions {
        jvmTarget = "1.8"
    }
    buildFeatures {
        buildConfig = true
    }
}

rust {
    rootDirRel = "../../../"
}

dependencies {
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = "tauri.build.gradle.kts")