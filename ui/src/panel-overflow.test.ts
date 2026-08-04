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
    expect(winner('.sats-sched', 'min-height')).toBe('0')
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
      for (const sel of [`[data-viewport='${vp}'] .sats-sched`, `[data-viewport='${vp}'] .sats-side`]) {
        expect(
          winner(sel, 'overflow'),
          `${sel}: .sats-view is the page scroller at ${vp} — a bounded inner scroller here ` +
            'is the one-scroll-owner violation the overhaul killed',
        ).toBe('visible')
      }
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
  it('.sats-sched is PINNED to the grower row (Next-up may be absent now)', () => {
    // The gate-ladder reorder means "Next up" can be missing while the
    // schedule renders; auto-placement would then land the schedule in the
    // `auto` row — an unbounded table under .sats-view's overflow:hidden.
    expect(winner('.sats-best', 'grid-row')).toBe('2')
    expect(winner('.sats-sched', 'grid-row')).toBe('3')
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
  it('the sticky detail heading (the ✕ home) has an opaque background', () => {
    // DIRECT CHILD. The detail card hosts the shared LogEntry strip, which has
    // its own <h2>; as a DESCENDANT rule this one out-cascaded `.log-entry h2`
    // (identical specificity, later in the sheet) and made a second sticky
    // heading inside the same scroller. The guard asks for the scoped form so
    // the descendant version cannot come back.
    //
    // ⚠️ EXACTLY WHAT THESE THREE LINES PROVE, and nothing beyond it. `winner`
    // matches rules by SELECTOR TEXT (`r.selector !== selector`), so this is a
    // cascade computed WITHIN one selector string, not across the selectors that
    // would match a given element:
    //   · line 1/2 — among rules literally written `.sats-detail > h2`, the
    //     winning `position` is `sticky` and the winning `background` is a
    //     `--bg*` variable. It does NOT prove that rule wins over anything else.
    //   · line 3 — NO rule in the sheet is literally written `.sats-detail h2`
    //     with a `position`. It does NOT rule out another descendant form that
    //     would also capture the strip's heading (`.sats-side h2`,
    //     `.sats-detail div h2`, …).
    // What is therefore NOT computed here is the thing the bug was: the cascade
    // between this rule and `.log-entry h2` against the real heading element.
    // That needs a matcher over a rendered DOM, which this CSS-text file has no
    // way to build — carried, not claimed.
    expect(winner('.sats-detail > h2', 'position')).toBe('sticky')
    const bg = winner('.sats-detail > h2', 'background')
    expect(bg, 'sticky surfaces need an opaque background (contract rule)').toContain('var(--bg')
    expect(
      winner('.sats-detail h2', 'position'),
      'the exact selector `.sats-detail h2` declares position again — it captures the log strip’s own heading',
    ).toBeNull()
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

describe('Grid joins the QTH row without re-arming the sideways overflow', () => {
  // The descope of the first Grid attempt named a REAL concern: the strip already
  // wraps at 24em, and a new field in a wrapping row is a new chance to overflow.
  // The answer is the same one `.le-state`/`.le-country` use — a narrow,
  // non-growing, shrinkable-to-nothing field — and these guards COMPUTE that it
  // survives the cascade rather than merely existing (`.le-row .settings-input`
  // is more specific and would otherwise win, which is exactly how a dead fix
  // ships).
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
