// QUARANTINED F0 RECORD — NON-PRODUCTION. Not wired into the mobile/ Gradle
// build and not built. See the module README and ADR-0003.
plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.serialization)
}

android {
    namespace = "remotecam.app"
    compileSdk = 36

    defaultConfig {
        applicationId = "com.remotecam.app"
        minSdk = 29
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    implementation(project(":core"))
    implementation(project(":protocol"))
    implementation(libs.androidx.camera.core)
    implementation(libs.androidx.camera.camera2)
    implementation(libs.androidx.camera.lifecycle)
    implementation(libs.androidx.lifecycle.runtime)
    implementation(libs.webrtc.android)
    implementation(libs.zxing.core)
    implementation(libs.okhttp)
    implementation(libs.okio)
    implementation(libs.coroutines.android)
}
