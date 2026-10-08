Every Nexus download carries its own Hamlib 4.7.1 in this folder:
rigctld for CAT rig control, rigctl to probe a rig, and rotctld for a
rotator. Nothing needs installing.

- Windows: Hamlib's own 4.7.1 Windows build, the programs and the DLLs
  they load.
- Linux (the .deb, the AppImage and the Raspberry Pi .debs) and macOS:
  Hamlib 4.7.1 built from source when Nexus is built, with libhamlib
  beside the programs.

Nexus starts the copy in this folder first. It falls back to a Hamlib
installed on the computer only if that copy is missing or, on Linux and
macOS, cannot run: first the one on PATH, then (Linux and macOS)
/opt/homebrew/bin, /usr/local/bin and /opt/local/bin. The .deb still
depends on libhamlib-utils, so a .deb install always has that fallback.
The AppImage and the Mac have one only if you installed Hamlib yourself.

Hamlib's own licence texts are the other files in this folder.
