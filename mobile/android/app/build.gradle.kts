// The Quillo app for Android: one WebView on the program, which the runtime
// (libquillo_mobile.so, in src/main/jniLibs, made by ../build.sh) serves on
// 127.0.0.1 inside the app. The program is in src/main/assets/program.
//
// The app's name, id, version and icon come from the program's app.toml, which
// ../build.sh passes as QUILLO_APP_* environment variables; a release is signed
// with the key in QUILLO_ANDROID_KEYSTORE and the rest (see ../build.sh).
plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

fun env(name: String, default: String) = System.getenv(name)?.takeIf { it.isNotEmpty() } ?: default
val keystore = System.getenv("QUILLO_ANDROID_KEYSTORE")?.takeIf { it.isNotEmpty() }

android {
    // The Kotlin code's own package, which the runtime's JNI entry is named after;
    // the app's id (applicationId) is the program's own.
    namespace = "org.quillolang.app"
    compileSdk = 36
    defaultConfig {
        applicationId = env("QUILLO_APP_ID", "org.quillolang.app")
        minSdk = 26
        targetSdk = 36
        versionCode = env("QUILLO_APP_BUILD", "1").toInt()
        versionName = env("QUILLO_APP_VERSION", "0.1.0")
        resValue("string", "app_name", env("QUILLO_APP_NAME", "Quillo"))
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }
    signingConfigs {
        if (keystore != null) {
            create("release") {
                storeFile = file(keystore)
                storePassword = System.getenv("QUILLO_ANDROID_STORE_PASSWORD")
                keyAlias = System.getenv("QUILLO_ANDROID_KEY_ALIAS")
                keyPassword = System.getenv("QUILLO_ANDROID_KEY_PASSWORD") ?: System.getenv("QUILLO_ANDROID_STORE_PASSWORD")
            }
        }
    }
    buildTypes {
        release {
            isMinifyEnabled = false
            if (keystore != null) signingConfig = signingConfigs.getByName("release")
        }
    }
    // The launcher icons ../build.sh makes from the program's icon.
    sourceSets["main"].res.srcDir(layout.buildDirectory.dir("generated/quillo/res"))
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    buildFeatures { resValues = true }
    // The program's files are served as they are: keep them uncompressed and whole.
    androidResources { noCompress += listOf("qwe", "db") }
}

kotlin { compilerOptions { jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17) } }

dependencies {
    // WebViewCompat: the native bridge for the program's own origin only, and a
    // script put on its pages before theirs.
    implementation("androidx.webkit:webkit:1.14.0")
    // FileProvider: the camera writes a photo where the page can be given it.
    implementation("androidx.core:core:1.16.0")
}
