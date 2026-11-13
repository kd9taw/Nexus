# IOTA island-group and islands lists — staged here at build time, never committed

The installer carries IOTA Ltd's island-group list (`iota_groups.json`, © RSGB and IOTA Ltd, as
shown on <https://www.iota-world.org/>), included with IOTA Ltd's permission, and its islands list
(`iota_islands.json`), used with written permission from IOTA Ltd and the RSGB (see NOTICE). The
permissions cover the installer, not this repository, so the lists are gitignored and fetched into
this folder when a build is made:

    node scripts/gen-iota-groups.mjs --download src-tauri/resources/iota

The script takes both files from the addresses IOTA Ltd publishes for software, with the same
limits as the app's own monthly check, and writes `iota_groups.manifest.json` beside them (the date,
the group and island counts, and each file's SHA-256). When the islands list fails, the groups are
still staged and the script exits 1. `scripts/release-prep`, `scripts/build-windows-cross.sh` and each installer job
in `.github/workflows/release.yml` run it, best-effort.

A build without them still works: island names are then checked by their shape only, and no
islands are offered to pick from, until the app's first monthly check downloads the lists.
