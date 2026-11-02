# Modern Day Pioneer: the app

The Windows app, built with Tauri 2 on top of [`../mdp-core`](../mdp-core).
The screens are plain HTML, CSS and JavaScript in `ui/`, with no build step.
The Rust side is in `src-tauri/`.

## Installing it

Download the newest `x64-setup.exe` from this repository's
[Releases](https://github.com/Modern-Day-Pioneer/modern-day-pioneer-app/releases) page
and run it. It installs into your own user profile
(`%LOCALAPPDATA%\Programs`), so it needs no administrator rights. The
installer isn't signed yet, so Windows may say it is unrecognized: choose
**More info**, then **Run anyway**. Installing a newer version over an older
one keeps your Journeys.

## Which version am I running?

Versions look like `0.<sprint>.<build number>`, for example `0.12.80`. The
version is in the installer's name, the window title, and Settings > About
this app (with the commit and date, and a Copy version button). A build made
on your own PC says "Local build". Settings > About also has **Check for a
newer version** and **Open the log folder**.

## Building it yourself

You need Rust, Node.js, CMake and LLVM (with `LIBCLANG_PATH` pointing at
LLVM's `bin` folder), because the app compiles whisper.cpp for speech to text.

    cd mdp-app
    npx @tauri-apps/cli@2 dev      # run it
    npx @tauri-apps/cli@2 build    # make the installer

Copies of HEIC, HEVC, MOV, Opus and M4A files are made with FFmpeg. Release
builds put an LGPL build of `ffmpeg.exe` beside the app (see
`src-tauri/tauri.ffmpeg.json`). On your own PC the app uses `MDP_FFMPEG`, or
`ffmpeg` on the PATH; without it, files are still brought in and kept.

## Checking the screens without Windows

`ui-check/` drives the screens in an ordinary browser against the real core
and runs an accessibility scan (axe) in light and dark mode:

    cd mdp-core && cargo build --example ui_server
    cd ../mdp-app/ui-check && npm i --no-save playwright axe-core
    node check12.mjs <folder for screenshots>

Each `check*.mjs` covers the screens of one sprint. `export-checklist.mjs`
opens every exported page offline, with scripts off and at phone width.
