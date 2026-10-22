plugins {
    alias(libs.plugins.android.application)
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
}
