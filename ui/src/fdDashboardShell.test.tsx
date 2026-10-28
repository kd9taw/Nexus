// @vitest-environment jsdom
//
// THE FIELD DAY VIEW WEARS ITS OWN SHELL — not the digital OPERATING one.
//
// THE REPORT (operator, 2026-08-30, looking at the dashboard during a club Field Day):
// "what screen is this supposed to represent? Why are theyere waterfalls in here when its
// not the primary working area?"
//
// THE CAUSE. `App.tsx` rendered `<ContestView>` through `threePane(...)`, which is not a
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
    expect(arm, 'the fieldDay arm no longer renders ContestView').toContain('<ContestView')
    expect(
      arm,
      'ContestView is not inside a `<main className="layout single">`. Its root is a ' +
        '`.panel`, so `.layout.single > .panel` is the shell that gives it its height, its ' +
        'deficit valve and its measure — the same one Logbook, Settings and Program use.',
    ).toContain('className="layout single"')
  })

  it('the class list those shell rules target is still the one ContestView renders', () => {
    // `.layout.single > .panel` is a `>` combinator against an exact class; rename either
    // half and every rule reasoned about here silently stops applying.
    expect(
      SRC('components/ContestView.tsx'),
      'ContestView no longer roots on `conversation panel fieldday` — the shell rules this ' +
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
        'window untouched, which is the trap the log-view rule above sets.',
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
        // The Logbook went the width of the window from md (2026-10-27), as Settings and Repeaters did.
        'panel log-view logbook': t === 'xs' || t === 'sm' ? '1100px' : 'none',
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

// ── THE LOWER SECTION USES THE WHOLE WINDOW (operator, 2026-10-12) ─────────────────────────────
// Looking at the Contest screen with a contest running on a wide window: "the logging is now the
// right size … everything on the bottom seems narrow in the center, cant this bottom section be
// widened to fit more information on the screen?" The pick: the contest controls, the club board
// and the scoreboard use the whole window, the five buttons sit in one row, and the entry and the
// last contacts stay the size they are.
//
// So the screen's panel loses its 1600px cap at md/lg/xl, as Settings and Repeaters did, and the
// entry's three blocks keep theirs. The prose the cap used to bound gets a measure of its own, on
// lg/xl only: md ends below 1600 effective px, so there the old cap never bound and nothing in the
// panel can be wider than it was, which keeps the 1024×768 floor (85 % zoom, md) as it was.

/** The value `prop` resolves to on `el` when it is `var(--x, fallback)`: the nearest declaring
 *  element's value, `el` itself first, as custom properties inherit; else the fallback. */
function resolveVar(el: Element, value: string): string {
  const m = /^var\(\s*(--[\w-]+)\s*,\s*(.+)\)$/.exec(value.trim())
  if (!m) return value
  for (let n: Element | null = el; n; n = n.parentElement) {
    const d = applied(n, m[1])
    if (d !== null) return d
  }
  return m[2].trim()
}

/** The Contest screen as its two windows build it: the entry's bar and log line, the last
 *  contacts, and the screen's panel with `inner` in it. */
function contestScreen(tier: string, host: 'main' | 'logger', inner = ''): HTMLElement {
  document.documentElement.setAttribute('data-viewport', tier)
  const blocks =
    '<section class="contest-logger-entry"><div class="contest-logger-bar"></div>' +
    '<div class="contest-logger-scaled"><div class="log-entry log-entry-fd"></div></div></section>' +
    '<section class="contest-logger-recent"><div class="contest-logger-recent-body"></div></section>' +
    `<main class="layout single"><section class="${FIELDDAY}">${inner}</section></main>`
  document.body.innerHTML =
    host === 'main'
      ? `<div class="app"><div class="contest-screen">${blocks}</div></div>`
      : `<div class="app detached contest-logger">${blocks}</div>`
  return document.body
}

describe('the lower section uses the whole window, and the entry keeps its size', () => {
  it('the screen’s panel has no cap at md, lg and xl — 1920 is lg, the window the report came from', () => {
    const capped = WIDE.map((t) => `[data-viewport='${t}'] → ${maxWidth(panel(FIELDDAY, t))}`).filter(
      (line) => !line.endsWith('→ none'),
    )
    expect(
      capped,
      'The Contest screen’s lower section is still held to a measure, so a wide window draws it as ' +
        `a centred column with empty gutters, the shape the operator asked to be rid of:\n  ${capped.join('\n  ')}\n`,
    ).toEqual([])
  })

  it('the entry’s bar, its log line and the last contacts keep 1600px, centred, at every tier in both windows', () => {
    const wrong: string[] = []
    for (const host of ['main', 'logger'] as const) {
      for (const t of TIERS) {
        const root = contestScreen(t, host)
        for (const sel of ['.contest-logger-bar', '.contest-logger-scaled > .log-entry-fd', '.contest-logger-recent-body']) {
          const el = root.querySelector(sel)!
          const got = `${maxWidth(el)} / ${applied(el, 'margin-inline') ?? 'unset'}`
          if (got !== '1600px / auto') wrong.push(`${host} [data-viewport='${t}'] ${sel} → ${got}`)
        }
      }
    }
    expect(wrong, `the entry or the last contacts changed size:\n  ${wrong.join('\n  ')}\n`).toEqual([])
  })
})

describe('the control row is a row that wraps, never a crushed column', () => {
  const header = (t: string) =>
    contestScreen(t, 'main', '<div class="panel-header fd-header"><div class="fd-export"></div><div class="fd-upload"></div></div>')

  it('the header row wraps at every tier, so no block in it can squeeze the five buttons to a column', () => {
    // THE MECHANISM BEHIND THE STACKED BUTTONS. `.panel-header` does not wrap, and the merge and
    // upload block asked for a whole line (`flex: 1 0 100%`, which cannot shrink). On one line that
    // took 100 % of the row beside the buttons, so the buttons shrank to one word wide, and the
    // block itself ran past the panel's edge, where the clip cut off the last upload boxes.
    const wrong = TIERS.map((t) => `[data-viewport='${t}'] → ${applied(header(t).querySelector('.fd-header')!, 'flex-wrap')}`).filter(
      (line) => !line.endsWith('→ wrap'),
    )
    expect(wrong, `the control row cannot wrap:\n  ${wrong.join('\n  ')}\n`).toEqual([])
  })

  it('the merge and its upload switch claim no line of their own, so they share one where there is room', () => {
    for (const t of TIERS) {
      const flex = applied(header(t).querySelector('.fd-upload')!, 'flex') ?? 'unset'
      expect(flex.split(/\s+/), `[data-viewport='${t}'] .fd-upload { flex: ${flex} }`).not.toContain('100%')
    }
  })
})

describe('the operator box stays beside Sending when the entry’s title line wraps', () => {
  // The large entry's title line wraps rather than squeezing (A+ at 160 %, Larger text in a 1024 px
  // logger window). The score chip takes the first line's room, so a wrapped line packs to the
  // right, and the box and Sending pack as a pair: neither carries an auto margin that would split
  // the room between them or leave Sending at the far left of the next line. Where the line breaks
  // is a real browser's to say; this computes the three declarations that keep the pair together.
  const titleLine = (scaled: boolean) => {
    document.documentElement.setAttribute('data-viewport', 'md')
    document.body.innerHTML =
      `<div class="${scaled ? 'contest-logger-scaled' : 'pane-body'}"><div class="log-entry log-entry-fd"><div class="le-fd-header">` +
      '<span class="le-fd-score"></span>' +
      (scaled ? '<label class="le-fd-operator"></label>' : '') +
      '<div class="le-fd-sent"></div></div></div></div>'
    const q = (s: string) => document.body.querySelector(s)
    return {
      header: applied(q('.le-fd-header')!, 'justify-content') ?? 'normal',
      box: scaled ? (applied(q('.le-fd-operator')!, 'margin-left') ?? '0') : null,
      sent: applied(q('.le-fd-sent')!, 'margin-left') ?? '0',
    }
  }

  it('the large entry: the line packs right, and neither the box nor Sending pushes away from the other', () => {
    expect(titleLine(true)).toEqual({ header: 'flex-end', box: '0', sent: '0' })
  })

  it('control: a cockpit’s strip, with no box, keeps Sending pushed to the end as it always has', () => {
    expect(titleLine(false)).toEqual({ header: 'normal', box: null, sent: 'auto' })
  })
})

describe('prose inside keeps a measure of its own where the panel outgrew the old cap', () => {
  const PROSE =
    '<div class="fd-mode-status"><p class="fd-mode-line">x</p></div><p class="fd-score-note">x</p>' +
    '<div class="fd-removed"><p class="fd-removed-note">x</p><p class="fd-removed-hint">x</p></div>'
  const measures = (t: string): string[] => {
    const root = contestScreen(t, 'main', PROSE)
    return ['.fd-mode-line', '.fd-score-note', '.fd-removed-note', '.fd-removed-hint'].map(
      (sel) => `${sel} → ${resolveVar(root.querySelector(sel)!, maxWidth(root.querySelector(sel)!))}`,
    )
  }

  it('lg and xl: every sentence block is held to 90ch, the measure Repeaters’ notes got when that page went full width', () => {
    for (const t of ['lg', 'xl'] as const) {
      expect(measures(t), `[data-viewport='${t}']`).toEqual([
        '.fd-mode-line → 90ch',
        '.fd-score-note → 90ch',
        '.fd-removed-note → 90ch',
        '.fd-removed-hint → 90ch',
      ])
    }
  })

  it('xs, sm and md: none, as before — the panel there is never wider than it was', () => {
    for (const t of ['xs', 'sm', 'md'] as const) {
      expect(measures(t), `[data-viewport='${t}']`).toEqual([
        '.fd-mode-line → none',
        '.fd-score-note → none',
        '.fd-removed-note → none',
        '.fd-removed-hint → none',
      ])
    }
  })

  it('the panel hands the club section and the scoreboard’s lone operator box their measures on lg/xl only', () => {
    // Those two are styled inline in ContestView.tsx (theme tokens kept out of this sheet), so they
    // read their measures through these two properties; ContestView.measure.test.tsx renders them.
    for (const t of TIERS) {
      const p = contestScreen(t, 'main').querySelector('.fieldday')!
      const wide = t === 'lg' || t === 'xl'
      expect([applied(p, '--fd-prose-measure'), applied(p, '--fd-control-measure')], `[data-viewport='${t}']`).toEqual(
        wide ? ['90ch', '520px'] : [null, null],
      )
    }
  })
})
