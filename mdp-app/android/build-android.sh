#!/usr/bin/env bash
# Build the Android app (sprint 9) on Linux: used by the GitHub workflow and
# by hand. Needs ANDROID_HOME and NDK_HOME, the aarch64-linux-android Rust
# target, Java 17 or newer, Ninja and LLVM's libclang. Extra arguments go to
# `tauri android build` (for example --debug).
set -euo pipefail
cd "$(dirname "$0")/.."
here="$(pwd)/android"
: "${ANDROID_HOME:?set ANDROID_HOME}" "${NDK_HOME:?set NDK_HOME}"
llvm="$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64"
clang_inc="$(ls -d "$llvm"/lib/clang/*/include | head -1)"
export CMAKE_TOOLCHAIN_FILE_aarch64_linux_android="$here/whisper-toolchain.cmake"
export CMAKE_GENERATOR_aarch64_linux_android=Ninja
export BINDGEN_EXTRA_CLANG_ARGS_aarch64_linux_android="--sysroot=$llvm/sysroot --target=aarch64-linux-android26 -I$clang_inc"
export LIBCLANG_PATH="${LIBCLANG_PATH:-$(ls -d /usr/lib/llvm-*/lib | sort -V | tail -1)}"
# whisper.cpp tuned for any 64-bit ARM phone, not the build machine.
export GGML_NATIVE=OFF
# What Android's app info shows as the version (Jared, 2026-10-20): the full
# version with its build, commit and date. GitHub jobs set MDP_VERSION,
# MDP_BUILD, MDP_COMMIT and MDP_BUILD_DATE; a build by hand gives its fix
# number as MDP_FIX (MDP_FIX=4 makes "0.9.0-fix4"), shown in Settings > About too.
version="${MDP_VERSION:-$(node -p "require('./src-tauri/tauri.conf.json').version")}"
export MDP_COMMIT="${MDP_COMMIT:-$(git rev-parse --short=7 HEAD)}"
export MDP_BUILD_DATE="${MDP_BUILD_DATE:-$(date +%Y-%m-%d)}"
if [ -n "${MDP_BUILD:-}" ]; then
  export MDP_VERSION_NAME="$version (build $MDP_BUILD, $MDP_COMMIT, $MDP_BUILD_DATE)"
else
  export MDP_BUILD="local${MDP_FIX:+ fix $MDP_FIX}"
  export MDP_VERSION_NAME="$version${MDP_FIX:+-fix$MDP_FIX} ($MDP_BUILD_DATE, $MDP_COMMIT)"
fi
echo "Version shown on the phone: $MDP_VERSION_NAME"
[ -d node_modules/@tauri-apps/cli ] || npm ci
npx tauri android build --apk --target aarch64 "$@"
