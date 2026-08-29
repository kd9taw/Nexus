import { describe, it, expect } from 'vitest'
import { readFileSync, readdirSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

// EVERY `var(--token)` MUST POINT AT A TOKEN THAT EXISTS.
//
// This guard exists because thirteen references did not. `--surface` and `--surface-2` were
// referenced thirteen times across the sheet and defined ZERO times — a whole second palette
// vocabulary that never existed — alongside `--text-2` and `--surface-3`. Nothing failed
// loudly, because CSS custom properties fail SILENTLY and in two different ways depending on
// something easy to miss:
//
//   `var(--nope, transparent)` → the fallback, so the surface still renders. Harmless where
//        the fallback was what you wanted, and a latent theme bug where it was not: five
//        recessed wells fell back to a fixed `rgba(0,0,0,.2)` black wash, which reads correctly
//        on a dark panel and rendered as a mid-grey slab on the light theme's near-white one.
//   `var(--nope)` with NO fallback → "invalid at computed-value time". A non-inherited
//        property (background) silently becomes its INITIAL value — transparent — and an
//        INHERITED one (color) silently takes the parent's. `.heartbeat-btn` hit both at once
//        and simply looked like a slightly different button.
//
// Neither shape produces a warning, a console message or a failing test. The only way to
// notice is to go looking, so this goes looking.
//
// A token counts as defined if any stylesheet declares it OR the app sets it at runtime
// (`--ui-zoom` and the rail widths are stamped from index.html and useScale/usePaneWidths;
// they are real, just not authored in CSS).

const uiDir = fileURLToPath(new URL('.', import.meta.url))
const read = (p: string) => readFileSync(p, 'utf8')

function walk(dir: string, test: (name: string) => boolean, out: string[] = []): string[] {
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    if (e.name === 'node_modules') continue
    const p = `${dir}/${e.name}`
    if (e.isDirectory()) walk(p, test, out)
    else if (test(e.name)) out.push(p)
  }
  return out
}

const cssFiles = walk(uiDir, (n) => n.endsWith('.css'))
const codeFiles = walk(uiDir, (n) => /\.(ts|tsx)$/.test(n) && !n.includes('.test.'))
const indexHtml = read(fileURLToPath(new URL('../index.html', import.meta.url)))

/** Tokens DECLARED in a stylesheet (`--x: value`).
 *
 * ⚠️ COMMENTS ARE STRIPPED FIRST, and that is not a nicety — this guard's first run reported
 * `--radius-md` as defined because a comment at styles.css:20624 says, in prose, "--radius-sm,
 * not --radius-md: no sheet defines --radius-md". The colon after the token name in an ordinary
 * English sentence read as a declaration and excused the exact token the sentence was warning
 * about. Same reason cockpit-panes.test.ts strips before it parses. */
const declared = new Set<string>()
for (const f of cssFiles) {
  const css = read(f).replace(/\/\*[\s\S]*?\*\//g, '')
  for (const m of css.matchAll(/(^|[;{\s])(--[a-zA-Z0-9-]+)\s*:/g)) declared.add(m[2])
}

/** Tokens the app supplies at RUNTIME — setProperty, inline style objects, the pre-paint
 *  seed in index.html. Mentioned anywhere in the code counts: this half is deliberately
 *  generous, because a false failure here would be a guard nobody trusts. */
const runtime = new Set<string>()
for (const src of [...codeFiles.map(read), indexHtml]) {
  for (const m of src.matchAll(/(--[a-zA-Z0-9-]+)/g)) runtime.add(m[1])
}

/** Every token REFERENCED through var() in a stylesheet, with where it was referenced. */
const references = new Map<string, string[]>()
for (const f of cssFiles) {
  const css = read(f).replace(/\/\*[\s\S]*?\*\//g, '') // a token named in prose is not a reference
  const lines = css.split('\n')
  lines.forEach((line, i) => {
    for (const m of line.matchAll(/var\(\s*(--[a-zA-Z0-9-]+)/g)) {
      const at = `${f.split('/').pop()}:${i + 1}`
      references.set(m[1], [...(references.get(m[1]) ?? []), at])
    }
  })
}

const undefinedRefs = [...references.keys()]
  .filter((t) => !declared.has(t) && !runtime.has(t))
  .sort()

/**
 * THE KNOWN SURVIVORS — a debt inventory, not an excuse list.
 *
 * Fixing `--surface`/`--surface-2` turned this guard on for the first time, and it found the
 * problem is not those two: SEVENTEEN distinct tokens are referenced and never defined, across
 * EIGHTY-THREE references. `--state-bad` (16), `--warn` (15), `--fs-small` (14) and
 * `--bg-elev-1` (10) are the bulk — and `--bg-elev-1` is the telling one, because `--bg-elev`
 * and `--bg-elev-2` both exist, so it reads as a typo that has been silently falling back for
 * as long as it has been there. Every one of these is the same silent-failure class described
 * at the top of this file, and each needs the same per-surface judgement about whether its
 * fallback was the intent — which is a piece of work in its own right, not something to fold
 * into a change about dropdown backgrounds.
 *
 * ONE ENTRY IS NOT DEBT: `--radix-dropdown-menu-content-available-height` is set at runtime by
 * Radix on its own portaled content. It is correct and should stay.
 *
 * Shrinking this list is the point. Growing it needs a reason as specific as the two below.
 *
 * The two that were looked at closely while fixing the thirteen:
 *
 * `--radius-md` — three references, every one carrying a fallback that matches an existing
 * token exactly (8px = `--radius-sm`, 12px = `--radius`). A mechanical rename with no visual
 * change, deliberately left for the maintainer to take rather than folded into a change about
 * backgrounds.
 *
 * `--surface-3` — one reference, `.cw-chip`, fallback `rgba(255,255,255,.06)`. Rendered in
 * both themes and it reads correctly in both (a subtle raise on dark, carried by its border on
 * light), so by the rule that a token which was never needed is not a bug, it stays. It does
 * leave the chip with a fill on dark and none on light — a cosmetic asymmetry, not a defect.
 *
 * Shrinking this list is the point. Growing it needs a reason as specific as these.
 */
const KNOWN_UNRESOLVED = [
  '--bg-elev-1',
  '--bg-hover',
  '--bg-input',
  '--bg-panel',
  '--bg-raised',
  '--critical',
  '--fs-sm',
  '--fs-small',
  '--mono',
  '--radius-md',
  '--radix-dropdown-menu-content-available-height',
  '--state-bad',
  '--state-info',
  '--surface-3',
  '--text-3',
  '--text-lg',
  '--warn',
  '--warning',
]

describe('CSS custom properties resolve', () => {
  it('is actually reading the sheets (control — an empty scan would pass everything)', () => {
    expect(cssFiles.length).toBeGreaterThanOrEqual(2)
    expect(declared.has('--bg-elev')).toBe(true)
    expect(declared.has('--space-6')).toBe(true)
    expect(references.size).toBeGreaterThan(50)
    // The runtime half must be real too, or it would excuse everything by matching nothing.
    expect(runtime.has('--ui-zoom')).toBe(true)
  })

  it('references no token that is neither declared nor set at runtime', () => {
    const detail = undefinedRefs
      .map((t) => `  ${t} — referenced at ${(references.get(t) ?? []).join(', ')}`)
      .join('\n')
    expect(undefinedRefs, `undefined custom properties referenced:\n${detail}`).toEqual(
      KNOWN_UNRESOLVED,
    )
  })

  it('FIRES: a reference to a token nobody defines is caught, with and without a fallback', () => {
    // The control that must trip, in both silent-failure shapes.
    const probe = (css: string) => {
      const decl = new Set<string>()
      for (const m of css.matchAll(/(^|[;{\s])(--[a-zA-Z0-9-]+)\s*:/g)) decl.add(m[2])
      const refs = [...css.matchAll(/var\(\s*(--[a-zA-Z0-9-]+)/g)].map((m) => m[1])
      return refs.filter((t) => !decl.has(t))
    }
    expect(probe(':root { --real: #fff } .a { color: var(--real) }')).toEqual([])
    expect(probe('.a { background: var(--ghost) }')).toEqual(['--ghost'])
    expect(probe('.a { background: var(--ghost, transparent) }')).toEqual(['--ghost'])
  })

  it('the vocabulary that caused this is gone', () => {
    // --surface and --surface-2 were the thirteen. They must not come back as references,
    // and they must not come back as DEFINITIONS either — inventing them would create a
    // second palette beside --bg/--bg-elev/--panel and nobody would know which to reach for.
    for (const ghost of ['--surface', '--surface-2']) {
      expect(references.has(ghost), `${ghost} is referenced again`).toBe(false)
      expect(declared.has(ghost), `${ghost} was defined — do not start a second palette`).toBe(
        false,
      )
    }
  })
})
