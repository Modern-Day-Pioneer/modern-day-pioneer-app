# whisper.cpp's CMake build for the phone (sprint 9). whisper-rs-sys builds it
# with cmake-rs, which doesn't set up the Android NDK by itself, so this file
# picks the phone's processor and oldest Android version, then hands over to
# the NDK's own toolchain file. Used through CMAKE_TOOLCHAIN_FILE_aarch64_linux_android
# (see build-android.sh); plain CMAKE_* variables would be passed on as defines.
set(ANDROID_ABI arm64-v8a)
set(ANDROID_PLATFORM android-26)
include($ENV{NDK_HOME}/build/cmake/android.toolchain.cmake)
