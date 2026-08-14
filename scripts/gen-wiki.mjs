#!/usr/bin/env node
// Generate the three DERIVATIVE wiki pages from their canonical docs/ sources.
//
//   docs/faq.md         ->  docs/launch/wiki/FAQ.md
//   docs/install.md     ->  docs/launch/wiki/Install.md
//   docs/quick-start.md ->  docs/launch/wiki/Quick-Start.md
//
// docs/launch/wiki/ is pasted by hand into BOTH the GitHub wiki and the SourceForge
// wiki. Hand-mirrored copies rot: the three pages above were 26-53% verbatim copies
// of their docs/ source that had drifted apart word by word, and each had picked up
// a defect the source did not have. Generating them means the docs tree stays the
// one place a fact is written down.
//
// *** THIS TOOL DELIBERATELY DOES NOT GENERATE Home.md, Rig-Setup.md,           ***
// *** _ProjectSummary.md OR Documentation.md. Those four are HAND-WRITTEN --    ***
// *** positioning copy, a per-brand rig guide, the SourceForge project summary  ***
// *** and the link hub -- with no docs/ source to derive them from. A generator ***
// *** that overwrote them would be a destructive tool wearing a helpful hat.    ***
// *** See docs/launch/wiki/README.md.                                           ***
//
// The transforms are the mechanical, systematic differences between a docs/ page and
// its wiki twin. Nothing here rewrites prose:
//
//   1. Screenshot TODO comments are dropped   (the wiki carries no images)
//   2. Links to docs/ pages that HAVE a wiki twin become bare wiki page names
//      ([Install & Verify](install.md) -> [Install](Install)); every other relative
//      docs link becomes an absolute GitHub URL, because the wiki is not in the tree
//   3. Download source: docs/ points at SourceForge, the wiki at GitHub Releases
//   4. US -> en-GB spelling, the house style of the wiki
//   5. Heading time estimates condense: "(about 3 minutes)" -> "(~3 min)"
//   6. Per-page head/tail: the download button and the wiki's own nav block, which
//      differ by construction because the wiki has different pages than docs/
//
// Deterministic: same input bytes -> same output bytes, no clock, no network.
//
// Run:    node scripts/gen-wiki.mjs
// Check:  node scripts/gen-wiki.mjs --check   (exit 1 if the committed pages are
//                                              stale; nothing is written)

import { readFileSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const DOCS = path.join(ROOT, 'docs')
const WIKI = path.join(DOCS, 'launch', 'wiki')

const REPO = 'https://github.com/kd9taw/Nexus'
const BLOB = `${REPO}/blob/main/docs/`
const TREE = `${REPO}/tree/main/docs/`
const RELEASES = `${REPO}/releases/latest`
const ISSUES = `${REPO}/issues/new/choose`
const SF_FILES = 'https://sourceforge.net/projects/nexus-ham-radio/files/'

// docs/ page -> [the label the wiki uses for it, the wiki page name].
// Only these three have a wiki twin; every other docs link leaves the wiki.
const WIKI_TWINS = {
  'quick-start.md': ['Quick Start', 'Quick-Start'],
  'install.md': ['Install', 'Install'],
  'faq.md': ['FAQ', 'FAQ'],
}

// docs/ links a sibling page by filename ("see [interop.md](interop.md)"), which reads
// fine in a repo and badly on a wiki once the target has become an absolute URL. Give
// those a human label. Link text that is already prose is left alone.
const LINK_LABELS = {
  'interop.md': 'interop and companion setup',
  'protocols/index.md': 'protocol overview',
  'protocols/tempofast.md': 'TempoFast',
  'protocols/tempodeep.md': 'TempoDeep',
  'troubleshooting.md': 'Troubleshooting',
}

// US -> en-GB. Only words that actually occur in the three sources; adding one is
// cheap, guessing at a list is how a generator starts mangling quoted UI strings.
// "license" is left alone on purpose: it is the FCC Part 97 term ("license class")
// and the wiki keeps the US spelling for it.
const SPELLING = [
  [/\bbehavior\b/g, 'behaviour'],
  [/\bbehaviors\b/g, 'behaviours'],
  [/\blabeled\b/g, 'labelled'],
  [/\bprioritizes\b/g, 'prioritises'],
]

// --- transforms ------------------------------------------------------------

// Drop the <!-- TODO: capture screenshot --> markers, and the blank line each one
// leaves behind, so paragraphs do not end up separated by three newlines.
const stripComments = (s) => s.replace(/<!--[\s\S]*?-->\n?\n?/g, '')

const rewriteLinks = (s) =>
  s
    // Link text that is just the target filename -> a human label
    .replace(/\[([A-Za-z0-9._/-]+\.md)\]\(\1\)/g, (m, file) =>
      LINK_LABELS[file] ? `[${LINK_LABELS[file]}](${file})` : m,
    )
    // ...to a docs page that has a wiki twin -> the wiki's own label + page name
    .replace(/\[[^\]]+\]\((quick-start|install|faq)\.md(#[^)]*)?\)/g, (_m, page, anchor) => {
      const [label, target] = WIKI_TWINS[`${page}.md`]
      return `[${label}](${target}${anchor ?? ''})`
    })
    // ...to a docs directory -> GitHub tree view (blob/ 404s on a directory)
    .replace(/\]\((?!https?:)([A-Za-z0-9._-]+)\/\)/g, (_m, dir) => `](${TREE}${dir}/)`)
    // ...to any other docs page -> absolute GitHub blob URL, anchor kept
    .replace(
      /\]\((?!https?:)([A-Za-z0-9._/-]+\.md)(#[^)]*)?\)/g,
      (_m, file, anchor) => `](${BLOB}${file}${anchor ?? ''})`,
    )

// The bare project URL means "the repository" in docs/; on the wiki that is GitHub.
// The negative lookahead leaves https://sourceforge.net/projects/.../files/... alone
// -- the wiki still names SourceForge as the download MIRROR.
const rewriteRepoUrls = (s) =>
  s
    .replace(/https:\/\/sourceforge\.net\/projects\/nexus-ham-radio(?![\w/-])/g, REPO)
    // GitHub splits repo from tracker, so an "issue tracker" pointer must be re-aimed
    .split(`issue tracker at <${REPO}>`)
    .join(`issue tracker at <${ISSUES}>`)

const rewriteSpelling = (s) => SPELLING.reduce((acc, [re, to]) => acc.replace(re, to), s)

// "## 1. Install ... (about 3 minutes)" -> "## 1. Install ... (~3 min)"
const condenseTimes = (s) => s.replace(/\(about (\d+) minutes?\b/g, '(~$1 min')

const setTitle = (s, title) => (title ? s.replace(/^# .*$/m, `# ${title}`) : s)

// --- pages -----------------------------------------------------------------
//
// `replacements` are REQUIRED: if a key stops matching its source the generator
// throws rather than silently emitting a page missing the transform. That is the
// whole point -- a quiet miss is how a wiki goes stale again.

const PAGES = [
  {
    source: 'faq.md',
    out: 'FAQ.md',
    title: 'FAQ',
    replacements: [],
    // The docs footer points at the protocol specs by relative path; the wiki's own
    // nav points at its sibling pages and sends the deep docs to GitHub.
    tailFrom: '**More:**',
    tail: `**More:** [Quick Start](Quick-Start) · [Install](Install) · [Rig Setup](Rig-Setup)
· [Documentation](Documentation) · the deep protocol docs
[on GitHub](${TREE.replace(/\/$/, '')}).

*License: GPL-3.0 · by KD9TAW · Repository: <${REPO}>*
`,
  },
  {
    source: 'install.md',
    out: 'Install.md',
    // The wiki page opens with the download button; docs/ opens straight into the
    // requirements, because the docs tree has no "get the file" job to do.
    head: `[**⬇ Download the latest release**](${RELEASES})\n\n`,
    replacements: [
      {
        // ⚠️ REKEYED 2026-08-14. a6097c92 (2026-08-09) rewrote this block in
        // docs/install.md — it used to name SourceForge primary and describe the
        // mirror in prose; it now lists GitHub and SourceForge neutrally. The old
        // `find` matched nothing from that commit on, so this generator THREW on
        // every run and three releases shipped over a wiki nobody could rebuild.
        // Nothing was red because nothing invoked it — which is why it is a CI
        // step now. The INTENT is unchanged: the wiki ranks GitHub first and
        // points its SourceForge link at the files section, not at latest/download.
        find: `- **GitHub Releases:** <${RELEASES}>
- **SourceForge:** <${SF_FILES}latest/download>`,
        replace: `- **GitHub Releases (primary):** <${RELEASES}>
- **SourceForge (mirror):** <${SF_FILES}>`,
      },
    ],
    tailFrom: '## See also',
    tail: `## See also

- [Quick Start](Quick-Start) — install to first QSO in 15 minutes.
- [Rig Setup](Rig-Setup) — CAT, PTT, and audio per brand.
- [FAQ](FAQ) — the common questions.
- [Documentation](Documentation) — the full manual set on GitHub.

---

*Nexus is GPL-3.0-only. Built by KD9TAW.*
`,
  },
  {
    source: 'quick-start.md',
    out: 'Quick-Start.md',
    replacements: [
      {
        find: `from the
[SourceForge download page](${SF_FILES})
(the source lives there too).`,
        replace: `from the
[**⬇ latest release**](${RELEASES})
(SourceForge mirrors it at <${SF_FILES}>).`,
      },
    ],
    tailFrom: '## Where to go next',
    tail: `## Where to go next

You're on the air. When you want to go deeper:

- [Rig Setup](Rig-Setup) — Yaesu, Icom, FlexRadio, Xiegu, and rotators.
- [Install](Install) — SHA-256 verification, where your data lives, backups.
- [FAQ](FAQ) — the common questions.
- [Documentation](Documentation) — the full manual set: section guides, protocols, interop.

Stuck? The [troubleshooting guide](${BLOB}troubleshooting.md)
on GitHub covers CAT failures, drivers, port conflicts, and audio.

---

*Nexus is GPL-3.0-only. Not affiliated with ARRL, the WSJT project, or any
rig manufacturer. Built by KD9TAW.*
`,
  },
]

// --- build -----------------------------------------------------------------

function render(page) {
  let s = readFileSync(path.join(DOCS, page.source), 'utf8')

  s = stripComments(s)

  for (const { find, replace } of page.replacements ?? []) {
    if (!s.includes(find)) {
      throw new Error(
        `gen-wiki: docs/${page.source} no longer contains the text a required ` +
          `replacement is keyed on. The source moved; update the rule in ` +
          `scripts/gen-wiki.mjs rather than shipping the page without it.\n\n` +
          `Looked for:\n${find}\n`,
      )
    }
    s = s.split(find).join(replace)
  }

  s = rewriteLinks(s)
  s = rewriteRepoUrls(s)
  s = rewriteSpelling(s)
  s = condenseTimes(s)
  s = setTitle(s, page.title)

  if (page.head) {
    // Immediately before the first horizontal rule, i.e. after the page's lead.
    const rule = s.indexOf('\n---\n')
    if (rule < 0) throw new Error(`gen-wiki: docs/${page.source} has no leading --- rule`)
    s = `${s.slice(0, rule + 1)}${page.head}${s.slice(rule + 1)}`
  }

  const tailAt = s.indexOf(page.tailFrom)
  if (tailAt < 0) {
    throw new Error(`gen-wiki: docs/${page.source} has no "${page.tailFrom}" block to replace`)
  }
  s = s.slice(0, tailAt) + page.tail

  return `${s.replace(/\n{3,}/g, '\n\n').trimEnd()}\n`
}

const check = process.argv.includes('--check')
let stale = 0

for (const page of PAGES) {
  const out = path.join(WIKI, page.out)
  const next = render(page)
  const prev = readFileSync(out, 'utf8')
  if (prev === next) {
    console.log(`  = ${page.out}`)
    continue
  }
  if (check) {
    console.error(`  ! ${page.out} is stale — run: node scripts/gen-wiki.mjs`)
    stale++
    continue
  }
  writeFileSync(out, next)
  console.log(`  → ${page.out}  (from docs/${page.source})`)
}

if (stale) process.exit(1)
