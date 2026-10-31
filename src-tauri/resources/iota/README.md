# IOTA island-group list — staged here at build time, never committed

The installer carries IOTA Ltd's island-group list (`iota_groups.json`, © RSGB and IOTA Ltd, as
shown on <https://www.iota-world.org/>), included with IOTA Ltd's permission (see NOTICE). The
permission covers the installer, not this repository, so the list is gitignored and fetched into
this folder when a build is made:

    node scripts/gen-iota-groups.mjs --download src-tauri/resources/iota

The script takes the file from the address IOTA Ltd publishes for software, with the same limits
as the app's own monthly check, and writes `iota_groups.manifest.json` beside it (its date, group
count and SHA-256). `scripts/release-prep`, `scripts/build-windows-cross.sh` and each installer job
in `.github/workflows/release.yml` run it, best-effort.

A build without it still works: island names are then checked by their shape only until the app's
first monthly check downloads the list.
