// THE PLACEHOLDER GUARD — a call site and its catalog entry must agree about the holes.
//
// The sibling guard (hardcoded-strings.test.ts) proves the English is out of the JSX, and the
// type system proves a key EXISTS (`MessageKey = keyof typeof EN`). Neither can see the gap
// between them: a key that exists, used at a call site that passes the WRONG values for it.
//
//   catalog:   'spots.postedBy': 'Spotted by {{callsign}}'
//   call site: t('spots.postedBy', { call: spotter })
//
// That compiles, ships, and puts the literal text `Spotted by {{callsign}}` on an operator's
// screen — `interpolate` deliberately leaves an unsupplied placeholder visible rather than
// blanking the sentence, on the grounds that a visible bug is a reportable one. This makes it
// a CI failure instead, which is cheaper than a bug report.
//
// It checks three disagreements, and each is a real shape:
//
//   MISSING VALUE   — the entry has {{x}} and the call site does not pass `x`. The operator
//                     sees `{{x}}`.
//   UNUSED VALUE    — the call site passes `x` and no form of the entry mentions it. Nothing
//                     renders wrong, but it is nearly always the other half of a rename: the
//                     entry says {{callsign}} and someone is still passing `call`. (`count` is
//                     exempt on a PLURAL entry — it selects the form whether or not a given
//                     form prints it.)
//   MARKUP MISMATCH — the entry carries a `<b>…</b>` marker that its call site does not
//                     declare in `tags`, or carries one at all while being read through `t()`,
//                     which does not parse markup. Either way the operator sees the angle
//                     brackets. (Extra `tags` are not an error: a call site may reasonably
//                     declare markers for a sentence a translation will add emphasis to.)
//
// IT COMPUTES, like its sibling: the TypeScript compiler answers what each call site passes.
// And it is proven to fire on every run — `the checker fires` below runs it over a fixture
// written to break each rule.
//
// ---------------------------------------------------------------------------------------
// ⚠️ SCOPE — what it cannot see.
// ---------------------------------------------------------------------------------------
//
//   • Only call sites whose key AND params are written literally. `t(row.labelKey, vals)` is
//     invisible to it, and so is `{...spread}`. The count of skipped sites is asserted to stay
//     small, so a refactor that hides every call site behind a variable fails here rather than
//     quietly emptying the guard.
//   • It checks EVERY file under ui/src, not just the migrated ones — a mismatch is a bug
//     wherever it lives, and there is no reason to wait for a file's migration batch.
//   • It cannot tell a WRONG key from a right one (`t('logbook.empty')` where
//     `t('logbook.none')` was meant). Both sides are consistent; only a human reading the
//     screen can see that.

import { describe, expect, it } from 'vitest'
import { readFileSync, readdirSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import ts from 'typescript'
import { EN, type MessageKey } from './index'
import { DE } from './de'
import type { PluralForms } from './types'

/** A `t()` / `<T>` call site with everything written literally enough to check. */
interface Site {
  file: string
  line: number
  key: string
  /** Value names the call site passes. */
  vals: string[]
  /** Marker names the call site declares (`<T tags={{ b: … }} />`); null for a `t()` call. */
  tags: string[] | null
}

interface Problem {
  file: string
  line: number
  key: string
  kind: 'missing-value' | 'unused-value' | 'markup-mismatch'
  detail: string
}

/** The names in an object literal — `{ a, b: x }` → ['a','b']. Null when it is not literal. */
function objectKeys(n: ts.Node | undefined): string[] | null {
  if (!n) return null
  if (ts.isJsxExpression(n)) return objectKeys(n.expression)
  if (!ts.isObjectLiteralExpression(n)) return null
  const out: string[] = []
  for (const p of n.properties) {
    if (ts.isShorthandPropertyAssignment(p)) out.push(p.name.text)
    else if (ts.isPropertyAssignment(p) && !ts.isComputedPropertyName(p.name))
      out.push(p.name.getText())
    // A spread or a computed name hides its contents — the whole site is unusable.
    else return null
  }
  return out
}

/** Every checkable call site in one source file, plus the ones that had to be skipped. */
export function findSites(file: string, src: string): { sites: Site[]; skipped: number } {
  const sf = ts.createSourceFile(file, src, ts.ScriptTarget.ES2020, true, ts.ScriptKind.TSX)
  const sites: Site[] = []
  let skipped = 0
  const line = (n: ts.Node) => sf.getLineAndCharacterOfPosition(n.getStart(sf)).line + 1

  const visit = (n: ts.Node): void => {
    if (ts.isCallExpression(n) && ts.isIdentifier(n.expression) && n.expression.text === 't') {
      const [k, params] = n.arguments
      if (k && ts.isStringLiteral(k)) {
        const vals = params === undefined ? [] : objectKeys(params)
        if (vals === null) skipped++
        else sites.push({ file, line: line(n), key: k.text, vals, tags: null })
      } else skipped++
    } else if (ts.isJsxSelfClosingElement(n) || ts.isJsxOpeningElement(n)) {
      if (n.tagName.getText(sf) !== 'T') {
        n.forEachChild(visit)
        return
      }
      const attr = (name: string) =>
        n.attributes.properties.find(
          (p): p is ts.JsxAttribute => ts.isJsxAttribute(p) && p.name.getText(sf) === name,
        )
      const k = attr('k')?.initializer
      const valsAttr = attr('vals')
      const tagsAttr = attr('tags')
      if (!k || !ts.isStringLiteral(k)) {
        skipped++
        n.forEachChild(visit)
        return
      }
      const vals = valsAttr ? objectKeys(valsAttr.initializer) : []
      const tags = tagsAttr ? objectKeys(tagsAttr.initializer) : []
      if (vals === null || tags === null) skipped++
      else sites.push({ file, line: line(n), key: k.text, vals, tags })
    }
    n.forEachChild(visit)
  }
  visit(sf)
  return { sites, skipped }
}

/** Every `{{name}}` in a catalog entry, across all plural forms. */
function placeholdersOf(entry: string | PluralForms): Set<string> {
  const texts = typeof entry === 'string' ? [entry] : Object.values(entry)
  const out = new Set<string>()
  for (const t of texts) for (const m of t.matchAll(/\{\{(\w+)\}\}/g)) out.add(m[1])
  return out
}

/** Every `<marker>` in a catalog entry — the opening tags `parseRich` would look for. */
function markersOf(entry: string | PluralForms): Set<string> {
  const texts = typeof entry === 'string' ? [entry] : Object.values(entry)
  const out = new Set<string>()
  for (const t of texts) for (const m of t.matchAll(/<(\w+)>/g)) out.add(m[1])
  return out
}

/** Compare one call site with its catalog entry. */
function problemsAt(site: Site): Problem[] {
  const entry = EN[site.key as MessageKey]
  // A key the catalog lacks is the sibling guard's job (and the type system's).
  if (entry === undefined) return []
  const want = placeholdersOf(entry)
  const got = new Set(site.vals)
  const isPlural = typeof entry !== 'string'
  const out: Problem[] = []
  const at = (kind: Problem['kind'], detail: string) => out.push({ ...site, kind, detail })

  for (const name of want)
    if (!got.has(name)) at('missing-value', `entry needs {{${name}}}, call site passes nothing`)
  for (const name of got)
    if (!want.has(name) && !(isPlural && name === 'count'))
      at('unused-value', `call site passes \`${name}\`, no form of the entry uses it`)

  const markers = markersOf(entry)
  if (site.tags === null) {
    for (const m of markers)
      at('markup-mismatch', `entry carries <${m}>, and t() does not parse markup — use <T>`)
  } else {
    const declared = new Set(site.tags)
    for (const m of markers)
      if (!declared.has(m)) at('markup-mismatch', `entry carries <${m}>, tags declares no \`${m}\``)
  }
  return out
}

// ── the real tree ─────────────────────────────────────────────────────────────────────

const SRC = fileURLToPath(new URL('..', import.meta.url))

function sourceFiles(dir = SRC, rel = ''): string[] {
  const out: string[] = []
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = rel ? `${rel}/${e.name}` : e.name
    if (e.isDirectory()) {
      if (e.name === 'node_modules' || e.name === 'dist') continue
      out.push(...sourceFiles(`${dir}/${e.name}`, p))
    } else if (/\.tsx?$/.test(e.name) && !/\.test\.tsx?$/.test(e.name) && !/\.d\.ts$/.test(e.name)) {
      out.push(p)
    }
  }
  return out
}

const scanned = sourceFiles().map((rel) => ({
  rel,
  ...findSites(rel, readFileSync(`${SRC}/${rel}`, 'utf8')),
}))
const allSites = scanned.flatMap((f) => f.sites)
const allSkipped = scanned.reduce((a, f) => a + f.skipped, 0)

describe('call sites agree with their catalog entries', () => {
  it('finds call sites at all — the extractor is not silently returning nothing', () => {
    // The control. Without it, a broken extractor makes every assertion below vacuously true,
    // which is this project's most-repeated defect class.
    expect(allSites.length, 'literal t()/<T> call sites found').toBeGreaterThan(1000)
    expect(allSites.some((s) => s.vals.length > 0), 'some site passes values').toBe(true)
    expect(allSites.some((s) => s.tags !== null), 'some site is a <T>').toBe(true)
  })

  it('leaves few call sites unreadable — a refactor cannot quietly empty this guard', () => {
    // 67 today. Every one of them is the same deliberate shape — a REGISTRY resolving its own
    // key at runtime (`t(v.labelKey)` in needVisuals, statusMeta, satVfo, features/registry,
    // the settings registry, the wizard's step tables) — which is how a label table gets
    // migrated without its dozen consumers changing. They are unchecked all the same, so the
    // number is pinned rather than ignored, and it was raised from 40 only after listing what
    // the new ones were: 24 files, all key tables, no call site hidden behind a variable by
    // accident. Raise it again the same way — by looking first.
    expect(allSkipped, 'call sites with a computed key or spread params').toBeLessThan(90)
  })

  it('supplies every value its entry asks for', () => {
    const bad = allSites.flatMap(problemsAt).filter((p) => p.kind === 'missing-value')
    expect(bad.map((p) => `${p.file}:${p.line} ${p.key} — ${p.detail}`)).toEqual([])
  })

  it('passes no value the entry does not use', () => {
    const bad = allSites.flatMap(problemsAt).filter((p) => p.kind === 'unused-value')
    expect(bad.map((p) => `${p.file}:${p.line} ${p.key} — ${p.detail}`)).toEqual([])
  })

  it('renders every markup marker through <T> with a matching tag', () => {
    const bad = allSites.flatMap(problemsAt).filter((p) => p.kind === 'markup-mismatch')
    expect(bad.map((p) => `${p.file}:${p.line} ${p.key} — ${p.detail}`)).toEqual([])
  })
})

// ── the checker fires (positive control) ──────────────────────────────────────────────

describe('the checker fires', () => {
  // Real keys, so the fixture is checked against the real catalog exactly as a source file is.
  const PLAIN = 'settings.search.matched' // 'matched “{{term}}”'
  const RICH = 'reveal.prompt' // carries <b> markers
  const NO_HOLES = 'reveal.notNow'

  const check = (src: string) => findSites('fixture.tsx', src).sites.flatMap(problemsAt)

  it('catches a value the entry needs and the call site does not pass', () => {
    const found = check(`const s = t('${PLAIN}')`)
    expect(found.map((p) => p.kind)).toEqual(['missing-value'])
    expect(found[0].detail).toContain('{{term}}')
  })

  it('catches a renamed value — the shape this guard exists for', () => {
    const found = check(`const s = t('${PLAIN}', { search: q })`)
    expect(found.map((p) => p.kind).sort()).toEqual(['missing-value', 'unused-value'])
  })

  it('catches markup read through t() instead of <T>', () => {
    const found = check(`const s = t('${RICH}', { achievement: a, feature: f })`)
    expect(found.map((p) => p.kind)).toContain('markup-mismatch')
  })

  it('catches a <T> whose tags do not declare the marker the entry uses', () => {
    const found = check(`const e = <T k="${RICH}" vals={{ achievement: a, feature: f }} />`)
    expect(found.map((p) => p.kind)).toContain('markup-mismatch')
  })

  it('passes the correct forms of all three — the guard is not simply always red', () => {
    expect(check(`const s = t('${PLAIN}', { term })`)).toEqual([])
    expect(check(`const s = t('${NO_HOLES}')`)).toEqual([])
    expect(
      check(`const e = <T k="${RICH}" tags={{ b: <strong /> }} vals={{ achievement: a, feature: f }} />`),
    ).toEqual([])
  })

  it('does not read a spread or a computed key as a clean call site', () => {
    const { sites, skipped } = findSites('fixture.tsx', `t('${PLAIN}', { ...vals }); t(row.key)`)
    expect(sites).toEqual([])
    expect(skipped).toBe(2)
  })
})

// ── every OTHER catalog must agree with English ───────────────────────────────────────
//
// The guard above checks call sites against the ENGLISH catalog. A translation is the other
// half of the same contract and it is the half nobody can eyeball: a reviewer who does not read
// German cannot see that `{{callsign}}` came back as `{{rufzeichen}}`, and the operator finds
// out when their screen says `{{rufzeichen}}` in the middle of a sentence.
//
// Three rules, and the third is the one with teeth:
//   1. Same placeholders as English — a translation may reorder them, never rename or drop one.
//   2. Same markup markers — `<b>` may move within the sentence, but a marker the call site
//      never declared renders as literal angle brackets.
//   3. NO DECIMAL COMMA IN A NUMBER. German writes 14,074 for fourteen-point-oh-seven-four.
//      A translator (human or machine) "fixing" a frequency in an example sentence would put a
//      comma into something an operator reads as a dial setting. Nothing else in this project
//      guards that, because nothing else looks at a translated catalog.
describe('translated catalogs agree with English', () => {
  // Every catalog the build ships, by locale. English is the source and is checked by the
  // suite above; this loop covers the rest, and is EMPTY until a translation lands — hence
  // the fixture control below, which proves the rules bite before there is anything to bite.
  const OTHER: Array<[string, Record<string, unknown>]> = [['de', DE as Record<string, unknown>]]

  const holesOf = (v: unknown): Set<string> => {
    const texts = typeof v === 'string' ? [v] : Object.values(v as Record<string, string>)
    const out = new Set<string>()
    for (const t of texts) for (const m of String(t).matchAll(/\{\{(\w+)\}\}/g)) out.add(m[1])
    return out
  }
  const marksOf = (v: unknown): Set<string> => {
    const texts = typeof v === 'string' ? [v] : Object.values(v as Record<string, string>)
    const out = new Set<string>()
    for (const t of texts) for (const m of String(t).matchAll(/<(\w+)>/g)) out.add(m[1])
    return out
  }
  /** A digit, a comma, then digits — `14,074`. The shape a decimal comma takes in prose. */
  const DECIMAL_COMMA = /\d,\d/

  /** The three checks, exported through the closure so the fixture can run them too. */
  function catalogProblems(locale: string, cat: Record<string, unknown>): string[] {
    const out: string[] = []
    for (const [key, value] of Object.entries(cat)) {
      const en = (EN as Record<string, unknown>)[key]
      if (en === undefined) {
        out.push(`${locale}/${key}: no such key in English`)
        continue
      }
      const wantHoles = [...holesOf(en)].sort()
      const gotHoles = [...holesOf(value)].sort()
      if (wantHoles.join() !== gotHoles.join())
        out.push(`${locale}/${key}: placeholders ${gotHoles.join(',')} ≠ English ${wantHoles.join(',')}`)
      const wantMarks = [...marksOf(en)].sort()
      const gotMarks = [...marksOf(value)].sort()
      if (wantMarks.join() !== gotMarks.join())
        out.push(`${locale}/${key}: markers ${gotMarks.join(',')} ≠ English ${wantMarks.join(',')}`)
      const texts = typeof value === 'string' ? [value] : Object.values(value as Record<string, string>)
      for (const t of texts)
        if (DECIMAL_COMMA.test(String(t)))
          out.push(`${locale}/${key}: DECIMAL COMMA in "${t}" — a number an operator may read as a dial`)
    }
    return out
  }

  it('the rules bite (positive control — the catalogs list is empty until a language ships)', () => {
    const broken = {
      'settings.search.matched': 'gefunden “{{suchbegriff}}”', // renamed hole
      'reveal.prompt': 'kein Markup hier', // dropped markers
      'reveal.notNow': 'Nicht jetzt — 14,074 MHz', // decimal comma
      'not.a.real.key': 'x',
    }
    const found = catalogProblems('de', broken)
    expect(found.some((p) => p.includes('placeholders'))).toBe(true)
    expect(found.some((p) => p.includes('markers'))).toBe(true)
    expect(found.some((p) => p.includes('DECIMAL COMMA'))).toBe(true)
    expect(found.some((p) => p.includes('no such key'))).toBe(true)
    // …and a correct translation passes all four.
    expect(
      catalogProblems('de', {
        'settings.search.matched': 'gefunden “{{term}}”',
        'reveal.notNow': 'Nicht jetzt',
      }),
    ).toEqual([])
  })

  for (const [locale, cat] of OTHER) {
    it(`${locale} carries English's holes, markers and no decimal comma`, () => {
      expect(catalogProblems(locale, cat)).toEqual([])
    })
  }
})
