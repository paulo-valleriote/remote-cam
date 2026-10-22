// AGP 9 provides built-in Kotlin support; the Kotlin Android plugin is not
// applied. The buildscript classpath pins the Kotlin Gradle plugin version that
// the built-in support uses, so the compiler matches `libs.versions.toml`.
buildscript {
    repositories {
        google()
        mavenCentral()
    }
    dependencies {
        classpath("org.jetbrains.kotlin:kotlin-gradle-plugin:2.4.10")
    }
}

plugins {
    alias(libs.plugins.android.application) apply false
    alias(libs.plugins.android.library) apply false
}
