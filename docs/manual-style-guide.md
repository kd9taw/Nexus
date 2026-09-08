# Manual style guide

House rules for `docs/guide/`. Written for whoever edits a chapter next.

The same pages ship three ways — the website, the PDF and the EPUB — from one
set of Markdown files. Everything below exists because one of those three, or one
of the CI gates, breaks when it is ignored.

## 1. Chapter shape

Every section chapter uses the same five parts, in this order. A reader who has
read one chapter can then skip straight to the part they need in any other.

```
# <Section name>

<Two to four sentences: what this section is for and when you would open it.>

## Before you start          (optional, but write it if there is anything)
## The tour
## Core workflows
## Honest limits
## Related guides
```

**The opening** says what the section is for, in the operator's terms. Not what
it is built out of, not what release it landed in.

**Before you start** is the prerequisites list — the settings, hardware or data
the section needs before any of it works. Three bullets at most, each one a link
to where it is set. Leave the heading out when there genuinely is nothing.

**The tour** is what is on the screen: the panes, the controls, what each one
does. Reference material, not steps.

**Core workflows** is the task half, and it is the part most readers came for.
One `###` per task, named for the task in the operator's words ("Work a spot in
one click", not "Spot handling"). Inside each:

- Numbered steps. **One action per step.** A step that contains two verbs is two
  steps.
- Each step says what you should see when it worked. "Click **Arm RX**. The pane
  head gains an AFC pill and text starts printing."
- One **Expected result** line at the end when the per-step results do not add up
  to an obvious finish.
- Say what to do when it does not work, or link to the place that does. If the
  recovery crosses chapters, it belongs in
  [Scenarios & troubleshooting](guide/scenarios.md) and the chapter links to it.

**Honest limits** is what the section does not do, and what a number on the
screen does not prove. The heading text is load-bearing — the PDF builder styles
the block by matching `## Honest limits` exactly, so do not reword it.

**Related guides** is a short list of the neighboring jobs.

### What does not go in a chapter

- **Implementation history.** "This was rewritten in 1.8 because the old
  sequencer…" belongs in the code comment or the release notes. The operator is
  holding a radio.
- **Rationale for a design decision**, unless the operator has to act on it. "The
  board admits VHF rows only with two near receivers" is operator-facing, because
  it explains a sparse board. "We chose a HashMap here" is not.
- **Code.** No struct names, no function names, no file paths into `src/`.
- **Advice dressed as description.** The station in the screenshots is one
  operator's. Its power, WPM, cluster host and band choices are illustrative,
  never a recommendation.

## 2. Evidence marks

Not every claim in the manual is the same strength. Keep them apart, and use
these three forms and no others:

| Form | Means | Write it as |
|---|---|---|
| Plain statement | Read off the source, or observed in the shipped build. | Ordinary prose. No marker. |
| **reported** | We have it from an operator or a bug report and have not reproduced it here. | "Reported on the FT-710: …" — name who and on what. |
| **not verified on hardware** | The code path exists and is tested, but nobody has run it against that rig, rotator or amplifier. | "The band-ladder mapping is not verified on hardware." |

Anything you cannot put in one of those three boxes does not go in the manual.
"Should", "presumably" and "ought to" are not evidence marks; they are guesses
with a hedge on them.

**The app's own evidence lines are a different thing** and the guide must not
blur them. On the Needed board an *evidence line* is the receipt for one row —
where the report came from and how old it is. Its four forms and their meanings
are documented once, in the [guide overview](guide/index.md#how-this-guide-marks-evidence).
A chapter that shows one explains the row it is on; it does not redefine the
legend.

## 3. Screenshots

### The problem this section exists to prevent

A full 1920-wide desktop capture placed in the manual is reduced to the column
width — about 176 mm in the PDF, less on a phone. UI text captured at 12 px lands
near 3 pt on the printed page. It is present, and it is unreadable. That is the
most common defect in the existing image set, and a tighter crop is the whole
fix.

### Crop first

1. **Decide what the reader needs to see**, then crop to exactly that: one panel,
   one dialog, one strip. Not the window.
2. **Exclude** loading panes, unrelated tooltips, and anything that is not the
   subject.
3. **Never upscale.** A small source stays small.
4. **Aim for legibility, not completeness.** If the thing you want to show cannot
   be read at column width, it is two images, not one.

Canonical settings, the same ones `scripts/build-manual-images.py` uses:

- **WEBP, quality 92, method 6**
- **Maximum width 1920 px**, downscaled with LANCZOS; never upscaled
- Crop box is `(left, top, right, bottom)` in source pixels, PIL order

```python
from PIL import Image
im = Image.open("<raw capture>")
im.crop((left, top, right, bottom)).save(
    "docs/img/manual/<name>.webp", "WEBP", quality=92, method=6
)
```

**Capture at a 1920×1080 logical window.** The app's auto-zoom resolves to
exactly 100% there, so type renders un-shrunk. A HiDPI capture arrives larger and
downscales cleanly; that extra detail is why the downscale looks good.

**Record every derived image**: published file, parent capture id, source file,
crop box, result dimensions. One table per branch, in `tasks/`, so the crop can be
re-derived without guessing.

### Naming

Lowercase, hyphenated, named for **what the image shows**, not for the capture it
came from and not for an issue number: `settings-radio.webp`, `cw-cockpit.webp`,
`wizard-station.webp`. `scripts/build-manual-images.py` carries the mapping from
capture stem to published name, so a rename is one entry there.

### Alt text

**Short. One sentence. What the image shows, for a reader who cannot see it.**

The existing set averages 582 characters of alt text, and the longest is 877 —
those are explanations that were put in the alt attribute because there was
nowhere else to put them. There is now: the caption and the prose.

- Good: `The Radio tab of Settings, with the Rig & CAT section open.`
- Bad: a paragraph naming every control in the frame.
- Never start with "Screenshot of" or "Image showing".
- Never put an instruction only in alt text.

### Captions

**Every image gets a visible caption**, on the line immediately after it, in
italics, carrying version context:

```markdown
![The Radio tab of Settings, with the Rig & CAT section open.](../img/manual/settings-radio.webp)
*Settings ▸ Radio on a fresh install, in Nexus 1.10.3.*
```

Use `*…*` and not an HTML `<figure>`: `scripts/build-manual-pdf.py` renders a
narrow Markdown subset of its own and escapes raw HTML, so a `<figure>` prints
as literal angle brackets in the PDF.

### Numbered legends

When an image has several things worth naming, number them in the caption and
refer to the numbers in the prose — do not try to explain the whole frame in the
alt text:

```markdown
*The Needed board in Nexus 1.10.3. (1) need chip, (2) evidence line, (3) age.*
```

### The instructions must work without the image

An image illustrates a procedure; it never carries a step. If the only place a
setting's name appears is inside a screenshot, the chapter is not finished.
Read the section with the images stripped out and check it still tells you what
to do.

### Privacy

These are captures of a real station. Before an image is committed:

- The callsign **KD9TAW** is a public amateur identity and is fine anywhere.
- A **personal name, email address, street address or private account
  identifier** must never appear. Re-crop to exclude it; if that is impossible,
  do not use the capture.
- **Third parties get the same treatment** — names, addresses and private
  comments in log entries.
- **Look at the pixels.** You cannot grep an image.

## 4. Navigation and links

- **`docs/guide/index.md` is the table of contents.** The PDF and EPUB builders
  take chapter order from the first bare `](<page>.md)` link in it. A link with
  an anchor — `](cw.md#macros)` — does not affect order, so a task index or a
  cross-reference can sit anywhere.
- **Every chapter must be linked from `index.md` with the bare form.** A CI gate
  fails on an orphan chapter, and an unlinked page is silently appended to the
  end of the book.
- **Link to the section, not the page.** `settings-reference.md#rig--cat` beats
  "see the Settings reference". A gate checks that every link into the settings
  reference resolves to a heading that exists.
- **Anchors are the heading, slugged**: lowercase, punctuation dropped, spaces to
  hyphens. `Rig & CAT` → `rig--cat` (the ampersand goes, its two spaces each
  become a hyphen).
- **Do not rename a heading casually.** Anchors are public URLs and roughly 70
  links point into the settings reference alone.
- **Name the current Settings tab.** A gate flags prose naming a tab or section
  the registry has declared legacy.

## 5. The gates that will fail you

Run these before pushing a doc change:

| Gate | What it checks |
|---|---|
| `node scripts/gen-settings-reference.mjs --check` | `settings-reference.md` structure still matches the settings registry. |
| `ui/src/docs-match-code.test.ts` | Published tables against the code they describe: CW macros, Settings tabs, Needed priority tiers, and every deep link into the settings reference. |
| `ui/src/docs-settings-pointers.test.ts` | No prose names a Settings tab or section the registry calls legacy. |
| `ui/src/docs-coverage.test.ts` | Every shipped section has a chapter, and `index.md` links every chapter. |
| `node scripts/release-docs.mjs` | Among other things, every `docs/guide/*.md` chapter is live on the website. **A new chapter fails this until the site is synced** — run `scripts/sync-manual.mjs` in the site checkout, push, re-deploy. |

What none of them check: **prose, screenshots, alt text and captions.** Those are
read by a person or they are not read at all.

## 6. Known rendering gap — wrapped list items in the PDF

`scripts/build-manual-pdf.py` renders its own Markdown subset, and it does not
carry a list item across a wrapped line. A bullet written as

```markdown
- **new mode** (30) — an entity you've worked, but never in this **mode class**
  (CW / Phone / digital) on **any** band.
```

comes out of that renderer as one `<li>` holding the first line, the list closed,
and the second line as a stray paragraph starting mid-sentence. It affects every
chapter, because every chapter wraps at ~78 columns. The website and the EPUB are
unaffected — the EPUB goes through pandoc, a real parser.

Until the renderer gains a continuation rule, know that a long list item is where
the printed manual looks broken, and prefer short list items in anything written
to be read at a bench.
