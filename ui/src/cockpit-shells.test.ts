import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import {
  resolveClamp,
  SCOPE_SPLIT_MAX,
  SCOPE_SPLIT_MIN,
  SSTV_STAGE_SPLIT_MAX,
  SSTV_STAGE_SPLIT_MIN,
  TX_SPLIT_MIN,
  WATERFALL_SPLIT_MAX,
  WATERFALL_SPLIT_MIN,
  type SplitClamp,
} from './features/paneSeam'

// Guards the DEFICIT VALVE (2026-07-30 layout assessment, mechanism C1/C2): every
// cockpit shell must resolve to `overflow-y: auto` so a genuine vertical deficit SCROLLS
// instead of clipping the log form / PTT tail unreachably. Operate joined on 2026-10-01
// (2026-10-01): its shell was `overflow: hidden`, so at a narrow effective size its decode panes
// were the only thing left to give and were crushed to their title bars.
//
// Why a cascade COMPUTER and not a regex: two scroll "fixes" shipped dead —
//   1. `.phone-cockpit { overflow-y:auto }` lost to `.layout.single.phone-cockpit
//      { overflow:hidden }` on specificity ((0,1,0) vs (0,3,0)), and
//   2. `.layout.single.cw-cockpit` declared `overflow-y:auto` and then reset it with
//      `overflow:hidden` LATER IN THE SAME BLOCK (the shorthand wins by declaration order).
// A regex sees the `auto` in both cases and passes. This test parses the sheet
// (brace/comment-aware, @media-aware) and computes the winning overflow-y the way the
// cascade does: same-block declaration order incl. the `overflow` shorthand resetting
// `overflow-y`, then specificity, then source order.

const css = (readFileSync(fileURLToPath(new URL('./styles.css', import.meta.url)), 'utf8') + '\n' +
  readFileSync(fileURLToPath(new URL('./remote-web/application.css', import.meta.url)), 'utf8'))
  // Strip comments first so prose can't be read as declarations (same trap documented
  // in cockpit-floors.test.ts).
  .replace(/\/\*[\s\S]*?\*\//g, '')

interface Rule {
  selector: string // one selector (lists are split on top-level commas)
  body: string
  order: number
  media: string | null // non-null ⇒ conditional; excluded from the unconditional cascade
}

/** Brace-aware rule walk. Handles @media/@supports nesting (rules inside carry the
 *  condition) and skips other at-rule bodies (@keyframes etc.) wholesale. */
function parseRules(sheet: string): Rule[] {
  const out: Rule[] = []
  let i = 0
  let order = 0
  const n = sheet.length

  function skipBalanced(): void {
    // positioned just past an opening '{'
    let depth = 1
    while (i < n && depth > 0) {
      if (sheet[i] === '{') depth++
      else if (sheet[i] === '}') depth--
      i++
    }
  }

  function parseBlock(media: string | null): void {
    let selStart = i
    while (i < n) {
      const ch = sheet[i]
      if (ch === '}') {
        i++
        return
      }
      if (ch === '{') {
        const sel = sheet.slice(selStart, i).trim()
        i++
        if (sel.startsWith('@')) {
          if (/^@(media|supports)\b/.test(sel)) parseBlock(sel)
          else skipBalanced()
        } else {
          const bodyStart = i
          while (i < n && sheet[i] !== '}' && sheet[i] !== '{') i++
          const body = sheet.slice(bodyStart, i)
          if (sheet[i] === '}') i++
          order++
          for (const s of sel.split(',')) {
            const one = s.trim().replace(/\s+/g, ' ')
            if (one) out.push({ selector: one, body, order, media })
          }
        }
        selStart = i
      } else if (ch === ';') {
        // stray at-statement (@import etc.)
        i++
        selStart = i
      } else {
        i++
      }
    }
  }

  parseBlock(null)
  return out
}

const RULES = parseRules(css)

/** One compound of class selectors ('.a.b') as its class list; null when the compound
 *  contains anything else (pseudos, valued attributes, tags, ids, or is empty). Rejecting
 *  is the fail-safe direction here: a selector this computer cannot evaluate must never
 *  count as a cascade winner by accident. A PRESENCE attribute (`[data-sized]`) is the one
 *  addition, kept as its own token ('[data-sized]'): a chain entry matches it only when the
 *  modelled element lists that token, so a rule keyed on an attribute never styles an
 *  element modelled without it — SSTV's sized stage (layout L6) is the element that has it. */
function compoundClasses(compound: string): string[] | null {
  if (/[\s>+~:#]/.test(compound)) return null
  const parts = compound.match(/\.[a-zA-Z0-9_-]+|\[[a-zA-Z0-9_-]+\]/g)
  if (!parts || parts.join('') !== compound) return null
  return parts.map((p) => (p.startsWith('.') ? p.slice(1) : p))
}

/** Right-to-left match of a class-only selector (descendant/child combinators OK)
 *  against an explicit ancestor CHAIN (outermost → the element; each entry is that
 *  ancestor's class set). The first computer here matched bare compounds only, which
 *  left the strongest known override INVISIBLE to the guard: `.app.detached >
 *  .layout.single { overflow: hidden }` is (0,4,0) — it outranks every (0,3,0) shell
 *  rule — and the census (overflow-cascade #8) had already named it a silent trap.
 *  Combinator rules now participate; that one correctly does not match the main-window
 *  chain (the shells' parent is `.shell`, and `.app` never carries `detached` there).
 *  Descendant matching walks up the chain, so a modeled chain may omit unclassed
 *  wrapper divs; a child combinator checks the entry just above, which slightly
 *  over-matches on such gaps — the fail-safe direction for a guard. Selectors with
 *  sibling combinators or pseudos never match (none targets these elements today).
 *  ⚠️ If a cockpit ever becomes detachable, add its `.app.detached`-rooted chain to
 *  SHELLS — the detached rule above wins there and the valve dies in that window. */
function matchesChain(selector: string, chain: Array<Set<string>>): boolean {
  const parts = selector.split(/\s*([>+~])\s*|\s+/).filter((p): p is string => !!p)
  const subj = compoundClasses(parts[parts.length - 1] ?? '')
  if (!subj || !subj.every((c) => chain[chain.length - 1].has(c))) return false
  let idx = chain.length - 2
  let childOnly = false
  for (let i = parts.length - 2; i >= 0; i--) {
    const p = parts[i]
    if (p === '>') {
      childOnly = true
      continue
    }
    if (p === '+' || p === '~') return false
    const comp = compoundClasses(p)
    if (!comp) return false
    if (childOnly) {
      if (idx < 0 || !comp.every((c) => chain[idx].has(c))) return false
      idx--
      childOnly = false
    } else {
      while (idx >= 0 && !comp.every((c) => chain[idx].has(c))) idx--
      if (idx < 0) return false
      idx--
    }
  }
  return true
}

/** Class-count specificity — every candidate here is a class-only compound, or carries a
 *  presence attribute, which counts as a class does. */
function specificity(selector: string): number {
  return (selector.match(/[.[]/g) ?? []).length
}

/** Final overflow-y a block computes, honouring in-block declaration order and the
 *  `overflow` shorthand (its y value is the last of up to two values). */
function blockOverflowY(body: string): string | null {
  let v: string | null = null
  for (const decl of body.split(';')) {
    const m = /^\s*(overflow(?:-y)?)\s*:\s*(\S[^]*?)\s*$/.exec(decl)
    if (!m) continue
    if (m[1] === 'overflow') {
      const parts = m[2].split(/\s+/)
      v = parts.length === 2 ? parts[1] : parts[0]
    } else {
      v = m[2]
    }
  }
  return v
}

/** Cascade winner of overflow-y for the element at the end of `chain`. `activeMedia`
 *  names the one media condition considered matched (null = none); a conditional rule
 *  adds no specificity, it simply participates when its condition is active. */
function winningOverflowY(
  chain: Array<Set<string>>,
  activeMedia: string | null,
): { value: string; selector: string } | null {
  let win: { value: string; selector: string; spec: number; order: number } | null = null
  for (const r of RULES) {
    if (r.media !== null && r.media !== activeMedia) continue
    if (!matchesChain(r.selector, chain)) continue
    const v = blockOverflowY(r.body)
    if (v === null) continue
    const spec = specificity(r.selector)
    if (!win || spec > win.spec || (spec === win.spec && r.order >= win.order)) {
      win = { value: v, selector: r.selector, spec, order: r.order }
    }
  }
  return win && { value: win.value, selector: win.selector }
}

it('hosted FT scrolls vertical deficits and contains its stacked message column', () => {
  const native = [new Set(['app']), new Set(['shell']), new Set(['layout', 'single', 'operate-cockpit'])]
  const remote = [new Set(['app', 'remote-workspace']), ...native.slice(1)]
  // The desktop's Operate shell scrolls its deficit too since 2026-10-01 (the SHELLS census below).
  expect(winningOverflowY(native, null)?.value).toBe('auto')
  expect(winningOverflowY(remote, null)?.value).toBe('auto')
  expect(winningOverflowY([...remote, new Set(['cockpit-body']), new Set(['cockpit-lower', 'classic']), new Set(['cockpit-qsocol'])], null)?.value).toBe('auto')
})

/** Every distinct media condition that carries an overflow-declaring rule matching
 *  the element — each is a cascade context the valve must survive. */
function mediaContexts(chain: Array<Set<string>>): Array<string | null> {
  const out = new Set<string | null>([null])
  for (const r of RULES) {
    if (r.media !== null && matchesChain(r.selector, chain) && blockOverflowY(r.body) !== null) {
      out.add(r.media)
    }
  }
  return [...out]
}

/** Subject (rightmost compound) of a selector. */
function subject(selector: string): string {
  const parts = selector.split(/\s*[>+~]\s*|\s+/)
  return parts[parts.length - 1]
}

/** Rules whose subject carries the given class (i.e. rules that style that element). */
function rulesOn(cls: string): Rule[] {
  return RULES.filter((r) => {
    const parts: string[] = subject(r.selector).match(/\.[a-zA-Z0-9_-]+/g) ?? []
    return parts.includes(`.${cls}`)
  })
}

/** Final flex-grow a block computes (longhand + shorthand, in-block order). */
function blockGrow(body: string): number | null {
  let grow: number | null = null
  for (const decl of body.split(';')) {
    let m = /^\s*flex-grow\s*:\s*([\d.]+)/.exec(decl)
    if (m) {
      grow = parseFloat(m[1])
      continue
    }
    m = /^\s*flex\s*:\s*(\S[^]*?)\s*$/.exec(decl)
    if (!m) continue
    const v = m[1]
    if (v === 'none' || v === 'initial') grow = 0
    else if (v === 'auto') grow = 1
    else {
      const first = /^([\d.]+)/.exec(v)
      grow = first ? parseFloat(first[1]) : 0
    }
  }
  return grow
}

/** Main-window ancestor chain for a shell `<main class="layout single X">`:
 *  `.app` → `.shell` → the shell element (App.tsx ~2336; cockpits mount in `.shell`). */
function shellChain(cls: string): Array<Set<string>> {
  return [new Set(['app']), new Set(['shell']), new Set(['layout', 'single', cls])]
}

/** PHONE'S STAGE (2026-10-03, the left side). Phone's scope, its divider, the TX strip and the pane
 *  region are not shell children in the DOM: they stand in a STAGE inside the left side's ROW, two
 *  wrappers that are always rendered (so the side showing never re-parents the voice keyer or the log
 *  form) and that carry `cockpit-flat` — no box at all, cockpit-panes.css — until the side shows, then
 *  `cockpit-leftrow` / `cockpit-stage`. A selector is matched against the DOM, so every Phone chain
 *  here is the real one, in BOTH states: a chain that skipped the wrappers would let a `>` rule that
 *  can no longer match pass these guards. */
const PHONE_STAGES: Array<[state: string, chain: Array<Set<string>>]> = [
  ['no side', [...shellChain('phone-cockpit'), new Set(['cockpit-flat']), new Set(['cockpit-flat'])]],
  ['left side shown', [...shellChain('phone-cockpit'), new Set(['cockpit-leftrow']), new Set(['cockpit-stage'])]],
]

/** Final flex-direction a block computes (longhand + the `flex-flow` shorthand, whose
 *  direction keyword may sit in either position). */
function blockFlexDirection(body: string): string | null {
  let v: string | null = null
  for (const decl of body.split(';')) {
    let m = /^\s*flex-direction\s*:\s*(\S+)\s*$/.exec(decl)
    if (m) {
      v = m[1]
      continue
    }
    m = /^\s*flex-flow\s*:\s*(\S[^]*?)\s*$/.exec(decl)
    if (!m) continue
    const dir = m[1].split(/\s+/).find((t) => /^(row|column)(-reverse)?$/.test(t))
    if (dir) v = dir
  }
  return v
}

/** Cascade winner of a per-block-computed property for the element at the end of
 *  `chain` (unconditional rules only) — the same winner walk as winningOverflowY. */
function winningValue<T>(
  chain: Array<Set<string>>,
  blockValue: (body: string) => T | null,
): { value: T; selector: string } | null {
  let win: { value: T; selector: string; spec: number; order: number } | null = null
  for (const r of RULES) {
    if (r.media !== null || !matchesChain(r.selector, chain)) continue
    const v = blockValue(r.body)
    if (v === null) continue
    const spec = specificity(r.selector)
    if (!win || spec > win.spec || (spec === win.spec && r.order >= win.order)) {
      win = { value: v, selector: r.selector, spec, order: r.order }
    }
  }
  return win && { value: win.value, selector: win.selector }
}

/** APRS's shell wears FOUR classes, and modelling all of them is the whole point
 *  (AprsCockpit.tsx ~878, pinned below). `.layout.single.needed-panel` (styles.css ~12598)
 *  and `.layout.single.aprs-cockpit` (~3523) BOTH match it and BOTH are (0,3,0), so the
 *  later needed-panel rule is the real winner and APRS's own block never decided its
 *  overflow at all. A chain naming only `aprs-cockpit` would report that block as the
 *  winner and go green on a valve written where it can never fire — the dead-fix mechanism
 *  this file exists for, one abstraction level up. */
const APRS_CHAIN: Array<Set<string>> = [
  new Set(['app']),
  new Set(['shell']),
  new Set(['layout', 'single', 'needed-panel', 'aprs-cockpit']),
]

const SHELLS: Array<[string, Array<Set<string>>]> = [
  ['.layout.single.phone-cockpit', shellChain('phone-cockpit')],
  ['.layout.single.cw-cockpit', shellChain('cw-cockpit')],
  ['.layout.single.rtty-cockpit', shellChain('rtty-cockpit')],
  // PSK (Keyboard Modes Phase 1) rides RTTY's region-less shell rule — one
  // comma group in styles.css, but the cascade is computed per shell here.
  ['.layout.single.psk-cockpit', shellChain('psk-cockpit')],
  // JS8 (2026-09) takes CW's REGION shell rule — its own block in styles.css, computed here.
  ['.layout.single.js8-cockpit', shellChain('js8-cockpit')],
  ['.layout.single.sstv-view', shellChain('sstv-view')],
  // APRS is the sixth cockpit and was ABSENT from this census until 2026-08-04: nothing had
  // ever computed its shell, and it was the one surface in the tree with no deficit valve —
  // its header (.np-head: the frequency picker, Re-tune, the TX arm latch, Monitor, both
  // health chips and the internet control) is an unshrinkable wrapping strip sitting DIRECTLY
  // against that clip, and the internet popover it hosts is `position: absolute` with the
  // shell as its nearest non-visible ancestor, so the panel's lower rows had nowhere to go.
  ['.layout.single.needed-panel.aprs-cockpit', APRS_CHAIN],
  // Operate (2026-10-01) was the one cockpit left out: `.layout.single.operate-cockpit` was
  // `overflow: hidden`, which made its lower region (Call Roster, Band Activity, Rx Frequency,
  // Stations) the only thing that could give. At the 1024×768 floor at 100 % the panes got 49 px
  // each and showed no row; at 1366×768 at 125 % the region was 0 px. The region now floors at
  // 18em, yielding to a short window (operate-classic-grid.test.ts), the waterfall yields first,
  // and past the floor this valve scrolls. At its end it has scrolled just far enough to show
  // the floor whole, so the QSO strip with Stop TX, directly above the region, stays on screen
  // whenever the shell is taller than the floor plus the strip. Chrome kept Stop TX on screen
  // and hit-testable at every scroll position at 1024×768 at 100 % and 125 %, 1366×768 at 125 %
  // and 150 %, and 1920×1080 at 175 %. And at 1366×768 at 175 % the header alone pushed the strip
  // below the window: with `hidden` here Stop TX could not be reached with the mouse at all, and
  // now it can be scrolled to.
  ['.layout.single.operate-cockpit', shellChain('operate-cockpit')],
]

it('the APRS shell still wears the class list this census reasons about', () => {
  // The chain above is hand-modelled; a class list drifting in the component would leave it
  // guarding a shell that no longer exists. Cheap pin, same technique as the splitter
  // callers below (which read their own source) and layout-single-deficit.test.tsx.
  const src = readFileSync(
    fileURLToPath(new URL('./components/AprsCockpit.tsx', import.meta.url)),
    'utf8',
  )
  expect(
    src,
    'AprsCockpit no longer roots on `layout single needed-panel aprs-cockpit` — the ' +
      'APRS entry in SHELLS models a shell that is not rendered any more.',
  ).toContain('className="layout single needed-panel aprs-cockpit"')
})

describe('cockpit shells are the deficit valve (winning overflow-y is auto)', () => {
  for (const [name, chain] of SHELLS) {
    it(`${name} resolves overflow-y:auto after the full cascade, in every media context`, () => {
      // Contexts: the plain cascade plus each @media condition that carries a matching
      // overflow rule (e.g. `@media (max-width:900px) { .layout { overflow-y:auto } }`,
      // styles.css ~10136 — same valve direction, but a future conditional `hidden`
      // must not sneak past this guard either).
      for (const ctx of mediaContexts(chain)) {
        const win = winningOverflowY(chain, ctx)
        expect(win, `${name}: no rule declares overflow at all`).not.toBeNull()
        expect(
          win!.value,
          `${name}${ctx ? ` (within \`${ctx}\`)` : ''}: the cascade winner is ` +
            `\`${win!.selector} { overflow-y: ${win!.value} }\` — a vertical deficit CLIPS the tail ` +
            '(log form / PTT / recall) unreachably instead of scrolling. This is the exact dead-fix ' +
            'mechanism of 2026-07-30; see the header comment.',
        ).toBe('auto')
      }
    })
  }
})

/** Final min-height a block computes (in-block declaration order; nothing sets it by
 *  shorthand). Declaration-split, so a `max-height` never answers a `min-height` probe. */
function blockMinHeight(body: string): string | null {
  let v: string | null = null
  for (const decl of body.split(';')) {
    const m = /^\s*min-height\s*:\s*(\S[^]*?)\s*$/.exec(decl)
    if (m) v = m[1].replace(/\s+/g, ' ')
  }
  return v
}

describe('the hosted summary column carries its own deficit', () => {
  // 2026-09-17. The hosted Awards view grew a second content-height strip (Responsiveness)
  // BELOW the summary. Neither strip shrinks and `.awards-journey` is `flex: 1` — sized from
  // ZERO — so at 390x844 x1.75 the summary computed to height 0, its tab bar and card scroller
  // spilled out of that zero-height box, and `elementFromPoint` over an award card returned the
  // strip painted on top of it: the cards were unreachable, not merely small.
  //
  // Two declarations hold it up, and both are COMPUTED here rather than grepped, because a
  // later or more specific rule quietly taking one back is this sheet's oldest failure mode.
  // What this cannot see is whether the floor is BIG ENOUGH — that is geometry, and its gate
  // is `summary content remains reachable` in remote/test/browser.test.mjs, which is where
  // this regression was caught and where the fix was watched go green.
  const view = [
    new Set(['app', 'remote-workspace']),
    new Set(['shell']),
    new Set(['remote-insights-view']),
  ]
  const journey = [...view, new Set(['awards-journey'])]

  it('.remote-insights-view scrolls its vertical deficit instead of clipping it', () => {
    const win = winningOverflowY(view, null)
    expect(win, '.remote-insights-view: no rule declares overflow at all').not.toBeNull()
    expect(
      win!.value,
      `the cascade winner is \`${win!.selector} { overflow-y: ${win!.value} }\` — whatever sits ` +
        'at the bottom of the column is clipped out of reach on a short window.',
    ).toBe('auto')
  })

  it('the summary keeps a floor under those strips, and the floor yields with the window', () => {
    const win = winningValue(journey, blockMinHeight)
    expect(win, '.awards-journey in the hosted column declares no min-height').not.toBeNull()
    expect(
      win!.value,
      `the cascade winner is \`${win!.selector} { min-height: ${win!.value} }\` — a zero floor ` +
        'lets `flex: 1` collapse the summary to nothing between the two content-height strips.',
    ).not.toMatch(/^0[a-z%]*$/)
    expect(
      win!.value,
      `min-height is \`${win!.value}\` — a floor under a bounded parent is written to yield, ` +
        'min(Xem, share of var(--vh-eff)). A fixed floor just moves the clip to a taller window.',
    ).toContain('var(--vh-eff')
  })
})

describe('the bespoke lower regions are gone, not just de-floored', () => {
  // This block used to allow `.ph-lower` / `.cw-lower` to exist and merely policed their
  // 18em floors (= 252px at the 14px body font) — the floors that guaranteed a band of
  // empty black above an unreachable log form once the cockpit-level scroll they assumed
  // was dead. The pane-grid rebuild deleted BOTH wrappers outright: Phone and CW share
  // `.cockpit-panes`, whose no-floor contract lives in cockpit-panes.test.ts, on a sheet
  // where a floor cannot be written at all.
  //
  // So the guard is now a negative census rather than a threshold. A threshold guard
  // silently passes on a class nobody uses; this one fails the moment a per-cockpit region
  // wrapper is re-introduced — which is how every one of these floors got here.
  // `.sstv-lower` joined the census in the SSTV pane pass (census growers.md #10): its
  // 50/50 `--pane-share` split gave each pane half the region whatever it held. SSTV's
  // blocks now render through CockpitPaneFrame as direct shell children (RTTY's
  // region-less shape — two content blocks cannot fill a multi-track template), so a
  // reborn wrapper is the same private-region mechanism as the other two.
  for (const cls of ['ph-lower', 'cw-lower', 'sstv-lower']) {
    it(`.${cls} no longer exists in the sheet`, () => {
      const hits = rulesOn(cls).map((r) => r.selector)
      expect(
        hits,
        `\`.${cls}\` is back:\n${hits.join('\n')}\nA cockpit's lower region is the shared ` +
          '`.cockpit-panes` grid (cockpit-panes.css). A private wrapper is where the ' +
          'per-cockpit growers, floors and overflow flips came from the last five times.',
      ).toEqual([])
    })
  }
})

// (The '.ph-band-pane is not a grower' guard lived here. The pane-grid rebuild deleted
// the class: Band Activity now renders through a CockpitPaneFrame inside .cockpit-panes.
// The hazard it watched for is NOT unrepresentable — styles.css can still size a pane
// through the shared chrome (`.phone-cockpit .pane-frame { flex: 1 1 0 }` names no fenced
// class) — so the guard moved rather than died: cockpit-panes.test.ts now scans styles.css
// for growers/floors on `.pane-frame`/`.pane-body`, with RTTY's documented shell-owned
// frame as the one exact-selector exception.)

/** Last declaration of `prop` across unconditional rules with the exact selector (later
 *  rule wins; same-spec). Declaration-split, so `min-width`/`max-width` never match a
 *  `width` probe. `prop` is a plain property name ([a-z-] only) — no regex metachars. */
function finalDecl(selector: string, prop: string): string | null {
  let v: string | null = null
  let at = -1
  for (const r of RULES) {
    if (r.media !== null || r.selector !== selector) continue
    let last: string | null = null
    for (const decl of r.body.split(';')) {
      const m = new RegExp(`^\\s*${prop}\\s*:\\s*(\\S[^]*?)\\s*$`).exec(decl)
      if (m) last = m[1].replace(/\s+/g, ' ')
    }
    if (last !== null && r.order >= at) {
      v = last
      at = r.order
    }
  }
  return v
}

/** Last max-height across the given exact selectors (later rule wins; same-spec). */
function finalMaxHeight(selectors: string[]): string | null {
  let v: string | null = null
  let at = -1
  for (const r of RULES) {
    if (r.media !== null || !selectors.includes(r.selector)) continue
    const m = [...r.body.matchAll(/max-height\s*:\s*([^;]+)/g)]
    if (m.length && r.order >= at) {
      v = m[m.length - 1][1].trim()
      at = r.order
    }
  }
  return v
}

describe('bounded, zoom-corrected caps', () => {
  for (const scope of ['.phone-cockpit .ph-scope-panel', '.cw-cockpit .ph-scope-panel']) {
    it(`${scope} max-height is a --vh-eff cap (drag-driven height, bounded)`, () => {
      const v = finalMaxHeight([scope])
      expect(v, `${scope}: no max-height declared`).not.toBeNull()
      expect(
        v!,
        `${scope} max-height is \`${v}\` — must reference var(--vh-eff): raw vh is zoom-blind ` +
          'and `none` lets the scope swallow the window.',
      ).toContain('var(--vh-eff')
    })
  }

  it('.mv-packs max-height references var(--vh-eff) (raw 86vh over-talls under UI zoom)', () => {
    const v = finalMaxHeight(['.mv-packs'])
    expect(v, '.mv-packs: no max-height declared').not.toBeNull()
    expect(v!, `.mv-packs max-height is \`${v}\``).toContain('var(--vh-eff')
  })
})

describe('the SSTV live canvas consumes the integer-step stamp', () => {
  // The picture-upscale fix (census #9) is two halves: SstvView stamps `--sstv-img-w`
  // as a whole multiple of the decode's native width (guarded live by
  // SstvView.structure.test.tsx), and this sheet rule spends it. This computes the
  // FINAL width across every rule on the selector, so a later rule silently replacing
  // the consumer — the dead-fix mechanism this file documents — fails here, not on air.
  // The selector is exercised by live renders in SstvView.test.tsx, so this is not a
  // dead-selector presence check.
  it('.sstv-live-canvas final width is min(100%, var(--sstv-img-w, 480px))', () => {
    let v: string | null = null
    let at = -1
    for (const r of RULES) {
      if (r.media !== null || r.selector !== '.sstv-live-canvas') continue
      // Declaration-split (like blockOverflowY): a matchAll over the body would also
      // hit the tail of `max-width`/`min-width`.
      let last: string | null = null
      for (const decl of r.body.split(';')) {
        const m = /^\s*width\s*:\s*(\S[^]*?)\s*$/.exec(decl)
        if (m) last = m[1].replace(/\s+/g, ' ')
      }
      if (last !== null && r.order >= at) {
        v = last
        at = r.order
      }
    }
    expect(v, '.sstv-live-canvas: no width declared at all').not.toBeNull()
    expect(
      v!,
      `.sstv-live-canvas width is \`${v}\` — a fixed width re-caps the decode at a postage ` +
        'stamp (census #9) and anything but min(100%, var(--sstv-img-w, …)) either drops the ' +
        'integer-step stamp or loses the shrink-to-fit yield for stages smaller than 1×.',
    ).toBe('min(100%, var(--sstv-img-w, 480px))')
  })
})

describe('the SSTV stamp has a real percentage base and the shell has real floors', () => {
  // Fix-round guards (review 2026-07-31). All computed FINAL values across the sheet —
  // a later rule that silently re-breaks any of them fails here, not on air. Honest
  // limit: these read the sheet, not a layout engine; the rendered-width proof for this
  // round was measured in headless Chromium (fix evidence), and jsdom cannot repeat it.
  it(".sstv-live final width is 100% — the base of the canvas's min(100%, stamp) clamp", () => {
    // As a shrink-to-fit flex item the box's width was its own max-content, so min()
    // ALWAYS took the 100% arm: the integer stamp was inert (every window rendered ~1×)
    // and a long caption widened the box into a FRACTIONAL scale — the two outcomes the
    // sstvScale ruling exists to forbid. Stretched, 100% is the same measured stage the
    // stamp was computed against, so the integer arm can actually win.
    expect(finalDecl('.sstv-live', 'width')).toBe('100%')
  })

  it('.sstv-canvas keeps an em floor (a basis-0 grower under deficit is a zero-height stage)', () => {
    const v = finalDecl('.sstv-canvas', 'min-height')
    expect(v, '.sstv-canvas: no min-height declared').not.toBeNull()
    expect(
      /^\d+(\.\d+)?em$/.test(v!),
      `.sstv-canvas min-height is \`${v}\` — must be a positive em floor (px are ` +
        'zoom-hostile). At 0 the composer strip (~246–438px, flex 0 0 auto) starves the ' +
        'basis-0 stage to ZERO height, and the shell valve cannot scroll to a box with no ' +
        'scroll extent. The floor is safe ONLY because the shell scrolls (guard above).',
    ).toBe(true)
  })

  it('.sstv-tx-bar is sticky at bottom 0 (Stop must never leave the scrollport)', () => {
    // The dock discipline (cockpit-panes.css .cockpit-txdock): with real pane floors the
    // shell valve engages routinely, and a bar that scrolls away is a Stop the operator
    // cannot reach mid-transmission — against the bar's own TX-LOCKED comment.
    expect(finalDecl('.sstv-tx-bar', 'position')).toBe('sticky')
    expect(finalDecl('.sstv-tx-bar', 'bottom')).toBe('0')
  })

  for (const shell of [
    '.layout.single.rtty-cockpit',
    '.layout.single.psk-cockpit',
    '.layout.single.sstv-view',
  ]) {
    it(`${shell} sets the fill-frame floor knob (--cockpit-fill-min, em units)`, () => {
      // Region-less cockpits: the bare fill frame's inline `min-height:
      // var(--cockpit-fill-min, 0)` is the ONLY floor channel a sheet cannot outrank —
      // `.rtty-cockpit > .pane-frame { min-height: 10em }` shipped dead against the
      // frame's inline `min-height: 0`. cockpit-panes.test.ts fences who may set it.
      const v = finalDecl(shell, '--cockpit-fill-min')
      expect(v, `${shell}: --cockpit-fill-min not set — its fill frame floors at 0`).not.toBeNull()
      expect(/^\d+(\.\d+)?em$/.test(v!), `${shell} --cockpit-fill-min is \`${v}\``).toBe(true)
    })
  }
})

/** Final flex-shrink a block computes (longhand + shorthand, in-block order). */
function blockShrink(body: string): number | null {
  let shrink: number | null = null
  for (const decl of body.split(';')) {
    let m = /^\s*flex-shrink\s*:\s*([\d.]+)/.exec(decl)
    if (m) {
      shrink = parseFloat(m[1])
      continue
    }
    m = /^\s*flex\s*:\s*(\S[^]*?)\s*$/.exec(decl)
    if (!m) continue
    const v = m[1]
    if (v === 'none' || v === 'initial') shrink = v === 'none' ? 0 : 1
    else if (v === 'auto') shrink = 1
    else {
      const parts = v.split(/\s+/)
      // flex: <grow> [<shrink>? <basis>?] — a second NUMBER is the shrink; one value
      // or <grow> <basis> leaves shrink at its shorthand default of 1.
      shrink = parts.length >= 2 && /^[\d.]+$/.test(parts[1]) ? parseFloat(parts[1]) : 1
    }
  }
  return shrink
}

describe('a strip divider keeps its 8 px in an overfull column (winning flex-shrink is 0)', () => {
  // The divider under a scope or waterfall strip (PaneSeam, `.pane-splitter.horizontal`) is a flex
  // item of the strip's column. With the default `flex: 0 1 auto` it gave up its own height when
  // the column overran: measured in Chrome at 1024×768, Phone's thinned to 4.8 CSS px when dragged
  // past the scope's ceiling. And PaneSeam measures the strip's ceiling by painting a basis no box
  // can take, which collapsed the divider to 0.02 px and read a ceiling 8 px above the one the
  // strip really stops at. Pinned, like the split divider (`.pane-splitter.seam`): it never shrinks.
  const STRIPS: Array<[string, string[]]> = [
    ['cw-cockpit', []],
    ['operate-cockpit', ['cockpit-body']],
    // JS8's waterfall divider (layout L2), a shell child like Phone's and CW's.
    ['js8-cockpit', []],
    // RTTY's and PSK's waterfall dividers (layout L6), the same kind; and SSTV's stage divider.
    ['rtty-cockpit', []],
    ['psk-cockpit', []],
    ['sstv-view', []],
  ]
  // Phone's sits in its stage (PHONE_STAGES) and carries `in-column` (its margins, below): both states.
  const STRIP_CHAINS: Array<[string, Array<Set<string>>]> = [
    ...STRIPS.map(([shell, between]): [string, Array<Set<string>>] => [shell, [...shellChain(shell), ...between.map((c) => new Set([c])), new Set(['pane-splitter', 'horizontal'])]]),
    ...PHONE_STAGES.map(([state, stage]): [string, Array<Set<string>>] => [`phone-cockpit (${state})`, [...stage, new Set(['pane-splitter', 'horizontal', 'in-column'])]]),
  ]
  for (const [shell, chain] of STRIP_CHAINS) {
    it(`.${shell}: the strip divider resolves flex-shrink 0`, () => {
      const win = winningValue(chain, blockShrink)
      expect(win, `.${shell} .pane-splitter.horizontal: no rule declares its flex — it shrinks by default`).not.toBeNull()
      expect(
        win!.value,
        `\`${win!.selector}\` leaves the ${shell} strip divider shrinkable (flex-shrink ${win!.value}): it ` +
          'thins in an overfull column, and the strip ceiling PaneSeam measures comes out its own height too high.',
      ).toBe(0)
    })
  }
})

/** Final value of a block-axis margin side in a block (in-block order): the `margin` shorthand
 *  (1–4 values), `margin-block` (1–2) and the side's own longhand. */
function blockMargin(side: 'top' | 'bottom'): (body: string) => string | null {
  return (body: string) => {
    let v: string | null = null
    for (const decl of body.split(';')) {
      const m = /^\s*(margin|margin-block|margin-top|margin-bottom)\s*:\s*(\S[^]*?)\s*$/.exec(decl)
      if (!m) continue
      const vals = splitSpaces(m[2])
      if (m[1] === 'margin') v = side === 'top' ? vals[0] : (vals[2] ?? vals[0])
      else if (m[1] === 'margin-block') v = side === 'top' ? vals[0] : (vals[1] ?? vals[0])
      else if (m[1] === `margin-${side}`) v = vals[0]
    }
    return v
  }
}

/** Split a value list at top-level spaces (parens-aware), so `calc(a - b) 0` is two values. */
function splitSpaces(v: string): string[] {
  const out: string[] = []
  let depth = 0
  let cur = ''
  for (const c of v.trim()) {
    if (c === '(') depth++
    else if (c === ')') depth--
    if (/\s/.test(c) && depth === 0) {
      if (cur) out.push(cur)
      cur = ''
    } else cur += c
  }
  if (cur) out.push(cur)
  return out
}

/** A length written with the spacing ladder, in px at a `--space-scale`: px, `var(--space-N)` as
 *  :root declares it, and calc() arithmetic over them. Anything else is null (the caller fails). */
function spacePx(v: string, scale: number): number | null {
  let expr = v
  for (let i = 0; i < 4 && /var\(/.test(expr); i++) {
    expr = expr
      .replace(/var\(\s*--space-scale\s*\)/g, String(scale))
      .replace(/var\(\s*(--space-\d)\s*\)/g, (_, name: string) => `(${finalDecl(':root', name) ?? 'NaN'})`)
  }
  expr = expr.replace(/calc\(/g, '(').replace(/(\d)px\b/g, '$1')
  if (!/^[\d.\s()+\-*/]+$/.test(expr)) return null
  return new Function(`return (${expr})`)() as number
}

describe('a y divider costs its column nothing: its margins give back the gap it adds', () => {
  // Each divider is one more flex child of its column, so it adds one more gap. Its own box nets 0
  // (8 px tall, -4 px each side), and its block margins also take back that gap, so it sits in the
  // gap its neighbours already had. Measured in Chrome before this: RTTY's log strip lost 23 px at
  // 1024×768 to two dividers, and SSTV's growing stage lost 6 px at 2560×1440, which dropped a
  // picture in flight from 4× to 3× (layout L6); Phone, CW, JS8 and Operate took the same rule in
  // L5 (ruling D-C). Computed at the spacing scale the dividers render at, 1: `--space-scale` drops
  // below 1 only at the sm/xs viewports, and those hide every divider
  // (`[data-viewport='sm'] .pane-splitter { display: none }`).
  const gapOf = (b: string) => {
    let g: string | null = null
    for (const decl of b.split(';')) {
      const m = /^\s*(gap|row-gap)\s*:\s*(\S[^]*?)\s*$/.exec(decl)
      if (m) g = splitSpaces(m[2])[0]
    }
    return g
  }
  /** A pane-grid column's gap. `.cockpit-col` lives in cockpit-panes.css, which this file does not
   *  parse and styles.css may not name; its one rule is a flat selector, so its gap is read there. */
  const columnGap = (): { value: string; selector: string } | null => {
    const sheet = readFileSync(fileURLToPath(new URL('./cockpit-panes.css', import.meta.url)), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '')
    let v: string | null = null
    for (const m of sheet.matchAll(/(^|})\s*\.cockpit-col\s*\{([^}]*)\}/g)) v = gapOf(m[2]) ?? v
    return v == null ? null : { value: v, selector: '.cockpit-col (cockpit-panes.css)' }
  }
  const OPERATE = shellChain('operate-cockpit')
  const BODY = [...OPERATE, new Set(['cockpit-body'])]
  const SIDE = [...BODY, new Set(['cockpit-lower', 'roster']), new Set(['cockpit-side'])]
  const QSOCOL = [...BODY, new Set(['cockpit-lower', 'classic']), new Set(['cockpit-qsocol'])]
  /** [what, the column's chain (null: a pane-grid column), the divider's own classes] */
  const CASES: Array<[string, Array<Set<string>> | null, Array<Set<string>>, string[]]> = [
    ...['rtty-cockpit', 'psk-cockpit', 'sstv-view', 'cw-cockpit', 'js8-cockpit'].map(
      (shell): [string, Array<Set<string>>, Array<Set<string>>, string[]] => [`.${shell}`, shellChain(shell), shellChain(shell), ['pane-splitter', 'horizontal']],
    ),
    // PHONE'S SCOPE DIVIDER (2026-10-03) stands in the stage (PHONE_STAGES) and gives back a column's
    // gap by `in-column`, like every divider whose parent this sheet may not name. With no side the
    // flat wrappers have no box, so the column whose gap it sits in is the SHELL; with the side shown
    // it is the stage, whose gap cockpit-panes.css declares (`.cockpit-stage`).
    [
      '.phone-cockpit (no side)',
      shellChain('phone-cockpit'),
      [...PHONE_STAGES[0][1]],
      ['pane-splitter', 'horizontal', 'in-column'],
    ],
    ['.phone-cockpit (left side shown)', null, [...PHONE_STAGES[1][1]], ['pane-splitter', 'horizontal', 'in-column']],
    ...['phone-cockpit', 'js8-cockpit'].map(
      (shell): [string, null, Array<Set<string>>, string[]] => [
        `.${shell} .cockpit-col`,
        null,
        [...shellChain(shell), new Set(['cockpit-panes']), new Set(['cockpit-col'])],
        ['pane-splitter', 'horizontal', 'seam', 'in-column'],
      ],
    ),
    ['.operate-cockpit .cockpit-body', BODY, BODY, ['pane-splitter', 'horizontal']],
    ['.operate-cockpit .cockpit-side', SIDE, SIDE, ['pane-splitter', 'horizontal', 'seam']],
    ['.operate-cockpit .cockpit-qsocol', QSOCOL, QSOCOL, ['pane-splitter', 'horizontal']],
    // FT'S ARRANGED COLUMNS (2026-10-07) are cockpit-panes.css's (`.op-stack`, FT's own 8 px gap), which this
    // sheet may not name, so the divider between two of their feeds gives the gap back by `in-op-stack`.
    [
      '.operate-cockpit .op-stack',
      null,
      [...BODY, new Set(['cockpit-lower', 'roster']), new Set(['op-stack'])],
      ['pane-splitter', 'horizontal', 'seam', 'in-op-stack'],
    ],
  ]
  /** `.cockpit-stage`'s gap, read where it lives (cockpit-panes.css, a flat selector), as the
   *  pane grid's columns' is. */
  const stageGap = (): { value: string; selector: string } | null => {
    const sheet = readFileSync(fileURLToPath(new URL('./cockpit-panes.css', import.meta.url)), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '')
    let v: string | null = null
    for (const m of sheet.matchAll(/(^|})\s*\.cockpit-stage\s*\{([^}]*)\}/g)) v = gapOf(m[2]) ?? v
    return v == null ? null : { value: v, selector: '.cockpit-stage (cockpit-panes.css)' }
  }
  /** FT's arranged columns' gap, read where it lives (cockpit-panes.css, a flat selector). */
  const opStackGap = (): { value: string; selector: string } | null => {
    const sheet = readFileSync(fileURLToPath(new URL('./cockpit-panes.css', import.meta.url)), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '')
    let v: string | null = null
    for (const m of sheet.matchAll(/(^|})\s*\.op-stack\s*\{([^}]*)\}/g)) v = gapOf(m[2]) ?? v
    return v == null ? null : { value: v, selector: '.op-stack (cockpit-panes.css)' }
  }
  for (const [what, columnChain, parent, own] of CASES) {
    it(`${what}: a divider's height, both margins and the column's gap sum to zero`, () => {
      const chain = [...parent, new Set(own)]
      const gap = columnChain
        ? winningValue(columnChain, gapOf)
        : what.includes('left side')
          ? stageGap()
          : what.includes('op-stack')
            ? opStackGap()
            : columnGap()
      const height = winningValue(chain, blockLonghand('height'))
      const top = winningValue(chain, blockMargin('top'))
      const bottom = winningValue(chain, blockMargin('bottom'))
      expect(gap && height && top && bottom, `${what}: gap, height or a margin is not declared`).toBeTruthy()
      const px = [gap!, height!, top!, bottom!].map((w) => spacePx(w.value, 1))
      expect(px.every((x) => x !== null), `unreadable: ${[gap, height, top, bottom].map((w) => w!.value).join(' | ')}`).toBe(true)
      const [g, h, t, b] = px as number[]
      expect(
        h + t + b + g,
        `${what}: the divider takes ${(h + t + b + g).toFixed(2)} px of the column (height ${h}, margins ` +
          `${t} / ${b} from \`${top!.selector}\` / \`${bottom!.selector}\`, gap ${g} from \`${gap!.selector}\`): ` +
          'a layout nobody has divided is no longer the one it was.',
      ).toBeCloseTo(0, 6)
    })
  }
})

describe('the scope splitter drag is respected (winning flex-grow is 0)', () => {
  // The Splitter (PhoneCockpit.tsx ~728 / CwCockpit) drives --ph-scope-h / --cw-scope-h
  // as the scope's flex-BASIS. A basis only sets the rendered height while flex-grow
  // is 0: with grow 1 on the column's only grower the resolved height is
  // clamp(shellH − siblings, floor, cap) at EVERY basis — algebraically independent of
  // the drag — so the operator's live control (and the persisted %) silently did
  // nothing (review 2026-07-31; CW split the surplus 1:1 with `.cw-lower` and tracked
  // the pointer at half rate instead). Operate is the pattern: `.cockpit-waterfall
  // { flex: 0 1 var(--cockpit-wf-h, 22%) }` with the decode scroller as the grower.
  // JS8's (layout L2), RTTY's and PSK's (L6) waterfall dividers drive the same kind of basis.
  for (const shell of ['js8-cockpit', 'rtty-cockpit', 'psk-cockpit']) {
    it(`.${shell} .waterfall-wrap resolves flex-grow 0 (its waterfall divider)`, () => {
      const win = winningValue([...shellChain(shell), new Set(['waterfall-wrap'])], blockGrow)
      expect(win, `.${shell} .waterfall-wrap: no rule declares flex at all`).not.toBeNull()
      expect(
        win!.value,
        `\`${win!.selector}\` gives the ${shell} waterfall flex-grow ${win!.value}: its divider drives the ` +
          'flex-basis, which only sets the rendered height while grow is 0.',
      ).toBe(0)
    })
  }

  // SSTV's stage (layout L6) is the column's GROWER until the operator sizes it — the one shape
  // that follows that view at every window — and only then takes the strip shape, on the
  // `data-sized` its divider sets. Both halves: the sized stage follows the basis, and the stock
  // stage is still the grower (a sized rule leaking onto it would fix the stock picture's size).
  it('.sstv-view .sstv-canvas[data-sized] resolves flex-grow 0, and the stock stage keeps its grower', () => {
    const sized = winningValue([...shellChain('sstv-view'), new Set(['sstv-canvas', '[data-sized]'])], blockGrow)
    expect(sized, '.sstv-canvas[data-sized]: no rule declares flex at all').not.toBeNull()
    expect(
      sized!.value,
      `\`${sized!.selector}\` gives the sized SSTV stage flex-grow ${sized!.value}: its divider drives the ` +
        'flex-basis, which only sets the rendered height while grow is 0.',
    ).toBe(0)
    const stock = winningValue([...shellChain('sstv-view'), new Set(['sstv-canvas'])], blockGrow)
    expect(stock, '.sstv-canvas: no rule declares flex at all').not.toBeNull()
    expect(
      stock!.value,
      `\`${stock!.selector}\` gives the stock SSTV stage flex-grow ${stock!.value}: until the operator sizes ` +
        'it the stage must grow with the view, or the picture loses the upscale a tall window gave it.',
    ).toBeGreaterThan(0)
  })

  // Operate Classic's Tx1–Tx6 machine (layout L5) keeps its content height until its divider sizes
  // it; sized, grow 0 as every strip, a floor that yields, and never taller than its rows.
  it('.cockpit-qsocol > .tx-panel[data-sized]: grow 0, a yielding 4em floor, capped at its content', () => {
    const qsocol = [...shellChain('operate-cockpit'), new Set(['cockpit-body']), new Set(['cockpit-lower', 'classic']), new Set(['cockpit-qsocol'])]
    const sized = [...qsocol, new Set(['tx-panel', 'panel', 'tx-panel-compact', '[data-sized]'])]
    const stock = [...qsocol, new Set(['tx-panel', 'panel', 'tx-panel-compact'])]
    expect(winningValue(sized, blockGrow)?.value, 'the dragged basis would not be what renders').toBe(0)
    expect(winningValue(stock, blockLonghand('min-height'))?.selector, 'the sized floor leaked onto the stock machine').not.toBe('.cockpit-qsocol > .tx-panel[data-sized]')
    const floor = winningValue(sized, blockLonghand('min-height'))!
    expect(floor.value, `\`${floor.selector}\``).toBe('min(4em, 100%)')
    const cap = winningValue(sized, blockLonghand('max-height'))!
    expect(cap.value, `\`${cap.selector}\`: a sized machine could grow past its own rows`).toBe('max-content')
    // Its divider's floor is the sheet's (the 4em arm, where the yield does not bind).
    for (const fontPx of [14, 16]) expect(resolveClamp(TX_SPLIT_MIN, { fontPx, vhEff: 768 })).toBe(4 * fontPx)
  })

  const SCOPES: Array<[string, Array<Set<string>>]> = [
    ['cw-cockpit', shellChain('cw-cockpit')],
    ...PHONE_STAGES.map(([state, stage]): [string, Array<Set<string>>] => [`phone-cockpit (${state})`, stage]),
  ]
  for (const [shell, host] of SCOPES) {
    it(`.${shell} .ph-scope-panel resolves flex-grow 0 (grow ≥1 voids the dragged basis)`, () => {
      const chain = [...host, new Set(['ph-scope-panel'])]
      const win = winningValue(chain, blockGrow)
      expect(win, `.${shell} .ph-scope-panel: no rule declares flex at all`).not.toBeNull()
      expect(
        win!.value,
        `\`${win!.selector}\` gives the ${shell} scope flex-grow ${win!.value} — as the column's ` +
          'grower its height no longer follows the flex-basis the Splitter drives, so the drag ' +
          'and the persisted % are inert. Keep grow 0; pick the surplus sink deliberately.',
      ).toBe(0)
    })
  }
})

describe('the dock rows that key the rig cannot shrink (winning flex-shrink is 0)', () => {
  // The TX dock itself is `flex: 0 0 auto` (cockpit-panes.css, guarded there) — but that
  // is ONE rule in ANOTHER file, and the rebuild briefly shipped the rows beneath it as
  // default `flex: 0 1 auto` shrink victims on the strength of it. These compute the
  // winner for each row that keys the rig, so loosening either layer alone fails a test
  // (fix-round D4, 2026-07-31). Chains include .cockpit-txdock as the parent; its own
  // sizing rules live in the other sheet, which this scan deliberately cannot see.

  const ROWS: Array<[string, string, string[]]> = [
    ['phone-cockpit', 'ph-ptt-row', ['ph-ptt-row']],
    ['cw-cockpit', 'cw-macros', ['cw-macros']],
    ['cw-cockpit', 'cw-send', ['cw-send']],
    // RTTY's F1–F8 and its fixed row (Call, set switch, TX latch, Esc/Stop) sit in ONE dock child,
    // `.rtty-dock-keys`, which lays them on one line or two by width — so that wrapper is the row
    // the dock's column can squeeze, and the one that carries the pin.
    ['rtty-cockpit', 'rtty-dock-keys (F1–F8, Call, TX latch, Esc/Stop)', ['rtty-dock-keys']],
    ['rtty-cockpit', 'cw-macros.rtty-auto-row (auto-sequencer)', ['cw-macros', 'rtty-auto-row']],
    ['rtty-cockpit', 'cw-send (compose bar)', ['cw-send']],
    // PSK, which had NO entry here at all until #316 — the one keyboard cockpit whose dock
    // rows nothing pinned, while Phone's, CW's, RTTY's and JS8's all were. Its dock took
    // RTTY's `.rtty-dock-keys` structure with that change (the eight keys on one line with
    // the Call box, set switch, TX latch and Esc/Stop, folding to two by width), so it is the
    // same row that must not squeeze — and listing it here is what makes a future
    // shell-scoped rule that wins for one cockpit and not the other visible.
    ['psk-cockpit', 'rtty-dock-keys (F1–F8, Call, TX latch, Esc/Stop)', ['rtty-dock-keys']],
    ['psk-cockpit', 'cw-send (compose bar)', ['cw-send']],
    ['js8-cockpit', 'js8-dock-row (compose + beacon rows)', ['js8-dock-row']],
  ]
  for (const [shell, name, rowClasses] of ROWS) {
    it(`.${shell} dock ${name} resolves flex-shrink 0`, () => {
      const chain = [...shellChain(shell), new Set(['cockpit-txdock']), new Set(rowClasses)]
      const win = winningValue(chain, blockShrink)
      expect(win, `${name} in .${shell}: no rule declares flex at all — the pin is gone`).not.toBeNull()
      expect(
        win!.value,
        `\`${win!.selector}\` leaves ${name} shrinkable (flex-shrink ${win!.value}) — under ` +
          'deficit the control that keys the rig squeezes before anything scrolls.',
      ).toBe(0)
    })
  }
})

describe('Journey cards win their row direction against .panel', () => {
  // `<div className="jy-marathon panel">` / `<section className="jy-hero panel">`
  // (JourneyView.tsx ~60/~124): `.panel { flex-direction: column }` also targets the
  // element, so the card's `row` must actually WIN the cascade — a bare (0,1,0)
  // `.jy-marathon` earlier in the sheet loses to the later (0,1,0) `.panel` and ships
  // dead, the exact mechanism this file exists to catch (review 2026-07-31).
  for (const card of ['jy-marathon', 'jy-hero']) {
    it(`.${card}.panel resolves flex-direction row`, () => {
      // journey-view is a true ancestor of both cards (jy-marathon sits below an
      // unclassed wrapper too — descendant matching walks past it).
      const chain = [
        new Set(['app']),
        new Set(['shell']),
        new Set(['layout', 'single']),
        new Set(['journey-view']),
        new Set([card, 'panel']),
      ]
      const win = winningValue(chain, blockFlexDirection)
      expect(win, `.${card}: no rule declares flex-direction at all`).not.toBeNull()
      expect(
        win!.value,
        `the cascade winner is \`${win!.selector} { flex-direction: ${win!.value} }\` — the ` +
          `card stacks vertically and the .${card} row rule is dead. Outrank .panel ` +
          `(e.g. \`.panel.${card}\`) instead of relying on source order.`,
      ).toBe('row')
    })
  }
})

describe('Connect strip cap caps the PANES, not the grid track', () => {
  // css-grid §11.6 (maximize tracks) grows ANY fixed-max track to its growth limit
  // before the fr rows expand — the min sizing function is irrelevant — so both
  // `minmax(0, X)` and `minmax(auto, X)` FLOOR the strip at its full X on a tall
  // window: a "cap" that always pays out, stealing X from the globe's 1fr rows
  // (verified empirically in Chrome, review 2026-07-31; the census' prescribed
  // `minmax(auto, …)` spelling was wrong). The working shape: the track is `auto`
  // (content-sized) and the ceiling lives on the strip's pane children as max-height,
  // which DOES bound a box — a tall pane scrolls inside `.pane-body`.
  // Since 2026-09-13 (Connect close + resize) the strip rides an IMPLICIT row: the template
  // declares the map row alone and `grid-auto-rows` sizes the strip's, so a closed strip leaves
  // no track and no gap. The guard is the same claim one property over — the strip's track
  // must be `auto`, never a fixed max — plus "exactly one explicit row", because a second
  // explicit row is where a fixed max would come back.
  it('.connect strip row is auto (no fixed max — it would always pay out)', () => {
    const lastOn = (prop: string): string | null => {
      let v: string | null = null
      let at = -1
      for (const r of RULES) {
        if (r.media !== null || r.selector !== '.connect') continue
        const m = [...r.body.matchAll(new RegExp(`(?:^|;|\\s)${prop}\\s*:\\s*([^;]+)`, 'g'))]
        if (m.length && r.order >= at) {
          v = m[m.length - 1][1].trim()
          at = r.order
        }
      }
      return v
    }
    const rows = lastOn('grid-template-rows')
    const auto = lastOn('grid-auto-rows')
    expect(rows, '.connect: no grid-template-rows declared').not.toBeNull()
    // …unless the OPERATOR has sized the strip (layout L7): its divider writes `--cn-strip-h`, and
    // the row is then exactly the height asked for — not a cap that pays out. Unset, the row's max is
    // the `auto` this guard has always required, over the 4em floor that keeps a pane's title bar
    // below the floor (connect-layout.test.ts computes it); any other spelling (a fixed max, a bare
    // length, a different fallback) still fails here.
    expect(
      auto,
      '.connect: the strip row is implicit, so grid-auto-rows IS its track — and unsized its max must be auto',
    ).toBe('var(--cn-strip-h, minmax(4em, auto))')
    // Paren-aware top-level track split (minmax(a, b) is one track).
    const tracks: string[] = []
    let depth = 0
    let cur = ''
    for (const ch of rows!) {
      if (ch === '(') depth++
      if (ch === ')') depth--
      if (/\s/.test(ch) && depth === 0) {
        if (cur) tracks.push(cur)
        cur = ''
      } else cur += ch
    }
    if (cur) tracks.push(cur)
    expect(
      tracks,
      `.connect rows are \`${rows}\` — the map row alone is explicit, flexible over the rails' floor. ` +
        'A second explicit row is where a fixed max would return: it is maximized to its full value ' +
        'before the fr row expands (§11.6), so it is a floor, not a cap.',
    ).toEqual(['minmax(max(calc(8em + var(--space-3)), min(calc(10em + var(--space-3)), calc(100% - 4em - var(--space-3)))), 1fr)'])
  })

  it('.connect-strip > .pane-frame carries the zoom-corrected max-height cap', () => {
    const v = finalMaxHeight(['.connect-strip > .pane-frame'])
    expect(v, '.connect-strip > .pane-frame: no max-height — the strip is unbounded').not.toBeNull()
    expect(v!, `cap is \`${v}\``).toContain('var(--vh-eff')
  })
})

/** A box whose computed overflow-y makes it a scroll container. */
const SCROLLS = (v: string) => v === 'auto' || v === 'scroll'

/** Final scrollbar-width a block computes (in-block declaration order). */
function blockScrollbarWidth(body: string): string | null {
  let v: string | null = null
  for (const decl of body.split(';')) {
    const m = /^\s*scrollbar-width\s*:\s*(\S[^]*?)\s*$/.exec(decl)
    if (m) v = m[1]
  }
  return v
}

/** The Connect grid's ancestor chain (ConnectView.tsx ~314: `<main class="layout single">`
 *  → `.connect-shell` → `.connect`). Connect's shell carries no cockpit class. */
const CONNECT_HOST: Array<Set<string>> = [
  new Set(['app']),
  new Set(['shell']),
  new Set(['layout', 'single']),
  new Set(['connect-shell']),
  new Set(['connect']),
]

/** Every place the SHARED `.pane-frame`/`.pane-body` family actually mounts — Connect's
 *  PaneFrame (rails + bottom strip) and CockpitPaneFrame (region columns, and the bare
 *  shell children of the two region-less cockpits). */
const PANE_HOSTS: Array<[string, Array<Set<string>>]> = [
  // Rail frames sit in a `.connect-rail` column since the close + resize work (2026-09-13).
  ['Connect rail', [...CONNECT_HOST, new Set(['connect-rail'])]],
  ['Connect bottom strip', [...CONNECT_HOST, new Set(['connect-strip'])]],
  ...PHONE_STAGES.map(([state, stage]): [string, Array<Set<string>>] => [
    `Phone pane region (${state})`,
    [...stage, new Set(['cockpit-panes']), new Set(['cockpit-col'])],
  ]),
  // Phone's LEFT SIDE (2026-10-03): its frames stand in the side's own column, beside the stage.
  ['Phone left side', [...shellChain('phone-cockpit'), new Set(['cockpit-leftrow']), new Set(['cockpit-left']), new Set(['cockpit-left-col'])]],
  [
    'CW pane region',
    [...shellChain('cw-cockpit'), new Set(['cockpit-panes']), new Set(['cockpit-col'])],
  ],
  ['RTTY (region-less, bare shell child)', shellChain('rtty-cockpit')],
  ['SSTV (region-less, bare shell child)', shellChain('sstv-view')],
]

const paneBodyChain = (host: Array<Set<string>>) => [
  ...host,
  new Set(['pane-frame']),
  new Set(['pane-body']),
]

describe('the shared pane body is the first legal fate of vertical deficit', () => {
  // `.pane-body { flex:1; min-height:0; overflow:auto }` (styles.css ~1583) is the whole
  // per-pane scroll contract — every cockpit and Connect depend on it, and CockpitPaneFrame
  // refuses a className precisely so a pane cannot opt out. Yet nothing COMPUTED it: the
  // shells' valve is guarded above, the region tiers in cockpit-panes.test.ts, and the only
  // test that touched this block was a regex-presence match on `scrollbar-width: thin` in
  // connectLayout.test.ts — the exact form CLAUDE.md forbids, because a dead selector passes
  // it. A later `.phone-cockpit .pane-body { overflow: hidden }` is (0,2,0) against the base
  // (0,1,0): every Phone pane would CLIP instead of scroll and the whole suite stayed green.
  //
  // Scope note, and it is a composition rather than a gap: this scan reads styles.css only,
  // and the other sheet cannot reach these boxes at all — cockpit-panes.test.ts ('panes are
  // sized by the grid, never by themselves') fails the build if cockpit-panes.css so much as
  // names .pane-frame/.pane-head/.pane-body. Between the two, the family's cascade is closed.
  for (const [name, host] of PANE_HOSTS) {
    const chain = paneBodyChain(host)

    it(`${name}: .pane-body resolves a scrolling overflow-y in every media context`, () => {
      for (const ctx of mediaContexts(chain)) {
        const win = winningOverflowY(chain, ctx)
        expect(win, `${name}: no rule declares overflow on .pane-body at all`).not.toBeNull()
        expect(
          SCROLLS(win!.value),
          `${name}${ctx ? ` (within \`${ctx}\`)` : ''}: the cascade winner is ` +
            `\`${win!.selector} { overflow-y: ${win!.value} }\` — the pane CLIPS. Deficit inside a ` +
            'pane has exactly one legal fate and this is it; with the body clipping, the pane ' +
            'edge is a hard cut with no scrollbar, and neither the column valve nor the shell ' +
            'valve can reach content inside a box that has no scroll extent.',
        ).toBe(true)
      }
    })

    it(`${name}: .pane-body keeps a visible scrollbar affordance`, () => {
      // What the retired connectLayout regex meant to assert, computed: a scroller the
      // operator cannot SEE reads as clipped content. `thin` is the shipped value; `none`
      // is the failure this replaces a presence-match with a cascade winner to catch.
      const win = winningValue(chain, blockScrollbarWidth)
      expect(win, `${name}: no rule declares scrollbar-width on .pane-body`).not.toBeNull()
      expect(
        win!.value,
        `${name}: the cascade winner is \`${win!.selector} { scrollbar-width: ${win!.value} }\` — ` +
          'the pane scrolls with no visible affordance, which reads to the operator as content ' +
          'that simply ends.',
      ).not.toBe('none')
    })
  }
})

// ===========================================================================
// The band-scope strip: who owns its size, and does the drag reach both ends
// ===========================================================================
// `.ph-scope-panel` is rendered by exactly two hosts (PhoneCockpit.tsx ~1027,
// CwCockpit.tsx ~1203) and BOTH override the base rule's size at (0,2,0). The base's
// `flex/height/min-height/max-height` were therefore dead — but a dead rule carrying a
// px floor AND a px ceiling is a loaded gun: a third host rendering the strip without a
// cockpit prefix would silently be pinned 120–220 px, at every window and every zoom.
// So size is HOST-owned here, and these compute both halves of that claim.

/** Final value of one longhand in a block (in-block declaration order). Anchored, so a
 *  `height` probe never matches `min-height`/`max-height`. */
function blockLonghand(prop: string): (body: string) => string | null {
  const re = new RegExp(`^\\s*${prop}\\s*:\\s*(\\S[^]*?)\\s*$`)
  return (body: string) => {
    let v: string | null = null
    for (const decl of body.split(';')) {
      const m = re.exec(decl)
      if (m) v = m[1].replace(/\s+/g, ' ')
    }
    return v
  }
}

/** A reference geometry a length is resolved against: `fontPx` is the strip's computed
 *  font size (what `em` means) and `vhEff` is `--vh-eff` in CSS px (what the sheet's
 *  `calc(f * var(--vh-eff))` caps mean). Two of these with different numbers is what
 *  separates a geometry-relative clamp from a px constant that merely happens to agree
 *  at one window size. */
interface Geom {
  fontPx: number
  vhEff: number
}

/** Split a comma list at top level (parens-aware). */
function splitTop(s: string): string[] {
  const out: string[] = []
  let depth = 0
  let start = 0
  for (let i = 0; i < s.length; i++) {
    const c = s[i]
    if (c === '(') depth++
    else if (c === ')') depth--
    else if (c === ',' && depth === 0) {
      out.push(s.slice(start, i))
      start = i + 1
    }
  }
  out.push(s.slice(start))
  return out
}

/** Resolve a CSS length THIS SHEET writes for the scope/waterfall strips to CSS px at
 *  `g`. Handles `<n>px`, `<n>em`, `calc(<f> * var(--vh-eff…))` and `min()`/`max()` over
 *  them. Anything else returns null — the caller then fails loudly rather than a guard
 *  passing on a value it could not read. */
function lengthPx(v: string, g: Geom): number | null {
  const s = v.trim()
  for (const [fn, pick] of [
    ['min', Math.min],
    ['max', Math.max],
  ] as Array<[string, (...n: number[]) => number]>) {
    const m = new RegExp(`^${fn}\\(([^]*)\\)$`).exec(s)
    if (!m) continue
    const parts = splitTop(m[1]).map((p) => lengthPx(p, g))
    return parts.every((x) => x !== null) ? pick(...(parts as number[])) : null
  }
  let m = /^(-?[\d.]+)px$/.exec(s)
  if (m) return parseFloat(m[1])
  m = /^(-?[\d.]+)em$/.exec(s)
  if (m) return parseFloat(m[1]) * g.fontPx
  m = /^calc\(\s*(-?[\d.]+)\s*\*\s*var\(\s*--vh-eff\s*(?:,[^)]*)?\)\s*\)$/.exec(s)
  if (m) return parseFloat(m[1]) * g.vhEff
  return null
}

/** The two hosts of the strip, as DOM chains — Phone's in both of its stage's states. */
const SCOPE_HOSTS: Array<[string, () => Array<Set<string>>]> = [
  ...PHONE_STAGES.map(([state, stage]): [string, () => Array<Set<string>>] => [`phone-cockpit (${state})`, () => stage]),
  ['cw-cockpit', () => shellChain('cw-cockpit')],
]
const SIZE_PROPS = ['flex', 'height', 'min-height', 'max-height']

describe('the band-scope strip is sized by its HOST, never by the shared base rule', () => {
  for (const prop of SIZE_PROPS) {
    it(`.ph-scope-panel (base) declares no ${prop}`, () => {
      const v = finalDecl('.ph-scope-panel', prop)
      expect(
        v,
        `\`.ph-scope-panel { ${prop}: ${v} }\` is a structural size on the SHARED base rule. ` +
          'Both shipped hosts override it at (0,2,0), so it renders nowhere today — but it is ' +
          'live for any future host that forgets the cockpit prefix, and a px floor/ceiling ' +
          'pinned there is invisible until an operator reports it. The size belongs to the ' +
          'cockpit rules below; the base keeps chrome only.',
      ).toBeNull()
    })
  }

  for (const [shell, host] of SCOPE_HOSTS) {
    const chain = () => [...host(), new Set(['ph-scope-panel'])]

    it(`.${shell} .ph-scope-panel declares the whole size itself`, () => {
      for (const prop of SIZE_PROPS.filter((p) => p !== 'height')) {
        const win = winningValue(chain(), blockLonghand(prop))
        expect(
          win,
          `.${shell} .ph-scope-panel: nothing declares ${prop}. With the base rule retired ` +
            'the host is the only owner — an unfloored scope collapses to the drag basis alone.',
        ).not.toBeNull()
        expect(
          win!.selector,
          `.${shell} .ph-scope-panel ${prop} comes from \`${win!.selector}\` — the shared base ` +
            'rule must not size this strip.',
        ).not.toBe('.ph-scope-panel')
      }
    })

    it(`.${shell} .ph-scope-panel clamps scale with the geometry (no px floor/ceiling)`, () => {
      // A px clamp is the defect in one word: it means the same absolute height at
      // 1440-tall and at 175%-pinned zoom, where the same strip is half the window.
      for (const prop of ['min-height', 'max-height']) {
        const v = winningValue(chain(), blockLonghand(prop))!.value
        const a = lengthPx(v, { fontPx: 14, vhEff: 768 })
        const b = lengthPx(v, { fontPx: 28, vhEff: 1536 })
        expect(a, `.${shell} .ph-scope-panel ${prop} is \`${v}\` — unreadable as a length`).not.toBeNull()
        expect(
          b! / a!,
          `.${shell} .ph-scope-panel ${prop} is \`${v}\`: doubling BOTH the font size and ` +
            `--vh-eff leaves it at ${b}px (was ${a}px). A clamp that does not move with the ` +
            'geometry is a px pin — the same absolute strip at 1440-tall and at 175% zoom.',
        ).toBeCloseTo(2, 6)
      }
    })
  }
})

describe('the RTTY/PSK waterfall floor YIELDS (and is the lower end of its divider)', () => {
  // `.rtty-cockpit .waterfall-wrap` shares the strip shape with `.ph-scope-panel`, and
  // carried the same `min-height: 120px` — and until layout L6 RTTY gave the operator no drag
  // handle, so on a short or pinned-zoom window that floor was unrecoverable: the strip simply
  // took its 120 px out of a window that has ~400 to spend. It has a divider now, but a divider
  // cannot go under the floor (it is WATERFALL_SPLIT_MIN), so the floor must still yield. The
  // shell valve keeps it from TRAPPING anything, which is why this is a share bug and not a
  // safety bug. PSK shares the strip rule (one comma group), so it is computed separately
  // here — a split of that group could regress one alone.
  for (const shell of ['rtty-cockpit', 'psk-cockpit']) {
    const chain = [...shellChain(shell), new Set(['waterfall-wrap'])]

    it(`.${shell} resolves a min-height at all (the canvas needs real device pixels)`, () => {
      const win = winningValue(chain, blockLonghand('min-height'))
      expect(win, `.${shell} .waterfall-wrap: nothing declares min-height`).not.toBeNull()
      // Not "delete the floor" — at a normal window the strip must still be a strip.
      const v = win!.value
      expect(
        lengthPx(v, { fontPx: 14, vhEff: 768 }),
        `.${shell} .waterfall-wrap min-height is \`${v}\` — at a 768 window the floor must ` +
          'still keep a drawable strip (≥ 6em).',
      ).toBeGreaterThanOrEqual(6 * 14)
    })

    it(`.${shell} never claims more than 30% of the effective viewport`, () => {
      const v = winningValue(chain, blockLonghand('min-height'))!.value
      for (const vhEff of [1440, 768, 439, 384, 300]) {
        const floor = lengthPx(v, { fontPx: 14, vhEff })
        expect(floor, `min-height \`${v}\` is unreadable as a length`).not.toBeNull()
        expect(
          floor!,
          `.${shell} .waterfall-wrap min-height is \`${v}\` = ${floor}px at --vh-eff ${vhEff} ` +
            `— ${((100 * floor!) / vhEff).toFixed(0)}% of the window for a glance strip, with ` +
            'a floor its divider cannot go under. Write the floor to yield: min(Xem, share).',
        ).toBeLessThanOrEqual(0.3 * vhEff)
      }
    })
  }
})

describe('beside the left side the scope counts at its FLOOR, not its content (2026-10-03)', () => {
  // With the side shown, the row's automatic minimum is the stage's INTRINSIC height, and a column's
  // intrinsic height takes each child at its preferred size: the scope counted at its content height
  // (134 px on a 1280×720 window) instead of the 8em it shrinks to under deficit, so the shell scrolled
  // 22 px further and the TX strip sat 22 px lower than without the side — measured in Chrome. Size
  // containment makes its intrinsic size the floor; laid out, it is still its basis, clamped. Only in
  // that state: the stock shell resolves the scope's basis against the shell, and nothing changes there.
  const blockContain = blockLonghand('contain')
  it('with the side shown the scope is size-contained', () => {
    const chain = [...PHONE_STAGES[1][1], new Set(['ph-scope-panel', 'ph-scope-panel--beside'])]
    const win = winningValue(chain, blockContain)
    expect(win, 'nothing contains the scope beside the side').not.toBeNull()
    expect(win!.value.split(/\s+/), `\`${win!.selector} { contain: ${win!.value} }\``).toContain('size')
  })
  it('without the side nothing contains it (control: the stock and hosted layouts are untouched)', () => {
    for (const host of [PHONE_STAGES[0][1], shellChain('cw-cockpit')]) {
      expect(winningValue([...host, new Set(['ph-scope-panel'])], blockContain)).toBeNull()
    }
  })
})

describe('THE RF SCOPE PANE beside FT’s waterfall turns the strip into a row, and only then (2026-10-04)', () => {
  // Operate's waterfall strip is a `.panel`, so a COLUMN, and the opt-in RF scope pane stands beside
  // the waterfall in it: the pane takes half the strip's WIDTH, none of the decode lists' height, and
  // the QSO strip with Stop TX under the strip does not move. The direction comes from
  // `.cockpit-rfbeside` in cockpit-panes.css, the structural sheet, which this file otherwise never
  // reads — so its rules are folded in here, ordered after styles.css's as main.tsx imports them, and
  // the winner is computed across both. A guard reading one sheet is how "the override lived in the
  // other file" ships.
  const panes = parseRules(
    readFileSync(fileURLToPath(new URL('./cockpit-panes.css', import.meta.url)), 'utf8').replace(/\/\*[\s\S]*?\*\//g, ''),
  ).map((r) => ({ ...r, order: r.order + 1_000_000 }))
  const strip = (classes: string[]) => [
    ...shellChain('operate-cockpit'),
    new Set(['cockpit-body']),
    new Set(['cockpit-waterfall', 'panel', ...classes]),
  ]
  const winner = (chain: Array<Set<string>>, blockValue: (body: string) => string | null) => {
    let win: { value: string; selector: string; spec: number; order: number } | null = null
    for (const r of [...RULES, ...panes]) {
      if (r.media !== null || !matchesChain(r.selector, chain)) continue
      const v = blockValue(r.body)
      if (v === null) continue
      const spec = specificity(r.selector)
      if (!win || spec > win.spec || (spec === win.spec && r.order >= win.order)) {
        win = { value: v, selector: r.selector, spec, order: r.order }
      }
    }
    return win
  }
  it('with the pane ticked the strip is a row with a gap', () => {
    const dir = winner(strip(['cockpit-rfbeside']), blockFlexDirection)
    expect(dir?.value, `the strip's direction comes from \`${dir?.selector}\``).toBe('row')
    expect(winner(strip(['cockpit-rfbeside']), blockLonghand('gap')), 'no gap between the waterfall and the pane').not.toBeNull()
  })
  it('without it the strip is the column it always was (control: the stock FT screen is untouched)', () => {
    expect(winner(strip([]), blockFlexDirection)?.value).toBe('column')
    expect(winner(strip([]), blockLonghand('gap'))).toBeNull()
  })
})

describe("the scope Splitter's declared range is the range the sheet HONOURS", () => {
  // The drag writes a flex-BASIS percentage; the sheet's min-height/max-height then clamp
  // the rendered box. Where the two disagree the surplus travel is DEAD: the pointer
  // moves, the variable moves, the panel does not. Phone declared 100 px and CW 90 px
  // against a `min-height: 8em` (= 112 px at the 14 px body font), and both declared
  // 420 px against `max-height: calc(0.45 * var(--vh-eff))` — 345.6 px on a 768-tall
  // window (so the last ~74 px of the drag were inert there) and 648 px on a 1440-tall
  // one (so 228 px of legal travel was unreachable).
  //
  // Computed on BOTH sides at three geometries: one point can be matched by a lucky
  // constant, three cannot — only a clamp that actually tracks the geometry passes.
  const GEOMS: Geom[] = [
    { fontPx: 14, vhEff: 768 }, // the supported floor
    { fontPx: 14, vhEff: 1440 }, // a tall display
    { fontPx: 16, vhEff: 768 }, // the same window with a larger body font
    // A short window (768 tall at 175 %): the only one here where a YIELDING floor (JS8's waterfall,
    // min(8em, 28 % of --vh-eff)) yields, so a declared clamp that dropped the yield fails here.
    { fontPx: 14, vhEff: 384 },
  ]
  const DECLARED: Record<string, SplitClamp> = {
    SCOPE_SPLIT_MIN,
    SCOPE_SPLIT_MAX,
    WATERFALL_SPLIT_MIN,
    WATERFALL_SPLIT_MAX,
    SSTV_STAGE_SPLIT_MIN,
    SSTV_STAGE_SPLIT_MAX,
  }

  /** The `<PaneSeam …/>` that drives `varName`, as prop → source expression. */
  function splitterProps(src: string, varName: string): Record<string, string> {
    const at = src.indexOf(`varName="${varName}"`)
    expect(at, `no <PaneSeam varName="${varName}" …> in the source`).toBeGreaterThan(-1)
    const el = src.slice(src.lastIndexOf('<PaneSeam', at), src.indexOf('/>', at))
    const out: Record<string, string> = {}
    for (const m of el.matchAll(/(\w+)=\{([^{}]*)\}/g)) out[m[1]] = m[2].trim()
    return out
  }

  /** Resolve a declared clamp to CSS px the way PaneSeam does: an exported
   *  SplitClamp by name, else a px literal. */
  function declaredPx(expr: string | undefined, g: Geom): number | null {
    if (expr === undefined) return null
    if (expr in DECLARED) return resolveClamp(DECLARED[expr], g)
    const n = Number(expr)
    return Number.isFinite(n) ? n : null
  }

  /** [shell, source, variable, the strip as its selector tokens (the element's classes, and a
   *  presence attribute where the strip's sized shape keys on one)]. */
  const CALLERS: Array<[string, string, string, string[]]> = [
    ['phone-cockpit', './components/PhoneCockpit.tsx', '--ph-scope-h', ['ph-scope-panel']],
    ['cw-cockpit', './components/CwCockpit.tsx', '--cw-scope-h', ['ph-scope-panel']],
    // JS8's waterfall divider (layout L2): the RTTY/PSK waterfall's yielding floor, the scope's cap.
    ['js8-cockpit', './components/Js8Cockpit.tsx', '--js8-wf-h', ['waterfall-wrap']],
    // RTTY's and PSK's (layout L6): the same clamps; PSK's strip rides RTTY's rule and variable.
    ['rtty-cockpit', './components/RttyCockpit.tsx', '--rtty-wf-h', ['waterfall-wrap']],
    ['psk-cockpit', './components/PskCockpit.tsx', '--rtty-wf-h', ['waterfall-wrap']],
    // SSTV's stage divider (layout L6) sizes the stage in its SIZED shape, which is the one its
    // clamps must agree with: the stage's own 16em floor and the sized rule's ceiling.
    ['sstv-view', './components/SstvView.tsx', '--sstv-stage-h', ['sstv-canvas', '[data-sized]']],
  ]
  for (const [shell, file, varName, tokens] of CALLERS) {
    // Phone's strip stands in its stage (PHONE_STAGES); its clamps are the same in both states.
    const chain = [...(shell === 'phone-cockpit' ? PHONE_STAGES[1][1] : shellChain(shell)), new Set(tokens)]
    const strip = tokens.map((t) => (t.startsWith('[') ? t : `.${t}`)).join('').slice(1)
    for (const [end, prop] of [
      ['min', 'min-height'],
      ['max', 'max-height'],
    ]) {
      it(`.${shell}: the splitter's ${end} equals the sheet's ${prop} at every geometry`, () => {
        const props = splitterProps(
          readFileSync(fileURLToPath(new URL(file, import.meta.url)), 'utf8'),
          varName,
        )
        const sheet = winningValue(chain, blockLonghand(prop))
        expect(sheet, `.${shell} .${strip}: nothing declares ${prop}`).not.toBeNull()
        for (const g of GEOMS) {
          const want = lengthPx(sheet!.value, g)
          expect(want, `\`${sheet!.value}\` is unreadable as a length`).not.toBeNull()
          const got = declaredPx(props[end], g)
          expect(
            got,
            `${file} declares no readable \`${end}\` for the ${shell} strip divider`,
          ).not.toBeNull()
          expect(
            got!,
            `${file} declares ${end}=${props[end]} → ${got}px, but \`${sheet!.selector} { ` +
              `${prop}: ${sheet!.value} }\` honours ${want}px at font ${g.fontPx}px / --vh-eff ` +
              `${g.vhEff}px. The ${Math.abs(got! - want!).toFixed(1)}px of disagreement is DEAD ` +
              'TRAVEL at that end of the drag — declare the clamp in the sheet\'s own units ' +
              '(SCOPE_SPLIT_* / WATERFALL_SPLIT_* in features/paneSeam.ts).',
          ).toBeCloseTo(want!, 6)
        }
      })
    }
  }
})
