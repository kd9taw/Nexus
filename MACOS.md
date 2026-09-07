# Building & running Nexus on macOS

The counterpart to [LINUX.md](LINUX.md) and [WINDOWS.md](WINDOWS.md). macOS has been a shipped
platform since 1.5.0 and [`docs/install.md`](docs/install.md) covers installing the signed,
notarized `.dmg` — which is what most operators want. This page is for building it yourself, and
it exists because until now the only description of how was `.github/workflows/release.yml`
(reported in issue #123).

Nexus's modem (`libtempo`) is **Fortran + C/C++ + FFTW**, built through CMake by
`tempo-fast-sys`'s build script — the same **native** (host == target) path Linux uses. The Tauri
v2 shell renders in the OS's own **WKWebView**, so unlike Windows there is no web runtime to
install. Audio is **CoreAudio** via `cpal`, no extra libraries. CAT and PTT go through Hamlib's
`rigctld`, which the bundle carries — see [Hamlib](#hamlib-is-not-in-the-repo-either) below,
because on macOS nothing makes you stage it.

## Apple Silicon

The published `.dmg` is **arm64 only, deliberately**, and there is no universal binary. A
universal build means building `libtempo` — Fortran, FFTW and Boost through CMake — twice and
`lipo`-ing the results, and Homebrew's `gfortran` is single-arch: there is no x86_64 Fortran and
FFTW toolchain on an Apple Silicon machine to build the other half with. Apple Silicon covers
every Mac sold since 2020.

Intel is a **source build**, and it works: nothing below is arm-specific, and
`tempo-fast-sys/build.rs` asks the toolchain where its own libraries are rather than hardcoding a
prefix, so it is correct on both `/opt/homebrew` and `/usr/local`. It is simply not a platform
anyone ships binaries for. An Intel-built `.dmg` will not run on Apple Silicon.

## What you need

Xcode's Command Line Tools for `clang` and `ld`, then Homebrew:

```sh
xcode-select --install
brew install cmake ninja gcc fftw boost node
brew install libusb          # only if you will stage Hamlib for a bundle (see below)
```

Plus [rustup](https://rustup.rs) with the pinned toolchain — CI uses **1.93.1**, and matching it
locally is what makes `cargo clippy` agree with CI — and the Tauri CLI:
`cargo install tauri-cli --version "^2"`.

Some of that is worth a word:

- **`gcc` is what provides `gfortran`.** There is no standalone `gfortran` formula, and macOS
  ships no system Fortran compiler at all.
- **`fftw`** brings the single-precision `fftw3f` the modem needs, and — because the shipped app
  links it statically — its `libfftw3f.a` as well. **`boost`** is for the header-only CRC-14 in
  `crc14.cpp`.
- **No WebView package**, unlike Windows' WebView2: WKWebView is part of macOS.
- **No Hamlib package.** Unlike an older macOS build that expected `brew install hamlib`, the
  bundle now carries its own `rigctld`, built from source by `scripts/fetch-hamlib-unix.sh` —
  which is also why `libusb` appears above.

There is no `--target` to add: the default rustup host toolchain
(`aarch64-apple-darwin`, or `x86_64-apple-darwin` on Intel) links against the same `clang` and
`gfortran` everything else here uses. There is no `-gnu`-versus-`-msvc` choice to get wrong.

## The one trap that costs an afternoon: `PKG_CONFIG_PATH`

**If MacPorts is installed, `/opt/local` comes first on `PATH` and its `pkg-config` cannot see
Homebrew's `.pc` files at all.** The build then fails with what looks like a missing dependency
on a machine where the dependency is plainly installed:

```
-- Found PkgConfig: /opt/local/bin/pkg-config (found version "0.29.2")
-- Checking for module 'fftw3f'
--   No package 'fftw3f' found
CMake Error at .../FindPkgConfig.cmake:645 (message):
  The following required packages were not found:
   - fftw3f
```

Two things read `pkg-config` here, so the export fixes both: `libtempo`'s CMake project finds
FFTW with it, and `tempo-fast-sys/build.rs` runs `pkg-config --libs-only-L fftw3f` to learn where
to point the linker.

```sh
export PKG_CONFIG_PATH="$(brew --prefix)/lib/pkgconfig"
pkg-config --exists fftw3f && echo ok      # the check, before you build anything
```

A shell that never ran `brew shellenv` leaves `PKG_CONFIG_PATH` unset entirely and hits the same
wall without MacPorts being involved. Note that MacPorts shadows **`cmake`** too — the error
above came from MacPorts' CMake 3.31 on a machine that also had Homebrew's 4.3.1 — so if you are
diagnosing something stranger, check `which -a cmake pkg-config` before anything else.

## Build

```sh
npm --prefix ui ci                # NOT `npm install` — it rewrites package-lock.json
npm --prefix ui run build         # tsc -b && vite build

cargo build                       # workspace (modem, engine, net)
cargo test --workspace            # headless: no sound card, no radio

cd src-tauri && cargo tauri dev   --features radio                        # live radio loop
cd src-tauri && cargo tauri build --features radio,custom-protocol \
                                  --bundles app,dmg                       # the .app + .dmg
```

Artifacts land under `src-tauri/target/release/bundle/` — `Nexus.app` in `macos/`, the `.dmg` in
`dmg/`. Without `--features radio` the app builds and runs the whole UI but does not key the
radio, which is what you want for UI work.

`cargo test --workspace` **does not cover `src-tauri`**: the desktop shell is a standalone crate
with its own `[workspace]` (see [CONTRIBUTING.md](CONTRIBUTING.md)), so it is built and tested
separately, with `--features radio`.

Two things will stop a `cargo tauri build` that has otherwise succeeded:

- **The updater payload.** `createUpdaterArtifacts` is on and the updater's *public* key is in
  `tauri.conf.json`, so Tauri emits a self-update tarball and then fails signing it — the private
  key is a CI secret. A build you are not going to publish has no use for a payload it cannot
  sign; turn it off:
  `cargo tauri build --features radio,custom-protocol --bundles app,dmg --config '{"bundle":{"createUpdaterArtifacts":false}}'`
- **The DeepCW model.** `src-tauri/resources/deepcw/model.onnx` is gitignored (AGPL-3.0, © e04),
  so a fresh clone or a git worktree does not have it, and `src-tauri/build.rs` fails a *release*
  build rather than silently shipping an app with no AI CW decoder. Stage it (see
  `src-tauri/resources/deepcw/README.md`) or set `NEXUS_ALLOW_MISSING_AICW=1` to say you meant it.

## The Fortran runtimes are linked statically, and that is not a preference

On macOS the modem links **`libgfortran.a`, `libquadmath.a` and `libfftw3f.a` statically**, plus
GCC's **`libemutls_w.a`**. Only the OS's own `libc++` and `libSystem` are dynamic. This matters to
anyone building a `.app` they intend to hand to somebody else, and it is invisible until they do:

- A dynamically-linked build **links and runs perfectly on the machine that built it**, because
  that machine has Homebrew's gcc. On any Mac that does not, it aborts at load with
  `dyld: Library not loaded: /opt/homebrew/opt/gcc/lib/gcc/current/libgfortran.5.dylib`. The
  first macOS artifact ever built did exactly this and was caught by the release smoke job on a
  clean runner, not by any build check.
- `libemutls_w.a` is there because **static libgfortran on Darwin uses emulated TLS** and
  `___emutls_get_address` lives in GCC's own archive — Apple's linker and compiler-rt provide no
  such symbol. The first static link failed on precisely this. It also sits in gcc's *versioned*
  subdirectory rather than beside the other two, so `build.rs` probes its full path separately.
- **libstdc++ is deliberately absent.** The C++ objects in `libtempo` are compiled by AppleClang,
  not `g++`, so linking GNU libstdc++ would be both wrong and another Homebrew dependency.

None of this needs setting up — `tempo-fast-sys/build.rs` does it whenever the Cargo *target* is
macOS. It is written down because the failure mode is a working build that is dead on arrival
elsewhere. To check a binary you built:

```sh
otool -L path/to/Nexus.app/Contents/MacOS/Nexus | grep -i 'homebrew\|gfortran\|fftw'
```

Silence is the correct answer. (Confirm the check itself works by running it against something
that *is* dynamically linked to Homebrew — `otool -L $(brew --prefix)/bin/rigctld` — otherwise a
clean result proves nothing.)

One consequence worth knowing: the linker search path `build.rs` emits runs through Homebrew's
version-stable `current` symlink and is deliberately not canonicalized, so a `brew upgrade gcc`
does not strand you on a stale versioned path.

## Hamlib is not in the repo either

`src-tauri/resources/hamlib/` holds only Hamlib's five tracked licence texts; the binaries are
gitignored and staged by `scripts/fetch-hamlib-unix.sh`, which builds Hamlib 4.7.1 from source
and sets the loader path so the staged programs find their own `libhamlib` inside the bundle.

**On macOS nothing makes you run it.** `src-tauri/build.rs` has a bundled-Hamlib presence gate,
but it is Windows-targets-only by design, and the Tauri bundle glob `resources/hamlib/*` still
matches the licence texts — so a `cargo tauri build` produces a green `.app` containing a hamlib
directory with no Hamlib in it. Every Hamlib-backed rig is then CAT-dead while a native CI-V
Icom works perfectly, which is indistinguishable from a code regression. If you are building a
bundle you will actually operate with, run it first:

```sh
bash scripts/fetch-hamlib-unix.sh     # idempotent; skips if already staged
```

## Gatekeeper: your own build will not open on a double-click

A build you produce yourself is not signed with a Developer ID and not notarized, so Gatekeeper
refuses it — "cannot be opened because the developer cannot be verified", or "is damaged and
can't be opened" on some versions. That is expected for a source build, not a fault in the app.
**Right-click (or Control-click) `Nexus.app` → Open**, then confirm; once per build.

This is not advice to pass on to anyone else. It only makes sense for a build you compiled
yourself and therefore trust, and the published `.dmg` needs none of it. A real signed build
takes an Apple Developer ID, `codesign` and notarization through `notarytool`; Tauri v2 picks the
credentials up from the environment, and `release.yml`'s `macos` job is the worked example —
`APPLE_CERTIFICATE` / `APPLE_CERTIFICATE_PASSWORD` imported into an ephemeral keychain for
signing, and an App Store Connect API key (`APPLE_API_KEY`, `APPLE_API_ISSUER`,
`APPLE_API_KEY_PATH`) for notarization. Nothing in `tauri.conf.json` changes to support it;
without those variables `cargo tauri build` still succeeds and simply produces the unsigned build
described above.

The release pipeline does not take a human's word for any of this: `smoke-macos` mounts the
built `.dmg` on a runner that never saw the signing keychain and asks `spctl` — the authority
Gatekeeper itself consults — for a verdict, then checks that the microphone entitlement is in
the *signature* of the shipped app rather than merely in the repo.

## Running it

Full setup is in [`docs/install.md`](docs/install.md) and the manual; only the macOS-specific
parts are here.

- **Microphone permission.** First launch prompts for it — that is the rig's RX audio arriving
  through a USB CODEC, which macOS classifies as a microphone. Dismiss it and Nexus hears
  nothing; re-enable under System Settings → Privacy & Security → Microphone.
- **Serial ports** appear as `/dev/tty.usbserial-*` or `/dev/cu.usbserial-*`, not `/dev/ttyUSB*`.
  Some USB-serial bridges (notably CH340/CH341 and older Prolific parts) need the vendor's driver
  before the port shows up at all.
- **Settings live in `~/.config/tempo/`**, not `~/Library/Application Support`. Nexus resolves its
  config directory from `XDG_CONFIG_HOME` (falling back to `$HOME/.config`) on every non-Windows
  platform, macOS included. Worth knowing if you script backups or profile switching.
- **Keep the clock accurate** — decoding is slot-timed. macOS uses network time by default; do
  not turn it off.
