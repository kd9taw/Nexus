import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { parseRules } from './cssCascade'

// The CALL field of the Logbook edit form is the only one that shares its grid track with a
// button (the QRZ lookup). `.logbook-form-grid` is `repeat(auto-fit, minmax(140px, 1fr))`, so a
// ~900px form gives tracks of exactly 140px; the button (~56px) plus the gap (8px) then leaves
// the input a ~50px text box — five characters at 14px, which is how a logged `WW9WTF` reads
// back as "WW9WT" in the form (operator report, 2026-09-08). jsdom lays nothing out, so the fix
// is guarded here as CSS text plus the one thing that would silently rot it: the class actually
// being on the element. The DATA half — that the form loads and saves the whole call — is
// measured in components/Logbook.test.tsx.
const css = readFileSync(fileURLToPath(new URL('./styles.css', import.meta.url)), 'utf8')
const tsx = readFileSync(
  fileURLToPath(new URL('./components/Logbook.tsx', import.meta.url)),
  'utf8',
)

/** `parseRules` hands back the comment above a rule glued to its selector — strip it, or a
 *  matcher stops matching the moment somebody writes a comment over the rule. */
const clean = (s: string) =>
  s
    .replace(/\/\*[\s\S]*?\*\//g, ' ')
    .replace(/^[\s\S]*\*\//, ' ')
    .trim()

const declFor = (selector: string, prop: string): string | null => {
  const hit = parseRules(css)
    .filter((r) => clean(r.selector) === selector)
    .flatMap((r) => r.decls.filter((d) => d.prop === prop))
    .pop()
  return hit?.value ?? null
}

describe('the Logbook edit form fits a whole callsign in CALL', () => {
  it('wires the CALL cell to its own rule', () => {
    // Without the class the two rules below are dead CSS and the field silently goes back to
    // ~5 visible characters.
    expect(tsx).toMatch(/className="logbook-field logbook-field-call"/)
  })

  it('lets the CALL row wrap instead of squeezing the input', () => {
    // WRAP, not a min-width floor: a floor would overflow the 140px track this grid most
    // often produces (the `.logbook-when` idiom works only because that control is rare).
    expect(declFor('.logbook-field-call .settings-input-row', 'flex-wrap')).toBe('wrap')
  })

  it('gives the CALL input a text-box floor to wrap against', () => {
    // `.settings-input-row .settings-input` is `flex: 1` (basis 0), which never forces a
    // wrap — the input just shrinks to whatever is left. A real basis is what makes the
    // button drop to its own line when the track is tight.
    const flex = declFor('.logbook-field-call .settings-input', 'flex')
    expect(flex).not.toBeNull()
    const basis = Number(/(\d+)px\s*$/.exec(flex ?? '')?.[1] ?? 0)
    expect(basis, `CALL input flex-basis must leave room for a full call, got "${flex}"`)
      .toBeGreaterThanOrEqual(110)
  })
})
