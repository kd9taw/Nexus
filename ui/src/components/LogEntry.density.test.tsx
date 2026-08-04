// @vitest-environment jsdom
//
// THE SHARED LOG STRIP MUST COMMIT ABOVE THE FOLD.
//
// THE DEFECT (operator, 0.28.2, at low resolution): "the Log button ends up below the fold
// with a half-typed QSO above it." Measured by the Phone density pass at the shipped default
// window (tauri.conf.json: 1200x720): the strip stands ~422 px inside a ~392 px log pane
// body, so the commit row sits ~30 px past the fold. Nothing is TRAPPED — `.pane-body` is
// `overflow: auto`, so the button can be scrolled to — but the operator is scrolling to reach
// the primary action of the pane, mid-QSO, which is the whole complaint.
//
// WHAT THIS FILE PINS. Two chrome boxes stand between the top of the pane body and that
// button and neither carries information the frame does not already carry:
//   1. `<h2>Log this QSO</h2>` — the CockpitPaneFrame head two lines above it already reads
//      "LOG" and is the frame's accessible name. ~30 px.
//   2. `.log-entry`'s own card (border + padding) inside `.pane-body`'s card. `.log-entry`
//      carries no `panel` class, so the shipped `.pane-body > .panel` flatten misses it —
//      the same miss `.pane-body > .rtty-stream` already documents. ~13 px above the button
//      (26 px of the pane's total content height).
//
// HOW THIS GUARD COMPUTES, and what each number is. jsdom lays nothing out, so the stack is
// summed rather than measured — but only the MEASURED half is a constant here:
//   · COMPUTED, live, from the real sheet through the real cascade on the real rendered tree:
//     every margin / border / padding / min-height / font-size / line-height of every box the
//     density pass touches. Those are the terms that move, so a rule that comes back — or one
//     that is added and LOSES the cascade — moves this number and fails here. Selector
//     matching is jsdom's own (`Element.matches`), so `>` and `[data-viewport]` behave; the
//     cascade (importance, then specificity, then source order) is resolved in this file
//     because jsdom's getComputedStyle neither substitutes `var()` nor expands shorthands.
//   · MEASURED, and inert: FIELDS_BLOCK, the run of boxes between the heading and the commit
//     row (two field rows, the park row, notes, the override disclosure, the summary line).
//     This pass does not touch any of them, so it is carried as one calibrated constant
//     rather than modelled control by control — derived by subtracting the computed terms
//     from the density pass's measured 422 px. It is ballast: it cannot detect anything.
//
// WHAT THIS DOES NOT PROVE. No pixel here is verified against a layout engine. If the field
// rows re-wrap (a wider or narrower log column, a new field), FIELDS_BLOCK is wrong and this
// test is measuring the wrong window — re-measure, do not nudge the constant. What it does
// prove is that the two chrome boxes above the commit row are gone and stay gone, and that
// the surface each of them served is still served: see the per-host tests below.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { LogEntry } from './LogEntry'
import { CockpitPaneFrame } from './panes/CockpitPaneFrame'
import type { AppSnapshot } from '../types'

vi.mock('../api', () => ({
  fdLogManual: vi.fn(async () => ({})),
  logQso: vi.fn(async () => ({})),
  getLog: vi.fn(async () => []),
  lookupPark: vi.fn(async () => null),
  lookupParkLive: vi.fn(async () => null),
  qrzLookup: vi.fn(async () => null),
  resolveEntity: vi.fn(async () => null),
  searchParks: vi.fn(async () => []),
  setCwPeerInfo: vi.fn(async () => {}),
}))

const snap = {
  radio: { band: '20m', dialMhz: 14.2 },
  hunt: null,
} as unknown as AppSnapshot

// ── the sheet, and the cascade over it ──────────────────────────────────────────────────

const FLAT: { rule: CSSStyleRule; order: number }[] = []
/** `:root` custom properties, resolved to px at --space-scale: 1 (the default viewport). */
const TOKENS = new Map<string, string>()

function srcFile(rel: string): string {
  return readFileSync(resolve(process.cwd(), 'src', rel), 'utf8')
}

beforeAll(() => {
  for (const sheet of ['styles.css', 'cockpit-panes.css']) {
    const style = document.createElement('style')
    style.textContent = srcFile(sheet)
    document.head.appendChild(style)
    const walk = (rules: CSSRuleList) => {
      for (let i = 0; i < rules.length; i++) {
        const r = rules[i]
        if ((r as CSSStyleRule).selectorText) FLAT.push({ rule: r as CSSStyleRule, order: FLAT.length })
        else if ((r as CSSGroupingRule).cssRules) walk((r as CSSGroupingRule).cssRules)
      }
    }
    walk(style.sheet!.cssRules)
  }
  // The token table is the FIRST `:root` block only — the later ones are theme overrides
  // and the [data-viewport] blocks, none of which apply at the default window.
  for (const { rule } of FLAT) {
    if (rule.selectorText !== ':root') continue
    for (let i = 0; i < rule.style.length; i++) {
      const name = rule.style[i]
      if (name.startsWith('--') && !TOKENS.has(name)) TOKENS.set(name, rule.style.getPropertyValue(name).trim())
    }
  }
})

afterEach(cleanup)

/** Specificity as one comparable number: (ids, classes+attrs+pseudo-classes, elements). */
function spec(sel: string): number {
  const s = sel.replace(/:where\([^)]*\)/g, '')
  const ids = (s.match(/#[\w-]+/g) ?? []).length
  const cls = (s.match(/\.[\w-]+|\[[^\]]+\]|:[a-z-]+(?:\([^)]*\))?/g) ?? []).filter(
    (t) => !t.startsWith('::'),
  ).length
  const el = (s.match(/(?:^|[\s>+~(])([a-z][\w-]*)/g) ?? []).length
  return ids * 10000 + cls * 100 + el
}

/** Substitute `var()` and evaluate the sheet's `calc(<n>px * <n>)` form. */
function resolveVal(raw: string, depth = 0): string {
  if (depth > 8) return raw
  let v = raw.trim()
  v = v.replace(/var\(\s*(--[\w-]+)\s*(?:,\s*([^)]*))?\)/g, (_m, name, fallback) =>
    resolveVal(TOKENS.get(name) ?? fallback ?? '0', depth + 1),
  )
  v = v.replace(/calc\(\s*([\d.]+)px\s*\*\s*([\d.]+)\s*\)/g, (_m, a, b) => `${Number(a) * Number(b)}px`)
  return v.trim()
}

/** Split a resolved box shorthand into its four sides (CSS 1/2/3/4-value form). */
const SIDES = ['top', 'right', 'bottom', 'left'] as const
type Side = (typeof SIDES)[number]
function sideOf(shorthand: string, side: Side): string {
  const p = shorthand.split(/\s+/).filter(Boolean)
  if (p.length === 0) return '0'
  const four = p.length === 1 ? [p[0], p[0], p[0], p[0]]
    : p.length === 2 ? [p[0], p[1], p[0], p[1]]
      : p.length === 3 ? [p[0], p[1], p[2], p[1]]
        : p.slice(0, 4)
  return four[SIDES.indexOf(side)]
}

/** The value one RULE contributes for `prop`, falling back to the shorthand it lives in.
 *  jsdom expands a box shorthand into its longhands ONLY when the whole declaration parses,
 *  and `padding: var(--space-3) var(--space-4)` does not — so the sheet's own idiom would
 *  otherwise read as "not declared" and this whole model would silently measure zero. */
function fromRule(style: CSSStyleDeclaration, prop: string): string | null {
  const direct = style.getPropertyValue(prop)
  if (direct) return direct
  const box = /^(padding|margin)-(top|right|bottom|left)$/.exec(prop)
  if (box) {
    const sh = style.getPropertyValue(box[1])
    return sh ? sideOf(resolveVal(sh), box[2] as Side) : null
  }
  const bw = /^border-(top|bottom)-width$/.exec(prop)
  if (bw) {
    for (const sh of [style.getPropertyValue(`border-${bw[1]}`), style.getPropertyValue('border')]) {
      if (!sh) continue
      const v = resolveVal(sh)
      if (/\bnone\b/.test(v)) return '0px'
      const m = /(^|\s)(-?[\d.]+)(px)?(\s|$)/.exec(v)
      return m ? `${m[2]}px` : '0px'
    }
  }
  return null
}

/** The winning declaration of `prop` for a RENDERED element: every rule the real selector
 *  engine matches, ordered by importance, then specificity, then source order. */
function css(el: Element, prop: string): string | null {
  let win: { value: string; important: boolean; spec: number; order: number } | null = null
  for (const { rule, order } of FLAT) {
    let matches = false
    try {
      matches = el.matches(rule.selectorText)
    } catch {
      continue
    }
    if (!matches) continue
    const value = fromRule(rule.style, prop)
    if (!value) continue
    const important = rule.style.getPropertyPriority(prop) === 'important'
    const s = spec(rule.selectorText)
    const better =
      win === null ||
      (important && !win.important) ||
      (important === win.important && (s > win.spec || (s === win.spec && order >= win.order)))
    if (better) win = { value, important, spec: s, order }
  }
  return win ? resolveVal(win.value) : null
}

/** `css` as a px number; 0 when the property is not declared anywhere that matches. */
function pxOf(el: Element, prop: string): number {
  const v = css(el, prop)
  if (!v) return 0
  const m = /^(-?[\d.]+)(px)?$/.exec(v.trim())
  return m ? Number(m[1]) : 0
}

/** Top + bottom border of an element, resolved through the same cascade. */
function borderY(el: Element): number {
  return pxOf(el, 'border-top-width') + pxOf(el, 'border-bottom-width')
}
function borderTop(el: Element): number {
  return pxOf(el, 'border-top-width')
}

// ── the stack ───────────────────────────────────────────────────────────────────────────

/** THE SHIPPED DEFAULT WINDOW is 1200x720 (src-tauri/tauri.conf.json). The log pane body
 *  measured 392 px there (Phone density pass, 2026-08-04). */
const PANE_BODY_H = 392

/** MEASURED + CALIBRATED, and inert (see the header): the run of boxes between the heading
 *  and the commit row — the exchange row, the QTH row, the park row, the notes box, the
 *  override disclosure and the summary line — at that same window. Derived from the pass's
 *  measured 422 px total by subtracting the computed terms below. Nothing in this pass
 *  touches any of them. */
const FIELDS_BLOCK = 329

/** MEASURED: the line box of a bare <button> at the UA's default control font — the height
 *  of `.le-actions`, which has no `--touch` floor (its children are buttons, not inputs). */
const BUTTON_LINE = 16

/**
 * Distance from the TOP of the pane body's border box to the BOTTOM of the Log button,
 * in the stacked-block order the strip actually renders.
 */
function commitRowBottom(paneBody: Element, strip: Element, actions: Element): number {
  const h2 = strip.querySelector('h2')
  const h2Outer = h2
    ? (() => {
        const fs = pxOf(h2, 'font-size') || 16
        const lhRaw = css(h2, 'line-height')
        const lh = lhRaw ? (/px$/.test(lhRaw) ? Number(lhRaw.replace('px', '')) : Number(lhRaw) * fs) : fs * 1.2
        return Math.round(lh) + pxOf(h2, 'margin-bottom') + pxOf(h2, 'margin-top')
      })()
    : 0
  const logBtn = actions.querySelector('.le-log-btn')!
  const actionsRow =
    pxOf(logBtn, 'padding-top') + pxOf(logBtn, 'padding-bottom') + borderY(logBtn) + BUTTON_LINE
  return (
    pxOf(paneBody, 'padding-top') +
    borderTop(strip) + // only the TOP border stands above the button
    pxOf(strip, 'padding-top') +
    h2Outer +
    FIELDS_BLOCK +
    pxOf(actions, 'margin-top') +
    actionsRow
  )
}

/** The framed shape, exactly as PhoneCockpit and CwCockpit build it. That the two HOSTS
 *  still pass `titled={false}` is pinned where the hosts are rendered — their own
 *  *.structure.test.tsx suites — because the prop defaults to TRUE and a host that stops
 *  passing it would silently get the duplicate heading back. */
function renderFramed() {
  const { container } = render(
    <CockpitPaneFrame title="Log" paneId="log">
      <LogEntry snap={snap} mode="SSB" defaultRst="59" exchange="terrestrial" titled={false} />
    </CockpitPaneFrame>,
  )
  return {
    paneBody: container.querySelector('.pane-body')!,
    strip: container.querySelector('.log-entry')!,
    actions: container.querySelector('.le-actions')!,
    container,
  }
}

describe('the framed log strip commits above the fold', () => {
  it('the Log button is inside the pane body at the shipped default window', () => {
    const { paneBody, strip, actions } = renderFramed()
    const bottom = commitRowBottom(paneBody, strip, actions)
    expect(
      bottom,
      `the Log button's bottom edge is ${bottom}px from the top of a ${PANE_BODY_H}px pane body ` +
        `— ${bottom - PANE_BODY_H}px past the fold. The chrome above it: ` +
        `card border+padding ${borderTop(strip) + pxOf(strip, 'padding-top')}px, ` +
        `heading ${strip.querySelector('h2') ? 'present' : 'absent'}.`,
    ).toBeLessThanOrEqual(PANE_BODY_H)
  })

  it('the card-in-card flatten WINS for a .log-entry inside a .pane-body', () => {
    // Computed, not matched: `.cw-cockpit .log-entry` and `.sats-log .log-entry` are both
    // (0,2,0) and both re-declare padding, so a flatten that merely EXISTS can lose. Assert
    // the resolved winner on the real framed element instead.
    const { strip } = renderFramed()
    expect(pxOf(strip, 'padding-top'), '.log-entry keeps card padding inside a pane frame').toBe(0)
    expect(pxOf(strip, 'padding-bottom')).toBe(0)
    expect(borderY(strip), '.log-entry keeps a card border inside a pane frame').toBe(0)
  })
})

describe('the recovered chrome survives the small viewports', () => {
  // THE OPERATOR THIS IS FOR is on 1024x768 (the supported floor) or a pinned 175%, where
  // `[data-viewport]` tightens --space-scale — and where a density win that came only out of
  // token spacing would shrink with it. It does not: the heading's dominant term is an 18px
  // LINE BOX, and no font was made smaller by this pass. Computed at each shipped scale by
  // overriding the one token the viewport blocks change.
  //
  // The classes these scales belong to (useViewport.classifyViewport, effective width):
  //   1.00  md/lg/xl — 1280x800, 1366x768, 1200x750 @80% (eff 1500), 3440x1440
  //   0.94  sm       — 1024x768 @100%, 1600x900 @175% (eff 914)
  //   0.86  xs       — 1024x768 @175% (eff 585)
  const SCALES: [number, string][] = [
    [1, 'md/lg/xl'],
    [0.94, 'sm'],
    [0.86, 'xs'],
  ]

  function atScale<T>(scale: number, fn: () => T): T {
    const prev = TOKENS.get('--space-scale')
    TOKENS.set('--space-scale', String(scale))
    try {
      return fn()
    } finally {
      if (prev === undefined) TOKENS.delete('--space-scale')
      else TOKENS.set('--space-scale', prev)
    }
  }

  it.each(SCALES)('at --space-scale %s (%s) the framed strip pays none of it', (scale, cls) => {
    const framed = renderFramed()
    atScale(scale, () => {
      expect(framed.strip.querySelector('h2'), `${cls}: the heading came back`).toBeNull()
      expect(
        borderTop(framed.strip) + pxOf(framed.strip, 'padding-top'),
        `${cls}: the card-in-card came back above the commit row`,
      ).toBe(0)
    })
    cleanup()

    // …and the same component UNFRAMED still pays it, which is what the framed hosts stopped
    // paying. Measured on the base `.log-entry` rule — the one Phone paid before this pass.
    const { container } = render(
      <LogEntry snap={snap} mode="SSB" defaultRst="59" exchange="terrestrial" />,
    )
    const bare = container.querySelector('.log-entry')!
    const h2 = bare.querySelector('h2')!
    atScale(scale, () => {
      const fs = pxOf(h2, 'font-size')
      const recovered =
        Math.round(fs * 1.2) + pxOf(h2, 'margin-bottom') + borderTop(bare) + pxOf(bare, 'padding-top')
      expect(fs, `${cls}: the heading's type changed — this pass shrank no font`).toBe(18)
      // Computed on this tree: 43px / 41.8px / 40.2px. The floor is 38 so that ONE more
      // --space-scale step is not a false alarm, while losing the ~30px heading box (which
      // would leave ~13) still fires. It is not a target to tune toward.
      expect(
        recovered,
        `${cls}: only ${recovered}px of chrome stands above the commit row — the win has ` +
          'evaporated into token spacing, which is the failure this walk exists to catch',
      ).toBeGreaterThanOrEqual(38)
    })
  })
})

describe('what the deleted heading said is still said', () => {
  it('the framed strip renders no heading — the frame head names the surface', () => {
    const { container, strip } = renderFramed()
    expect(strip.querySelector('h2')).toBeNull()
    // …and the name it carried is the frame's accessible name, not a lost string.
    expect(container.querySelector('.pane-frame')!.getAttribute('aria-label')).toBe('Log')
    expect(container.querySelector('.pane-title')!.textContent).toBe('Log')
  })

  it('an UNFRAMED host still gets a heading — the default is unchanged', () => {
    // RULING (2026-08-04): SatellitesView hosts this component in a plain `.sats-log` div
    // with no title above it, so deleting the heading outright would leave that surface
    // untitled. The prop defaults to today's behaviour and Satellites never passes it.
    const { container } = render(
      <div className="sats-log">
        <LogEntry snap={snap} mode="SSB" defaultRst="59" exchange="satellite" />
      </div>,
    )
    const h2 = container.querySelector('.log-entry h2')
    expect(h2, 'the unframed host lost its only title').not.toBeNull()
    expect(h2!.textContent).toBe('Log this QSO')
  })
})
