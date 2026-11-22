plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.opentube.app"
    compileSdk = 36

    defaultConfig {
        applicationId = "com.opentube.app"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
        }
    }

    packaging {
        resources {
            excludes += setOf("META-INF/*.kotlin_module", "META-INF/AL.txt", "META-INF/LICENSE")
        }
    }
}

dependencies {
    implementation("androidx.appcompat:appcompat:1.7.1")
}
