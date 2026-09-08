// @vitest-environment jsdom
//
// EVERY POP-OUT WINDOW MUST HAVE SOMEWHERE FOR ITS DEFICIT TO GO.
//
// THE DEFECT THIS FILE EXISTS FOR (first beta, operator report): "On the pota/sota section,
// when someone uses the popout, there is no ability to scroll down." The torn-off POTA/SOTA
// board had ZERO scroll owners anywhere in its subtree and clipped at its own root, so every
// spot below the fold — and the source line under them — was unreachable. Nothing was
// mis-sized; there was simply no valve.
//
// HOW IT HAPPENED, because it is a two-commit bug and neither commit is wrong alone.
// `.pota-view` used to carry its own `overflow-y: auto` to escape `.panel { overflow: hidden }`.
// ed50c50e (2026-08-04) deleted it as dead, correctly at the time: `.layout.single > .panel`
// had just been given the identical declaration at a higher specificity, and the comment it
// left behind says why that was safe — "POTA renders nowhere else". e6e0ec5b (2026-08-29)
// then made POTA render somewhere else. A pop-out's root is `.app.detached`, not
// `.layout.single`, so the board arrived in a host whose only overflow declaration for it was
// the `.panel` clip.
//
// THE TWO HOSTS, and the fix is to make them agree. Docked, the panel is the scroll owner —
// `.layout.single > .panel { height: 100%; overflow-y: auto }`, fate #1 of the cockpit-panes
// model ("a scrollbar inside the pane that overflowed"). Detached, the panel IS the window, so
// it takes the height and owns the same deficit: `.app.detached > .panel { flex: 1;
// min-height: 0; overflow-y: auto }` — the shape `.app.detached .needed-panel` and
// FdClubSection's own `detached` branch already use.
//
// HOW THIS GUARD COMPUTES. The REAL components render (a stub would prove prop-passing, not
// this seam) under the REAL styles.css, and the cascade is resolved here — importance, then
// specificity, then source order — because jsdom's getComputedStyle does not expand the
// `overflow` shorthand into `overflow-y`. A rule that exists and LOSES therefore fails here,
// which is the failure mode that shipped two dead fixes pre-overhaul. The resolver is the one
// layout-single-deficit.test.tsx uses, on purpose: this is the same defect class one host over.
//
// WHAT IT DOES NOT PROVE. jsdom does not lay out — getBoundingClientRect is 0 — so no claim
// here is that anything is the right SIZE, or that a scrollbar appears at any given window.
// What is verified is structural and is the whole of the operator's complaint: that the
// deficit a torn-off board makes has a box that can be scrolled to reach it, and that the
// board is inside that box.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, cleanup, act, screen } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { DetachedPanel } from './DetachedPanel'
import type { AppSnapshot } from './types'

// The snapshot every branch polls for. Read through a global rather than closed over,
// because vi.mock factories are hoisted above this module's own initialisation.
const SNAP = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  stations: [],
  conversations: [],
  activePeer: null,
  decodes: [],
  recentDecodes: [],
  hunt: null,
  link: { tier: 'ft8' },
  radio: {
    band: '20m',
    dialMhz: 14.074,
    txAllowed: true,
    phoneSegLo: null,
    phoneSegHi: null,
    sideband: 'USB',
    rigMode: 'USB',
    catOk: true,
    txBusyReason: null,
    transmitting: false,
    slot: 0,
    rxOffsetHz: 1500,
    txOffsetHz: 1500,
    amp: null,
  },
  // Field Day ON with a club, so the `fieldday` and `fdclub` arms render their BOARDS
  // rather than their "not running" / "sync off" copy — an inactive branch is one short
  // paragraph and could not overflow anything.
  fieldDay: {
    myClass: '2A',
    mySection: 'WI',
    running: true,
    state: 'Running',
    qsoCount: 3,
    sections: 2,
    points: 6,
    log: [],
    club: {
      syncState: 'synced',
      queued: 0,
      offlineSinceUnix: 0,
      hosting: true,
      event: 'ARRL FD',
      hostCall: 'W1AW',
      score: 10,
      qsos: 3,
      sections: 2,
      skewSecs: 0,
      lastError: null,
      dupes: [],
      board: [],
    },
  },
} as unknown as AppSnapshot
;(globalThis as unknown as { __detachedSnap: unknown }).__detachedSnap = SNAP

// Every engine call resolves to something harmless; the three feeds that decide whether a
// board has ROWS are populated, because an empty board cannot overflow and a census of empty
// boards would go vacuous.
vi.mock('./api', async () => {
  const actual = await vi.importActual<Record<string, unknown>>('./api')
  const out: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    out[k] = typeof actual[k] === 'function' ? vi.fn().mockResolvedValue(null) : actual[k]
  }
  out.subscribeSnapshot = vi.fn((cb: (s: unknown) => void) => {
    cb((globalThis as unknown as { __detachedSnap: unknown }).__detachedSnap)
    return () => {}
  })
  out.getBandPlan = vi.fn().mockResolvedValue([])
  out.getNeedAlerts = vi.fn().mockResolvedValue([])
  out.getAllSpots = vi.fn().mockResolvedValue([])
  out.getLog = vi.fn().mockResolvedValue([])
  out.parksCount = vi.fn().mockResolvedValue(0)
  out.huntedParksCount = vi.fn().mockResolvedValue(0)
  // Read through the global for the same reason as the snapshot: this factory runs while
  // `./DetachedPanel` is being imported, before this module's own consts initialise.
  out.getOtaSpots = vi.fn(() =>
    Promise.resolve((globalThis as unknown as { __otaSpots: unknown }).__otaSpots),
  )
  return out
})
vi.mock('./toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((f: () => Promise<unknown>) => f().catch(() => null)),
  subscribeToasts: vi.fn(() => () => {}),
  dismissToast: vi.fn(),
}))

/** Two POTA spots — enough that the board has a list, a Hunt button per row, and the
 *  source line beneath them. LAST_HUNT is the row this file walks out from. */
const OTA_SPOTS = [
  {
    program: 'POTA',
    reference: 'K-1234',
    activator: 'W1AW',
    freqKhz: 14074,
    mode: 'FT8',
    name: 'First Park',
    spotter: 'K0ABC',
    comment: '',
    newPark: false,
    bandOpen: true,
  },
  {
    program: 'POTA',
    reference: 'K-5678',
    activator: 'K9XYZ',
    freqKhz: 7035,
    mode: 'CW',
    name: 'Last Park',
    spotter: 'K0ABC',
    comment: '',
    newPark: true,
    bandOpen: false,
  },
]
;(globalThis as unknown as { __otaSpots: unknown }).__otaSpots = OTA_SPOTS

// ── the cascade, computed ───────────────────────────────────────────────────────────────
// Lifted from layout-single-deficit.test.tsx, which pins the docked half of this same rule.

function src(rel: string): string {
  return readFileSync(resolve(process.cwd(), 'src', rel), 'utf8')
}

const FLAT: { rule: CSSStyleRule; order: number }[] = []

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  const style = document.createElement('style')
  style.textContent = src('styles.css')
  document.head.appendChild(style)
  const walk = (rules: CSSRuleList) => {
    for (let i = 0; i < rules.length; i++) {
      const r = rules[i]
      if ((r as CSSStyleRule).selectorText) FLAT.push({ rule: r as CSSStyleRule, order: FLAT.length })
      else if ((r as CSSGroupingRule).cssRules) walk((r as CSSGroupingRule).cssRules)
    }
  }
  walk(style.sheet!.cssRules)
  // The responsive state a pop-out publishes for itself (DetachedPanelBody mirrors App).
  // `md` is the ordinary tear-off; the `[data-viewport]` rules are live either way.
  document.documentElement.setAttribute('data-viewport', 'md')
  document.documentElement.style.setProperty('--vh-eff', '768px')
  document.documentElement.style.setProperty('--vw-eff', '1024px')
})

afterEach(() => cleanup())

/** Selector specificity as one comparable number. `:where()` contributes nothing;
 *  `:is()/:not()/:has()` contribute their most specific argument, per selectors-4. */
function specificity(sel: string): number {
  let a = 0
  let b = 0
  let c = 0
  let rest = sel
  rest = rest.replace(/:(where|is|not|has|matches|any)\(([^()]*)\)/g, (_m, name: string, args: string) => {
    if (name !== 'where') {
      const inner = Math.max(0, ...args.split(',').map((s) => specificity(s.trim())))
      a += Math.floor(inner / 10000)
      b += Math.floor((inner % 10000) / 100)
      c += inner % 100
    }
    return ' '
  })
  rest = rest.replace(/::[a-z-]+/g, () => { c++; return ' ' })
  rest = rest.replace(/#[\w-]+/g, () => { a++; return ' ' })
  rest = rest.replace(/\.[\w-]+|\[[^\]]*\]|:[a-z-]+(\([^()]*\))?/g, () => { b++; return ' ' })
  rest.replace(/[a-zA-Z][\w-]*/g, () => { c++; return ' ' })
  return a * 10000 + b * 100 + c
}

interface Cand { prop: string; value: string; important: boolean; spec: number; order: number }

function better(a: Cand, b: Cand | null): boolean {
  if (!b) return true
  if (a.important !== b.important) return a.important
  if (a.spec !== b.spec) return a.spec > b.spec
  return a.order >= b.order
}

/** The overflow-y that actually applies to `el`: the winner across the `overflow` shorthand
 *  and the `overflow-y` longhand over every matching rule of the real sheet, plus the inline
 *  style. Same two documented simplifications as layout-single-deficit.test.tsx, both of
 *  which can only make this stricter. */
function overflowY(el: Element): string {
  let win: Cand | null = null
  const consider = (decl: CSSStyleDeclaration, spec: number, order: number) => {
    for (const prop of ['overflow', 'overflow-y'] as const) {
      const value = decl.getPropertyValue(prop).trim()
      if (!value) continue
      const cand: Cand = {
        prop,
        value,
        important: decl.getPropertyPriority(prop) === 'important',
        spec,
        order,
      }
      if (better(cand, win)) win = cand
    }
  }
  for (const { rule, order } of FLAT) {
    let hit = false
    try {
      hit = el.matches(rule.selectorText)
    } catch {
      continue // a selector this jsdom cannot parse cannot be applying either
    }
    if (hit) consider(rule.style, specificity(rule.selectorText), order)
  }
  consider((el as HTMLElement).style, Number.MAX_SAFE_INTEGER, Number.MAX_SAFE_INTEGER)
  if (!win) return 'visible'
  const w = win as Cand
  if (w.prop === 'overflow-y') return w.value
  const parts = w.value.split(/\s+/)
  return parts[1] ?? parts[0] // `overflow: <x> <y>` — one value sets both
}

/** A box that lets its descendants' block-end overflow escape outward. */
const escapes = (v: string) => v === 'visible'
/** A box that can be SCROLLED to reach the overflow it holds. */
const scrolls = (v: string) => v === 'auto' || v === 'scroll'

const name = (n: Element) => `${n.tagName.toLowerCase()}${[...n.classList].map((c) => `.${c}`).join('')}`

/** The fate of block-end deficit produced at `el`: walk outward to `stopAt` (inclusive) and
 *  report the FIRST box that does not let the overflow escape. */
function deficitFate(el: Element, stopAt: Element): { fate: 'scroll' | 'clip' | 'none'; at: string } {
  let node: Element | null = el
  while (node) {
    const oy = overflowY(node)
    if (!escapes(oy)) return { fate: scrolls(oy) ? 'scroll' : 'clip', at: `${name(node)} {overflow-y:${oy}}` }
    if (node === stopAt) break
    node = node.parentElement
  }
  return { fate: 'none', at: name(stopAt) }
}

// ── the host ────────────────────────────────────────────────────────────────────────────

/** Render `?panel=<panel>` the way main.tsx does, and hand back the pieces this file
 *  reasons about: the pop-out root, and the panel content hanging under it.
 *
 *  `<Toasts/>` is excluded from "content". It is a fixed overlay the shell mounts beside
 *  every branch (never the panel), and its viewport is legitimately its own scroller — it
 *  would otherwise answer "is there a scroll owner?" yes for every panel, which is exactly
 *  how this guard would go vacuous. */
async function mountPopout(panel: string): Promise<{ app: Element; roots: Element[] }> {
  let container!: HTMLElement
  await act(async () => {
    ;({ container } = render(<DetachedPanel panel={panel} />))
  })
  const app = container.querySelector('.app.detached')!
  const roots = Array.from(app.children).filter(
    (c) => !c.classList.contains('ui-toast-viewport') && !c.querySelector('.ui-toast-viewport'),
  )
  return { app, roots }
}

/** Every scroll owner inside a pop-out's content (the toast overlay already excluded). */
function scrollOwners(roots: Element[]): string[] {
  const out: string[] = []
  for (const root of roots) {
    for (const el of [root, ...Array.from(root.querySelectorAll('*'))]) {
      const oy = overflowY(el)
      if (scrolls(oy)) out.push(`${name(el)} {overflow-y:${oy}}`)
    }
  }
  return out
}

// ── 1. the reported bug, at the surface that had it ─────────────────────────────────────

describe('the torn-off POTA/SOTA board can be scrolled', () => {
  it('reaches the Hunt button on the last spot instead of clipping it away', async () => {
    const { app } = await mountPopout('pota')
    // The bottom-most operator ACTION on the board — the thing a long spot list pushes off
    // the window, and what the operator was reaching for when they filed this.
    const hunt = screen.getByRole('button', { name: /K9XYZ/ })
    const { fate, at } = deficitFate(hunt, app)
    expect(
      fate,
      'The torn-off POTA/SOTA board makes its deficit inside a box that ' +
        `${fate === 'clip' ? `CLIPS it (${at})` : 'nothing owns'} — every spot below the ` +
        'fold, and the Hunt button on it, is unreachable. The docked board gets its valve ' +
        'from `.layout.single > .panel`; a pop-out has no `.layout.single`, so `.app.detached` ' +
        'must give the panel the same one.',
    ).toBe('scroll')
  })

  it('has exactly ONE scroll owner, and it is the board itself', async () => {
    const { roots } = await mountPopout('pota')
    // One scroll owner per surface (the layout contract). The board is a single flowing
    // column — header, banner, activation, park list, filters, spots, source line — so the
    // owner is the column, not a scroller around one strip of it. A second one here would
    // mean a nested scrollport, which is the other half of this bug class.
    expect(scrollOwners(roots)).toEqual(['section.panel.pota-view.pota-hunter {overflow-y:auto}'])
  })

  it('leaves the DOCKED board exactly as it was', async () => {
    // The fix is scoped to `.app.detached >`, so the docked chain must still resolve through
    // `.layout.single > .panel` — the rule layout-single-deficit.test.tsx pins. Mounted as a
    // bare class chain (that file already renders the real view); what is asked here is only
    // that the new rule did not reach a host it was not written for.
    const main = document.createElement('main')
    main.className = 'layout single'
    const panel = document.createElement('section')
    panel.className = 'panel pota-view pota-hunter'
    main.appendChild(panel)
    document.body.appendChild(main)
    expect(overflowY(panel)).toBe('auto')
    expect(getComputedStyle(panel).height).toBe('100%')
    main.remove()
  })
})

// ── 2. the same question of every other pop-out ─────────────────────────────────────────

describe('every pop-out the shell can host', () => {
  // The complete list — `openPanelWindow` accepts these and nothing else, and DetachedPanel
  // has an arm for each. Kept whole rather than sampled: this bug reached a shipped beta
  // because one arm was reasoned about in isolation.
  const PANELS = [
    'needed', 'memories', 'connect', 'dxped', 'sats', 'pota', 'fieldday', 'fdclub',
    'operate', 'operatemap', 'bandmapPhone', 'bandmapCw', 'waterfall',
  ]

  it('never clips its content with no scroller interposed', async () => {
    // THE CONTRACT SENTENCE, applied to the detached host: an `overflow: hidden` ancestor may
    // never contain hard-floored descendants without an interposed scroller. A pop-out's
    // content root is the box that holds the whole surface, so if IT clips, whatever the
    // column below cannot shrink is gone — unless something inside it scrolls.
    //
    // A root that does NOT clip is exempt and correctly so: `.app.detached` is a
    // definite-height flex column with `overflow: visible`, so a root that lets overflow past
    // it hands the deficit to the document, which scrolls. That is how the waterfall, the two
    // band maps and the bare map surfaces get away with no scroller of their own — they are
    // fixed frequency/canvas surfaces with nothing to scroll — and the check must not demand
    // one of them.
    //
    // Collected, not asserted panel by panel, so a failure names EVERY offender and also
    // everything that was already safe; a guard that stops at the first cannot show it is
    // distinguishing anything.
    const trapped: string[] = []
    for (const panel of PANELS) {
      const { roots } = await mountPopout(panel)
      for (const root of roots) {
        if (escapes(overflowY(root))) continue
        if (scrolls(overflowY(root))) continue
        const inner = scrollOwners([root])
        if (inner.length === 0) {
          trapped.push(`${panel}: ${name(root)} {overflow-y:${overflowY(root)}} holds no scroller`)
        }
      }
      cleanup()
    }
    expect(
      trapped,
      'These pop-outs clip their own content with nothing between the clip and the content. ' +
        'Whatever the surface cannot shrink is off the bottom of the window with no way to ' +
        `reach it:\n  ${trapped.join('\n  ')}\n`,
    ).toEqual([])
  }, 60_000)
})
