import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import {
  resolveClamp,
  SCOPE_SPLIT_MAX,
  SCOPE_SPLIT_MIN,
  type SplitClamp,
} from './components/Splitter'

// Guards the DEFICIT VALVE (2026-07-30 layout assessment, mechanism C1/C2): every
// non-Operate cockpit shell must resolve to `overflow-y: auto` so a genuine vertical
// deficit SCROLLS instead of clipping the log form / PTT tail unreachably.
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

const css = readFileSync(fileURLToPath(new URL('./styles.css', import.meta.url)), 'utf8')
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
 *  contains anything else (pseudos, attributes, tags, ids, or is empty). Rejecting is
 *  the fail-safe direction here: a selector this computer cannot evaluate must never
 *  count as a cascade winner by accident. */
function compoundClasses(compound: string): string[] | null {
  if (/[\s>+~:[#]/.test(compound)) return null
  const parts = compound.match(/\.[a-zA-Z0-9_-]+/g)
  if (!parts || parts.join('') !== compound) return null
  return parts.map((p) => p.slice(1))
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

/** Class-count specificity — every candidate here is a class-only compound. */
function specificity(selector: string): number {
  return (selector.match(/\./g) ?? []).length
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
  ['.layout.single.sstv-view', shellChain('sstv-view')],
  // APRS is the sixth cockpit and was ABSENT from this census until 2026-08-04: nothing had
  // ever computed its shell, and it was the one surface in the tree with no deficit valve —
  // its header (.np-head: the frequency picker, Re-tune, the TX arm latch, Monitor, both
  // health chips and the internet control) is an unshrinkable wrapping strip sitting DIRECTLY
  // against that clip, and the internet popover it hosts is `position: absolute` with the
  // shell as its nearest non-visible ancestor, so the panel's lower rows had nowhere to go.
  ['.layout.single.needed-panel.aprs-cockpit', APRS_CHAIN],
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

  for (const shell of ['.layout.single.rtty-cockpit', '.layout.single.sstv-view']) {
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

describe('the scope splitter drag is respected (winning flex-grow is 0)', () => {
  // The Splitter (PhoneCockpit.tsx ~728 / CwCockpit) drives --ph-scope-h / --cw-scope-h
  // as the scope's flex-BASIS. A basis only sets the rendered height while flex-grow
  // is 0: with grow 1 on the column's only grower the resolved height is
  // clamp(shellH − siblings, floor, cap) at EVERY basis — algebraically independent of
  // the drag — so the operator's live control (and the persisted %) silently did
  // nothing (review 2026-07-31; CW split the surplus 1:1 with `.cw-lower` and tracked
  // the pointer at half rate instead). Operate is the pattern: `.cockpit-waterfall
  // { flex: 0 1 var(--cockpit-wf-h, 22%) }` with the decode scroller as the grower.
  for (const shell of ['phone-cockpit', 'cw-cockpit']) {
    it(`.${shell} .ph-scope-panel resolves flex-grow 0 (grow ≥1 voids the dragged basis)`, () => {
      const chain = [...shellChain(shell), new Set(['ph-scope-panel'])]
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

  const ROWS: Array<[string, string, string[]]> = [
    ['phone-cockpit', 'ph-ptt-row', ['ph-ptt-row']],
    ['cw-cockpit', 'cw-macros', ['cw-macros']],
    ['cw-cockpit', 'cw-send', ['cw-send']],
    ['rtty-cockpit', 'cw-macros (macro row)', ['cw-macros']],
    ['rtty-cockpit', 'cw-macros.rtty-auto-row (auto-sequencer)', ['cw-macros', 'rtty-auto-row']],
    ['rtty-cockpit', 'cw-send (compose bar)', ['cw-send']],
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
  it('.connect grid-template-rows strip track is auto (no fixed max — it would always pay out)', () => {
    let rows: string | null = null
    let at = -1
    for (const r of RULES) {
      if (r.media !== null || r.selector !== '.connect') continue
      const m = [...r.body.matchAll(/grid-template-rows\s*:\s*([^;]+)/g)]
      if (m.length && r.order >= at) {
        rows = m[m.length - 1][1].trim()
        at = r.order
      }
    }
    expect(rows, '.connect: no grid-template-rows declared').not.toBeNull()
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
    expect(tracks.length, `.connect rows are \`${rows}\``).toBe(3)
    expect(
      tracks[2],
      `.connect strip track is \`${tracks[2]}\` — a fixed max is maximized to its full value ` +
        'before the fr rows expand (§11.6), so it is a floor, not a cap.',
    ).toBe('auto')
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
  ['Connect rail', CONNECT_HOST],
  ['Connect bottom strip', [...CONNECT_HOST, new Set(['connect-strip'])]],
  [
    'Phone pane region',
    [...shellChain('phone-cockpit'), new Set(['cockpit-panes']), new Set(['cockpit-col'])],
  ],
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

const SCOPE_HOSTS = ['phone-cockpit', 'cw-cockpit']
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

  for (const shell of SCOPE_HOSTS) {
    const chain = () => [...shellChain(shell), new Set(['ph-scope-panel'])]

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

describe('the RTTY waterfall floor YIELDS (RTTY is the one scope with no Splitter)', () => {
  // `.rtty-cockpit .waterfall-wrap` shares the strip shape with `.ph-scope-panel`, and
  // carried the same `min-height: 120px` — but RTTY gives the operator no drag handle,
  // so on a short or pinned-zoom window that floor is unrecoverable: the strip simply
  // takes its 120 px out of a window that has ~400 to spend. The shell valve keeps it
  // from TRAPPING anything, which is why this is a share bug and not a safety bug.
  const chain = [...shellChain('rtty-cockpit'), new Set(['waterfall-wrap'])]

  it('resolves a min-height at all (the canvas needs real device pixels)', () => {
    const win = winningValue(chain, blockLonghand('min-height'))
    expect(win, '.rtty-cockpit .waterfall-wrap: nothing declares min-height').not.toBeNull()
    // Not "delete the floor" — at a normal window the strip must still be a strip.
    const v = win!.value
    expect(
      lengthPx(v, { fontPx: 14, vhEff: 768 }),
      `.rtty-cockpit .waterfall-wrap min-height is \`${v}\` — at a 768 window the floor must ` +
        'still keep a drawable strip (≥ 6em).',
    ).toBeGreaterThanOrEqual(6 * 14)
  })

  it('never claims more than 30% of the effective viewport', () => {
    const v = winningValue(chain, blockLonghand('min-height'))!.value
    for (const vhEff of [1440, 768, 439, 384, 300]) {
      const floor = lengthPx(v, { fontPx: 14, vhEff })
      expect(floor, `min-height \`${v}\` is unreadable as a length`).not.toBeNull()
      expect(
        floor!,
        `.rtty-cockpit .waterfall-wrap min-height is \`${v}\` = ${floor}px at --vh-eff ${vhEff} ` +
          `— ${((100 * floor!) / vhEff).toFixed(0)}% of the window for a glance strip, and RTTY ` +
          'has no Splitter to take it back. Write the floor to yield: min(Xem, share).',
      ).toBeLessThanOrEqual(0.3 * vhEff)
    }
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
  ]
  const DECLARED: Record<string, SplitClamp> = { SCOPE_SPLIT_MIN, SCOPE_SPLIT_MAX }

  /** The `<Splitter …/>` that drives `varName`, as prop → source expression. */
  function splitterProps(src: string, varName: string): Record<string, string> {
    const at = src.indexOf(`varName="${varName}"`)
    expect(at, `no <Splitter varName="${varName}" …> in the source`).toBeGreaterThan(-1)
    const el = src.slice(src.lastIndexOf('<Splitter', at), src.indexOf('/>', at))
    const out: Record<string, string> = {}
    for (const m of el.matchAll(/(\w+)=\{([^{}]*)\}/g)) out[m[1]] = m[2].trim()
    return out
  }

  /** Resolve a declared clamp to CSS px the way Splitter.tsx does: an exported
   *  SplitClamp by name, else a px literal. */
  function declaredPx(expr: string | undefined, g: Geom): number | null {
    if (expr === undefined) return null
    if (expr in DECLARED) return resolveClamp(DECLARED[expr], g)
    const n = Number(expr)
    return Number.isFinite(n) ? n : null
  }

  const CALLERS: Array<[string, string, string]> = [
    ['phone-cockpit', './components/PhoneCockpit.tsx', '--ph-scope-h'],
    ['cw-cockpit', './components/CwCockpit.tsx', '--cw-scope-h'],
  ]
  for (const [shell, file, varName] of CALLERS) {
    const chain = [...shellChain(shell), new Set(['ph-scope-panel'])]
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
        expect(sheet, `.${shell} .ph-scope-panel: nothing declares ${prop}`).not.toBeNull()
        for (const g of GEOMS) {
          const want = lengthPx(sheet!.value, g)
          expect(want, `\`${sheet!.value}\` is unreadable as a length`).not.toBeNull()
          const got = declaredPx(props[end], g)
          expect(
            got,
            `${file} declares no readable \`${end}\` for the ${shell} scope splitter`,
          ).not.toBeNull()
          expect(
            got!,
            `${file} declares ${end}=${props[end]} → ${got}px, but \`${sheet!.selector} { ` +
              `${prop}: ${sheet!.value} }\` honours ${want}px at font ${g.fontPx}px / --vh-eff ` +
              `${g.vhEff}px. The ${Math.abs(got! - want!).toFixed(1)}px of disagreement is DEAD ` +
              'TRAVEL at that end of the drag — declare the clamp in the sheet\'s own units ' +
              '(SCOPE_SPLIT_MIN / SCOPE_SPLIT_MAX in Splitter.tsx).',
          ).toBeCloseTo(want!, 6)
        }
      })
    }
  }
})
