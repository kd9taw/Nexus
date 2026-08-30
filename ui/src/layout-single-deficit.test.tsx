// @vitest-environment jsdom
//
// `.layout.single`: VERTICAL DEFICIT MUST HAVE A LEGAL FATE IN EVERY .panel-ROOTED VIEW.
//
// THE DEFECT THIS FILE EXISTS FOR (2026-08-04). Three rules in styles.css combined into a
// dead scrollbar by CONSTRUCTION, not by content:
//
//   .layout.single          { display: block; overflow-y: auto }   ← the valve
//   .layout.single > .panel { height: 100% }                       ← what welded it shut
//   .panel                  { overflow: hidden }                   ← what ate the deficit
//
// `height: 100%` resolves against the layout's CONTENT box, so the layout's scrollHeight can
// never exceed its clientHeight and its `overflow-y: auto` can never paint a thumb. The panel
// is then a hard clip with nothing between it and the content, so whatever the flex column
// could not shrink went off the bottom and stayed there: Logbook's "Log" submit at 1024×768
// with the manual form open, Program's whole delivery row (Export for CHIRP…/Export CSV/Save
// to Memory Bank/Clear). Five views had already escaped that rule one at a time — the two
// `:has()` rules, `.aprs-cockpit`, the triple-class cockpit shells, and POTA's own
// `overflow-y: auto` — which is the tell that the base rule, not the views, was wrong.
//
// WHY THE VALVE IS ON THE PANEL AND NOT ON THE LAYOUT. Letting the panel grow past 100%
// (`min-height: 100%`) would make the LAYOUT scroll — and would destroy every view that
// works today, because the panel's DEFINITE height is exactly what bounds the inner
// scrollers. With an auto-height panel a `flex: 1; min-height: 0` child contributes its
// max-content height to the container's intrinsic main size (CSS Flexbox §9.9.1), so
// `.settings-scroll`, `.log-scroll` (the virtualizer's scroll element) and `.rp-results`
// would each inflate to their full content and stop scrolling. The panel keeps its definite
// height and owns its own deficit instead — fate #1 in the cockpit-panes model, "a scrollbar
// inside the pane that overflowed". Both halves are pinned below.
//
// HOW THIS GUARD COMPUTES. jsdom parses the REAL styles.css; matching is done by the REAL
// selector engine (so `:has()`, `>` and `[data-viewport]` behave), and this file resolves the
// cascade itself — importance, then specificity, then source order — because jsdom's
// getComputedStyle does NOT expand the `overflow` shorthand into `overflow-y`. A rule that
// exists and LOSES therefore fails here, which is the failure mode that shipped two dead
// fixes. The two named traps are RENDERED (real components, real markup) and the guard walks
// from the trapped control outward to `.layout.single`, asking the only question that
// matters: going out, is the first box that does not let overflow escape a SCROLLER or a CLIP?
//
// WHAT IT DOES NOT PROVE. jsdom does not lay out, so no pixel claim here is verified. The
// 1024×768 @115%/@120% cases are pinned as the RESPONSIVE STATE they produce
// (data-viewport='sm' plus --vh-eff 667.8px / 640px), which is what a `[data-viewport]` rule
// could re-clip under. THAT the deficit exists at those sizes is an operator observation;
// that it has somewhere to go is what this file computes.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, cleanup, waitFor } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { Logbook } from './components/Logbook'
import { RadioProgView } from './components/RadioProgView'
import { FieldDayView } from './components/FieldDayView'
import type { FieldDayStatus } from './types'
import { classifyViewport } from './useViewport'

vi.mock('./api', () => {
  const fn = () => vi.fn().mockResolvedValue(undefined)
  return {
    getLog: vi.fn().mockResolvedValue([]),
    getSettings: vi.fn().mockResolvedValue(null),
    setSettings: fn(), exportLog: fn(), openPanelWindow: fn(),
    deleteQso: fn(), editQso: fn(), exportGeneralLog: fn(), importAdif: fn(),
    logOperators: () => Promise.resolve([]), exportLogForOperator: fn(),
    logQso: fn(), markQslSent: fn(), purgeLog: fn(), qrzLookup: fn(),
    syncLotwReport: fn(), uploadLotwReport: fn(), qrzPushQso: fn(),
    clublogPushQso: fn(), hrdlogPushQso: fn(),
    downloadLotwReport: fn(), syncQrz: fn(), importPotaAdif: fn(),
    saveTextToDownloads: fn(),
    exportChannels: fn(), geocodeCity: fn(), repeaterSearch: fn(), repeaterTune: fn(),
    radioprogListProjects: vi.fn().mockResolvedValue([]),
    radioprogSaveProject: fn(), radioprogDeleteProject: fn(),
  }
})
vi.mock('./toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn() }))

/** Read a source file of this UI. The jsdom environment leaves `import.meta.url` a non-file
 *  URL, so the node-env sheets' `new URL(…, import.meta.url)` idiom is unavailable here;
 *  vitest's cwd is the `ui` project root. */
function src(rel: string): string {
  return readFileSync(resolve(process.cwd(), 'src', rel), 'utf8')
}

/** Every style rule of the real sheet, flattened out of its at-rule blocks, in source order.
 *  (The sheet's only at-rules are `prefers-reduced-motion` — size-based `@media` is banned
 *  by the layout contract — so descending unconditionally cannot admit a rule that would not
 *  apply.) */
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
})

afterEach(() => cleanup())

// ── the cascade, computed ───────────────────────────────────────────────────────────────

/** Selector specificity as one comparable number. `:where()` contributes nothing;
 *  `:is()/:not()/:has()` contribute their most specific argument, per selectors-4. */
function specificity(sel: string): number {
  let a = 0
  let b = 0
  let c = 0
  let rest = sel
  // Functional pseudo-classes first — their arguments are scored, not counted.
  rest = rest.replace(/:(where|is|not|has|matches|any)\(([^()]*)\)/g, (_m, name: string, args: string) => {
    if (name !== 'where') {
      const inner = Math.max(0, ...args.split(',').map((s) => specificity(s.trim())))
      a += Math.floor(inner / 10000)
      b += Math.floor((inner % 10000) / 100)
      c += inner % 100
    }
    return ' '
  })
  rest = rest.replace(/::[a-z-]+/g, () => { c++; return ' ' }) // pseudo-elements
  rest = rest.replace(/#[\w-]+/g, () => { a++; return ' ' })
  rest = rest.replace(/\.[\w-]+|\[[^\]]*\]|:[a-z-]+(\([^()]*\))?/g, () => { b++; return ' ' })
  rest.replace(/[a-zA-Z][\w-]*/g, () => { c++; return ' ' }) // type selectors
  return a * 10000 + b * 100 + c
}

interface Cand { prop: string; value: string; important: boolean; spec: number; order: number }

function better(a: Cand, b: Cand | null): boolean {
  if (!b) return true
  if (a.important !== b.important) return a.important
  if (a.spec !== b.spec) return a.spec > b.spec
  return a.order >= b.order
}

/**
 * The overflow-y that actually applies to `el`: the winner across the `overflow` shorthand
 * and the `overflow-y` longhand, over every rule of the real sheet that matches, plus the
 * inline style. jsdom's getComputedStyle cannot answer this — it reports the two properties
 * from different rules and never expands the shorthand.
 *
 * Two known simplifications, both of which can only make this STRICTER, never laxer:
 * no rule in the sheet declares the shorthand and a longhand in the same block (checked, 0),
 * so declaration order WITHIN a block is not modelled; and the used-value coupling — a box
 * whose other axis is not `visible` computes this one to `auto` — is not applied, so a
 * one-axis declaration reads here as an escape rather than as a scroller.
 */
function overflowY(el: Element): string {
  let win: Cand | null = null
  const consider = (
    decl: CSSStyleDeclaration,
    spec: number,
    order: number,
  ) => {
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
  // Inline style: after every author rule, and not specificity-ordered against them.
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

/**
 * The fate of block-end deficit produced at `el`: walk outward to `stopAt` (inclusive) and
 * report the FIRST box that does not let the overflow escape. `scroll` = reachable,
 * `clip` = trapped, `none` = it reached the view root with nothing owning it.
 */
function deficitFate(el: Element, stopAt: Element): { fate: 'scroll' | 'clip' | 'none'; at: string } {
  const name = (n: Element) => `${n.tagName.toLowerCase()}.${[...n.classList].join('.')}`
  let node: Element | null = el
  while (node) {
    const oy = overflowY(node)
    if (!escapes(oy)) return { fate: scrolls(oy) ? 'scroll' : 'clip', at: `${name(node)} {overflow-y:${oy}}` }
    if (node === stopAt) break
    node = node.parentElement
  }
  return { fate: 'none', at: name(stopAt) }
}

// ── the two operator-reported window states ─────────────────────────────────────────────

const ZOOMS = [1.15, 1.2] as const
const WINDOW_W = 1024
const WINDOW_H = 768

function applyViewport(zoom: number): void {
  const d = document.documentElement
  d.setAttribute('data-viewport', classifyViewport(WINDOW_W / zoom))
  d.style.setProperty('--ui-zoom', String(zoom))
  d.style.setProperty('--vw-eff', `${WINDOW_W / zoom}px`)
  d.style.setProperty('--vh-eff', `${WINDOW_H / zoom}px`)
}

/** `<main class="layout single">` with the view inside it, exactly as App.tsx mounts it. */
function mountSingle(view: React.ReactElement) {
  return render(<main className="layout single">{view}</main>)
}

describe('.layout.single cannot own its deficit — so the .panel it wraps must', () => {
  it('Logbook: the "Log" submit stays reachable with the manual form open', async () => {
    for (const zoom of ZOOMS) {
      applyViewport(zoom)
      const { getByRole, container } = mountSingle(
        <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="SSB" />,
      )
      // Open the manual entry form — the un-shrinkable block that makes the deficit.
      getByRole('button', { name: 'Log QSO' }).click()
      const submit = await waitFor(() => getByRole('button', { name: 'Log' }))
      const root = container.querySelector('main.layout.single')!
      const { fate, at } = deficitFate(submit, root)
      expect(
        fate,
        `1024×768 @ ${Math.round(zoom * 100)}%: the deficit the open log form makes is ` +
          `${fate === 'clip' ? `CLIPPED at ${at}` : 'owned by nothing'} — the "Log" button ` +
          'goes off the bottom with no scroller between it and the clip.',
      ).toBe('scroll')
      cleanup()
    }
  })

  it('Program: the delivery row (Export for CHIRP…/CSV/Memory Bank/Clear) stays reachable', async () => {
    for (const zoom of ZOOMS) {
      applyViewport(zoom)
      const { getByRole, container } = mountSingle(<RadioProgView myGrid="EN52" catOk={false} />)
      const chirp = await waitFor(() => getByRole('button', { name: /Export for CHIRP/ }))
      const root = container.querySelector('main.layout.single')!
      const { fate, at } = deficitFate(chirp, root)
      expect(
        fate,
        `1024×768 @ ${Math.round(zoom * 100)}%: the delivery row sits below two 120px-floored ` +
          `scrollers and its deficit is ${fate === 'clip' ? `CLIPPED at ${at}` : 'owned by nothing'} ` +
          '— Export for CHIRP…, Export CSV, Save to Memory Bank and Clear all go off the bottom.',
      ).toBe('scroll')
      cleanup()
    }
  })
})

describe('the .panel-rooted views of .layout.single, as a class', () => {
  // The views App.tsx mounts as `.layout.single > .panel`. Two were the reported traps;
  // Settings and POTA were already safe and are here so the check cannot go vacuous — they
  // were green before the fix and must stay green after it.
  //
  // The Field Day DASHBOARD joined them on 2026-08-30, when it was moved out of the
  // three-pane operating workspace (see the describe below for why). It arrives already
  // safe — this rule is where its valve now comes from — but it is the view whose column
  // has a proven deficit, so it is the one that most needs to stay in the census.
  const PANEL_VIEWS: { file: string; classes: string; what: string }[] = [
    { file: 'components/Logbook.tsx', classes: 'panel log-view logbook', what: 'Logbook' },
    { file: 'components/RadioProgView.tsx', classes: 'radioprog panel', what: 'Program' },
    { file: 'components/SettingsPanel.tsx', classes: 'panel settings-panel', what: 'Settings' },
    { file: 'components/PotaSotaView.tsx', classes: 'panel pota-view pota-hunter', what: 'POTA/SOTA' },
    { file: 'components/FieldDayView.tsx', classes: 'conversation panel fieldday', what: 'Field Day dashboard' },
  ]

  /** The panel as App.tsx mounts it, attached so ancestor selectors really match. */
  function mountPanel(classes: string): HTMLElement {
    const main = document.createElement('main')
    main.className = 'layout single'
    const panel = document.createElement('section')
    panel.className = classes
    main.appendChild(panel)
    document.body.appendChild(main)
    return panel
  }

  it('each still roots on the class list this file reasons about', () => {
    for (const v of PANEL_VIEWS) {
      expect(src(v.file), `${v.what} no longer roots on \`${v.classes}\``).toContain(
        `className="${v.classes}"`,
      )
    }
  })

  it('each resolves overflow-y to a SCROLL, not the .panel clip, at every viewport class', () => {
    // Collected rather than asserted view by view, so the failure names EVERY offender and
    // — just as usefully — everything that was already safe. A guard that stops at the first
    // view cannot show it is distinguishing anything.
    const trapped: string[] = []
    for (const vp of ['xs', 'sm', 'md', 'lg', 'xl'] as const) {
      document.documentElement.setAttribute('data-viewport', vp)
      for (const v of PANEL_VIEWS) {
        const panel = mountPanel(v.classes)
        const oy = overflowY(panel)
        if (!scrolls(oy)) trapped.push(`${v.what} [data-viewport='${vp}'] → overflow-y: ${oy}`)
        panel.parentElement!.remove()
      }
    }
    expect(
      trapped,
      'These `.layout.single > .panel` views resolve overflow-y to a CLIP. Their parent is ' +
        'welded shut by `> .panel { height: 100% }` (scrollHeight can never exceed ' +
        'clientHeight), so whatever the panel\'s flex column cannot shrink is content the ' +
        `operator can never reach:\n  ${trapped.join('\n  ')}\n`,
    ).toEqual([])
  })

  it('keeps the DEFINITE height that bounds the inner scrollers', () => {
    // The other half of the fix, and the reason it is not `min-height`. `.settings-scroll`,
    // `.log-scroll` and `.rp-results` are `flex: 1` boxes: they bound only while the panel's
    // height is definite. Relax this to `min-height` and each inflates to its whole content
    // (Flexbox §9.9.1) — Settings would stop scrolling and the Logbook virtualizer, which
    // measures `.log-scroll`, would mount every row.
    for (const v of PANEL_VIEWS) {
      const panel = mountPanel(v.classes)
      expect(
        getComputedStyle(panel).height,
        `${v.what}: the panel's height is no longer definite — the inner scrollers lose their bound`,
      ).toBe('100%')
      panel.parentElement!.remove()
    }
  })
})

// ── the same defect class, one level down inside the Field Day column ────────────────────
//
// Field Day has TWO faces, and this file is about the DASHBOARD one. It is now a plain
// `.layout.single > .panel` view — `<main class="layout single">` → `<section class=
// "conversation panel fieldday">` — and is in the PANEL_VIEWS census above accordingly.
// (It used to mount in the three-pane operating workspace, `.layout[data-three-pane]` →
// `.grid-center`; the operator asked on 2026-08-30 why a scoreboard was wearing a waterfall,
// and App.tsx moved it. The shell swap is guarded in fdDashboardShell.test.tsx; what is
// guarded HERE is that the column's own deficit story survived it, which is why this block
// re-mounts the view in the new chain rather than trusting that it did.)
//
// (The other face, the operating COCKPIT, is `main.layout.single.fd-cockpit`. It is out of
// scope here for a structural reason rather than by omission: it is not `.layout.single >
// .panel`, so the census correctly excludes it, and its own valve is computed in
// `cockpit-shells.test.ts` SHELLS.)
//
// THE DEFECT (2026-08-04). The Field Day column is: banner, header, operator row, score
// tiles, sections board, the Bonuses disclosure, the log. Every one of them is
// `flex: 0 0 auto` EXCEPT the sections board (`SECTIONS_BOARD_WRAP { flex: 1 1 auto;
// minHeight: 0 }`), which made the board the column's single unfloored absorber: it paid
// for every sibling's growth all the way down to zero, and past zero `.panel`'s clip ate
// the rest. `.fd-bonuses-list`'s `max-height: calc(0.3 * var(--vh-eff))` was written as the
// fix and carried a comment claiming "the growth now stops here instead of travelling to
// the siblings" — but a cap is a fraction of the SAME viewport the slack comes from, so it
// bounds the list's own height and does nothing about that height displacing anyone. Open
// Bonuses at 1200×750 and the club-loved sections board still went to a blank strip, with
// the residual off the bottom of the log and no scrollbar anywhere in the chain.
//
// THE TWO HALVES, and neither works alone. A floor on the board with no valve above it
// converts the crush into a CLIP (strictly worse); a valve with no floor never fires,
// because an unfloored `flex: 1 1 auto` child shrinks to zero before the column can
// overflow. So: the board carries the floor, and `.fieldday` owns the deficit past it.
//
// WHAT THIS CANNOT PROVE: jsdom does not lay out, so "the board keeps 180px" is not
// verified here — what is verified is that the floor exists on the box the outer flex
// actually sizes, that nothing inside it competes for that floor, and that the deficit the
// floor creates has somewhere to go.
describe('Field Day: opening Bonuses must not pay for itself out of the sections board', () => {
  const FD: FieldDayStatus = {
    myClass: '2A',
    mySection: 'WI',
    running: false,
    state: 'Idle',
    qsoCount: 0,
    sections: 0,
    points: 0,
    log: [],
  }

  /** The view inside the real chain App.tsx mounts it in — the single-column panel shell
   *  (App.tsx, the `case 'fieldDay':` dashboard branch), NOT the three-pane workspace it
   *  left. Pinned to the source in fdDashboardShell.test.tsx. */
  function mountFieldDay() {
    return render(
      <main className="layout single">
        <FieldDayView fieldDay={FD} onSetMode={() => {}} />
      </main>,
    )
  }

  it('the log the open Bonuses list pushes down has a legal fate at every viewport class', async () => {
    // The subject is the LOG HEAD, deliberately: the bonus rows themselves sit in
    // `.fd-bonuses-list`, which scrolls, so a walk started there stops at the list's own
    // scroller and reports success no matter how broken the column is. The log head is the
    // displaced sibling — `flex: 0 0 auto`, no scroller of its own — so it asks the real
    // question the cap never answered.
    const trapped: string[] = []
    for (const vp of ['xs', 'sm', 'md', 'lg', 'xl'] as const) {
      document.documentElement.setAttribute('data-viewport', vp)
      const { getByRole, container } = mountFieldDay()
      getByRole('button', { name: /Bonuses/ }).click()
      await waitFor(() => getByRole('group', { name: 'Claimed FD bonuses' }))
      const head = container.querySelector('.fd-log-head')!
      const { fate, at } = deficitFate(head, container.querySelector('main.layout')!)
      if (fate !== 'scroll') trapped.push(`[data-viewport='${vp}'] → ${fate} at ${at}`)
      cleanup()
    }
    expect(
      trapped,
      'With Bonuses open the log is pushed below the fold and its deficit is owned by ' +
        `nothing that can be scrolled:\n  ${trapped.join('\n  ')}\n` +
        '`.fd-bonuses-list { max-height: calc(0.3 * var(--vh-eff)) }` cannot fix this — it ' +
        'caps the LIST, not the displacement. The column needs the valve.',
    ).toEqual([])
  })

  /** The board wrap, and the grid inside it (header + grid; the grid is the scrolling one). */
  function boardBoxes() {
    const { container } = mountFieldDay()
    const wrap = container.querySelector<HTMLElement>('[aria-label="Worked sections board"]')!
    return { wrap, grid: wrap.lastElementChild as HTMLElement }
  }
  const px = (v: string) => parseFloat(v) || 0

  it('the sections board carries the floor the column stops shrinking at', () => {
    const board = boardBoxes().wrap
    expect(
      px(board.style.minHeight),
      `SECTIONS_BOARD_WRAP declares min-height: ${board.style.minHeight || '(none)'}. It is the ` +
        'ONLY flex-grow child of the Field Day column, so with no floor it is where every ' +
        "sibling's growth is charged — the operator's blank-strip sections board. The floor " +
        'must be on THIS box: it is the one the outer flex sizes.',
    ).toBeGreaterThan(0)
  })

  it("the board's own scroller does not compete for that floor", () => {
    const grid = boardBoxes().grid
    expect(
      px(grid.style.minHeight),
      `SECTIONS_GRID declares min-height: ${grid.style.minHeight || '(none)'}. This box is the ` +
        'board\'s interposed SCROLLER; a hard floor on it is invisible to the outer column ' +
        'and cannot shrink, so once the wrap is squeezed the grid paints straight through ' +
        'the Bonuses section below it. The floor belongs on the wrap, which yields to it.',
    ).toBe(0)
  })
})
