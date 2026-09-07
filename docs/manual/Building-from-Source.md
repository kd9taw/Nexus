# Building from Source

You don't need to build Nexus to use it — most operators should just grab the installer (see [Getting Started](Getting-Started.md)). Build from source if you'd rather not run an unsigned binary, want to develop, or are porting.

Nexus's modem (`libtempo`) is **Fortran + C/C++ + FFTW**, so the Windows build uses the **GNU toolchain** (MSVC has no Fortran) and the Rust **`x86_64-pc-windows-gnu`** target. The full, authoritative build guide is [`WINDOWS.md`](../../WINDOWS.md); developer setup and crate layout are in [`CONTRIBUTING.md`](../../CONTRIBUTING.md). This page condenses both.

---

## Path A — Native Windows (MSYS2 UCRT64)

Produces the NSIS installer + MSI.

1. **Install [MSYS2](https://www.msys2.org/)**, open the **MSYS2 UCRT64** shell, and install the toolchain:

   ```bash
   pacman -S --needed \
     mingw-w64-ucrt-x86_64-gcc \
     mingw-w64-ucrt-x86_64-gcc-fortran \
     mingw-w64-ucrt-x86_64-cmake \
     mingw-w64-ucrt-x86_64-ninja \
     mingw-w64-ucrt-x86_64-fftw \
     mingw-w64-ucrt-x86_64-boost \
     mingw-w64-ucrt-x86_64-pkgconf
   ```

2. **Install Rust (GNU target) + Node + WebView2:**
   - Rust via rustup, then `rustup default stable-x86_64-pc-windows-gnu` (must be **`-gnu`**, not `-msvc`).
   - [Node.js LTS](https://nodejs.org/) for the UI build.
   - `cargo install tauri-cli --version "^2"`.
   - WebView2 runtime: preinstalled on Windows 11; on Windows 10 install Microsoft's Evergreen runtime.

3. **Build** — from the **MSYS2 UCRT64** shell (so gfortran/cmake/ninja/FFTW/Boost are on `PATH`):

   ```bash
   ./scripts/build-windows.sh
   ```

   …or from PowerShell (it finds MSYS2 and runs the above inside UCRT64):

   ```powershell
   scripts\build-windows.ps1
   ```

   Useful flags: `--no-radio`, `--dev`, `--check`.

The script handles toolchain checks, icons, and UI deps. To do it by hand: `npm --prefix ui install` once, then `cargo tauri dev --features radio` (live) or `cargo tauri build --features radio` (release).

---

## Path B — Cross-compile from Linux / WSL2

No MSYS2 needed; produces the same installer.

1. **Install the cross toolchain** (Debian/Ubuntu/WSL2):

   ```bash
   sudo apt install gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64 \
     gfortran-mingw-w64-x86-64 cmake ninja-build nodejs npm nsis
   ```

2. Add the Rust target: `rustup target add x86_64-pc-windows-gnu`, and `cargo install tauri-cli --version "^2"`.

3. **Build:**

   ```bash
   ./scripts/build-windows-cross.sh
   ```

   The script cross-builds a static single-precision **FFTW3f** for MinGW (cached), links the whole modem stack statically (libtempo → gfortran → quadmath → stdc++ → fftw3f) so the result needs no MinGW runtime DLLs, and produces the NSIS installer.

> The **published Windows binaries are produced this way** — cross-compiled on Linux and unsigned, which is why SmartScreen warns and why each release publishes a SHA-256. Report what breaks. The Linux and Raspberry Pi packages are built natively; see `scripts/build-linux.sh` and the release workflow.

---

## Path C — macOS (Apple Silicon)

Produces `Nexus.app` and the `.dmg`. The full guide is [`MACOS.md`](../../MACOS.md); this is the short version.

1. **Install the toolchain:**

   ```bash
   xcode-select --install
   brew install cmake ninja gcc fftw boost node
   ```

   `gcc` is what provides `gfortran` — there is no standalone formula, and macOS ships no system Fortran compiler. Then rustup (the default host toolchain; no target to add) and `cargo install tauri-cli --version "^2"`.

2. **Point `pkg-config` at Homebrew** — `export PKG_CONFIG_PATH="$(brew --prefix)/lib/pkgconfig"`. Both the CMake project and `tempo-fast-sys/build.rs` find FFTW this way, and a MacPorts `pkg-config` earlier on `PATH` cannot see Homebrew's `.pc` files: the failure reads as `No package 'fftw3f' found` on a machine where FFTW is installed.

3. **Build:**

   ```bash
   npm --prefix ui ci && npm --prefix ui run build
   cd src-tauri && cargo tauri build --features radio,custom-protocol --bundles app,dmg
   ```

**Apple Silicon only, and there is no universal binary:** a universal build means building the Fortran/FFTW modem twice and `lipo`-ing it, and Homebrew's `gfortran` is single-arch. Intel is a source build that works but ships no binaries. The modem's Fortran runtimes (gfortran, quadmath, fftw3f, plus GCC's `libemutls_w.a`) are linked **statically** — a dynamically-linked `.app` runs on the machine that built it and dies at load on any Mac without your Homebrew tree. And a build you sign yourself is not notarized, so Gatekeeper blocks it on a double-click: right-click ▸ **Open** once. `MACOS.md` covers all three.

---

## Headless modem / engine tests (any platform)

You don't need WebView2 or a radio to run the test suite — just the native modem toolchain so `ft1-sys` can build `libtempo` via CMake.

On Debian/Ubuntu (or WSL2):

```bash
sudo apt install gfortran cmake ninja-build libfftw3-dev libboost-dev pkg-config
```

You need **single-precision FFTW3** (in `libfftw3-dev`), **Boost headers**, **CMake**, **Ninja**, and **gfortran**, plus a Rust toolchain. Then from the repo root:

```bash
cargo build
cargo test          # modem FFI, engine, QSO/Field Day, networking, TempoDeep round-trips
```

These run headless — no sound card or radio required. UI build: `npm --prefix ui install` then `npm --prefix ui run build`.

Before opening a PR, the project asks for a clean `cargo fmt --all`, `cargo clippy --all-targets`, and `cargo test` (plus the UI build if you touched `ui/`). See [`CONTRIBUTING.md`](../../CONTRIBUTING.md).

---

## What gets built

- `Nexus_<version>_x64-setup.exe` — the NSIS installer (per-user; bundles offline WebView2 + Hamlib `rigctld`).
- `nexus.exe` — the app.
- `win_smoke.exe` — a fully-static modem self-test.

The GUI is built with `--features radio,custom-protocol` (`radio` = `device` + `serial`; `custom-protocol` embeds the UI assets so the WebView shows the bundled UI).

---

## See also

- [Architecture and Protocol](Architecture-and-Protocol.md) — the layer-by-layer design.
- [`WINDOWS.md`](../../WINDOWS.md) — the full Windows build/setup reference.
- [`MACOS.md`](../../MACOS.md) — the full macOS build reference.
- [`CONTRIBUTING.md`](../../CONTRIBUTING.md) — crate layout, code style, PR workflow.
