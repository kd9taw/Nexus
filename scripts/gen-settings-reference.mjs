#!/usr/bin/env node
// Generate the STRUCTURE of docs/guide/settings-reference.md from the settings registry.
//
// WHY THIS EXISTS. The manual mirrors the Settings panel heading-for-heading, and
// `ui/src/docs-match-code.test.ts:420-484` enforces that mirror as a hard CI gate: the `##`
// headings must equal SETTINGS_TABS in panel order, the nav paragraph must list the same
// labels with matching anchors, and every `](…settings-reference.md#anchor)` link in the
// WHOLE docs tree must resolve to a heading here (67 of them today). So a Settings reorg
// does not just outdate a page — it strands links across the doc set. The 0.17 consolidation
// did exactly that, and commit 270c36b6 had to sweep nineteen files by hand.
//
// *** THIS TOOL OWNS STRUCTURE ONLY. The prose under every heading is HAND-WRITTEN        ***
// *** documentation and is CARRIED ACROSS VERBATIM, matched on its heading text. The      ***
// *** generator decides which headings exist and in what order; it never writes a         ***
// *** sentence of body copy, and it refuses to run rather than drop a paragraph a human   ***
// *** wrote (see --allow-dropped-prose). A generator that overwrote this page's prose     ***
// *** would be a destructive tool wearing a helpful hat.                                  ***
//
// What it owns:
//   1. the `##` tab set and its order            (from SETTINGS_TABS)
//   2. the `###` section set and order per tab   (from SETTINGS_SECTIONS)
//   3. the nav paragraph under "The tabs, in the order they appear:"
//   4. the "organized into <n> tabs" count in the lead
// Everything else — the lead, the screenshot, every body, the trailing "Related guides" —
// is moved, not rewritten.
//
// Prose is matched by HEADING TEXT GLOBALLY, not per tab, so moving a section to another
// tab carries its documentation with it. That is the whole point: after this exists, a
// reorg is a `tab:` edit in the registry plus a re-run, not a nineteen-file sweep.
//
// Deterministic: same input bytes -> same output bytes, no clock, no network.
//
// Run:    node scripts/gen-settings-reference.mjs
// Check:  node scripts/gen-settings-reference.mjs --check     (exit 1 if the committed page
//                                                              is stale; nothing is written)
// Flags:  --out <path>        write somewhere else (round-trip proofs; leaves the doc alone)
//         --registry <path>   read a different registry.ts (positive controls)
//         --panel <path>      read a different SettingsPanel.tsx (keeps the cross-check
//                             meaningful when --registry points at a snapshot)
//         --allow-dropped-prose   downgrade the "a human wrote this and it has nowhere to
//                                 go" error to a warning
//
// It prints every path it resolved, because a control that silently read the real registry
// instead of your copy proves nothing.

import { readFileSync, writeFileSync, readdirSync, statSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const DOCS = path.join(ROOT, 'docs')
const DOC = path.join(DOCS, 'guide', 'settings-reference.md')

// `## Related guides` is page furniture, not a tab. This mirrors the CHROME set in
// docs-match-code.test.ts:434 — the gate allows exactly this one non-tab `##`, and only
// after the last tab section, so the two lists must not drift apart.
const CHROME = 'Related guides'

const NUMBER_WORDS = [
  'zero', 'one', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight', 'nine', 'ten',
  'eleven', 'twelve', 'thirteen', 'fourteen', 'fifteen', 'sixteen', 'seventeen', 'eighteen',
  'nineteen', 'twenty',
]

// The nav paragraph wraps here. 76 reproduces the hand-written wrap byte-for-byte.
const NAV_WIDTH = 76

// ---------------------------------------------------------------------------
// TypeScript literal parsing. Same approach as docs-match-code.test.ts: scan with
// quote/comment awareness rather than regex the whole literal, because a keyword
// list is full of brackets and apostrophes.
// ---------------------------------------------------------------------------

/** The `[...]` or `{...}` starting at `start`, balanced, skipping string bodies. */
function balancedSpan(src, start) {
  const open = src[start]
  const close = open === '[' ? ']' : '}'
  let depth = 0
  let quote = null
  for (let i = start; i < src.length; i++) {
    const c = src[i]
    if (quote) {
      if (c === '\\') i++
      else if (c === quote) quote = null
      continue
    }
    if (c === "'" || c === '"' || c === '`') quote = c
    else if (c === '/' && src[i + 1] === '/') i = src.indexOf('\n', i)
    else if (c === '/' && src[i + 1] === '*') i = src.indexOf('*/', i) + 1
    else if (c === open) depth++
    else if (c === close && --depth === 0) return src.slice(start, i + 1)
    if (i < 0) break
  }
  throw new Error(`unbalanced ${open} at ${start}`)
}

/** The initialiser span of `const <name> ... = <span>`. */
function declValue(src, name, where) {
  const m = new RegExp(`\\b(?:const|let|var)\\s+${name}\\b[^=]*=\\s*`).exec(src)
  if (!m) throw new Error(`${where}: no declaration of ${name}`)
  return balancedSpan(src, m.index + m[0].length)
}

/** Top-level `{...}` literals inside an array span, in source order. */
function objectsIn(arraySpan) {
  const out = []
  let quote = null
  for (let i = 1; i < arraySpan.length - 1; i++) {
    const c = arraySpan[i]
    if (quote) {
      if (c === '\\') i++
      else if (c === quote) quote = null
      continue
    }
    if (c === "'" || c === '"' || c === '`') quote = c
    else if (c === '/' && arraySpan[i + 1] === '/') i = arraySpan.indexOf('\n', i)
    else if (c === '/' && arraySpan[i + 1] === '*') i = arraySpan.indexOf('*/', i) + 1
    else if (c === '{') {
      const span = balancedSpan(arraySpan, i)
      out.push(span)
      i += span.length - 1
    }
  }
  return out
}

/**
 * `key: value` pairs of one object literal. String literals come back unquoted; nested
 * arrays/objects come back as their raw span.
 *
 * Unlike the test's parser this takes the balanced span of a `[`/`{` value instead of
 * reading to the next comma. A section's multi-line `keywords: [...]` would otherwise end
 * the scan, and every field written AFTER it — `parent`, `advanced` — would silently read
 * as absent. A missed `parent` promotes a nested disclosure to its own `###`, which is a
 * wrong page rather than a crash, so it must not depend on field order.
 */
function parseObjectLiteral(span) {
  const out = {}
  const body = span.slice(1, -1)
  let i = 0
  while (i < body.length) {
    while (i < body.length && /[\s,]/.test(body[i])) i++
    if (body.startsWith('//', i)) {
      i = body.indexOf('\n', i) + 1 || body.length
      continue
    }
    if (body.startsWith('/*', i)) {
      i = body.indexOf('*/', i) + 2
      continue
    }
    // A QUOTED key may hold anything — SECTION_ALIASES is keyed by slug (`'rig-cat'`), and an
    // identifier-only pattern silently stops the scan at the first hyphen, yielding {} rather
    // than an error. Bare keys stay identifier-shaped.
    const key = /^(?:'([^']*)'|"([^"]*)"|([A-Za-z_$][\w$]*))\s*:\s*/.exec(body.slice(i))
    if (!key) break
    key[1] = key[1] ?? key[2] ?? key[3]
    i += key[0].length
    const q = body[i]
    if (q === "'" || q === '"' || q === '`') {
      let v = ''
      for (i++; i < body.length && body[i] !== q; i++) {
        if (body[i] === '\\') i++
        v += body[i]
      }
      i++
      out[key[1]] = v
    } else if (q === '[' || q === '{') {
      const nested = balancedSpan(body, i)
      out[key[1]] = nested
      i += nested.length
    } else {
      const start = i
      while (i < body.length && body[i] !== ',' && body[i] !== '\n') i++
      out[key[1]] = body.slice(start, i).trim()
    }
  }
  return out
}

// ---------------------------------------------------------------------------
// Markdown
// ---------------------------------------------------------------------------

/**
 * GitHub's heading-anchor slug — the one every `](#...)` in the doc set resolves against.
 *
 * ⚠️ This is NOT `sectionSlug()` from registry.ts, and must not be replaced by it. That
 * helper collapses runs of whitespace (`\s+` -> one `-`) where GitHub replaces each space
 * separately (`\s` -> `-`). Wherever a label has a character GitHub deletes BETWEEN two
 * spaces — `&`, `—`, `→`, `/` — the two disagree: "Spots & Alerts" is `#spots--alerts` on
 * GitHub and `#spots-alerts` from `sectionSlug()`. 13 of the 46 current labels differ, and
 * the live doc plus all 67 inbound links use the GitHub form. Generating anchors from
 * `sectionSlug()` would strand every one of them and fail the gate, whose own `slug()` this
 * mirrors (docs-match-code.test.ts:234-242).
 */
const slug = (text) =>
  text
    .toLowerCase()
    .replace(/`/g, '')
    .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/[*_]/g, '')
    .replace(/[^\w\s-]/g, '')
    .trim()
    .replace(/\s/g, '-')

/**
 * The registry's OWN `sectionSlug()` (registry.ts:427) — whitespace runs collapse to one `-`.
 *
 * This is the wrong function for an anchor (see `slug` above) but the RIGHT one for matching a
 * heading against a section `id`: ids were formed this way, so "Operator & Radio" gives
 * `operator-radio`, which is the id, where the GitHub slug gives `operator--radio`, which is not.
 * Used only to recognise a renamed section, never to emit a link.
 */
const idSlug = (label) =>
  label
    .toLowerCase()
    .replace(/[^a-z0-9\s-]/g, '')
    .trim()
    .replace(/\s+/g, '-')

/** Lines outside fenced code blocks, so a `#` in a sample can't read as a heading. */
function proseLines(md) {
  let fenced = false
  return md.split('\n').map((l) => {
    if (/^\s*(```|~~~)/.test(l)) {
      fenced = !fenced
      return null
    }
    return fenced ? null : l
  })
}

/** Trim blank lines and a trailing horizontal rule off a block. */
const trimBlock = (s) => s.replace(/\n+\s*---\s*$/, '').trim()

/** Every .md under docs/, absolute — the corpus the gate's link check walks. */
function docCorpus() {
  const out = []
  const walk = (dir) => {
    for (const name of readdirSync(dir)) {
      const abs = path.join(dir, name)
      if (statSync(abs).isDirectory()) walk(abs)
      else if (name.endsWith('.md')) out.push(abs)
    }
  }
  walk(DOCS)
  return out.sort()
}

// ---------------------------------------------------------------------------
// Reading the existing page apart
// ---------------------------------------------------------------------------

/**
 * Split the page into the pieces the generator moves around:
 *   head    — everything before the first `##` (lead, nav, screenshot)
 *   tabs    — per `##`: its opener prose and its `###` sections IN DOC ORDER
 *   bodies  — every `###` body keyed by heading text, GLOBALLY, plus where it came from.
 *             Global is what lets a section keep its documentation when it changes tab.
 *   chrome  — the `## Related guides` block, verbatim
 */
function parseDoc(md) {
  const lines = proseLines(md)
  const raw = md.split('\n')
  const marks = []
  lines.forEach((l, i) => {
    const m = l && /^(#{2,3})\s+(.+?)\s*$/.exec(l)
    if (m) marks.push({ level: m[1].length, text: m[2], i })
  })
  if (!marks.length) throw new Error(`${DOC}: no ## headings found`)

  const head = trimBlock(raw.slice(0, marks[0].i).join('\n'))
  const tabs = new Map()
  const bodies = new Map()
  let chrome = null

  const h2s = marks.filter((m) => m.level === 2)
  for (let t = 0; t < h2s.length; t++) {
    const start = h2s[t]
    const end = t + 1 < h2s.length ? h2s[t + 1].i : raw.length
    if (start.text === CHROME) {
      chrome = trimBlock(raw.slice(start.i, end).join('\n'))
      continue
    }
    const subs = marks.filter((m) => m.level === 3 && m.i > start.i && m.i < end)
    const opener = trimBlock(raw.slice(start.i + 1, subs.length ? subs[0].i : end).join('\n'))
    const order = []
    subs.forEach((s, k) => {
      const sEnd = k + 1 < subs.length ? subs[k + 1].i : end
      const body = trimBlock(raw.slice(s.i + 1, sEnd).join('\n'))
      order.push(s.text)
      if (bodies.has(s.text)) {
        throw new Error(
          `${DOC}: two "### ${s.text}" headings. Prose is carried across by heading text, ` +
            `so a duplicate heading has no single body to move. Rename one.`,
        )
      }
      bodies.set(s.text, { body, tab: start.text, index: k })
    })
    tabs.set(start.text, { opener, order })
  }
  return { head, tabs, bodies, chrome }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/** `[A](#a) · [B](#b) ·` wrapped, separator staying on the line it follows. */
function navParagraph(labels) {
  const items = labels.map((l) => `[${l}](#${slug(l)})`)
  const out = []
  let line = ''
  for (const item of items) {
    const next = line ? `${line} · ${item}` : item
    if (line && next.length > NAV_WIDTH) {
      out.push(`${line} ·`)
      line = item
    } else line = next
  }
  out.push(line)
  return out.join('\n')
}

/** Rewrite the lead's tab count and the nav paragraph inside the head. */
function renderHead(head, labels, warn) {
  const count = NUMBER_WORDS[labels.length] ?? String(labels.length)
  const COUNT_RE = /(organized into )([\w-]+)( tabs)/
  if (!COUNT_RE.test(head)) {
    throw new Error(
      `${DOC}: the lead no longer says "organized into <n> tabs", so the tab count cannot be ` +
        `kept true. Restore the phrasing or update COUNT_RE in scripts/gen-settings-reference.mjs.`,
    )
  }
  let out = head.replace(COUNT_RE, `$1${count}$3`)

  // The nav is the run of consecutive lines carrying same-page links. The gate reads EVERY
  // `](#…)` before the first `##` as nav, so a second such paragraph would silently break it.
  const ls = out.split('\n')
  const hits = ls.map((l, i) => (/\]\(#/.test(l) ? i : -1)).filter((i) => i >= 0)
  if (!hits.length) throw new Error(`${DOC}: no nav paragraph (no "](#…)" link before the first ##)`)
  const [first, last] = [hits[0], hits[hits.length - 1]]
  if (last - first + 1 !== hits.length) {
    throw new Error(
      `${DOC}: the same-page links before the first ## are not one paragraph (lines ` +
        `${hits.map((i) => i + 1).join(', ')}). docs-match-code.test.ts reads all of them as the ` +
        `tab nav, so a second such paragraph breaks the gate. Move it below the first ##.`,
    )
  }
  ls.splice(first, hits.length, navParagraph(labels))
  out = ls.join('\n')

  // The screenshot's alt text describes the panel it captured, including how many tabs it shows.
  // When the tab set changes the IMAGE goes stale, and only a human with a fresh capture can fix
  // that — rewriting the alt text would make it lie about the picture it labels. So this warns and
  // never edits.
  //
  // It counts the number-word rather than diffing against the previous tab set on purpose: a
  // warning derived from what the page USED to say goes silent the first time the page is
  // regenerated, leaving the wrong screenshot described by nobody. This one keeps firing until
  // the capture is actually replaced.
  const owned = out.match(COUNT_RE)?.[0]
  for (const m of out.matchAll(/\b([a-z]+)\s+tabs\b/gi)) {
    if (owned && m[0] === owned.slice(owned.indexOf(count))) continue
    const said = NUMBER_WORDS.indexOf(m[1].toLowerCase())
    if (said < 0 || said === labels.length) continue
    warn.stale.push(
      `"${m[0]}" in the lead/screenshot alt text, but the panel now has ${count} — the capture ` +
        `itself predates the change. Re-shoot ../img/manual/settings-radio.webp and reword its ` +
        `alt text by hand; this tool will not edit a description of a picture.`,
    )
  }
  return out
}

const sectionStub = (s) =>
  `<!-- TODO(settings-reference): "${s.label}" (registry id \`${s.id}\`) has no prose yet.\n` +
  `     Write it here; scripts/gen-settings-reference.mjs carries it across from now on. -->\n\n` +
  `_Undocumented so far._`

const openerStub = (tab) =>
  `<!-- TODO(settings-reference): the "${tab.label}" tab (id \`${tab.id}\`) has no lead\n` +
  `     paragraph yet. One or two lines saying what belongs on this tab. -->`

/**
 * Which heading in the page carries the prose for each registry section.
 *
 * The LABEL is what the page prints, but the `id` is the stable name (registry.ts:23-26): ids are
 * deliberately independent of the label so a section can be renamed without breaking links. Prose
 * has to follow the same rule, or renaming "Rig Control" to "Rig & CAT" silently strands a hundred
 * lines of documentation and replaces it with a stub. Three passes, most specific first:
 *
 *   1. the heading text IS the label                    — the ordinary case
 *   2. the heading slugs to the section's id            — "Rig Control" -> `rig-control`
 *   3. SECTION_ALIASES maps the heading's slug to the id — a rename the registry declares
 *
 * A heading is consumed by the first section to claim it and a matched section is never
 * re-matched, so `Radios` keeps its own body even though `setup-health` aliases onto `radios`.
 */
function matchBodies(sections, bodies, aliases, warn) {
  const byId = new Map()
  const used = new Set()
  for (const s of sections) {
    const hit = bodies.get(s.label)
    if (hit && !used.has(s.label)) {
      byId.set(s.id, { heading: s.label, ...hit })
      used.add(s.label)
    }
  }
  for (const s of sections) {
    if (byId.has(s.id)) continue
    for (const [heading, at] of bodies) {
      if (used.has(heading)) continue
      const sl = idSlug(heading)
      if (sl !== s.id && aliases[sl] !== s.id) continue
      byId.set(s.id, { heading, ...at })
      used.add(heading)
      warn.renamed.push(
        `"${heading}" → "${s.label}" (matched on the stable id \`${s.id}\`; prose carried across)`,
      )
      break
    }
  }
  return { byId, used }
}

function render(reg, doc, warn) {
  const labels = reg.tabs.map((t) => t.label)
  const { byId, used } = matchBodies(reg.sections, doc.bodies, reg.aliases, warn)

  // A `###` in the page that no registry section claims. Its prose is real, so it is kept at
  // the index it already had, and reported — either the registry is missing a section or the
  // page is documenting something that no longer exists. The generator cannot tell which.
  const orphans = new Map()
  for (const [label, at] of doc.bodies) {
    if (used.has(label)) continue
    if (!orphans.has(at.tab)) orphans.set(at.tab, [])
    orphans.get(at.tab).push({ label, ...at })
    warn.orphans.push(
      `"### ${label}" (under "${at.tab}") matches no section in the registry — kept in place. ` +
        `Add it to SETTINGS_SECTIONS, or fold its prose into a section that exists.`,
    )
  }

  const blocks = []
  for (const tab of reg.tabs) {
    const fromDoc = doc.tabs.get(tab.label)
    const chunks = [`## ${tab.label}`]
    // An EXISTING tab with no lead paragraph wrote itself that way on purpose (Frequencies
    // opens straight onto its one section). Only a tab the page has never seen gets a stub.
    if (fromDoc) chunks.push(fromDoc.opener)
    else {
      chunks.push(openerStub(tab))
      warn.newTabs.push(tab.label)
    }

    const rendered = reg.sections
      .filter((s) => s.tab === tab.id && !s.parent)
      .map((s) => {
        const hit = byId.get(s.id)
        if (!hit) warn.newSections.push(`${tab.label} ▸ ${s.label}`)
        else if (hit.tab !== tab.label) {
          warn.moved.push(`"${s.label}": ${hit.tab} → ${tab.label} (prose carried across)`)
        }
        return { label: s.label, body: hit ? hit.body : sectionStub(s) }
      })

    // Splice orphans back at the index they held, so a page with no registry change round-trips
    // byte-for-byte rather than quietly reordering itself.
    for (const o of (orphans.get(tab.label) ?? []).sort((a, b) => a.index - b.index)) {
      rendered.splice(Math.min(o.index, rendered.length), 0, { label: o.label, body: o.body })
    }

    for (const s of rendered) {
      chunks.push(`### ${s.label}`)
      if (s.body) chunks.push(s.body)
    }
    blocks.push(chunks.filter(Boolean).join('\n\n'))
  }

  // Prose with nowhere left to go: a dropped tab's opener, or an orphan whose tab vanished.
  for (const [label, t] of doc.tabs) {
    if (labels.includes(label)) continue
    warn.droppedTabs.push(label)
    if (t.opener) warn.dropped.push({ what: `the "## ${label}" tab lead`, text: t.opener })
    for (const o of orphans.get(label) ?? []) {
      warn.dropped.push({ what: `"### ${o.label}" (under the dropped "${label}" tab)`, text: o.body })
    }
  }

  const head = renderHead(doc.head, labels, warn)
  const tail = doc.chrome ?? `## ${CHROME}`
  return `${[head, ...blocks, tail].join('\n\n---\n\n')}\n`
}

// ---------------------------------------------------------------------------
// Self-check: the output must satisfy the gate, not merely look right
// ---------------------------------------------------------------------------

function verify(md, labels) {
  const problems = []
  const h2 = []
  proseLines(md).forEach((l) => {
    const m = l && /^(#{2,3})\s+(.+?)\s*$/.exec(l)
    if (m && m[1].length === 2) h2.push(m[2])
  })
  const tabH2 = h2.filter((h) => h !== CHROME)
  if (String(tabH2) !== String(labels)) {
    problems.push(`## headings [${tabH2.join(' | ')}] != SETTINGS_TABS [${labels.join(' | ')}]`)
  }
  if (h2.length && h2[h2.length - 1] !== CHROME) {
    problems.push(`"${CHROME}" must be the last ## section`)
  }
  const head = md.slice(0, md.indexOf('\n## '))
  const nav = [...head.matchAll(/\[([^\]]+)\]\(#([^)]+)\)/g)]
  if (String(nav.map((m) => m[1])) !== String(labels)) {
    problems.push(`nav lists [${nav.map((m) => m[1]).join(' | ')}] != [${labels.join(' | ')}]`)
  }
  for (const m of nav) {
    if (m[2] !== slug(m[1])) problems.push(`nav "${m[1]}" points at #${m[2]}, anchor is #${slug(m[1])}`)
  }
  return problems
}

/** Inbound `](…settings-reference.md#anchor)` links the new heading set would strand. */
function strandedLinks(md) {
  const anchors = new Set()
  proseLines(md).forEach((l) => {
    const m = l && /^#{1,6}\s+(.+?)\s*$/.exec(l)
    if (m) anchors.add(slug(m[1]))
  })
  const out = []
  for (const abs of docCorpus()) {
    const isRef = abs === DOC
    // The page's OWN same-page links must be read off the new text, not the stale copy still
    // on disk, or the old nav reports itself as stranded by the nav that replaced it.
    const src = isRef ? md : readFileSync(abs, 'utf8')
    for (const m of src.matchAll(/\]\(([^)\s]*settings-reference\.md)?#([^)\s]+)\)/g)) {
      if (!m[1] && !isRef) continue
      if (!anchors.has(m[2])) {
        const line = src.slice(0, m.index).split('\n').length
        out.push(`${show(abs)}:${line} -> #${m[2]}`)
      }
    }
  }
  return out
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

const argv = process.argv.slice(2)
const flag = (name) => argv.includes(name)
const opt = (name, fallback) => {
  const i = argv.indexOf(name)
  return i >= 0 && argv[i + 1] ? path.resolve(argv[i + 1]) : fallback
}

const registryPath = opt('--registry', path.join(ROOT, 'ui', 'src', 'settings', 'registry.ts'))
const panelPath = opt('--panel', path.join(ROOT, 'ui', 'src', 'components', 'SettingsPanel.tsx'))
const outPath = opt('--out', DOC)
const check = flag('--check')

console.log(`registry: ${registryPath}`)
console.log(`panel:    ${panelPath}`)
console.log(`page:     ${DOC}`)
console.log(`out:      ${check ? '(nothing — --check)' : outPath}`)

const registrySrc = readFileSync(registryPath, 'utf8')

const tabs = objectsIn(declValue(registrySrc, 'SETTINGS_TABS', registryPath)).map((span, i) => {
  const o = parseObjectLiteral(span)
  if (!o.id || !o.label) throw new Error(`${registryPath}: SETTINGS_TABS[${i}] needs id + label: ${span}`)
  return { id: o.id, label: o.label }
})
const sections = objectsIn(declValue(registrySrc, 'SETTINGS_SECTIONS', registryPath)).map((span, i) => {
  const o = parseObjectLiteral(span)
  for (const f of ['id', 'label', 'tab']) {
    if (!o[f]) throw new Error(`${registryPath}: SETTINGS_SECTIONS[${i}] has no ${f}: ${span}`)
  }
  return { id: o.id, label: o.label, tab: o.tab, parent: o.parent }
})

// Declared renames. `matchBodies` uses these so a section that changed label keeps the prose
// written under its old heading. Absent is fine — it just means no rename has been declared.
let aliases = {}
try {
  aliases = parseObjectLiteral(declValue(registrySrc, 'SECTION_ALIASES', registryPath))
} catch {
  console.log('note: no SECTION_ALIASES in the registry — renames match on id only')
}

// The gate parses SETTINGS_TABS out of SettingsPanel.tsx, NOT out of the registry — the panel
// keeps its own literal on purpose (SettingsPanel.tsx:411-414). Generating from a registry that
// had drifted from the panel would produce a page that fails the gate while looking correct
// here, so the two are compared before anything is written.
const panelTabs = objectsIn(
  declValue(readFileSync(panelPath, 'utf8'), 'SETTINGS_TABS', panelPath),
).map((span) => parseObjectLiteral(span).label)
const regLabels = tabs.map((t) => t.label)
if (String(panelTabs) !== String(regLabels)) {
  console.error(
    `\ngen-settings-reference: the registry and the panel disagree about the tabs.\n` +
      `  registry.ts:        [${regLabels.join(' | ')}]\n` +
      `  SettingsPanel.tsx:  [${panelTabs.join(' | ')}]\n` +
      `docs-match-code.test.ts checks the page against the PANEL, so generating from the ` +
      `registry now would ship a page that fails CI. Reconcile them first.`,
  )
  process.exit(1)
}

const warn = {
  orphans: [], moved: [], renamed: [], newTabs: [], newSections: [], dropped: [],
  droppedTabs: [], stale: [],
}
const existing = readFileSync(DOC, 'utf8')
const next = render({ tabs, sections, aliases }, parseDoc(existing), warn)

/** Repo-relative when it is in the tree, absolute when it is not — a control writing to a
 *  sandbox should never be reported as a path inside the repo. */
const show = (p) => (p.startsWith(ROOT + path.sep) ? path.relative(ROOT, p) : p)

const say = (title, lines) => {
  if (lines.length) console.log(`\n${title}\n${lines.map((l) => `  - ${l}`).join('\n')}`)
}
say('RENAMED (heading changed, prose kept via the stable id):', warn.renamed)
say('moved (prose followed its section):', warn.moved)
say('NEW — stub emitted, needs prose:', [
  ...warn.newTabs.map((t) => `tab lead: ${t}`),
  ...warn.newSections,
])
say('ORPHAN — in the page, not in the registry:', warn.orphans)
say('STALE — mentions a tab that no longer exists:', warn.stale)

const problems = verify(next, regLabels)
if (problems.length) {
  console.error(`\ngen-settings-reference: output would FAIL docs-match-code.test.ts:`)
  for (const p of problems) console.error(`  - ${p}`)
  process.exit(1)
}

if (warn.dropped.length && !flag('--allow-dropped-prose')) {
  console.error(
    `\ngen-settings-reference: REFUSING TO WRITE — hand-written prose has nowhere to go.\n` +
      `Its tab was removed from the registry and nothing inherited it. Re-home the text (or\n` +
      `add the section back), then re-run. --allow-dropped-prose discards it deliberately.\n`,
  )
  for (const d of warn.dropped) {
    console.error(`--- ${d.what} ---\n${d.text}\n`)
  }
  process.exit(1)
}
if (warn.dropped.length) {
  say('DROPPED (--allow-dropped-prose):', warn.dropped.map((d) => d.what))
}

const stranded = strandedLinks(next)
say('STRANDED inbound links (these fail the gate until swept):', stranded)

if (check) {
  if (existing === next) {
    console.log(`\n= ${show(DOC)} is up to date`)
  } else {
    console.error(
      `\n! ${show(DOC)} is stale — run: node scripts/gen-settings-reference.mjs`,
    )
    process.exit(1)
  }
} else if (outPath === DOC && existing === next) {
  console.log(`\n= ${show(DOC)} (no change)`)
} else {
  writeFileSync(outPath, next)
  console.log(`\n→ ${show(outPath)}`)
}
