---
name: Bug report
about: Report something in Tempo that is broken or behaves incorrectly
title: "[bug] "
labels: ["bug", "needs-triage"]
assignees: []
---

<!--
Thanks for helping improve Tempo. Please fill in every section you can.
Reminder: Tempo is beta and the FT1/DX1 waveforms are validated by simulation,
not yet on-air. A bug seen only on-air is still useful — just say so below.
-->

## Environment

- **Tempo version:** <!-- e.g. 0.1.0, or the git commit if you built from source -->
- **Windows version:** <!-- e.g. Windows 11 23H2, build 22631 -->
- **Install type:** <!-- NSIS installer (Tempo_x.y.z_x64-setup.exe) / built from source -->
- **Rig + interface:** <!-- e.g. Icom IC-7300 over built-in USB CODEC + CAT; or sound card + serial RTS/DTR PTT; or VOX -->
- **rigctld / CAT path:** <!-- Tempo-launched rigctld (default) / external rigctld / serial RTS-DTR / VOX -->

## Operating context

- **Tier:** <!-- Fast (FT1) / Robust (DX1) -->
- **Band / dial frequency:** <!-- e.g. 40 m, 7.078 MHz USB -->
- **Mode:** <!-- Chat / QSO / Field Day -->
- **On-air vs simulation/loopback:** <!-- Was this on a real RF path, or a loopback / simulated / file-based test? This matters a lot for triage. -->

## What happened

<!-- A clear description of the actual behavior. -->

## What you expected

<!-- What you expected to happen instead. -->

## Steps to reproduce

1.
2.
3.

## Logs / output

<!--
Paste relevant log output, console/stderr text, or error dialogs.
For decode/sequencing issues, include the SNR, audio offset, and dT shown in the UI if available.
Use a code block (```) for readability.
-->

```

```

## Additional context

<!-- Screenshots, waterfall captures, .wav samples, network captures (WSJT-X UDP / PSK Reporter), or anything else that helps. -->
