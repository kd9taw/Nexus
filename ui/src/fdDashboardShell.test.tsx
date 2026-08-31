// @vitest-environment jsdom
//
// THE FIELD DAY VIEW WEARS ITS OWN SHELL — not the digital OPERATING one.
//
// THE REPORT (operator, 2026-08-30, looking at the dashboard during a club Field Day):
// "what screen is this supposed to represent? Why are theyere waterfalls in here when its
// not the primary working area?"
//
// THE CAUSE. `App.tsx` rendered `<FieldDayView>` through `threePane(...)`, which is not a
// layout helper — it is the digital operating workspace, and it hardcodes its furniture:
// the stations/chat rail on the left, and on the right a rail of Waterfall + Band activity
// (OperateDecodes) + the mesh LinkPill. The dashboard is a setup / score / log screen. It
// neither reads nor writes any of that, so every one of those panes was answering a question
// the screen was not asking — and the centre column they left it (~640px at 1280, ~1180px at
// 1920) is why its header strip (class + section, RUN / S&P, the cockpit toggle and four
// export buttons) was crushed.
//
// WHAT REPLACES IT. The same `.layout.single > .panel` shell Logbook, Settings, Program and
// POTA already wear — the dashboard's root IS a `.panel`, so it needs no bespoke shell class.
//
// WHAT THIS FILE COMPUTES, and why the two halves are both here. (1) That App really has
// stopped mounting the dashboard in the operating workspace — read off the real source,
// because that decision lives in one `switch` arm and nothing else in the suite can see it.
// (2) That the shell it moved into does not hand it back a NARROWER measure than the centre
// column it left: `.layout.single > .panel` caps at the 1100px PROSE measure, which at 1920
// is less than the three-pane centre — so a shell swap alone would have made the crowding
// the report is about slightly worse. The cap is resolved through the real cascade rather
// than grepped, the settings-width.test.tsx technique, because an override that LOSES reads
// identical to one that is absent.
//
// WHAT IT DOES NOT PROVE. jsdom does not lay out. "The header row fits" is not measured
// here; what is measured is that nothing caps the panel at the prose measure and that the
// rails are gone. The deficit half — that whatever the column still cannot shrink has a
// scroller to go to in the NEW chain — is computed in layout-single-deficit.test.tsx, which
// owns that question for every `.layout.single > .panel` view.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { parseRules, cmpSpec } from './cssCascade'

const SRC = (rel: string): string => readFileSync(resolve(process.cwd(), 'src', rel), 'utf8')

// ⚠️ Comments are stripped BEFORE parsing: `parseRules` hands back the comment above a rule
// glued to that rule's selector (connectLayout.test.ts documents the same trap), and this
// sheet comments heavily over exactly the width rules below — an unstripped selector matches
// nothing, so the guard would go green while seeing none of them.
const RULES = parseRules(SRC('styles.css').replace(/\/\*[\s\S]*?\*\//g, ''))

/** The value of `prop` that ACTUALLY applies to `el` — the winner over every rule of the real
 *  sheet whose selector really matches (jsdom's own selector engine, so `>` and
 *  `[data-viewport]` behave), by specificity then source order. `null` = nothing declares it. */
function applied(el: Element, prop: string): string | null {
  let best: { value: string; spec: readonly [number, number, number]; order: number } | null = null
  for (const r of RULES) {
    const d = r.decls.filter((x) => x.prop === prop).pop()
    if (!d) continue
    let hit = false
    try {
      hit = el.matches(r.selector)
    } catch {
      continue // a selector this jsdom cannot parse cannot be applying either
    }
    if (!hit) continue
    if (!best || cmpSpec(r.spec, best.spec) > 0 || (cmpSpec(r.spec, best.spec) === 0 && r.order >= best.order)) {
      best = { value: d.value, spec: r.spec, order: r.order }
    }
  }
  return best ? best.value : null
}

/** `max-width` as the operator's window resolves it. `none` is the initial value, so "no rule
 *  declares it" and "a rule declares none" are the same answer and must read alike. */
const maxWidth = (el: Element): string => applied(el, 'max-width') ?? 'none'

/** A `.layout.single` panel with `classes`, attached so ancestor and `>` combinators match. */
function panel(classes: string, tier: string): HTMLElement {
  document.documentElement.setAttribute('data-viewport', tier)
  document.body.innerHTML = ''
  const main = document.createElement('main')
  main.className = 'layout single'
  const p = document.createElement('section')
  p.className = classes
  main.appendChild(p)
  document.body.appendChild(main)
  return p
}

const FIELDDAY = 'conversation panel fieldday'
const TIERS = ['xs', 'sm', 'md', 'lg', 'xl'] as const
/** The tiers where a 1100px cap can bind at all — classifyViewport starts md at 1100
 *  effective px, so below that the cap is inert by arithmetic. */
const WIDE = ['md', 'lg', 'xl'] as const

describe('the Field Day dashboard is not wearing the operating workspace', () => {
  /** The `case 'fieldDay':` arm of App's workspace switch, on its own. Read from source
   *  because this decision is one `switch` arm inside an 8k-line component that no test in
   *  the suite renders — and it is the whole subject of the report. */
  function fieldDayCase(): string {
    const app = SRC('App.tsx')
    const start = app.indexOf("case 'fieldDay':")
    expect(start, "App.tsx no longer has a `case 'fieldDay':` workspace arm").toBeGreaterThan(0)
    const end = app.indexOf("case 'logbook':", start)
    expect(end, 'the fieldDay arm is no longer followed by the logbook arm').toBeGreaterThan(start)
    return app.slice(start, end)
  }

  it('does not mount the dashboard through threePane', () => {
    // `threePane` is not a layout helper — it hardcodes `stationsPanel` and `waterfallRail`.
    // Going through it is exactly how the waterfall got onto a scoreboard.
    expect(
      fieldDayCase(),
      'The Field Day dashboard is back inside `threePane(...)`, which hardcodes the ' +
        'stations/chat rail and the waterfall + band-activity + link-pill rail. Those are ' +
        'the digital OPERATING surface; this screen is setup, score and log.',
    ).not.toContain('threePane(')
  })

  it('mounts it in the single-column panel shell instead', () => {
    const arm = fieldDayCase()
    expect(arm, 'the fieldDay arm no longer renders FieldDayView').toContain('<FieldDayView')
    expect(
      arm,
      'FieldDayView is not inside a `<main className="layout single">`. Its root is a ' +
        '`.panel`, so `.layout.single > .panel` is the shell that gives it its height, its ' +
        'deficit valve and its measure — the same one Logbook, Settings and Program use.',
    ).toContain('className="layout single"')
  })

  it('the class list those shell rules target is still the one FieldDayView renders', () => {
    // `.layout.single > .panel` is a `>` combinator against an exact class; rename either
    // half and every rule reasoned about here silently stops applying.
    expect(
      SRC('components/FieldDayView.tsx'),
      'FieldDayView no longer roots on `conversation panel fieldday` — the shell rules this ' +
        'file resolves are modelling a view that is not rendered any more.',
    ).toContain(`className="${FIELDDAY}"`)
  })
})

describe('the shell gives the scoreboard more room than the centre column did, not less', () => {
  it('is not held to the 1100px prose measure at any tier where it could bind', () => {
    // THE TRAP THIS EXISTS FOR. `.layout.single > .panel { max-width: 1100px }` is a PROSE
    // measure. The three-pane centre column it replaced was ~1180px at 1920 (a 1920 window
    // classifies **lg**, not xl), so inheriting the base cap would have answered "the
    // waterfall is gone" and made the crowded header row the report also names WORSE.
    const bound = WIDE.filter((t) => maxWidth(panel(FIELDDAY, t)) === '1100px').map(
      (t) => `[data-viewport='${t}'] → ${maxWidth(panel(FIELDDAY, t))}`,
    )
    expect(
      bound,
      'The Field Day dashboard is capped at the prose measure. It is not prose: it is a ' +
        'header strip of seven controls, a score-tile row, an 85-cell sections board and a ' +
        `log with columns — width there buys COLUMNS, not longer lines:\n  ${bound.join('\n  ')}\n`,
    ).toEqual([])
  })

  it('is wider than the prose measure at lg — the window a club laptop actually runs', () => {
    const v = maxWidth(panel(FIELDDAY, 'lg'))
    const px = v === 'none' ? Infinity : parseFloat(v)
    expect(
      px,
      `[data-viewport='lg'] → max-width: ${v}. 1920 is lg, and it is the size the report ` +
        'came from — a widening scoped to xl (≥2400 effective px) would leave the reported ' +
        'window untouched, which is the trap the log-view/radioprog rule above sets.',
    ).toBeGreaterThan(1100)
  })

  // POSITIVE CONTROL — the same resolver, same element shape, must still report the SHIPPED
  // value for the panels the prose measure is right for. If it simply answered "wide" to
  // everything, or if the override had been written unscoped, this is what fails.
  it('leaves the other .layout.single panels exactly as they were', () => {
    const wrong: string[] = []
    for (const t of TIERS) {
      const expected: Record<string, string> = {
        'panel pota-view pota-hunter': '1100px', // prose-shaped, and still is
        'panel log-view logbook': t === 'xl' ? '1600px' : '1100px',
      }
      for (const [classes, want] of Object.entries(expected)) {
        const got = maxWidth(panel(classes, t))
        if (got !== want) wrong.push(`[data-viewport='${t}'] .${classes} → ${got} (expected ${want})`)
      }
    }
    expect(
      wrong,
      `the Field Day widening leaked into other panels:\n  ${wrong.join('\n  ')}\n`,
    ).toEqual([])
  })

  it('does not reach below the widening — the 1024×768 supported floor is untouched', () => {
    // The safety argument, computed rather than asserted: at xs/sm the dashboard must resolve
    // to exactly what shipped, so the supported floor cannot have moved.
    for (const t of ['xs', 'sm'] as const) {
      expect(
        maxWidth(panel(FIELDDAY, t)),
        `[data-viewport='${t}']: the widening leaked below 1100px effective width.`,
      ).toBe('1100px')
    }
  })
})
