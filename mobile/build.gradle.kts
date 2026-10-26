// Both modules are pure-JVM Kotlin (ADR-0018); the Android Gradle Plugin joins
// in F2. The buildscript classpath pins the Kotlin Gradle plugin version that
// `org.jetbrains.kotlin.jvm` resolves to, so the compiler matches
// `libs.versions.toml`.
buildscript {
    repositories {
        google()
        mavenCentral()
    }
    dependencies {
        classpath("org.jetbrains.kotlin:kotlin-gradle-plugin:2.4.10")
    }
}
