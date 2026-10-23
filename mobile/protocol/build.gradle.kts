import org.gradle.testing.jacoco.tasks.JacocoReport

plugins {
    alias(libs.plugins.android.library)
    alias(libs.plugins.kotlin.serialization)
    jacoco
}

android {
    namespace = "remotecam.protocol"
    compileSdk = 36

    defaultConfig {
        minSdk = 29
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildTypes {
        debug {
            enableUnitTestCoverage = true
        }
    }

    testOptions {
        unitTests.all {
            it.useJUnitPlatform()
        }
    }
}

dependencies {
    implementation(libs.kotlinx.serialization.json)

    testImplementation(libs.junit.jupiter)
    testImplementation(libs.kotlin.test.junit5)
}

jacoco {
    toolVersion = libs.versions.jacoco.get()
}

tasks.register<JacocoReport>("jacocoTestReport") {
    dependsOn("testDebugUnitTest")
    onlyIf { layout.buildDirectory.file("jacoco/testDebugUnitTest.exec").get().asFile.exists() }

    reports {
        xml.required.set(true)
        html.required.set(true)
    }

    val javaClasses = fileTree(layout.buildDirectory.dir("intermediates/javac/debug/classes")) {
        exclude("**/R.class", "**/R\$*.class", "**/BuildConfig.*", "**/Manifest*.*")
    }
    val kotlinClasses = fileTree(layout.buildDirectory.dir("tmp/kotlin-classes/debug"))

    classDirectories.setFrom(javaClasses, kotlinClasses)
    sourceDirectories.setFrom(files("src/main/kotlin", "src/main/java"))
    executionData.setFrom(layout.buildDirectory.file("jacoco/testDebugUnitTest.exec"))
}
