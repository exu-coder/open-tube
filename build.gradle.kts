// Top-level build file for Open tube (Android app).
// Plugin versions live here with `apply false`; settings.gradle.kts supplies
// the repositories via pluginManagement and dependencyResolutionManagement.

plugins {
    id("com.android.application") version "8.13.2" apply false
    id("org.jetbrains.kotlin.android") version "2.3.21" apply false
}

tasks.register<Delete>("clean") {
    delete(rootProject.layout.buildDirectory)
}
