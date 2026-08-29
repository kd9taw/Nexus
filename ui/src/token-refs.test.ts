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

/** Every token REFERENCED through var() in a stylesheet, with where it was referenced.
 *
 * ⚠️ COMMENT BODIES ARE BLANKED IN PLACE, NOT DELETED — a token named in prose is not a
 * reference, but this half also reports WHERE, so the line arithmetic has to survive the
 * strip. Deleting a comment eats its newlines and every line after it shifts up: a probe
 * appended to the LAST line of styles.css was reported at :19856 of a 22k-line file, which
 * sends the next reader hunting through unrelated rules. styles-theme-cascade.test.ts blanks
 * for the same reason. `declared` above may delete freely — it records no positions. */
const references = new Map<string, string[]>()
for (const f of cssFiles) {
  const css = read(f).replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
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
 * THE LIST IS EMPTY OF DEBT, and that is the point — it held nineteen names and 142 references
 * when this guard was written, and every one has been resolved onto a token the sheet already
 * has (2026-08-28). Nothing here is excused any more.
 *
 * ONE ENTRY IS NOT DEBT: `--radix-dropdown-menu-content-available-height` is set at runtime by
 * Radix on its own portaled content. It is correct and should stay.
 *
 * WHAT THE NINETEEN TURNED OUT TO BE — recorded because the next undefined token will be one of
 * the same three shapes, and knowing which saves re-deriving it:
 *
 *   A SECOND SPELLING of a role the sheet already names (the bulk). `--danger`/`--state-bad`'s
 *   destructive half → `--state-weak`, the sheet's red (`.conn-dot.bad`, `.conn-test-result.fail`);
 *   `--ok` → `--state-good`; `--warn`/`--warning`/`--state-bad`'s warn half → `--alert-warning`;
 *   `--state-info` → `--alert-info`; `--critical` → `--state-weak`; `--fs-small`/`--fs-sm` →
 *   `--fs-label`; `--text-lg` → `--fs-title`; `--text-3` → `--text-faint`; `--bg-panel` →
 *   `--panel`; `--mono` → `--font-mono`; `--bg-hover`/`--surface-3` → `--bg-elev-2`.
 *
 *   A TYPO in a ladder that exists. `--bg-elev-1` is `--bg-elev` — `.mv-grid th` paints
 *   `--bg-elev` and `.mv-section-row td`, a header band in the SAME table, painted
 *   `--bg-elev-1`. `--radius-md`'s fallbacks were 8px and 12px, exactly `--radius-sm` and
 *   `--radius`. `--bg-raised` is `--bg-elev`.
 *
 *   THE WRONG FAMILY ENTIRELY, which no rename fixes. `var(--warn, #e6b800)` on `.rp-star.on`
 *   and `.mv-star.on` was never a warning: they are FAVOURITE stars, and #e6b800 is the "second,
 *   slightly-off gold" that `.sat-star.on` and `.sat-fav-mark` already carry a comment against.
 *   Those took the app's one chase gold (#f5a524, light-overridden to #9d6500) — a literal,
 *   because that is how the other three stars express it, not a new token.
 *
 * Three sites were RECESSED roles, so they took `--bg` rather than the ladder step their name
 * implied: two inputs and a log well, where `--bg-elev` equals `--panel` in the light theme and
 * would have rendered them invisible. `.sstv-tx-progress-track` carries the same note.
 *
 * Growing this list needs a reason as specific as the Radix entry.
 */
const KNOWN_UNRESOLVED = ['--radix-dropdown-menu-content-available-height']

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
