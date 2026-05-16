plugins {
    id("com.android.application")
}

android {
    namespace = "com.pianopro.app"
    compileSdk = 35
    ndkVersion = "29.0.14206865"

    defaultConfig {
        applicationId = "com.pianopro.app"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.4.0"
    }

    buildTypes {
        debug {
            isDebuggable = true
        }
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    sourceSets {
        getByName("main") {
            jniLibs.srcDirs("src/main/jniLibs")
        }
    }
}
