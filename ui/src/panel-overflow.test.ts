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

/** One declaration plus the three things the cascade actually orders by. */
interface Decl {
  value: string
  important: boolean
  spec: number
  order: number
}

/** The winning declaration for an EXACT selector. `winner` above answers the
 *  VALUE only, which is enough to check one selector against itself but cannot
 *  compare two DIFFERENT selectors that both match the same element — the
 *  question `.le-grid` vs `.le-row .settings-input` asks. */
function decl(selector: string, prop: string): Decl | null {
  let win: Decl | null = null
  const re = new RegExp(`(?:^|;)\\s*${prop}\\s*:\\s*([^;]+)`, 'g')
  for (const r of RULES) {
    if (r.selector !== selector) continue
    const all = [...r.body.matchAll(re)]
    if (!all.length) continue
    const raw = all[all.length - 1][1].trim()
    const important = /!\s*important$/.test(raw)
    const d: Decl = {
      value: raw.replace(/!\s*important$/, '').trim(),
      important,
      spec: specificity(r.selector),
      order: r.order,
    }
    if (!win || (win.important === d.important && (d.spec > win.spec || (d.spec === win.spec && d.order >= win.order))) || (d.important && !win.important)) {
      win = d
    }
  }
  return win
}

/** Does `a` beat `b` on an element BOTH selectors match? Importance, then
 *  specificity, then source order — the CSS cascade, in that order. */
function beats(a: Decl, b: Decl): boolean {
  if (a.important !== b.important) return a.important
  if (a.spec !== b.spec) return a.spec > b.spec
  return a.order > b.order
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

describe('Band Activity: the filter chips wrap instead of clipping in the rail', () => {
  // The same nowrap-atom mechanism as `.log-actions` above, re-armed by the CQ+73 chip
  // (2026-07-31). `.od-controls` wraps, but the chip group inside it does not, so the
  // seven chips are ONE unbreakable atom whose min-content is their sum. In roster mode
  // the full bar renders in `.cockpit-decodes-side`, inside `.cockpit-side` AND
  // `.cockpit-panes` — both `overflow-x: hidden`, so past the floor the row is CLIPPED,
  // never scrolled. The right rail clamps to RIGHT_MIN = 260px (usePaneWidths), minus
  // panel padding, which the seven-chip sum exceeds: the trailing chips (B4 / New) stop
  // existing for the operator. Adding the WIDEST label to a nowrap row is what tipped it.
  it('.od-filters wraps', () => {
    const v = winner('.od-filters', 'flex-wrap')
    expect(
      v,
      '.od-filters has no flex-wrap: the seven chips (All/CQ/CQ+73/To me/On RX/B4/New) do not ' +
        'fit the 260px minimum rail, and .cockpit-side clips overflow-x — the last chips become ' +
        'unreachable instead of moving to a second line.',
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

describe('Satellites schedule: a fit-content strip + ONE scroll owner (the discovery-band split)', () => {
  // The 2026-08 schedule rethink splits .sats-sched into a control strip that never
  // scrolls (the h2 + the "Other birds overhead" disclosure chip — the chip is the
  // discoverability mechanism, so it must be permanently visible) and one inner
  // scroller owning ALL of column 1's overflow. Same scroll-owner count as before,
  // one level deeper. These are cascade-computed winners, not presence checks — a
  // later or more specific rule winning silently is this sheet's documented
  // historical failure mode.
  it('.sats-sched is the flex shell, not the scroller', () => {
    expect(winner('.sats-sched', 'display')).toBe('flex')
    expect(winner('.sats-sched', 'flex-direction')).toBe('column')
    expect(
      winner('.sats-sched', 'overflow-y') ?? winner('.sats-sched', 'overflow'),
      'the shell must clip, never scroll — the inner scroller owns overflow',
    ).toBe('hidden')
  })
  it('.sats-sched can never become a 0-px scroller', () => {
    // MEASURED FAILURE, 2026-08-04, at the 1024×768 floor: on a 12-transmitter
    // bird with the chooser's two disclosures open, `.sats-sched` shrank to 0.
    // Its inner scroller then had a 0-px box, and a scroller you cannot scroll
    // is not a scroller — the entire 48 h schedule was unreachable. `min-height:
    // 0` here was what allowed it, and it was the wrong half of the idiom: the
    // scroller INSIDE needs `min-height: 0` (asserted below), the shell needs a
    // floor that yields.
    const v = winner('.sats-sched', 'min-height')
    expect(v, '.sats-sched declares no min-height — under pressure it shrinks to 0').not.toBeNull()
    expect(
      v!.replace(/\s+/g, ''),
      `min-height is \`${v}\` — a bare floor under a bounded column is the contract's named ` +
        'bug, and no floor at all is a dead scrollbar. The idiom is min(Xem, 100%): a few rows ' +
        'when there is room, out of the way when the column itself is shorter than that.',
    ).toBe('min(9em,100%)')
  })
  it('.sats-sched-strip is fit=content; .sats-sched-scroll is THE scroll owner', () => {
    expect(winner('.sats-sched-strip', 'flex')).toBe('0 0 auto')
    expect(winner('.sats-sched-scroll', 'flex')).toBe('1 1 auto')
    expect(winner('.sats-sched-scroll', 'overflow-y')).toBe('auto')
    expect(winner('.sats-sched-scroll', 'min-height')).toBe('0')
  })
  it('the discovery band is bounded by ROW COUNT, never a pixel height', () => {
    // A max-height on any of these would be a second nested scroller inside the
    // schedule scroller — the exact .sats-favmgr ul violation this design refused.
    for (const sel of ['.sats-sched-scroll', '.sats-sched tbody.more', '.sats-sched table']) {
      expect(winner(sel, 'max-height'), `${sel} must not carry a max-height`).toBeNull()
    }
  })
  it('sm/xs: the page scroller owns the column, but the TABLE keeps its horizontal escape', () => {
    for (const vp of ['sm', 'xs']) {
      for (const sel of [
        `[data-viewport='${vp}'] .sats-sched`,
        `[data-viewport='${vp}'] .sats-side`,
        `[data-viewport='${vp}'] .sats-plan`,
        `[data-viewport='${vp}'] .sats-radio`,
      ]) {
        expect(
          winner(sel, 'overflow'),
          `${sel}: the PAGE scrolls at ${vp} (\`main.layout.single\`, not \`.sats-view\` — see ` +
            'the sticky-travel guard) — a bounded inner scroller here is the one-scroll-owner ' +
            'violation the overhaul killed',
        ).toBe('visible')
      }
      // …and the two bounds that only make sense against a DEFINITE column
      // height come off with the scrollers. Both would resolve their percentage
      // against `.sats-plan`, which is content-height here — the indefinite-
      // percentage trap that made `.sat-globe-box`'s floor compute to 0.
      // MEASURED: `flex: 1 1 0` left the whole 42-row schedule at ZERO px at a
      // pinned 175% on 1600×900 while the column around it kept its 596 px.
      const sched = `[data-viewport='${vp}'] .sats-sched`
      expect(
        winner(sched, 'flex'),
        `${sched}: the base rule's \`flex: 1 1 0\` has no definite free space to grow into here, ` +
          'so the schedule resolves to 0 px and vanishes',
      ).toBe('0 0 auto')
      expect(
        winner(sched, 'min-height'),
        `${sched}: the base floor is a percentage of an indefinite height — release it`,
      ).toBe('0')
      expect(
        winner(`[data-viewport='${vp}'] .sats-radio`, 'max-height'),
        `[data-viewport='${vp}'] .sats-radio: the base cap's 55% has nothing definite to resolve ` +
          'against here and collapses the box',
      ).toBe('none')
      // The 11-column nowrap table's min-content (~700–800 px) exceeds these
      // widths while .sats-view keeps overflow-x hidden, so the table's OWN
      // container must own overflow-x — or Status/Needed/⏰/▶ Work are
      // right-clipped with NO path to them (including the sats pop-out at its
      // 420 px minimum, which stamps data-viewport=xs). This is the contract's
      // .dxm precedent: wide content scrolls inside its own container; the
      // page body never scrolls sideways. Vertically the box stays
      // content-sized, so the page scroller still owns the column.
      const scrollSel = `[data-viewport='${vp}'] .sats-sched-scroll`
      expect(
        winner(scrollSel, 'overflow-x'),
        `${scrollSel} must declare overflow-x:auto — the schedule table's only horizontal ` +
          "escape under .sats-view's overflow-x:hidden",
      ).toBe('auto')
      expect(
        winner(scrollSel, 'overflow'),
        `${scrollSel}: an \`overflow\` shorthand here re-releases BOTH axes and kills the ` +
          'horizontal escape (the exact round-1 regression) — declare overflow-x only',
      ).toBeNull()
    }
  })
  it('the schedule is the grower inside a BOUNDED planning column', () => {
    // The reason this guard exists is unchanged and still load-bearing: "Next
    // up" can be missing while the schedule renders, and if the schedule ever
    // ends up in an unbounded track it is an uncapped table under
    // .sats-view's overflow:hidden — the overhaul's own bug class.
    //
    // 2026-08-03 (the pass rebuild): the three column-1 sections are no longer
    // grid children pinned to explicit rows. They are flex children of
    // `.sats-plan`, which is itself the grid grower. The invariant is now
    // stated in flex and cannot be re-ordered into the bug: the strip and the
    // radio quadrant are fit-content, the schedule is the only thing that
    // grows, and the shell that holds all three is bounded and clips.
    expect(winner('.sats-plan', 'grid-row'), 'the planning column owns the grower row').toBe('3')
    expect(winner('.sats-plan', 'display')).toBe('flex')
    expect(winner('.sats-plan', 'flex-direction')).toBe('column')
    expect(winner('.sats-plan', 'min-height'), 'a flex column that cannot shrink is not bounded').toBe('0')
    expect(winner('.sats-best', 'flex'), 'the strip cannot use surplus height').toBe('0 0 auto')
    expect(
      winner('.sats-sched', 'flex'),
      'the schedule is the only grower — and its BASIS must be 0, not auto: with `auto` the ' +
        "basis is the 42-row table's own height (~1,090 px), so this column's free space is " +
        'always hugely negative and any shrinkable sibling gets squeezed by a number that has ' +
        'nothing to do with what the column can hold',
    ).toBe('1 1 0')
  })

  it('nothing in the planning column can be clipped with no way to reach it', () => {
    // ⚠️ THE TRAP THIS REPLACES WAS REACHABLE IN TWO CLICKS, and it is measured,
    // not imagined. `.sats-plan` used to be `overflow: hidden` over TWO
    // fit-content children with only the schedule able to give. At the 1024×768
    // floor, on a 12-transmitter bird with the chooser's "show all N" and "show N
    // inactive" both open: the schedule went to 0 px (unreachable, above),
    // `.sats-radio` grew to 1,123 px in a 637 px column and 667 px of it was
    // CLIPPED — including the control that would have collapsed it again. Even at
    // 1920×1080 the schedule went to 0 and 60 px of the chooser was clipped.
    //
    // Three computed conditions kill the class. Each is necessary: a cap without
    // a scroller is still a clip, a scroller without a cap still starves the
    // schedule, and neither can prove the column itself has an escape.
    expect(
      winner('.sats-plan', 'overflow-y') ?? winner('.sats-plan', 'overflow'),
      'the planning column must own a backstop scroller: it holds two fit-content children ' +
        '(`.sats-best`, `.sats-radio`) that cannot shrink to fit, and the contract forbids a ' +
        'clipping box over unshrinkable descendants with no interposed scroller. It is DORMANT ' +
        'at every swept size (measured 0 px of overflow at all ten, expanded chooser included).',
    ).toMatch(/^(auto|scroll)$/)
    expect(
      winner('.sats-radio', 'flex'),
      'the radio quadrant must be able to give before the column has to scroll',
    ).toBe('0 1 auto')
    const cap = winner('.sats-radio', 'max-height')
    expect(
      cap,
      '.sats-radio declares no max-height — TP_ALIVE_CAP bounds the CARDS, not the box: two ' +
        'clicks on the chooser take it past 1,100 px',
    ).not.toBeNull()
    expect(
      cap!.replace(/\s+/g, ''),
      `max-height is \`${cap}\` — a bare ceiling cannot yield when the column is shorter than ` +
        'it. The idiom is min(Xem, share).',
    ).toBe('min(28em,55%)')
    expect(
      winner('.sats-radio', 'overflow-y') ?? winner('.sats-radio', 'overflow'),
      'a max-height without an overflow rule is a CLIP, which is the bug this cap was added to ' +
        'fix — the capped box needs its own scroller',
    ).toMatch(/^(auto|scroll)$/)
    expect(
      winner('.sats-radio', 'min-height'),
      'a flex item with the default `min-height: auto` cannot shrink below its content, so the ' +
        'cap would never bind',
    ).toBe('0')
  })

  it('the radio quadrant is what BOUNDS the schedule — so it may never floor itself', () => {
    // The operator asked for the schedule to be "made smaller and scrollable to
    // free up more real estate". There is deliberately NO max-height and no row
    // cap on the schedule (see the guard above and the one below): what stops
    // it eating the page is that a fit-content block sits under it in a bounded
    // column, so the window sets the row count instead of a constant.
    //
    // That only works while the quadrant genuinely yields. A min-height FLOOR
    // here would drive the only shrinkable child — the schedule scroller,
    // min-height 0 — to zero and then clip the quadrant itself. (`min-height: 0`
    // on `.sats-radio` is the opposite of a floor and is asserted in the
    // no-clip guard above: it is what lets the box shrink at all.)
    for (const sel of ['.sats-radio-cell', '.sats-best']) {
      expect(winner(sel, 'min-height'), `${sel} must not floor its height`).toBeNull()
    }
    for (const sel of ['.sats-radio', '.sats-radio-cell', '.sats-best']) {
      expect(winner(sel, 'height'), `${sel} must not fix a height`).toBeNull()
    }
    expect(
      winner('.sats-radio', 'min-height'),
      '.sats-radio must declare `min-height: 0` and nothing else — any positive floor is the ' +
        'bug described above',
    ).toBe('0')
    // `.sats-plan` is the exception and the opposite case: its `min-height: 0`
    // is what makes it shrinkable at all (asserted in the guard above), so a
    // floor is only forbidden here in the form of a fixed height.
    expect(winner('.sats-plan', 'height'), '.sats-plan must not fix a height').toBeNull()
    // Both tracks shrinkable: a min-content floor in either cell would blow the
    // quadrant out sideways under .sats-view's overflow-x:hidden.
    expect(winner('.sats-radio', 'grid-template-columns')).toBe('minmax(0, 1fr) minmax(0, 1fr)')
    expect(winner('.sats-radio-cell', 'min-width')).toBe('0')
  })
  it('the Next/Best strip stays fit-content — no floors, the side tags never grow', () => {
    // The strip sits in the auto row above the schedule grower with a budget
    // of four content rows (the 2026-08 two-pairs ruling: within one
    // row-height of the old three-row strip). A height floor here, or a group
    // tag that grows, is how the strip would squeeze the grower — the
    // overhaul's own bug class.
    for (const sel of ['.sats-best', '.sats-best-group', '.sats-best-rows', '.sats-best-row']) {
      expect(winner(sel, 'height'), `${sel} must not fix a height`).toBeNull()
      expect(winner(sel, 'min-height'), `${sel} must not floor its height`).toBeNull()
    }
    expect(winner('h2.sats-best-tag', 'flex'), 'the group tag is a fixed side label').toBe(
      '0 0 3.5em',
    )
    expect(
      winner('.sats-best-rows', 'min-width'),
      'the rows column must be shrinkable (long why-lines under a narrow schedule column)',
    ).toBe('0')
  })
  it('NO heading rule in this section can reach the shared log strip', () => {
    // THE BUG THIS REPLACES, kept because it is the reason for the shape below.
    // The detail card used to carry the bird's name as `.sats-detail > h2`,
    // STICKY, because the sky dome and the globe stacked were together taller
    // than the column at every window size and a ✕ at the top scrolled away.
    // Written as a DESCENDANT (`.sats-detail h2`) that rule out-cascaded
    // `.log-entry h2` — identical specificity, later in the sheet — and both
    // shrank the log strip's own "Log this QSO" and pinned the wrong heading to
    // the top of the scroller. It shipped that way once.
    //
    // The 2026-08-03 pass rebuild retires the whole shape rather than defending
    // it: the bird's identity and the ✕ live in `.sats-armbar`, a non-scrolling
    // grid row; the log strip is a SIBLING of `.sats-detail`, not a descendant;
    // and `.sats-detail` has no heading at all. No heading, no sticky, no trap.
    //
    // ⚠️ WHAT THIS GUARD PROVES, and its honest limit. `winner` matches rules by
    // SELECTOR TEXT, so it cannot compute the cascade across every selector that
    // would match a real element — that needs a matcher over a rendered DOM, and
    // `SatellitesView.console.test.tsx` carries the rendered half. What is
    // computed here is that no rule in the sheet is written in a form that COULD
    // capture a heading inside the log strip: the strip lives in `.sats-side`,
    // beside `.sats-detail` and `.sats-favmgr` and near `.sats-radio`, and each
    // of those is a plausible place for someone to reach for `X h2` by reflex.
    for (const sel of [
      '.sats-detail > h2',
      '.sats-detail h2',
      '.sats-side h2',
      '.sats-log h2',
      '.sats-radio h2',
      '.sats-plan h2',
    ]) {
      for (const prop of ['position', 'font-size', 'text-transform', 'background']) {
        expect(
          winner(sel, prop),
          `\`${sel}\` declares ${prop} — a heading rule in that form reaches the shared ` +
            'LogEntry’s own <h2> and re-creates the cascade bug this section already shipped once',
        ).toBeNull()
      }
    }
    // …and the card headings that DO exist are matched by a class or by a box
    // that cannot contain the strip. `.sats-radio-title` is the new one, and it
    // is a class for exactly this reason.
    expect(winner('.sats-radio-title', 'font-size'), 'the quadrant titles lost their styling').toBe(
      'var(--fs-label)',
    )
    // The strip's own scoped compaction still out-ranks the base component
    // rules: (0,2,0) vs (0,1,0), position-independent.
    expect(winner('.sats-log .log-entry', 'padding')).toBe('var(--space-2) var(--space-3)')
  })

  it('the arm bar is a non-scrolling grid row, and sticky ONLY where the page scrolls', () => {
    // It carries the ■ stop, the ✕ and the five readiness gates. At md+ it is
    // grid row 2 of a bounded shell — nothing scrolls past it, so it needs no
    // sticky and must not have one (a sticky in a non-scrolling context is dead
    // weight that later reads as a fix). At sm/xs the page really does scroll
    // (`main.layout.single`), and there it sticks with an OPAQUE surface.
    expect(winner('.sats-armbar', 'grid-row')).toBe('2')
    expect(winner('.sats-armbar', 'grid-column')).toBe('1 / -1')
    expect(winner('.sats-armbar', 'position'), 'no sticky at md+ — nothing scrolls past it').toBeNull()
    for (const vp of ['sm', 'xs']) {
      const sel = `[data-viewport='${vp}'] .sats-armbar`
      expect(winner(sel, 'position'), `${sel}: the page scroller would carry the ■ stop away`).toBe(
        'sticky',
      )
      expect(
        winner(sel, 'background'),
        `${sel}: a sticky surface needs an opaque background (contract rule)`,
      ).toContain('var(--bg')
    }
  })

  it('…and the sm/xs sticky has TRAVEL — the three conditions, not the declaration', () => {
    // ⚠️ THIS GUARD EXISTS BECAUSE THE RULE ABOVE SHIPPED DEAD, and the guard
    // above passed while it did. `position: sticky` is not a behaviour; it is a
    // request that two properties of the ANCESTOR have to grant. Both were
    // refused, so at a pinned 175% on 1600×900 the arm bar — the ■ stop and the
    // ✕ — scrolled straight off the top. Measured in a real browser at that
    // size, bar-top vs the scrollport across a 0/300/600 px scroll:
    //     with .sats-view as grid + overflow-y:auto   47.6 → -252.4 → -552.4
    //     with .sats-view as flex + overflow:visible  47.6 →   12.0 →   12.0
    // The three conditions, all computed from this sheet:
    //
    //   1. THE STICKY IS DECLARED (the guard above).
    //   2. THE PARENT IS NOT A GRID. A grid item's sticky containing block is its
    //      own grid AREA; `.sats-view`'s tracks are `auto` at these widths, so the
    //      area is exactly the item's height and the travel is zero. One column
    //      means flex and grid lay out identically here, so this costs nothing.
    //   3. THE PARENT IS NOT ITSELF A SCROLL CONTAINER. Sticky positions against
    //      the NEAREST scroll container. `.sats-view` declared `overflow-y: auto`
    //      AND `height: auto`, i.e. a scrollport that can never scroll — pinning
    //      the bar to something that never moves. `visible` hands the job to
    //      `main.layout.single`, which is the real page scroller.
    for (const vp of ['sm', 'xs']) {
      const view = `[data-viewport='${vp}'] .sats-view`
      expect(
        winner(view, 'display'),
        `${view} is a grid: a grid item's sticky containing block is its own auto-sized grid ` +
          'area, so the arm bar has ZERO travel and its sticky is decoration',
      ).not.toBe('grid')
      const of = winner(view, 'overflow')
      const ofy = winner(view, 'overflow-y')
      expect(
        [of, ofy].some((v) => v != null && /^(auto|scroll)$/.test(v)),
        `${view} declares a scrolling overflow (overflow: ${of}, overflow-y: ${ofy}). That makes ` +
          'it the nearest scroll container for the arm bar — and with `height: auto` it can ' +
          'never scroll, so the bar is pinned to a scrollport that never moves. This is exactly ' +
          'how the sticky shipped dead.',
      ).toBe(false)
      expect(
        winner(view, 'height'),
        `${view} must stay content-height here — see the rule's own comment (a 100% height with ` +
          'negative free space resolved every child to min-content)',
      ).toBe('auto')
    }
  })

  it('the arm bar has ONE trailing anchor, so the ■ stop stops moving under the cursor', () => {
    // `.sats-tracking-badge` and `.sats-detail-close` BOTH declared
    // `margin-left: auto`, and two auto margins on one flex line SPLIT the free
    // space between them. Measured at the 1024×768 floor: the ■ stop sat 403.6
    // effective px in from the bar's right edge, and because the badge's own text
    // is re-rendered on the 2 s poll ("cmd az 214° el 46°"), it moved. That is
    // the thing this bar's chosen-wrap-point design was built to prevent.
    // Computed here as a cascade question between two DIFFERENT selectors that
    // match the same element, which `winner` (one selector at a time) cannot ask.
    const solo = decl('.sats-detail-close', 'margin-left')
    const paired = decl('.sats-tracking-badge + .sats-detail-close', 'margin-left')
    expect(decl('.sats-tracking-badge', 'margin-left')?.value, 'the badge is the anchor').toBe(
      'auto',
    )
    expect(solo?.value, 'a lone ✕ (no track running) still needs its own auto margin').toBe('auto')
    expect(
      paired,
      'nothing gives the ✕ back a fixed margin when it FOLLOWS the badge — so both carry ' +
        '`margin-left: auto`, the free space splits, and neither goes flush right',
    ).not.toBeNull()
    expect(paired!.value, 'the ✕ must give its auto margin up beside the badge').toBe('0')
    expect(
      beats(paired!, solo!),
      '`.sats-tracking-badge + .sats-detail-close` (0,2,0) must out-rank `.sats-detail-close` ' +
        '(0,1,0) — and it must win on SPECIFICITY, not on sheet order, or moving either rule ' +
        'silently restores the split',
    ).toBe(true)
  })

  it('the pass column: the two graphics are ABREAST and both capped in --vh-eff', () => {
    // THE MOVE THAT MAKES THE NO-SCROLL PROMISE ARITHMETICALLY POSSIBLE. Stacked,
    // the sky dome and the ground-track globe cost ~916 px of a 713 px column at
    // the 1024×768 floor — which is why the ✕ above them had to be sticky.
    // Abreast and capped they cost the height of the taller one and stop growing
    // with the window.
    expect(winner('.sats-pass-graphics', 'display')).toBe('grid')
    expect(
      winner('.sats-pass-graphics', 'grid-template-columns'),
      'both tracks must be minmax(0,…) — a min-content floor here blows the column out sideways',
    ).toBe('minmax(0, 1fr) minmax(0, 0.85fr)')

    // ⚠️ THE DOME CAP HAS TO OUT-RANK `.sat-sky.live .sat-dome`, WHICH IS (0,3,0).
    // A cap written only on `.sat-dome` is (0,1,0) and loses for the whole of
    // every live pass — i.e. at exactly the moment the size matters. That is this
    // sheet's documented failure mode (two fixes shipped dead that way), so both
    // selectors must resolve to the SAME capped value.
    const plain = winner('.sat-dome', 'max-width')
    const live = winner('.sat-sky.live .sat-dome', 'max-width')
    expect(plain, '.sat-dome declares no max-width').not.toBeNull()
    expect(plain!, 'the cap must be --vh-eff-relative, never a raw vh (zoom-blind)').toContain(
      'var(--vh-eff',
    )
    expect(
      live,
      'a `.sat-sky.live .sat-dome` rule that sets max-width without the cap would out-rank it ' +
        '(0,3,0 vs 0,1,0) and the dome would be uncapped for the whole of every live pass',
    ).toBe(plain)

    // THE CAP COEFFICIENT IS THE OPERATOR'S OWN NUMBER, and it is not free to
    // drift: TAG_FS (SatellitesView.tsx) is DERIVED from it — the on-plate az/el
    // is drawn in SVG viewBox units, so rendered px = unit × dome px / 248, and
    // 12.3 × (0.31·904) / 248 = 13.9 px against 18.5 before, the −24.8% he asked
    // for by name ("the actual aos, los and az, el text could be made smaller by
    // 25%"). A round-1 build shipped 0.28 here while that arithmetic still said
    // 0.31, which silently made the cut −32%. Computed rather than eyeballed.
    const coef = Number(/([\d.]+)\s*\*\s*var\(--vh-eff/.exec(plain!)?.[1] ?? NaN)
    expect(coef, `could not read a --vh-eff coefficient out of \`${plain}\``).toBeGreaterThan(0)
    const TAG_FS = 12.3 // must equal SatellitesView.tsx's constant
    const before = 10 * (458 / 248) // TAG_FS 10 on the pre-rebuild 458 px dome
    const after = TAG_FS * ((coef * 904) / 248) // 904 = --vh-eff at the 1024×768 floor
    expect(
      1 - after / before,
      `at ${coef}·--vh-eff the dome is ${(coef * 904).toFixed(0)} px at the floor and the az/el ` +
        `plate lands at ${after.toFixed(1)} px — a ${((1 - after / before) * 100).toFixed(1)}% ` +
        'cut. The instruction was 25%. Change the coefficient and TAG_FS together or not at all.',
    ).toBeCloseTo(0.25, 2)

    // The globe is capped the same way. It carries NO floor: `min-height: min(
    // 180px, 100%)` was INERT — the percentage resolves against an `auto` grid
    // row, i.e. an indefinite height, so it computed to 0 — and a real floor
    // under this bounded column is the contract's named bug. Width +
    // aspect-ratio already give a square that shrinks with the column.
    const g = winner('.sat-globe-box', 'width')
    expect(g!).toContain('var(--vh-eff')
    expect(
      winner('.sat-globe-box', 'min-height'),
      'a floor here is either inert (a percentage against an indefinite grid row) or a clip ' +
        '(a bare px value under a bounded column) — the box is sized by width + aspect-ratio',
    ).toBeNull()
    expect(winner('.sat-globe-box', 'aspect-ratio'), 'the globe letterboxes without it').toBe('1 / 1')

    // The passband plot grew with its column off a 320×102 viewBox — 224 px tall
    // at 1024×768 and 551 px at 3440×1440, the fastest-growing block in the
    // section. Capping the WIDTH is what caps the height.
    expect(winner('.sat-pb-plot', 'max-width'), 'the plot is unbounded again').toBe('300px')
  })

  it('at sm/xs the dome is capped in em, so UI zoom can actually enlarge the az/el', () => {
    // `--vh-eff` is `innerHeight / --ui-zoom`, so `0.31 · --vh-eff` LAYOUT px is
    // `0.31 · innerHeight` DEVICE px at every zoom: the dome is the one surface
    // whose physical size does NOT change when the operator raises UI zoom. The
    // az/el plate is SVG units scaled by the dome, so at a pinned 175% those
    // numbers became the smallest type on screen — on the instrument a
    // manual-rotor operator steers by, while everything around them grew 1.75×.
    // At md+ that is disclosed and deliberate (the effective viewport at high
    // zoom is genuinely short: 514 px at 175% on 1600×900). At sm/xs the PAGE
    // scrolls, height is not the constraint, and `em` — which `zoom` does
    // multiply — is the honest unit. Measured: dome 144 → 364 effective px there.
    for (const vp of ['sm', 'xs']) {
      const plain = winner(`[data-viewport='${vp}'] .sat-dome`, 'max-width')
      const live = winner(`[data-viewport='${vp}'] .sat-sky.live .sat-dome`, 'max-width')
      expect(plain, `[data-viewport='${vp}'] .sat-dome declares no cap of its own`).not.toBeNull()
      expect(
        plain!,
        'a --vh-eff cap here is zoom-invariant in device px — the whole defect this rule fixes',
      ).not.toContain('var(--vh-eff')
      expect(plain!, 'the cap must be font-relative so `zoom` multiplies it').toContain('em')
      expect(
        live,
        `[data-viewport='${vp}'] .sat-sky.live .sat-dome must carry the same cap: the plain form ` +
          "is (0,2,0) and loses to the base sheet's `.sat-sky.live .sat-dome` (0,3,0) for the " +
          'whole of every live pass — the exact way two earlier fixes shipped dead',
      ).toBe(plain)
    }
  })
})

describe('⊞ Panels popover: Undo / Reset stay reachable however long the list gets', () => {
  // The popover has always been an unbounded absolutely-positioned column, and the
  // unavailable/note affordance added a wrapped reason line under as many as four of CW's
  // eight entries (~238 → ~304 px). Its footer holds Undo and Reset — the two controls an
  // operator reaches for right after a mis-tick — so it is the last thing that may fall off
  // the bottom. At a pinned 175% UI zoom on an ordinary window it already did.
  it('.panels-menu-pop is height-capped against the EFFECTIVE viewport', () => {
    const v = winner('.panels-menu-pop', 'max-height')
    expect(v, '.panels-menu-pop declares no max-height — an unbounded overlay column').not.toBeNull()
    expect(
      v!,
      `max-height is \`${v}\` — it must reference var(--vh-eff): a raw vh unit is blind to ` +
        '`.app { zoom }`, which is precisely the pinned-zoom case that pushes the footer off.',
    ).toContain('var(--vh-eff')
  })

  it('.panels-menu-pop scrolls its own overflow (a cap without one just clips the footer)', () => {
    const v = winner('.panels-menu-pop', 'overflow-y') ?? winner('.panels-menu-pop', 'overflow')
    expect(
      v,
      'capping the popover without a scroller moves the problem rather than fixing it: the ' +
        'entries past the cap, and Undo / Reset below them, would be painted outside the clip ' +
        'with no path to them at all.',
    ).toBe('auto')
  })

  // The focus ring is painted OUTSIDE the box, and `opacity` composites the element's whole
  // rendering — outline included — with everything below it. So an opacity anywhere on the
  // chain from the popover down to the checkbox halves the ring of the entry the operator has
  // focused, against the `--bg-elev-2` backdrop, and takes it under contrast. A greyed
  // affordance shipped exactly that way (`input[aria-disabled] { opacity: .5 }`): the change
  // made to keep the entry keyboard-reachable is what made its focus indicator hard to see.
  // Grey with colour/background, never by fading the focusable element.
  //
  // `filter` is matched as well as `opacity`, and not as a courtesy: `filter: opacity(.5)`
  // fades identically, and EVERY filter function makes the element a composited group whose
  // outline goes through the filter with it. Matching the property outright (rather than
  // sniffing for the opacity() function) costs nothing here — nothing on this five-class
  // chain has any business carrying a filter — and it closes the "scans opacity only" hole
  // instead of leaving it written down.
  //
  // Unlike the cascade-computed guards above this is an ABSENCE check: no rule may declare
  // the property on the chain, and if none declares it none can win it. The walk descends
  // into @media, so a conditional dim is caught too.
  //
  // WHAT IT PROVES, EXACTLY — an earlier round of this claimed to make the hazard
  // unrepresentable, and it did not. It matched on the SUBJECT (last compound) of a
  // selector and only when that subject carried a chain class, so `.panels-menu-check > *
  // { opacity: .5 }` walked straight past it and faded the ring exactly as the deleted rule
  // had. The subject test below is therefore in two parts: a subject that NAMES a chain
  // class, and a subject with NO class of its own (`*`, or a bare `input`/`span`/`label`),
  // which can match the checkbox or one of its wrappers — those are flagged whenever the
  // selector is inside the ⊞ popover at all.
  //
  // The residual limit, which is real: a subject bearing some OTHER class
  // (`.panels-menu-pop .new-wrapper { opacity }`) is not flagged, because a class the guard
  // has never heard of cannot be placed on the chain by reading the sheet. RING_CHAIN is
  // the answer to that and it is a list a human keeps — ADD ANY NEW ELEMENT BETWEEN THE
  // POPOVER AND THE CHECKBOX TO IT. `.panels-menu-row` was added when the popped-out tag
  // was lifted out of the <label>.
  const RING_CHAIN = new Set([
    '.panels-menu',
    '.panels-menu-pop',
    '.panels-menu-item',
    '.panels-menu-row',
    '.panels-menu-check',
  ])
  it('nothing on the chain down to the entry fades it — the focus ring keeps its contrast', () => {
    const dimmed = RULES.filter((r) => {
      if (!/(?:^|;)\s*(?:opacity|filter)\s*:/.test(r.body)) return false
      const compounds = r.selector.split(/\s|>|\+|~/).filter(Boolean)
      const subject = compounds[compounds.length - 1] ?? ''
      const classesOf = (s: string) => s.match(/\.[a-z][a-zA-Z0-9-]*/g) ?? []
      const subjectClasses = classesOf(subject)
      if (subjectClasses.length) return subjectClasses.some((c) => RING_CHAIN.has(c))
      // Class-less subject: `*`, `input`, `span`, `label`… any of which can BE the
      // checkbox or wrap it. Flagged if anything else in the selector puts it in the
      // popover — which is the case the old shape test missed.
      return compounds.some((c) => classesOf(c).some((cls) => RING_CHAIN.has(cls)))
    })
    expect(
      dimmed.map((r) => r.selector),
      'these rules fade or filter an entry or one of its ancestors in the ⊞ popover, which ' +
        'composites the focus ring with it. Use `color` / `background` for the greyed look — ' +
        'see `.panels-menu-actions button:disabled`, which greys by colour alone.',
    ).toEqual([])
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

describe('Grid joins the exchange row without re-arming the sideways overflow', () => {
  // The descope of the first Grid attempt named a REAL concern: the strip already
  // wraps at 24em, and a new field in a wrapping row is a new chance to overflow.
  // The answer is the same one `.le-state`/`.le-country` use — a narrow,
  // NON-GROWING and non-shrinking field, so a wrapping row moves it to the next
  // line rather than squeezing it — and these guards COMPUTE that it survives the
  // cascade rather than merely existing (`.le-row .settings-input` is more
  // specific and would otherwise win, which is exactly how a dead fix ships).
  it('.le-grid neither grows nor shrinks, and BEATS the row default that would grow it', () => {
    const grid = decl('.le-grid', 'flex')
    const rowDefault = decl('.le-row .settings-input', 'flex')
    expect(grid, '.le-grid declares no flex — it inherits the growing row default').not.toBeNull()
    expect(rowDefault, "the row's own input default vanished").not.toBeNull()
    expect(grid!.value.replace(/\s+/g, ' ')).toBe('0 0 auto')
    expect(
      beats(grid!, rowDefault!),
      "`.le-row .settings-input` (0,2,0) out-specifies `.le-grid` (0,1,0), so the narrow rule " +
        'needs !important to win — without it Grid grows like Comment and squeezes the row. ' +
        'This is why `.le-state` and `.le-country` carry it too.',
    ).toBe(true)
  })

  it('.le-grid is capped far below the 24em pane minimum, so it can never BE the overflow', () => {
    // A wrapping row only overflows if a single item is wider than the line box.
    // 24em ≈ 384px at the pane floor; the cap is a quarter of that.
    const cap = winner('.le-grid', 'max-width')
    expect(cap, '.le-grid declares no max-width — an input defaults to ~20 characters').not.toBeNull()
    const px = Number(/^(\d+(?:\.\d+)?)px$/.exec(cap!.trim())?.[1] ?? NaN)
    expect(px, `max-width is \`${cap}\` — express the cap in px like its row neighbours`).toBeGreaterThan(0)
    expect(px, 'a field wider than the 24em pane floor is a sideways overflow by itself').toBeLessThan(24 * 16)
  })

  it('.le-grid fits the LINE BUDGET that keeps it off a new row of its own', () => {
    // THE NUMBER THE HEIGHT FIX RESTS ON, computed from this sheet rather than
    // trusted. Grid shares the exchange row's last wrapped line with Name and
    // Clear. At the two-column floor (.sats-view's `minmax(340px, …)`) that line
    // is 296px wide: 340 − 18 (.sats-detail border+padding) − 26 (.log-entry
    // border+padding). Name is `.le-row .settings-input`'s 9em basis at the
    // input's own 14px font = 126px; Clear measures 61px; two 8px gaps. What is
    // left for Grid is 93px, and a cap over it wraps Clear onto a FOURTH line:
    // measured at 94px the strip goes 451 → 491, handing back 40 of the 48px the
    // park row's removal bought. Not a regression at that width, but the whole
    // point of putting Grid in this row was that it costs NOTHING.
    //
    // Measured against real layout (headless Chrome, the section's own
    // wrappers): parent 499px → 451px at 340, and 368px → 320px at a 611px
    // column. Both DOWN by the whole park row.
    //
    // ⚠️ DO NOT restate 611px as "what a 1366-wide window gives" — that
    // derivation was published and was wrong. `.sats-view` sits BEHIND
    // `.mode-nav` (88px + 1px border) and INSIDE `main.layout.single`
    // (padding var(--gap) a side), none of which the arithmetic included.
    // These are column widths, which is what this test controls; the
    // window width that produces one is a separate measurement.
    const cap = Number(/^(\d+(?:\.\d+)?)px$/.exec(winner('.le-grid', 'max-width')!.trim())![1])
    const basis = /^1\s+1\s+(\d+(?:\.\d+)?)em$/.exec(
      decl('.le-row .settings-input', 'flex')!.value.replace(/\s+/g, ' ').trim(),
    )
    expect(basis, "the row default's wrap basis is no longer an em length — re-derive the budget").not.toBeNull()
    const nameBasis = Number(basis![1]) * 14 // .settings-input sets font-size: 14px
    const gap = 8 // --space-2 at --space-scale: 1
    const CLEAR_BTN = 61 // measured
    const line = 340 - 18 - 26
    expect(
      cap,
      `.le-grid is ${cap}px; the last wrapped line has ${line - nameBasis - CLEAR_BTN - 2 * gap}px ` +
        'left after Name and Clear. Over it, Clear wraps onto a FOURTH line (measured at 94px: ' +
        '451 → 491) and hands 40 of the park row\'s 48px straight back — re-measure before raising it.',
    ).toBeLessThanOrEqual(line - nameBasis - CLEAR_BTN - 2 * gap)
  })

  it('.le-grid keeps the released intrinsic floor — it does not re-declare min-width', () => {
    // `min-width: 0` on `.le-row .settings-input` is what let the row shrink at all
    // (the 2026-07-31 fix). Re-flooring it here would put the overflow straight back.
    expect(
      decl('.le-grid', 'min-width'),
      '.le-grid declares its own min-width — the row default releases the floor, leave it released',
    ).toBeNull()
    expect(winner('.le-row .settings-input', 'min-width')).toBe('0')
  })
})
