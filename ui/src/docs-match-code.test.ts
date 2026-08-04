import { describe, it, expect } from 'vitest'
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { join } from 'node:path'

// DOCS-MATCH-CODE — published tables are checked against the code they describe.
//
// Six contradictions shipped in one audit, every one of them the same shape: a table in
// the doc set and a literal in the source said different things, and nothing compared
// them. The worst was operational — docs/manual/CW.md published the F-key macros shifted
// by one from F3, so an operator pressing F3 to send a report sent "73 SK" and ended the
// contact instead.
//
// This file is the docs answer to what cockpit-panes.test.ts / cockpit-shells.test.ts do
// for CSS: they COMPUTE the cascade winner out of the stylesheet rather than regex-ing it
// for a string, which is why that bug class stopped recurring. So nothing below hardcodes
// an expected value. Each guard parses the values out of BOTH sides — the TypeScript
// literal and the published Markdown table — and compares them. A code change that
// outdates a doc fails here; so does a doc edit that drifts from the code. Neither side is
// the fixture. THE CODE IS THE ARBITER: when this file goes red, the doc is what changes,
// unless the code is genuinely wrong.
//
// Failure messages name the file, the row, and both values, because the person reading
// them is fixing a doc they did not write.
//
// What this CANNOT check is written down at the END OF THIS FILE. Read it before trusting
// a green run: prose claims, screenshots, in-app strings and anything not in a parseable
// table are outside every guard here.

const uiSrc = (rel: string) => fileURLToPath(new URL(`./${rel}`, import.meta.url))
const repo = (rel: string) => fileURLToPath(new URL(`../../${rel}`, import.meta.url))
const read = (abs: string) => readFileSync(abs, 'utf8')

// ---------------------------------------------------------------------------
// Source-literal parsing. Quote- and comment-aware scanning, never a regex over
// the whole literal: a macro body is full of `{`/`}` (the token syntax) and a
// bracket-counting regex would mis-slice the first one it met.
// ---------------------------------------------------------------------------

/** The `[...]` or `{...}` starting at `start`, balanced, skipping string bodies. */
function balancedSpan(src: string, start: number): string {
  const open = src[start]
  const close = open === '[' ? ']' : '}'
  let depth = 0
  let quote: string | null = null
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
function declValue(src: string, name: string): string {
  const m = new RegExp(`\\b(?:const|let|var)\\s+${name}\\b[^=]*=\\s*`).exec(src)
  if (!m) throw new Error(`no declaration of ${name}`)
  return balancedSpan(src, m.index + m[0].length)
}

/** Top-level `{...}` literals inside an array span, in source order. */
function objectsIn(arraySpan: string): string[] {
  const out: string[] = []
  let quote: string | null = null
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
 * `key: value` pairs of one object literal, values as written (string literals
 * unquoted, numbers raw). Field order is irrelevant to every caller, so this
 * tolerates the fields being reordered in the source.
 */
function parseObjectLiteral(span: string): Record<string, string> {
  const out: Record<string, string> = {}
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
    const key = /^['"]?([A-Za-z_$][\w$]*)['"]?\s*:\s*/.exec(body.slice(i))
    if (!key) break
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
    } else {
      const start = i
      while (i < body.length && body[i] !== ',' && body[i] !== '\n') i++
      out[key[1]] = body.slice(start, i).trim()
    }
  }
  return out
}

/** The string-literal members of `export type <name> = 'a' | 'b' | ...`, wrapped or not. */
function parseUnion(src: string, name: string): string[] {
  const lines = src.split('\n')
  const at = lines.findIndex((l) => new RegExp(`^\\s*(?:export\\s+)?type\\s+${name}\\s*=`).test(l))
  if (at < 0) throw new Error(`no type ${name}`)
  let decl = lines[at].slice(lines[at].indexOf('=') + 1)
  for (let i = at + 1; i < lines.length && lines[i].trim().startsWith('|'); i++) decl += lines[i]
  return [...decl.matchAll(/'([^']*)'/g)].map((m) => m[1])
}

// ---------------------------------------------------------------------------
// Markdown parsing. Tables come back as cells plus the heading they sit under
// and the line they are on, so a failure can say "CW.md:97" and not "somewhere".
// ---------------------------------------------------------------------------

interface Heading {
  level: number
  text: string
  line: number
}

interface MdTable {
  file: string
  heading: string
  headers: string[]
  /** Data rows only, each with its 1-based line number in the file. */
  rows: { cells: string[]; line: number }[]
}

/** Split a table row on unescaped pipes, dropping the border cells. */
function splitRow(line: string): string[] {
  const t = line.trim()
  const cells: string[] = []
  let cur = ''
  for (let i = 0; i < t.length; i++) {
    if (t[i] === '\\' && t[i + 1] === '|') {
      cur += '|'
      i++
    } else if (t[i] === '|') {
      cells.push(cur)
      cur = ''
    } else cur += t[i]
  }
  cells.push(cur)
  if (cells.length && cells[0].trim() === '') cells.shift()
  if (cells.length && cells[cells.length - 1].trim() === '') cells.pop()
  return cells.map((c) => c.trim())
}

const isDelimiterRow = (line: string) => /^\|?[\s:|-]+\|[\s:|-]*$/.test(line.trim()) && line.includes('-')

/** Lines outside fenced code blocks, so a `#` or `|` in a sample can't read as markup. */
function proseLines(md: string): (string | null)[] {
  let fenced = false
  return md.split('\n').map((l) => {
    if (/^\s*(```|~~~)/.test(l)) {
      fenced = !fenced
      return null
    }
    return fenced ? null : l
  })
}

function headings(md: string): Heading[] {
  const out: Heading[] = []
  proseLines(md).forEach((l, i) => {
    const m = l && /^(#{1,6})\s+(.+?)\s*$/.exec(l)
    if (m) out.push({ level: m[1].length, text: m[2], line: i + 1 })
  })
  return out
}

function tables(md: string, file: string): MdTable[] {
  const lines = proseLines(md)
  const out: MdTable[] = []
  let heading = ''
  for (let i = 0; i < lines.length; i++) {
    const l = lines[i]
    if (l == null) continue
    const h = /^#{1,6}\s+(.+?)\s*$/.exec(l)
    if (h) heading = h[1]
    const next = lines[i + 1]
    if (!l.trim().startsWith('|') || next == null || !isDelimiterRow(next)) continue
    const table: MdTable = { file, heading, headers: splitRow(l), rows: [] }
    for (let r = i + 2; r < lines.length; r++) {
      const row = lines[r]
      if (row == null || !row.trim().startsWith('|')) break
      table.rows.push({ cells: splitRow(row), line: r + 1 })
      i = r
    }
    out.push(table)
  }
  return out
}

/** A cell that is one `` `code span` `` — the whole cell, unwrapped verbatim. */
const unwrapCode = (cell: string) => (/^`[^`]*`$/.test(cell) ? cell.slice(1, -1) : cell)
/** The first `` `code span` `` in a cell that also carries prose. */
const firstCode = (cell: string) => /^`([^`]*)`/.exec(cell)?.[1] ?? null

/** GitHub's heading-anchor slug, which is what every in-doc `](#...)` link resolves against. */
const slug = (text: string) =>
  text
    .toLowerCase()
    .replace(/`/g, '')
    .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/[*_]/g, '')
    .replace(/[^\w\s-]/g, '')
    .trim()
    .replace(/\s/g, '-')

/** Every .md under docs/, absolute. */
function docCorpus(): string[] {
  const out: string[] = []
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const abs = join(dir, name)
      if (statSync(abs).isDirectory()) walk(abs)
      else if (name.endsWith('.md')) out.push(abs)
    }
  }
  walk(repo('docs'))
  return out.sort()
}

const rel = (abs: string) => abs.slice(repo('').length).replace(/\\/g, '/')

// ---------------------------------------------------------------------------
// 1. CW F-key macros — CwCockpit.tsx vs every doc that publishes the table.
// ---------------------------------------------------------------------------

describe('CW F-key macros match CwCockpit.tsx', () => {
  const CW_TSX = read(uiSrc('components/CwCockpit.tsx'))

  const sourceSet = (name: string) =>
    objectsIn(declValue(CW_TSX, name)).map((span, i) => {
      const o = parseObjectLiteral(span)
      for (const f of ['key', 'label', 'text']) {
        if (!(f in o)) throw new Error(`CwCockpit.tsx ${name}[${i}] has no ${f}: ${span}`)
      }
      return { key: o.key, label: o.label, text: o.text }
    })

  const SETS = {
    default: { const: 'DEFAULT_MACROS', macros: sourceSet('DEFAULT_MACROS') },
    fieldDay: { const: 'DEFAULT_FD_MACROS', macros: sourceSet('DEFAULT_FD_MACROS') },
  }

  // A published CW macro table is any table headed Key | Label | … on a CW page (every
  // per-mode page is `<mode>.md`, so RTTY's own four-macro table is not mistaken for this
  // one). The Field Day set is the table under a Field Day heading. Both classifications
  // are read off the doc, so a renamed section or a reordered column fails rather than
  // silently guarding nothing.
  const macroTables = docCorpus()
    .filter((abs) => /(^|\/)cw\.md$/i.test(rel(abs)))
    .flatMap((abs) => tables(read(abs), rel(abs)))
    .filter((t) => t.headers[0]?.toLowerCase() === 'key' && t.headers[1]?.toLowerCase() === 'label')
    .map((t) => ({ ...t, set: /field day/i.test(t.heading) ? SETS.fieldDay : SETS.default }))

  // A guard that quietly matches no table is worse than no guard, so what it found is
  // asserted before what it found is compared.
  it('finds a published table for both macro sets', () => {
    expect(SETS.default.macros).toHaveLength(8)
    expect(SETS.fieldDay.macros).toHaveLength(8)
    const files = [...new Set(macroTables.map((t) => t.file))]
    expect(files, 'no CW page publishes a "Key | Label" table — this guard is checking nothing').toContain(
      'docs/manual/CW.md',
    )
    const fd = macroTables.filter((t) => t.set === SETS.fieldDay).map((t) => `${t.file} — ${t.heading}`)
    expect(fd, 'no Field Day macro table found — DEFAULT_FD_MACROS is unguarded').not.toEqual([])
  })

  it.each(macroTables.map((t) => [`${t.file} — ${t.heading}`, t] as const))(
    'key→label agree in %s',
    (_name, t) => {
      const bad: string[] = []
      const docKeys = t.rows.map((r) => unwrapCode(r.cells[0]))
      const srcKeys = t.set.macros.map((m) => m.key)
      expect(docKeys, `${t.file}:${t.rows[0]?.line} publishes keys ${docKeys.join(',')}, ` +
        `CwCockpit.tsx ${t.set.const} declares ${srcKeys.join(',')}`).toEqual(srcKeys)

      for (const row of t.rows) {
        const key = unwrapCode(row.cells[0])
        const src = t.set.macros.find((m) => m.key === key)
        if (!src) continue // key-set mismatch already reported above
        const label = unwrapCode(row.cells[1])
        if (label !== src.label) {
          bad.push(
            `${t.file}:${row.line} ${key} label says '${label}', ` +
              `CwCockpit.tsx ${t.set.const} says '${src.label}'`,
          )
        }
        // Only a column headed "Content" claims to be the macro verbatim. A column headed
        // "Sends" is a prose gloss and is not compared — see the limitations block.
        if (t.headers[2]?.toLowerCase() === 'content') {
          const text = unwrapCode(row.cells[2])
          if (text !== src.text) {
            bad.push(
              `${t.file}:${row.line} ${key} content says '${text}', ` +
                `CwCockpit.tsx ${t.set.const} says '${src.text}'`,
            )
          }
        }
      }
      expect(bad).toEqual([])
    },
  )
})

// ---------------------------------------------------------------------------
// 2. The mode list — types.ts `Tier` vs docs claiming a mode is not implemented.
// ---------------------------------------------------------------------------

describe('no doc denies a mode that types.ts ships', () => {
  const TIERS = parseUnion(read(uiSrc('types.ts')), 'Tier')

  // Two grammars, because the mode sits on opposite sides of them:
  //   copular — "<subject> is/are not implemented"      → the modes are BEFORE the phrase
  //   verbal  — "does not support <object>"             → the modes are AFTER the phrase
  const COPULAR = /\b(?:is|are|were|was)\s+not\s+(?:yet\s+)?(?:implemented|supported|available|built|shipped)\b|\bnot\s+yet\s+(?:implemented|supported|available|built|shipped)\b/i
  const VERBAL = /\b(?:does|do)\s+not\s+(?:support|implement|decode|transmit|include|offer)\b|\b(?:doesn't|don't)\s+(?:support|implement|decode|transmit|include|offer)\b|\bno\s+support\s+for\b|\b(?:cannot|can't)\s+(?:decode|transmit|receive)\b/i

  /** Markdown blocks (bullets/paragraphs) rejoined from hard-wrapped lines, then split into
   *  sentences — a wrapped sentence in the guide pages spans three source lines. */
  function claims(md: string): { text: string; line: number }[] {
    const lines = proseLines(md)
    const blocks: { text: string; line: number }[] = []
    let cur: { text: string; line: number } | null = null
    lines.forEach((l, i) => {
      if (l == null || l.trim() === '' || /^\s*[#>|]/.test(l)) {
        cur = null
        return
      }
      if (cur && !/^\s*(?:[-*+]|\d+\.)\s/.test(l)) cur.text += ' ' + l.trim()
      else {
        cur = { text: l.trim(), line: i + 1 }
        blocks.push(cur)
      }
    })
    return blocks.flatMap((b) =>
      b.text.split(/(?<=[.;!?])\s+/).map((s) => ({ text: s, line: b.line })),
    )
  }

  // "The in-app note still says Nexus does not transmit JT65" REPORTS someone else's
  // claim — usually to correct it. A reporting verb ahead of the denial marks it as
  // quoted, not asserted.
  const REPORTED = /\b(?:says?|said|claims?|claimed|reads?|warns?|states?)\b/i

  const denials = docCorpus()
    // Release notes are a point-in-time record: "WSPR is not implemented" was TRUE in
    // 0.14.0 and rewriting history there would be the dishonest fix.
    .filter((abs) => !/RELEASE_NOTES-/.test(abs))
    .flatMap((abs) =>
      claims(read(abs)).flatMap(({ text, line }) => {
        const copular = COPULAR.exec(text)
        const verbal = VERBAL.exec(text)
        const before = text.slice(0, (copular ?? verbal)?.index ?? 0)
        if ((copular || verbal) && REPORTED.test(before)) return []
        // A copular subject is the clause immediately before the phrase, not the whole
        // sentence: in "**TempoFast auto-sequencer requires X**: unattended operation is
        // not implemented", the mode is the lead-in, not the subject. Commas are NOT a
        // boundary — the subject is routinely a comma-separated list of modes.
        const scope = copular
          ? before.split(/[:;—]|\s-\s/).pop()!
          : verbal
            ? text.slice(verbal.index + verbal[0].length)
            : null
        if (scope == null) return []
        const named = TIERS.filter((t) => new RegExp(`\\b${t}\\b`).test(scope))
        return named.length
          ? [`${rel(abs)}:${line} says ${named.join(', ')} ${copular ? copular[0] : verbal![0]} — types.ts Tier ships ${named.join(', ')}: "${text.trim()}"`]
          : []
      }),
    )

  it('parses the Tier union', () => {
    expect(TIERS.length).toBeGreaterThan(0)
    expect(TIERS).toContain('FT8')
  })

  it('no doc says Nexus does not do a shipped mode', () => {
    expect(denials).toEqual([])
  })
})

// ---------------------------------------------------------------------------
// 3. Settings tabs — SETTINGS_TABS vs the `##` sections of the settings reference.
// ---------------------------------------------------------------------------

describe('settings-reference.md matches SETTINGS_TABS', () => {
  const REF = 'docs/guide/settings-reference.md'
  const md = read(repo(REF))
  const TABS = objectsIn(declValue(read(uiSrc('components/SettingsPanel.tsx')), 'SETTINGS_TABS')).map(
    parseObjectLiteral,
  )
  const labels = TABS.map((t) => t.label)

  // `##` sections that are page furniture rather than a Settings tab. Everything else at
  // that level must be a tab — this is not a value allowlist, it is the boundary between
  // the tab body and the page's trailing nav.
  const CHROME = new Set(['Related guides'])
  const h2 = headings(md).filter((h) => h.level === 2)
  const tabHeadings = h2.filter((h) => !CHROME.has(h.text))

  it('publishes exactly the tabs the panel renders, in panel order', () => {
    expect(
      tabHeadings.map((h) => h.text),
      `${REF} publishes [${tabHeadings.map((h) => h.text).join(' | ')}], ` +
        `SettingsPanel.tsx SETTINGS_TABS renders [${labels.join(' | ')}]`,
    ).toEqual(labels)
  })

  it('keeps page furniture after the last tab section', () => {
    const lastTab = tabHeadings[tabHeadings.length - 1]?.line ?? 0
    const early = h2.filter((h) => CHROME.has(h.text) && h.line < lastTab)
    expect(early.map((h) => `${REF}:${h.line} '${h.text}' sits between tab sections`)).toEqual([])
  })

  it('the tab nav at the top lists the same tabs in the same order', () => {
    // The nav is the paragraph of same-page links before the first `##`. It is the thing
    // 40-odd cross-page links were modelled on, so it drifts the same way the headings do.
    const head = md.slice(0, md.indexOf('\n## '))
    const nav = [...head.matchAll(/\[([^\]]+)\]\(#([^)]+)\)/g)]
    expect(
      nav.map((m) => m[1]),
      `${REF} nav lists [${nav.map((m) => m[1]).join(' | ')}], SETTINGS_TABS is [${labels.join(' | ')}]`,
    ).toEqual(labels)
    const badAnchors = nav
      .filter((m) => m[2] !== slug(m[1]))
      .map((m) => `${REF} nav link '${m[1]}' points at #${m[2]}, the heading anchor is #${slug(m[1])}`)
    expect(badAnchors).toEqual([])
  })

  it('every link into the settings reference resolves to a section that exists', () => {
    // The dead-tab-name failure was not the headings alone: pages across the doc set link
    // INTO tab anchors, and a consolidated tab leaves those pointing at nothing.
    const anchors = new Set(headings(md).map((h) => slug(h.text)))
    const broken: string[] = []
    for (const abs of docCorpus()) {
      const src = read(abs)
      const isRef = abs === repo(REF)
      for (const m of src.matchAll(/\]\(([^)\s]*settings-reference\.md)?#([^)\s]+)\)/g)) {
        if (!m[1] && !isRef) continue // a same-page anchor in some other file
        if (!anchors.has(m[2])) {
          const line = src.slice(0, m.index).split('\n').length
          broken.push(`${rel(abs)}:${line} links to ${REF}#${m[2]}, which is not a section of it`)
        }
      }
    }
    expect(broken).toEqual([])
  })
})

// ---------------------------------------------------------------------------
// 4. Needed priority tiers — NEED_TIER vs the published priority table.
// ---------------------------------------------------------------------------

describe('Needed-and-Hunting.md matches NEED_TIER', () => {
  const DOC = 'docs/manual/Needed-and-Hunting.md'
  const md = read(repo(DOC))
  const TIER = parseObjectLiteral(declValue(read(uiSrc('features/needs.ts')), 'NEED_TIER'))

  const table = tables(md, DOC).find(
    (t) => /need tag/i.test(t.headers[0] ?? '') && /priority/i.test(t.headers[1] ?? ''),
  )

  it('publishes a priority table', () => {
    expect(table, `${DOC} has no "Need tag | Priority" table to check against NEED_TIER`).toBeTruthy()
  })

  it('publishes every tag NEED_TIER declares, and no other', () => {
    const doc = table!.rows.map((r) => firstCode(r.cells[0])).filter((t): t is string => !!t)
    expect(
      [...doc].sort(),
      `${DOC} publishes [${doc.join(', ')}], needs.ts NEED_TIER declares [${Object.keys(TIER).join(', ')}]`,
    ).toEqual(Object.keys(TIER).sort())
  })

  it('publishes the same number for every tag', () => {
    const bad = table!.rows.flatMap((r) => {
      const tag = firstCode(r.cells[0])
      if (!tag || !(tag in TIER)) return []
      const published = r.cells[1].replace(/[`*]/g, '').trim()
      return published === TIER[tag]
        ? []
        : [`${DOC}:${r.line} ${tag} says ${published}, needs.ts NEED_TIER says ${TIER[tag]}`]
    })
    expect(bad).toEqual([])
  })

  it('the stated tag count matches NEED_TIER', () => {
    // "Eleven need tags are recognized" is a number in prose, so it is parseable and it
    // drifts the instant a tag is added. Checked only when the sentence is there.
    const words = ['zero', 'one', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight', 'nine', 'ten',
      'eleven', 'twelve', 'thirteen', 'fourteen', 'fifteen', 'sixteen', 'seventeen', 'eighteen', 'nineteen', 'twenty']
    const m = /(\w+)\s+need tags are recognized/i.exec(md)
    if (!m) return
    const stated = words.indexOf(m[1].toLowerCase())
    expect(
      stated,
      `${DOC} says "${m[1]} need tags are recognized", needs.ts NEED_TIER declares ${Object.keys(TIER).length}`,
    ).toBe(Object.keys(TIER).length)
  })
})

// ---------------------------------------------------------------------------
// WHAT THESE GUARDS DO NOT CATCH — written down rather than chased with more
// guards, because overstating a guard's reach is how the next gap hides.
//
// - PROSE is unguarded. "F3 sends the report and F4 ends the contact" sits two lines under
//   the CW table and is checked by nobody; only table CELLS are compared. Same for every
//   "what it does for the operator" sentence under a settings field.
// - SCREENSHOTS, and the `<!-- TODO: capture screenshot -->` markers standing in for them.
// - The settings reference is checked at `##` (tab) level ONLY. Its ~90 `###` field blocks
//   are never compared against the panel's actual inputs, so a renamed, moved or deleted
//   setting is invisible here. That is the largest remaining gap on that page.
// - The mode guard reads a FIXED VOCABULARY of denial phrasings (see COPULAR/VERBAL).
//   "WSPR is on the roadmap", "no WSPR yet", or a mode simply MISSING from a support
//   matrix all pass. It also skips denials attributed to another surface ("the tooltip
//   says …"), which is how a doc honestly reports a stale in-app string.
// - It reads only Markdown under docs/. In-app strings are not checked against anything —
//   SettingsPanel.tsx's JT65 hint claiming Nexus "does not transmit it" is exactly this
//   class of contradiction and no guard in this repo sees it.
// - Column-3 macro text is compared only where the column is headed "Content".
//   docs/guide/cw.md heads its column "Sends" and glosses the macro in prose, so its
//   labels are guarded and its bodies are not.
// - Nothing here reaches the Rust side. NEED_TIER mirrors NeedTag::tier() in
//   crates/propagation/src/needalert.rs; that mirror is a Rust concern, guarded there.
// - Release notes are excluded from the mode guard by design (a point-in-time record:
//   "WSPR is not implemented" was true in 0.14.0).
// - A doc that STOPS publishing a table drops silently out of the per-table guards. Only
//   the CW macro tables, the settings tab list and the Needed priority table assert that
//   they found something to check.
// ---------------------------------------------------------------------------
