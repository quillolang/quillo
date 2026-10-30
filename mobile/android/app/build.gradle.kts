// The Quillo app for Android: one WebView on the program, which the runtime
// (libquillo_mobile.so, in src/main/jniLibs, made by ../build.sh) serves on
// 127.0.0.1 inside the app. The program is in src/main/assets/program.
plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "org.quillolang.app"
    compileSdk = 35
    defaultConfig {
        applicationId = "org.quillolang.app"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.6.0"
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }
    buildTypes {
        release { isMinifyEnabled = false }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    // The program's files are served as they are: keep them uncompressed and whole.
    androidResources { noCompress += listOf("qwe", "db") }
}
