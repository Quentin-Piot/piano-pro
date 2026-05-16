build-app:
	cargo build --release --bin neothesia
run-app:
	cargo run --release --bin neothesia

install-app:
	cargo install --path neothesia

check-recorder:
	cargo check -p neothesia-cli
build-recorder:
	cargo build --release -p neothesia-cli
run-recorder:
	cargo run --release -p neothesia-cli -- $(file)

build-web:
	cd neothesia-web && env -u NO_COLOR trunk build --release

serve-web:
	cd neothesia-web && env -u NO_COLOR trunk serve --open

ANDROID_PROJECT = neothesia-android/android-project
ANDROID_JNI_DIR = $(ANDROID_PROJECT)/app/src/main/jniLibs

check-android:
	cargo ndk -t arm64-v8a --platform 26 check -p neothesia-android

build-android:
	cargo ndk -t arm64-v8a -t armeabi-v7a --platform 26 \
		-o $(ANDROID_JNI_DIR) \
		build -p neothesia-android --release
	cd $(ANDROID_PROJECT) && ./gradlew assembleDebug

install-android:
	cd $(ANDROID_PROJECT) && ./gradlew installDebug

run-android:
	adb shell am start -n com.pianopro.app/android.app.NativeActivity
