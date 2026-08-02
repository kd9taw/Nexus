import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

// Guards the DECODE-FIRST Classic rebuild (2026-08, field feedback from an advanced DX
// operator): the Classic lower region is a THREE-column grid — Band Activity | the
// promoted Rx-Frequency column (decode pane + Tx1–Tx6 machine) | the Stations roster —
// and the merged operating strip replaces the old .cockpit-status row + the permanent
// TX-meters row.
//
// Like cockpit-shells.test.ts, this file COMPUTES cascade winners (specificity + source
// order over the parsed sheet) rather than grepping for rule presence — dead fixes pass
// a presence grep, which is how two dead fixes shipped pre-overhaul. Unlike that file it
// must evaluate attribute compounds ([data-cols='two'], [data-viewport='sm']), so it
// carries its own small matcher.

const css = readFileSync(fileURLToPath(new URL('./styles.css', import.meta.url)), 'utf8')
  // Strip comments first so prose is never read as a declaration.
  .replace(/\/\*[\s\S]*?\*\//g, '')

interface Rule {
  selector: string
  body: string
  order: number
  media: string | null
}

/** Brace-aware rule walk (same shape as cockpit-shells.test.ts). */
function parseRules(sheet: string): Rule[] {
  const out: Rule[] = []
  let i = 0
  let order = 0
  const n = sheet.length

  function skipBalanced(): void {
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

/** A modeled element: its class set + exact-match attributes. */
interface El {
  classes: Set<string>
  attrs?: Record<string, string>
}

/** Parse one compound into class + [attr='val'] tokens; null when it holds anything
 *  this matcher cannot evaluate (pseudos, ids, tags) — rejecting is the fail-safe
 *  direction: an unevaluable selector must never win by accident. */
function compoundTokens(compound: string): { classes: string[]; attrs: Array<[string, string]> } | null {
  const parts = compound.match(/\.[a-zA-Z0-9_-]+|\[[a-zA-Z0-9_-]+='[^']*'\]/g)
  if (!parts || parts.join('') !== compound) return null
  const classes: string[] = []
  const attrs: Array<[string, string]> = []
  for (const p of parts) {
    if (p.startsWith('.')) classes.push(p.slice(1))
    else {
      const m = /^\[([a-zA-Z0-9_-]+)='([^']*)'\]$/.exec(p)
      if (!m) return null
      attrs.push([m[1], m[2]])
    }
  }
  return { classes, attrs }
}

function compoundMatches(compound: string, el: El): boolean {
  const t = compoundTokens(compound)
  if (!t) return false
  if (!t.classes.every((c) => el.classes.has(c))) return false
  return t.attrs.every(([k, v]) => el.attrs?.[k] === v)
}

/** Right-to-left descendant/child match against an explicit ancestor chain. */
function matchesChain(selector: string, chain: El[]): boolean {
  const parts = selector.split(/\s*([>+~])\s*|\s+/).filter((p): p is string => !!p)
  if (parts.some((p) => p === '+' || p === '~')) return false
  const last = parts[parts.length - 1] ?? ''
  if (!compoundMatches(last, chain[chain.length - 1])) return false
  let idx = chain.length - 2
  let childOnly = false
  for (let i = parts.length - 2; i >= 0; i--) {
    const p = parts[i]
    if (p === '>') {
      childOnly = true
      continue
    }
    if (childOnly) {
      if (idx < 0 || !compoundMatches(p, chain[idx])) return false
      idx--
      childOnly = false
    } else {
      while (idx >= 0 && !compoundMatches(p, chain[idx])) idx--
      if (idx < 0) return false
      idx--
    }
  }
  return true
}

/** Class-bucket specificity: classes + attribute selectors count equally. */
function specificity(selector: string): number {
  return (selector.match(/\.[a-zA-Z0-9_-]+|\[[^\]]+\]/g) ?? []).length
}

/** Last declaration of `prop` inside a block (in-block order wins). */
function blockDecl(body: string, prop: string): string | null {
  let v: string | null = null
  for (const decl of body.split(';')) {
    const m = new RegExp(`^\\s*${prop}\\s*:\\s*(\\S[^]*?)\\s*$`).exec(decl)
    if (m) v = m[1].replace(/\s+/g, ' ')
  }
  return v
}

/** Cascade winner of `prop` for the element at the end of `chain` (unconditional rules). */
function winner(chain: El[], prop: string): { value: string; selector: string } | null {
  let win: { value: string; selector: string; spec: number; order: number } | null = null
  for (const r of RULES) {
    if (r.media !== null || !matchesChain(r.selector, chain)) continue
    const v = blockDecl(r.body, prop)
    if (v === null) continue
    const spec = specificity(r.selector)
    if (!win || spec > win.spec || (spec === win.spec && r.order >= win.order)) {
      win = { value: v, selector: r.selector, spec, order: r.order }
    }
  }
  return win && { value: win.value, selector: win.selector }
}

/** Paren-aware top-level track split (minmax(a, b) is one track). */
function splitTracks(v: string): string[] {
  const tracks: string[] = []
  let depth = 0
  let cur = ''
  for (const ch of v) {
    if (ch === '(') depth++
    if (ch === ')') depth--
    if (/\s/.test(ch) && depth === 0) {
      if (cur) tracks.push(cur)
      cur = ''
    } else cur += ch
  }
  if (cur) tracks.push(cur)
  return tracks
}

/** Final flex-grow a block computes (longhand + shorthand). */
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
      const first = /^([\d.]+|var\()/.exec(v)
      // A var() grow (share-driven panes) is ≥ MIN_SHARE by the writers' clamp — treat
      // as 1 for the "is a grower" question.
      grow = first ? (first[1] === 'var(' ? 1 : parseFloat(first[1])) : 0
    }
  }
  return grow
}

function winnerGrow(chain: El[]): { value: number; selector: string } | null {
  let win: { value: number; selector: string; spec: number; order: number } | null = null
  for (const r of RULES) {
    if (r.media !== null || !matchesChain(r.selector, chain)) continue
    const v = blockGrow(r.body)
    if (v === null) continue
    const spec = specificity(r.selector)
    if (!win || spec > win.spec || (spec === win.spec && r.order >= win.order)) {
      win = { value: v, selector: r.selector, spec, order: r.order }
    }
  }
  return win && { value: win.value, selector: win.selector }
}

/** The Classic lower grid's ancestor chain. `data-viewport` rides the app root
 *  (useViewport stamps it there); `data-cols` is the populated-column count the
 *  cockpit stamps on the grid itself. */
function lowerChain(viewport: string | null, dataCols: string): El[] {
  return [
    { classes: new Set(['app']), attrs: viewport ? { 'data-viewport': viewport } : {} },
    { classes: new Set(['shell']) },
    { classes: new Set(['layout', 'single', 'operate-cockpit']) },
    { classes: new Set(['cockpit-body']) },
    { classes: new Set(['cockpit-lower', 'classic']), attrs: { 'data-cols': dataCols } },
  ]
}

describe('the Classic lower region is the three-column decode-first grid', () => {
  for (const vp of ['lg', 'xl']) {
    it(`[data-viewport='${vp}'] data-cols='three': 3 minmax tracks, seam vars live, no bare fr`, () => {
      const win = winner(lowerChain(vp, 'three'), 'grid-template-columns')
      expect(win, 'no grid-template-columns rule matches the classic grid').not.toBeNull()
      const tracks = splitTracks(win!.value)
      expect(
        tracks.length,
        `winner \`${win!.selector}\` resolves \`${win!.value}\` — the WSJT-X pair + roster needs 3 columns`,
      ).toBe(3)
      for (const t of tracks) {
        expect(
          /^minmax\(/.test(t),
          `track \`${t}\` (via \`${win!.selector}\`) is not minmax(…) — a bare fr track carries a ` +
            'min-content floor that can force horizontal blow-out (the pre-overhaul bug class)',
        ).toBe(true)
      }
      // The column seam (SplitterSeam axis='x') paints --op-col-a/--op-col-b on the
      // grid. A template that stops consuming them makes the operator's drag silently
      // dead — the exact dead-fix mechanism this guard style exists to catch.
      expect(win!.value).toContain('var(--op-col-a')
      expect(win!.value).toContain('var(--op-col-b')
    })
  }

  it("data-cols='two': survivors flow into a 2-track template", () => {
    const win = winner(lowerChain('lg', 'two'), 'grid-template-columns')
    expect(win).not.toBeNull()
    const tracks = splitTracks(win!.value)
    expect(tracks.length, `winner \`${win!.selector}\` resolves \`${win!.value}\``).toBe(2)
    for (const t of tracks) expect(/^minmax\(/.test(t), `track \`${t}\` is not minmax(…)`).toBe(true)
  })

  it("data-cols='one': the sole survivor takes one minmax(0,1fr) track", () => {
    const win = winner(lowerChain('lg', 'one'), 'grid-template-columns')
    expect(win).not.toBeNull()
    // Whitespace-normalized: the sheet writes `minmax(0, 1fr)` (its house style).
    expect(
      splitTracks(win!.value).map((t) => t.replace(/\s+/g, '')),
      `winner \`${win!.selector}\``,
    ).toEqual(['minmax(0,1fr)'])
  })

  for (const vp of ['sm', 'xs']) {
    for (const cols of ['three', 'two', 'one']) {
      it(`[data-viewport='${vp}'] outranks data-cols='${cols}': single column, row-auto-flow`, () => {
        const win = winner(lowerChain(vp, cols), 'grid-template-columns')
        expect(win).not.toBeNull()
        expect(
          splitTracks(win!.value).map((t) => t.replace(/\s+/g, '')),
          `winner \`${win!.selector}\` resolves \`${win!.value}\` — the narrow stack must beat ` +
            'every data-cols override or columns crush at phone/pinned-zoom widths',
        ).toEqual(['minmax(0,1fr)'])
        const rows = winner(lowerChain(vp, cols), 'grid-auto-rows')
        expect(rows, 'stacked children need grid-auto-rows to share the height').not.toBeNull()
        expect(rows!.value.replace(/\s+/g, '')).toBe('minmax(0,1fr)')
      })
    }
  }
})

describe('inside the promoted column, the decode feed is the grower', () => {
  // Design-3 invariant folded in: the Rx-Frequency feed fills its column while the
  // Tx1–Tx6 machine stays content-sized — reversed, the pane the operator runs the
  // QSO from collapses to a strip under its own Tx machine.
  it('.cockpit-rxfreq in the classic qsocol resolves flex-grow ≥ 1', () => {
    const chain = [
      ...lowerChain('lg', 'three'),
      { classes: new Set(['cockpit-qsocol']) },
      { classes: new Set(['cockpit-rxfreq', 'panel']) },
    ]
    const win = winnerGrow(chain)
    expect(win, '.cockpit-rxfreq: no flex rule at all').not.toBeNull()
    expect(
      win!.value,
      `\`${win!.selector}\` gives the promoted decode pane flex-grow ${win!.value}`,
    ).toBeGreaterThanOrEqual(1)
  })

  it('the Tx1–Tx6 machine resolves flex-grow 0 (content-sized beneath the pane)', () => {
    const chain = [
      ...lowerChain('lg', 'three'),
      { classes: new Set(['cockpit-qsocol']) },
      { classes: new Set(['tx-panel', 'tx-panel-compact', 'panel']) },
    ]
    const win = winnerGrow(chain)
    expect(win, '.tx-panel: no flex rule at all').not.toBeNull()
    expect(win!.value, `\`${win!.selector}\` lets the Tx machine grow`).toBe(0)
  })
})

/** The strip's ancestor chain (no data-viewport: unconditional geometry). */
const STRIP_CHAIN: El[] = [
  { classes: new Set(['app']) },
  { classes: new Set(['shell']) },
  { classes: new Set(['layout', 'single', 'operate-cockpit']) },
  { classes: new Set(['cockpit-body']) },
  { classes: new Set(['cockpit-qso', 'panel']) },
]

describe('the merged operating strip is a wrapping ROW', () => {
  // The strip carries BOTH .cockpit-qso and .panel, and `.panel` declares
  // flex-direction: column. If .cockpit-qso stops declaring row explicitly,
  // .panel wins the cascade and the one-row strip renders as a clipped
  // column-wrap jumble — computed against the whole sheet, exactly the
  // "recompute a changed rule against the sheet" failure mode.
  it('.cockpit-qso resolves flex-direction: row against the whole sheet', () => {
    const win = winner(STRIP_CHAIN, 'flex-direction')
    expect(win, 'no flex-direction rule reaches the strip at all').not.toBeNull()
    expect(
      win!.value,
      `\`${win!.selector}\` wins flex-direction for the strip — .panel's column turns ` +
        'the one-row strip into a clipped column jumble',
    ).toBe('row')
  })

  it('.cockpit-qso resolves flex-wrap: wrap (the deliberate-break discipline needs it)', () => {
    const win = winner(STRIP_CHAIN, 'flex-wrap')
    expect(win, 'no flex-wrap rule reaches the strip').not.toBeNull()
    expect(win!.value, `\`${win!.selector}\` wins flex-wrap`).toBe('wrap')
  })
})

describe('TX-cluster position stability: row neighbours resolve state-independent widths', () => {
  // The other half of OperateQsoStrip.geometry.test.tsx (which pins DOM order:
  // nothing state-variable precedes the clusters). Here: nothing sharing the
  // clusters' row may resolve a width that varies with QSO/TX state — measured
  // before the fix, Stop TX drifted 376 → 648 → 688 across idle/QSO/TX.
  it('.cq-now is a fixed-basis cell, never a grower (its message ellipsizes inside)', () => {
    const chain = [...STRIP_CHAIN, { classes: new Set(['cq-now']) }]
    const grow = winnerGrow(chain)
    expect(grow, '.cq-now: no flex rule at all').not.toBeNull()
    expect(
      grow!.value,
      `\`${grow!.selector}\` lets .cq-now grow — a growing now-cell splits surplus with ` +
        '.cq-spacer and every readout-width change shifts the free-text/Log group',
    ).toBe(0)
  })

  it('.cq-spacer is the single mid-row absorber (flex-grow ≥ 1)', () => {
    const chain = [...STRIP_CHAIN, { classes: new Set(['cq-spacer']) }]
    const grow = winnerGrow(chain)
    expect(grow, '.cq-spacer: no flex rule at all').not.toBeNull()
    expect(grow!.value, `\`${grow!.selector}\``).toBeGreaterThanOrEqual(1)
  })

  it('the TX On/Off toggle carries a fixed em min-width (label flips with txEnabled)', () => {
    const chain = [...STRIP_CHAIN, { classes: new Set(['op-btn', 'monitor']) }]
    const win = winner(chain, 'min-width')
    expect(win, '.op-btn.monitor: no min-width — Tune/Stop TX shift when TX On becomes TX Off').not.toBeNull()
    expect(/^\d+(\.\d+)?em$/.test(win!.value), `min-width is \`${win!.value}\` — must be a fixed em`).toBe(true)
  })

  it('the next-slot countdown carries a fixed em min-width (digit count ticks every second)', () => {
    const chain = [...STRIP_CHAIN, { classes: new Set(['cq-next']) }]
    const win = winner(chain, 'min-width')
    expect(win, '.cq-next: no min-width — the period/Skip Tx1 pills shift as the countdown crosses 10 s').not.toBeNull()
    expect(/^\d+(\.\d+)?em$/.test(win!.value), `min-width is \`${win!.value}\` — must be a fixed em`).toBe(true)
  })
})

describe('the deliberate wrap point engages on the EFFECTIVE-width vocabulary', () => {
  // Tier decision (2026-08): one row is guaranteed to fit only at xl (≥2400
  // effective). lg spans 1600–2400 and INCLUDES the 1366x768 laptop (85 %
  // auto-fit → effective 1607), where the one-row minimum plus the HOUND badge
  // or rotor strip overflowed and wrapped at an arbitrary member — measured
  // 38→70 px mid-QSO. So the break is hidden at xl only.
  const breakChain = (vp: string): El[] => [
    { classes: new Set(['app']), attrs: { 'data-viewport': vp } },
    { classes: new Set(['shell']) },
    { classes: new Set(['layout', 'single', 'operate-cockpit']) },
    { classes: new Set(['cockpit-body']) },
    { classes: new Set(['cockpit-qso', 'panel']) },
    { classes: new Set(['cq-break']) },
  ]

  it('xl: .cq-break resolves display:none (the one row always fits)', () => {
    const win = winner(breakChain('xl'), 'display')
    expect(win, 'no display rule hides the break at xl').not.toBeNull()
    expect(win!.value, `\`${win!.selector}\``).toBe('none')
  })

  for (const vp of ['lg', 'md', 'sm', 'xs']) {
    it(`${vp}: .cq-break engages (stable two-row wrap, never a mid-QSO content wrap)`, () => {
      const win = winner(breakChain(vp), 'display')
      expect(
        win?.value ?? null,
        win
          ? `\`${win.selector}\` hides the break at ${vp} — a 1366x768 laptop is lg and needs it`
          : '',
      ).not.toBe('none')
    })
    it(`${vp}: .cq-break spans the full row (flex-basis 100%)`, () => {
      const win = winner(breakChain(vp), 'flex-basis')
      expect(win, 'no flex-basis rule reaches the break').not.toBeNull()
      expect(win!.value, `\`${win!.selector}\``).toBe('100%')
    })
  }
})

describe('the merged operating strip replaced the status row and the meters row', () => {
  // Negative census (the cockpit-shells '.ph-lower is gone' discipline): the old
  // .cockpit-status strip duplicated the strip's TX state + txNow echo and was ~42px of
  // the reported blank gray space. A rule on any of its classes means the row is
  // creeping back. `.cs-opt` (cockpit-source) is a different control and is NOT listed.
  for (const cls of ['cockpit-status', 'cs-state', 'cs-msg', 'cs-spacer', 'cs-period', 'cs-skiptx1', 'cs-next']) {
    it(`.${cls} no longer exists in the sheet`, () => {
      const hits = RULES.filter((r) => {
        const subject = r.selector.split(/\s*[>+~]\s*|\s+/).pop() ?? ''
        return (subject.match(/\.[a-zA-Z0-9_-]+/g) ?? ([] as string[])).includes(`.${cls}`)
      }).map((r) => r.selector)
      expect(
        hits,
        `\`.${cls}\` is back:\n${hits.join('\n')}\nThe TX state / period / Skip Tx1 / next-Ns ` +
          'controls live in the merged .cockpit-qso strip now (cq-* classes).',
      ).toEqual([])
    })
  }

  it('.cq-telemetry is a fixed-width, non-growing cell (zero layout shift per TX cycle)', () => {
    // The 722ef273 anti-bounce ruling at zero vertical cost: the cell's geometry must be
    // independent of whether the rig is keyed. A grow, an auto width, or a px width
    // (zoom-hostile) all re-open the every-15s layout bounce.
    const chain = [
      { classes: new Set(['app']) },
      { classes: new Set(['shell']) },
      { classes: new Set(['layout', 'single', 'operate-cockpit']) },
      { classes: new Set(['cockpit-body']) },
      { classes: new Set(['cockpit-qso', 'panel']) },
      { classes: new Set(['cq-telemetry']) },
    ]
    const grow = winnerGrow(chain)
    expect(grow, '.cq-telemetry: no flex rule at all').not.toBeNull()
    expect(grow!.value, `\`${grow!.selector}\` lets the telemetry cell grow`).toBe(0)
    const w = winner(chain, 'width')
    expect(w, '.cq-telemetry: no width declared — the cell breathes with the TX cycle').not.toBeNull()
    expect(/^\d+(\.\d+)?em$/.test(w!.value), `.cq-telemetry width is \`${w!.value}\` — must be a fixed em width`).toBe(true)
  })

  it('.cq-free input caps at a 16ch basis (maxLength is 13 — a full row was pure surplus)', () => {
    const chain = [
      { classes: new Set(['app']) },
      { classes: new Set(['shell']) },
      { classes: new Set(['layout', 'single', 'operate-cockpit']) },
      { classes: new Set(['cockpit-body']) },
      { classes: new Set(['cockpit-qso', 'panel']) },
      { classes: new Set(['cq-free']) },
      { classes: new Set([]) },
    ]
    // The input is a bare <input> (no class): compute over rules whose subject is
    // exactly `.cq-free input` instead.
    void chain
    let win: { value: string; selector: string; order: number } | null = null
    for (const r of RULES) {
      if (r.media !== null || r.selector !== '.cq-free input') continue
      const v = blockDecl(r.body, 'flex')
      if (v !== null && (!win || r.order >= win.order)) win = { value: v, selector: r.selector, order: r.order }
    }
    expect(win, '.cq-free input: no flex declared').not.toBeNull()
    expect(
      win!.value,
      `.cq-free input flex is \`${win!.value}\` — must be a shrinkable 16ch basis, never a grower`,
    ).toBe('0 1 16ch')
  })
})
