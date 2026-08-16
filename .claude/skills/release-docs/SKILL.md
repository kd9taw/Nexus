---
name: release-docs
description: Use as the FINAL step of every Nexus release, after the installers are uploaded and the update feeds are live — the documentation refresh. Runs the mechanised doc checks, ranks what a human still has to look at, and names the surfaces (SourceForge wiki, GitHub wiki, the site's manual copy) that nothing in the repo can reach.
---

# Closing a release: the documentation pass

Docs rot one release at a time and nothing goes red about it. 1.3.0 shipped with roughly
thirty-four files describing an eight-tab Settings panel that had become nine, three manual
screenshots of the old panel, and a CHANGELOG crediting `(#2, #8)` as fixed while both were open.
All three were found by hand, after the fact.

The mechanism to catch most of it already existed and was wired to nothing:
`scripts/gen-wiki.mjs` had thrown on every invocation since 2026-08-09 and three releases shipped
over the top of it, because no job and no step ever ran it.

So this step is one command plus a short list of things only a person can do.

## 1. Order — this runs LAST, and the order is not cosmetic

Run it **after** the release is live: GitHub Release published, `publish.yml` finished, site
`version.json` bumped, SF FRS folder populated. Not before. Two reasons:

- the checks read the RELEASED version out of `src-tauri/tauri.conf.json` and compare doc prose
  against it, so running early compares against a version nobody has;
- the fixes it produces are ordinary doc commits, and a doc commit pushed to `main` mid-release
  moves the tree the release was cut from.

Everything upstream of this is the release runbook (maintainer-side, machine-local). That runbook
is where the release lives; this is only its last step.

## 2. The command

```
node scripts/release-docs.mjs
```

Read-only: it writes nothing, stages nothing, and every git call it makes is a query. It is safe
in the shared checkout while other agents are working.

`--version X.Y.Z` overrides the version it reads. `--no-tests` skips the vitest doc gates when CI
has just run them on this exact commit.

## 3. Reading the report

**FAIL** — mechanically wrong, exits 1, fix before calling the release closed.

| Failure | What to do |
|---|---|
| `gen-settings-reference.mjs --check` | Re-run it without `--check`, then read the RENAMED / NEW / ORPHAN / STALE lists it prints. It moves prose across renames; it never writes a sentence. |
| `gen-wiki.mjs --check` | Either the three derived wiki pages are behind their `docs/` source (re-run it) or a required replacement stopped matching (fix the rule in the script — do NOT ship the page without the transform). |
| the vitest doc gates | `npm --prefix ui exec -- vitest run src/docs-match-code.test.ts src/docs-settings-pointers.test.ts`. The code is the arbiter: the doc is what changes. |
| `docs/RELEASE_NOTES-<ver>.md` missing | Write it. Without it the update feed's `notes` degrades to the GitHub release body. |
| CHANGELOG issue credits | An issue cited as fixed is still open. **A released section is immutable history** — correct it in the next version's section, not by editing the shipped one, and never close the issue to make the check green. |

**REVIEW** — a human decides, exits 0, printed last so it is the final thing on screen. It does
not fail the run on purpose: a check that is red every single release is a check that gets
switched off, and the FAILs go with it.

- *screenshots older than the UI they show* — ranked by how many commits landed on the source
  since the capture. Work down the list, not across. A source change is not proof the frame
  changed, so open the image first. Re-shoot at a **1920×1080 logical** window (the app's auto-zoom
  resolves to exactly 100% there, so type renders un-shrunk), run `scripts/build-manual-images.py`,
  and **reword the alt text** — it describes the picture, so a new picture under the old sentence
  is a new lie. The 1.3.0 settings captures were ten days and three releases stale.
- *older versions named in prose* — grouped by the sentence, because one editorial decision gets
  pasted into several pages. Some are correct history ("1.0.0 installs over 0.27.0"); some are a
  banner that stopped being news three releases ago.

## 4. By hand — nothing in the repo can reach these

**The SourceForge wiki does NOT sync from `docs/launch/wiki/*.md`.** Those files are the source;
the wiki is a separate copy that stays stale until someone pastes into it. It is a CodeMirror
editor, so `textarea.value` is clobbered on submit — set
`document.querySelector('.CodeMirror').CodeMirror.setValue(<text>)` and Save. The Allura bearer
token does REST; the SSH key does git and FRS; they are not interchangeable.

The **GitHub wiki** is the same paste from the same source — the three generated pages plus the
four hand-written ones (`Home`, `Rig-Setup`, `_ProjectSummary`, `Documentation`, which have no
`docs/` source and must never be generated over).

The **site's copy of the manual** needs the site checkout, which exists only on the maintainer
machine; CI cannot do it. Deploy, then **curl the live URL** — `wrangler pages deploy` can exit 0
having deployed nothing.

The **SourceForge Default Download** flip is browser-only (a scripted POST returns 401). Check
both Windows and Linux; `best_release.json` is the authoritative confirmation and the Files-page
banner caches and lies.

**macOS is a shipped platform (since 1.5.0, 2026-08-16).** The docs pass covers it too: any
platform table or install prose must name the Apple Silicon DMG (Intel = source build), the
Homebrew Hamlib note (`brew install hamlib` for CAT), and the verification list includes the
DMG's release URL, the site's `downloads.macos` entry, and the DMG in the SF release folder.

## 5. What none of this covers

Written down rather than chased with more checks, because overstating reach is how the next gap
hides.

- **Prose accuracy.** Every guard compares NAMES, TABLES and DATES. A paragraph that describes a
  workflow that changed passes everything here. Reading the release's own CHANGELOG against the
  pages that document those features is still a human pass.
- **In-app strings.** Tooltips and hint text are checked against nothing, in either direction.
- **A rename the registry never declared.** `docs-settings-pointers.test.ts` flags only names
  `TAB_ALIASES`/`SECTION_ALIASES` declare dead — which is what keeps it from firing on Windows'
  or N3FJP's Settings menus, and what makes it blind to a rename nobody wrote down.
- **A control that MOVED between two live sections.** This is the biggest gap and the dominant
  failure mode of a reorg, so treat it as a manual pass every time Settings is rearranged: read
  the release's own "moved X to Y" CHANGELOG lines and grep the doc set for the old home. In
  1.3.0 four pointers were green and wrong — the CAT broker cited under `Rig & CAT` (it is now
  `Transmit limits & sharing ▸ Share this radio with other programs`), and N3FJP/N1MM host and
  port cited under `Contesting ▸ Field Day Setup` (they are under `Logging & Connectors`). The
  guard cannot see these: it knows whether a NAME is alive, not whether that section still holds
  the control. The obvious mechanisation was tried and measured and does not work — the reasons
  are recorded at the bottom of `ui/src/docs-settings-pointers.test.ts`, read them before
  attempting it again.
- **Field labels inside `settings-reference.md`.** `gen-settings-reference.mjs` owns STRUCTURE
  only; the ~90 `###` field blocks are compared against nothing. 1.3.0 published three field
  names that do not exist ("Share my radio (CAT broker)", "CAT broker port", "Broker PTT").
- **Whether the paste actually happened.** Nothing in this repo can see the SourceForge or GitHub
  wiki. Verify by loading the page.

## 6. Then close the release

Per the runbook's own last step, in the same pass: record the release, and fix or retire whatever
the release superseded in the maintainer's working notes. Supersession happens at release events,
which is why the sweep is a release step rather than a periodic tidy.
