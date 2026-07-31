import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

// FOUR VERIFIED OVERFLOW/FLOOR DEFECTS OUTSIDE THE COCKPITS
// (2026-07-30 assessment — gap-closure/verified.md V14 + census/floors.md #2/#10 +
// census/verified-top.md #10). Each is the same shape as the cockpit bug this batch
// rebuilt — an un-shrinkable box under an `overflow: hidden` ancestor — in a view the
// pane-grid does not reach, so each gets the cheap structural fix and one guard here.
//
// These are CSS-text guards, and the honest limit of that is on record: a rule can exist
// and still lose the cascade (the 0.18.0 fix shipped dead exactly that way). So each guard
// below resolves the WINNING declaration for its selector by specificity then source
// order, the way cockpit-shells.test.ts does — "the rule is present" is not the assertion.
// jsdom cannot lay out, so the pixel claims in the reports stay unverified by test; what
// is verified is that the escape hatch exists and wins.

const CSS = readFileSync(fileURLToPath(new URL('./styles.css', import.meta.url)), 'utf8')

interface Rule {
  selector: string
  body: string
  order: number
}

/** Brace-aware rule walk that DESCENDS into at-rule blocks (@media/@supports), so a rule
 *  nested in a media query is still seen — a conditional override that outranks the fix is
 *  exactly the failure this file exists to catch. */
function parseRules(sheet: string, out: Rule[] = [], counter = { n: 0 }): Rule[] {
  let i = 0
  let selStart = 0
  while (i < sheet.length) {
    if (sheet[i] === '{') {
      const sel = sheet.slice(selStart, i).trim()
      i++
      const bodyStart = i
      let depth = 1
      while (i < sheet.length && depth > 0) {
        if (sheet[i] === '{') depth++
        else if (sheet[i] === '}') depth--
        i++
      }
      const body = sheet.slice(bodyStart, i - 1)
      if (sel.startsWith('@')) {
        parseRules(body, out, counter)
      } else {
        counter.n++
        for (const s of sel.split(',')) {
          const one = s.trim().replace(/\s+/g, ' ')
          if (one) out.push({ selector: one, body, order: counter.n })
        }
      }
      selStart = i
    } else {
      i++
    }
  }
  return out
}

const RULES = parseRules(CSS.replace(/\/\*[\s\S]*?\*\//g, ''))

/** (classes + attributes + pseudo-classes, ids) — enough to order these comparisons. */
function specificity(sel: string): number {
  return (sel.match(/\.[a-z][a-zA-Z0-9-]*|\[[^\]]+\]|#[a-z][a-zA-Z0-9-]*/g) ?? []).length
}

/** The value of `prop` that WINS for an exact selector: highest specificity, then last. */
function winner(selector: string, prop: string): string | null {
  let win: { value: string; spec: number; order: number } | null = null
  const re = new RegExp(`(?:^|;)\\s*${prop}\\s*:\\s*([^;]+)`, 'g')
  for (const r of RULES) {
    if (r.selector !== selector) continue
    const all = [...r.body.matchAll(re)]
    if (!all.length) continue
    const value = all[all.length - 1][1].trim()
    const spec = specificity(r.selector)
    if (!win || spec > win.spec || (spec === win.spec && r.order >= win.order)) {
      win = { value, spec, order: r.order }
    }
  }
  return win?.value ?? null
}

describe('Field Day: the Bonuses list cannot crush the Sections board', () => {
  // census/floors.md #2 — `.fd-bonuses-section { flex: 0 0 auto }` wraps a 15-row grid with
  // no cap, inside `.panel { overflow: hidden }` with no panel-level scroller. Opening
  // Bonuses at 1200×750 drives the only shrinkable child (the sections board) to 0 and
  // takes the residual off the bottom of the log, with no scrollbar anywhere in the chain.
  it('.fd-bonuses-list is height-capped against the effective viewport', () => {
    const v = winner('.fd-bonuses-list', 'max-height')
    expect(v, '.fd-bonuses-list declares no max-height — an un-shrinkable 15-row grid').not.toBeNull()
    expect(
      v!,
      `max-height is \`${v}\` — it must reference var(--vh-eff): a raw vh unit is blind to ` +
        "`.app { zoom }` and would cap against the wrong height at every non-100% UI scale.",
    ).toContain('var(--vh-eff')
  })

  it('.fd-bonuses-list scrolls its own overflow instead of pushing siblings out', () => {
    const v = winner('.fd-bonuses-list', 'overflow-y') ?? winner('.fd-bonuses-list', 'overflow')
    expect(
      v,
      'a capped box with no scroller just clips: the 15 bonus rows must reach the operator ' +
        'inside their own list, which is what stops the growth reaching the sections board.',
    ).toBe('auto')
  })
})

describe('Connect at the xs viewport: the stacked panes are reachable', () => {
  // census/verified-top.md #10 — the xs rule stacks the globe (a 280px floor) plus five
  // auto rows under `.layout.single:has(>.connect-shell){overflow:hidden}` → `.connect-shell`
  // → `.connect`, none of which scrolls. Everything below the globe is painted past the clip
  // line with no path to it.
  it("[data-viewport='xs'] .connect scrolls (nothing above it in the chain does)", () => {
    const v =
      winner("[data-viewport='xs'] .connect", 'overflow-y') ??
      winner("[data-viewport='xs'] .connect", 'overflow')
    expect(
      v,
      'the xs stack puts a 280px-floored globe above five auto rows inside an unbroken ' +
        'overflow:hidden chain — without a scroller here the rail panes and the bottom strip ' +
        'exist only outside the clip.',
    ).toBe('auto')
  })
})

describe('Logbook header: ten touch-floored buttons wrap instead of squeezing', () => {
  // census/floors.md #10 — `.log-actions` is an inline-flex with no wrap holding ten
  // `min-height: var(--touch)` buttons: ~1424px of min-content in a panel hard-capped at
  // 1100px, so every label wraps to 2–3 lines at EVERY geometry and the un-shrinkable
  // header steals 60–90px from the log rows below.
  it('.log-actions wraps', () => {
    const v = winner('.log-actions', 'flex-wrap')
    expect(
      v,
      '.log-actions has no flex-wrap: ten 44px buttons cannot fit the 1100px panel cap at ' +
        'any window size, so they squeeze to min-content and eat the log instead.',
    ).toBe('wrap')
  })
})

describe('SSTV: the band view re-arms its own floor', () => {
  // gap-closure/verified.md V14 — `.sstv-band { min-height: 220px }` inside
  // `.sstv-canvas { flex: 1.1 1 0; min-height: 0; align-items: center }`, whose own comment
  // calls px floors on flex children "the documented vertical-clip bug". A row flex parent
  // centres on the block axis, so the band overflows SYMMETRICALLY and the caption is
  // clipped first, with no scrollbar in the chain (the SSTV shell is overflow:hidden).
  it('.sstv-band min-height self-disarms below its own comfortable size', () => {
    const v = winner('.sstv-band', 'min-height')
    expect(v, '.sstv-band declares no min-height at all').not.toBeNull()
    expect(
      v!.replace(/\s+/g, ''),
      `min-height is \`${v}\` — a bare px floor here is the clip. The idiom the parent's own ` +
        'comment demands is min(220px, 100%): comfortable when there is room, and it yields ' +
        'the moment there is not.',
    ).toBe('min(220px,100%)')
  })
})

// ── The log form wraps DOWN, never overflows RIGHT ────────────────────────────────────
// Inside the pane grid the log column can be as narrow as 24em; without wrap the .le-row
// min-content (sum of intrinsic input widths) overflowed the pane sideways and half the
// fields sat behind a horizontal scrollbar (operator report, 2026-07-31). Guard the two
// declarations that make the form flow into the room the pane actually has.
import { describe as describeLe, it as itLe, expect as expectLe } from 'vitest'
import { readFileSync as readLe } from 'node:fs'
describeLe('log-entry rows wrap instead of overflowing the pane', () => {
  const css = readLe(new URL('./styles.css', import.meta.url), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '')
  itLe('.le-row declares flex-wrap: wrap', () => {
    // Anchored at a rule boundary: '.cw-cockpit .log-entry .le-row' also contains the
    // substring and its block (a margin only) must not satisfy this guard.
    const m = css.match(/(?:^|\})\s*\.le-row\s*\{[^}]*\}/)
    expectLe(m, 'a .le-row block must exist').not.toBeNull()
    expectLe(m![0]).toMatch(/flex-wrap:\s*wrap/)
  })
  itLe('.le-row inputs carry a wrap basis and a released min-width', () => {
    const m = css.match(/\.le-row\s+\.settings-input\s*\{[^}]*\}/)
    expectLe(m).not.toBeNull()
    expectLe(m![0]).toMatch(/flex:\s*1\s+1\s+\d/)
    expectLe(m![0]).toMatch(/min-width:\s*0/)
  })
})
