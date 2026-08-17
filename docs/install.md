# Install & Verify

Everything you need to install Nexus, verify the download, upgrade, uninstall, and
know where your data lives. Windows is written out step by step below; Linux and
Raspberry Pi are packaged too and are covered where they differ. If you just want to
get on the air, the [Quick Start](quick-start.md) covers install in three paragraphs;
come here for the complete picture.

---

## What you need

- **Windows 10 or 11, 64-bit (x64)**, or **Linux**, or a **Mac with Apple Silicon** (M-series, macOS 12 or later), or a **64-bit Raspberry Pi**
  (Pi 3/4/5). All three build from the same tree and ship together every release.
  Intel Macs are source-build only. On a slower Pi, **Settings ▸ Digital ▸ Decode depth ▸
  Fast** keeps FT8 and FT4 decoding in real time.
- **On a PC, Linux means Ubuntu 24.04 or newer** — Debian 13 (trixie), Fedora 40+, Mint 22
  and anything else built on a C library of at least that vintage. Both PC Linux files are
  built on Ubuntu 24.04, and the AppImage does not change this: an AppImage carries the
  application's own libraries but not the system C library, so it needs the same minimum the
  `.deb` does. On something older — Ubuntu 22.04, or Mint 21.x, which is built on it — the
  package installs without complaint and then the app does not start, which is a poor way to
  find out. Check with `ldd --version`: you need 2.39 or higher. The two Raspberry Pi packages
  are separate and each names the Pi OS it is built for.
- **A radio with CAT + audio**, or a network rig (FlexRadio, remote `rigctld`).
  You can install and explore without a radio — the wizard and every panel open —
  but you need a rig connected to transmit.
- **Nothing else to install on Windows.** The installer bundles the **WebView2**
  runtime and **Hamlib** (`rigctld.exe` plus its DLLs), so CAT and rotor control work
  offline out of the box. There is no separate Hamlib, WebView2, or driver download for
  supported radios. (USB bridge-chip drivers are the one exception — see
  [Troubleshooting → drivers](troubleshooting.md#driver-hint-usb-bridge-chip-detected-but-the-rig-wont-open).)
  On **Linux and the Pi**, CAT uses the system Hamlib instead: the `.deb` pulls
  `libhamlib-utils` in automatically, and AppImage users run
  `sudo apt install libhamlib-utils` once.

The installer is roughly **250 MB** because it carries the WebView2 runtime,
Hamlib, and the DSP stack so a bare PC works with no internet. Expect the
WebView2 step to take a few quiet minutes — that is normal, let it finish.

---

## Download

Five files ship per release:

| File | Platform |
|---|---|
| `Nexus_<version>_x64-setup.exe` | Windows 10/11 x64 — NSIS, per-user, bundles WebView2 and Hamlib |
| `Nexus_<version>_aarch64.dmg` | macOS on Apple Silicon — signed + notarized; `brew install hamlib` for CAT |
| `Nexus_<version>_amd64.AppImage` | Linux on a PC, portable — one file, updates itself in place (Ubuntu 24.04 or newer) |
| `Nexus_<version>_pc_amd64.deb` | Debian / Ubuntu on a PC (Ubuntu 24.04 / Debian 13 or newer) |
| `Nexus_<version>_pi_arm64_bookworm.deb` | Raspberry Pi OS bookworm, 64-bit |
| `Nexus_<version>_pi_arm64_trixie.deb` | Raspberry Pi OS trixie, 64-bit |

The `.deb` names changed at 1.0.0. Before that the PC and Pi packages were told apart
only by `amd64` versus `arm64`, so picking the right one meant already knowing that
`amd64` means "PC" here. They say `pc` and `pi` now, and the two Pi files name the Pi
OS base they are built against — match yours (`cat /etc/os-release`).

Get them from:

- **GitHub Releases:** <https://github.com/kd9taw/Nexus/releases/latest>
- **SourceForge:** <https://sourceforge.net/projects/nexus-ham-radio/files/latest/download>

Both host the identical binary and its SHA-256 checksum. Use whichever is faster
for you.

---

## Verify the download

Because the installer is **unsigned** (see the next section), verifying the
checksum is the way to confirm you have an untampered copy. Each release publishes
a `SHA-256` alongside the `.exe`.

In PowerShell, from the folder where you saved the installer:

```powershell
Get-FileHash .\Nexus_<version>_x64-setup.exe -Algorithm SHA256
```

Compare the printed hash against the value on the release page — they must match
exactly (case doesn't matter). If they differ, delete the file and download again
from the official source above.

<!-- TODO: capture screenshot — the GitHub release page showing the Nexus installer asset and its published SHA-256 checksum -->

---

## Install and the SmartScreen warning

Run the installer. The published binaries are cross-compiled and **unsigned**, so
Windows SmartScreen shows a blue *"Windows protected your PC"* dialog. This is
expected for an unsigned installer and does not indicate a problem with the file —
which is exactly why the SHA-256 check above is worth doing.

Click **More info**, then **Run anyway**.

<!-- TODO: capture screenshot — Windows SmartScreen "Windows protected your PC" dialog with More info expanded, showing the Run anyway button -->

If you would rather avoid the prompt entirely, you can
[build from source](manual/Building-from-Source.md) instead.

### Where it installs

Nexus installs **per-user** — no administrator rights, no system-wide changes. The
program files land under your user profile (`%LOCALAPPDATA%\Programs\` for the
default NSIS per-user install), and a Start-menu entry is created for your account
only.

---

## Upgrading

**Nexus updates itself on Windows and on the Linux AppImage.** A new version
downloads quietly in the background and then offers to install. Nothing installs
behind your back and nothing happens on a schedule: the button waits for you, and it
stands down while you are transmitting, tuning, in a contact or running CQ, and tells
you which — restarting mid-contact would lose the contact. Every update is signed and
verified before it is applied, and an altered installer is refused.

**The `.deb` packages are managed by your package system** — the PC one and both
Raspberry Pi ones. Nexus notifies you that a new version exists rather than replacing
a file apt owns.

To upgrade by hand at any time, download the newer file and install it over the
existing version. Your settings and logbook live in a separate location (below) and
are left untouched, so upgrading never disturbs your data — 1.0.0 installs over 0.27.0
and reads your existing log, settings and layouts as they are.

To confirm you're on the build you expect, check the build hash in the Settings
header against the release you installed.

---

## Uninstalling

On Windows, uninstall from **Settings ▸ Apps ▸ Installed apps** (or the Start-menu
uninstaller) like any other program. On Linux and the Pi, remove the package with
your package manager (`apt remove`), or delete the AppImage file. Every route removes
the program files but **leaves your data** — settings and logbook — in place, so
reinstalling later picks up exactly where you left off. If you want a truly clean
removal, delete the data folders below by hand after uninstalling.

---

## Where your data lives

Windows keys off `%APPDATA%`; Linux and Raspberry Pi key off `$XDG_CONFIG_HOME`, which
is `~/.config` unless you have set it. The folder is called `tempo` on both, from the
app's original name.

| What | Windows | Linux / Raspberry Pi | Notes |
|---|---|---|---|
| Settings | `%APPDATA%\tempo\settings.json` | `~/.config/tempo/settings.json` | JSON, camelCase keys; partial files merge with defaults, so it's safe to hand-edit |
| **Logbook** | `%APPDATA%\tempo\log.adi` | `~/.config/tempo/log.adi` | ADIF 3.1.4 — **this is the file to back up** |
| Received-audio recordings | `%APPDATA%\tempo\recordings\` | `~/.config/tempo/recordings/` | Only if you enable audio saving; can get large |
| UI state | `%LOCALAPPDATA%\com.kd9taw.tempo\` | the webview's own store for `com.kd9taw.tempo` | Theme, UI scale, panel layout, wizard-seen flag, board filters |

Running a second instance against a second radio puts its settings in a
profile-suffixed folder beside the first (`tempo-<profile>`), and both instances
share the one `log.adi`. `NEXUS_DATA_DIR` moves that shared logbook somewhere else —
a NAS or a synced folder — for a multi-PC shack.

Two things worth understanding:

- **`log.adi` is the irreplaceable file.** Everything else can be rebuilt or
  re-entered; your contacts can't. Back it up. It's plain ADIF, so any logger can
  read it, and Nexus round-trips it faithfully.
- **UI preferences don't roam with settings.** Theme, UI scale, and layout live in the
  webview's own store — WebView2 on Windows, WebKitGTK on Linux and the Pi — not in
  `settings.json`. Copying `settings.json` to another machine carries your rig and
  station config but not your theme or window layout, and clearing that store resets
  them to defaults.

Credentials for online services (LoTW, QRZ, ClubLog, eQSL, HRDLog) are **not** in
any of these files — they live in the OS keychain (Windows Credential Manager, or the
Secret Service keyring on Linux and the Pi) and are never written to config or logs.

---

## Backing up

Before a reinstall, a PC migration, or just periodically, copy:

- `log.adi` — your logbook (**the important one**)
- `settings.json` — your rig/station config, to save re-entering it

Both live in `%APPDATA%\tempo\` on Windows and `~/.config/tempo/` on Linux and the
Pi. To restore, install Nexus, then drop those files back into that folder before
launching. Online-service credentials will need to be re-entered from Settings,
since they don't leave the origin machine's keychain.

---

## See also

- [Quick Start](quick-start.md) — install to first QSO in 15 minutes.
- [Getting Started](manual/Getting-Started.md) — the longer setup walkthrough.
- [Rig and Audio Setup](manual/Rig-and-Audio-Setup.md) — CAT, PTT, and audio in depth.
- [Troubleshooting](troubleshooting.md) — when something doesn't work.
