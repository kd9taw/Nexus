# Building & running Nexus on Linux

The counterpart to [WINDOWS.md](WINDOWS.md). The apt line in
[CONTRIBUTING.md](CONTRIBUTING.md) is enough to run `cargo test --workspace` — it installs the
**modem** toolchain and nothing else. Building the actual desktop app needs the GTK/WebKit stack
too, and finding that out one missing package at a time is the reason this page exists
(reported in issue #18).

## What you need

Nexus's modem (`libtempo`) is **Fortran + C/C++ + FFTW**, built through CMake by
`tempo-fast-sys`'s build script. The shell is **Tauri v2**, so the app itself needs
**WebKitGTK** and **GTK 3**. CAT and PTT go through Hamlib's `rigctld`.

On Debian/Ubuntu — this is the complete list, and it is the same one
`scripts/build-linux.sh` checks for:

```sh
sudo apt install \
  build-essential cmake ninja-build gfortran pkg-config \
  libfftw3-dev libboost-dev libssl-dev \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  librsvg2-dev libxdo-dev libsoup-3.0-dev \
  patchelf libfuse2 \
  nodejs npm \
  libhamlib-utils
```

Plus [rustup](https://rustup.rs) with the pinned toolchain — CI uses **1.93.1**, and matching it
locally is what makes `cargo clippy` agree with CI.

A few of those are worth a word:

- **`libwebkit2gtk-4.1-dev`** is the one people miss. It is not needed for `cargo test`, so a
  headless developer never hits it, and its absence shows up as a `pkg-config` failure deep in the
  Tauri build rather than as anything that names WebKit.
- **`libfuse2`** is only for running the built AppImage. The bundler itself works without it
  (`APPIMAGE_EXTRACT_AND_RUN=1`, which is what CI sets).
- **`libhamlib-utils`** gives you `rigctl` and `rigctld`. Installing it **changes which tests
  actually run** — see below.

## Build

```sh
cargo build                       # workspace (modem, engine, net)
cargo test --workspace            # headless: no sound card, no radio

npm --prefix ui ci
npm --prefix ui run build         # tsc -b && vite build

./scripts/build-linux.sh          # the .deb + AppImage, as shipped
```

`build-linux.sh` reports anything missing before it starts, rather than failing partway.

## Tests that depend on what you have installed

This surprises people, and it has cost real time twice:

- **With `libhamlib-utils` installed**, the tests that drive a real `rigctl`/`rigctld` actually run.
  **Without it they SKIP**, and skipping prints as success. A contributor investigating issue #18
  reported a test as "does not reproduce" when it had in fact never executed — seven silent skips in
  a clean container. If you are chasing a rigctld-related failure somebody else sees, check first
  that you have Hamlib at all.
- The reverse also bit us: a test that spawns a real daemon hung every CI run for six hours while
  passing instantly on any machine without Hamlib installed.

So: `which rigctld` before drawing a conclusion from a green run in this area.

## The two things that are NOT in the repo

Both are gitignored, and both fail quietly rather than loudly if you build without them — worth
knowing when you build from a fresh clone or a **git worktree**, which checks out tracked files
only:

- **`src-tauri/resources/deepcw/model.onnx`** — the AI CW decoder weights (AGPL-3.0, © e04). Absent,
  the build succeeds and simply has no AI CW decoder. `build.rs` offers
  `NEXUS_ALLOW_MISSING_AICW=1` to say you meant it.
- **`src-tauri/resources/hamlib/*.exe` / `*.dll`** — the bundled Windows Hamlib, used by the
  Windows installer and by `scripts/gen-hamlib-serial-speeds.mjs`.

## Which Linux

The published `.deb` and AppImage are built on **Ubuntu 24.04** and need **glibc 2.39 or newer** —
Ubuntu 24.04+, Debian 13+, Fedora 40+, Mint 22+. Check with `ldd --version`. Building from source
has no such floor: you get whatever your own toolchain produces.

The AppImage is no exception to the floor. An AppImage carries the application's libraries, not the
system C library, so it needs the same minimum the `.deb` does.

## Cross-building the Windows installer from Linux

`./scripts/build-windows-cross.sh` produces `Nexus.exe` and the NSIS installer using the MinGW-w64
cross toolchain, with no MSYS2 and no Windows machine. It lists its own prerequisites
(`gcc-mingw-w64-x86-64`, `g++-mingw-w64-x86-64`, `gfortran-mingw-w64-x86-64`, and the rest) and
reports what is missing before it starts.
