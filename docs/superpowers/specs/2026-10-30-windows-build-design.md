# Windows 11 build — design (2026-10-30)

## Intent

The user is setting up a Windows 11 virtual machine and wants the project to build and run
there: server, client and the X-Plane overlay plugin, with the same features as on Linux.
Release archives for Windows come from GitHub Actions like the Linux and macOS ones.

## User decisions

- The X-Plane plugin is built for Windows too (`win.xpl`).
- Whisper on Windows uses the same Vulkan backend as on Linux. The Vulkan SDK is installed
  on the build machine (the VM, and the CI runner); a GPU is not needed to build. One
  Windows variant only — no separate CPU-only build.
- The scripts in `tools/` get PowerShell versions.
- GitHub Actions builds Windows (hosted `windows-2022` runner).

## Goals

1. `cargo build --release` of `atc-server`, `atc-client` and `atc-xplane-plugin` works on
   Windows 11 x86-64. `tools/setup-windows.ps1` (elevated) installs everything with winget
   and builds: Git, Rust (rustup, MSVC toolchain), Visual Studio 2022 Build Tools (C++
   workload; its CMake is put on PATH via vswhere), LLVM (libclang: whisper-rs-sys's
   pre-generated bindings lack the Vulkan functions, so bindgen must run) and the Vulkan SDK.
   It also enables `LongPathsEnabled`: whisper.cpp's Vulkan shader generator builds deep
   under `target\` and MSBuild fails past 260 characters; the checkout should be short
   (`C:\vatc`). Verified in a Windows 11 VM on 2026-10-30: all three build, both executables
   start.
2. `cargo test --workspace` passes on Windows.
3. The plugin builds as a DLL installed as `Resources/plugins/atc-overlay/64/win.xpl`, linked
   to X-Plane's `XPLM_64.dll` without the X-Plane SDK (`#[link(name = "XPLM_64", kind =
   "raw-dylib")]` on Windows; Linux unchanged).
4. PowerShell scripts next to the bash ones, same behaviour and arguments:
   `fetch-whisper.ps1`, `fetch-piper.ps1` (Windows Piper `piper_windows_amd64.zip`, same
   release `2023.11.14-2`, same voices), `install-xplane-plugin.ps1` (default X-Plane path:
   the Steam default `C:\Program Files (x86)\Steam\steamapps\common\X-Plane 11`, argument
   overrides), `fetch-gateway-apt.ps1` (no `jq`/`base64` — PowerShell built-ins).
5. Config: the example configs work on Windows unchanged where possible. The Piper binary
   path: the server accepts the path with or without `.exe` (on Windows, if the configured
   file does not exist, try it with `.exe`) so one example config fits both systems.
6. CI: `ci.yml` gains a Windows job (build + tests; fmt/clippy stay on Linux), `build.yml`
   gains `windows-x86_64` producing a `.zip` with `bin\*.exe`, `plugin\atc-overlay\64\win.xpl`
   and `objects\`, config (personal configs from the examples), data, scenarios, tools, docs.
   Vulkan SDK installed in CI by a pinned action or the official installer (builder's choice,
   pinned version).
7. Docs: README "Tested with"/platform note and a "Windows" section in RUNNING.md
   (prerequisites with download links, build, scripts, plugin install, Vulkan note: without a
   Vulkan-capable driver Whisper runs on the CPU — to be confirmed in the VM — and on the CPU a
   smaller model than `large-v3-turbo` is advisable).

## Non-goals

- MSFS / X-Plane 12 (roadmap item 7).
- A Windows installer (MSI) or code signing.
- Cross-compiling from Linux.
- CUDA or other GPU backends.

## Anti-goals

- Breaking or changing the Linux/macOS build, scripts or behaviour.
- Requiring the X-Plane SDK or libclang for a normal build.

## Edge cases / my calls

- Unix-only code (`std::os::unix` in atc-stations store) stays behind `#[cfg(unix)]`; on Windows
  the OpenAIP key file relies on the per-user profile directory's ACLs (documented).
- Tests that assume Unix paths or line endings are fixed to be portable, not skipped, unless
  impossible (then `#[cfg(unix)]` with a comment).
- Git line endings: add `.gitattributes` so scripts and golden transcripts keep LF on Windows
  checkouts (golden comparisons must not break on CRLF).
- The terminal UI (crossterm) and audio (cpal/WASAPI), gamepad (gilrs) are expected to work;
  anything that does not compile is fixed in place.

## Left to the builder

- Exact CI action for the Vulkan SDK and its version.
- How the plugin's extern block is split per platform.
- PowerShell details (progress output, error handling), as long as the arguments match the
  bash scripts.

## Acceptance (user, in the VM)

Install the prerequisites, clone, `cargo build --release`, run `tools\fetch-whisper.ps1` and
`tools\fetch-piper.ps1`, start the server and the client in `--text` mode, get a controller
answer; if X-Plane is installed in the VM, install the plugin and see the overlay.
