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
    // 29 today against 1,402 checkable ones. Dynamic keys are legitimate — the settings
    // registry resolves `labelKey` at runtime — but they are unchecked, so their number is
    // pinned rather than ignored. Raise this only with a reason: every increment is a call
    // site nothing verifies.
    expect(allSkipped, 'call sites with a computed key or spread params').toBeLessThan(40)
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
