// @vitest-environment jsdom
//
// OUR OWN KEYED TEXT IS DRAWN AS OURS, AND THE STREAM SHEDS THE CARD LOOK BY SPECIFICITY (#379).
//
// THE SHIPPED DEFECT. The transcript echo (cbcdbccd) added `.rtty-sent` by inserting its comment
// and rule INSIDE the selector `.pane-body > .rtty-stream`, so the sheet read
//
//     .pane-body > /* …comment… */ .rtty-sent { color: var(--accent); border-left: … }
//     .rtty-stream { padding: 0; border: 0; background: transparent }
//
// A comment is whitespace to a selector, so the first rule is `.pane-body > .rtty-sent` — which
// no sent span ever matches (they sit three levels below `.pane-body`) — and the second lost its
// frame scope. Every sent character rendered exactly like received copy, while the span's class
// and title said otherwise, and the unscoped flatten went on winning only because it happens to
// sit below `.cw-decode` in the file.
//
// WHY IT IS COMPUTED, NOT GREPPED. A regex for `.rtty-sent {` passes the broken sheet: the text is
// there, the selector is dead. So this mounts the REAL cockpit, takes the elements it really
// renders, and resolves each rule of the real sheet against them with jsdom's selector engine —
// then picks the winner the way the cascade does (specificity, then source order).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { parseRules, cmpSpec, type Rule } from '../cssCascade'
import { RttyCockpit } from './RttyCockpit'
import type { RttyState } from '../types'

globalThis.ResizeObserver ??= class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver

// A CQ off the air, then our answer — the second half keyed by this station.
const RX = 'CQ W1AW '
const TX = 'W1AW DE W9XYZ K '
const state: RttyState = {
  armed: true,
  afcHz: 0,
  afcLocked: false,
  text: RX + TX,
  charConf: [],
  charTx: [...RX].map(() => false).concat([...TX].map(() => true)),
  baud: 45.45,
  shiftHz: 170,
  markHz: 2125,
  spaceHz: 2295,
  backend: 'afsk',
  sending: false,
  latched: false,
  keyerError: null,
  auto: false,
  seqState: 'idle',
  peer: null,
  peerExchange: [],
  heardCq: null,
}

vi.mock('../api', () => ({
  getRttyState: vi.fn(async () => state),
  rttyAutoArm: vi.fn(async () => state),
  getLicensedBandPlan: vi.fn(async () => []),
}))
vi.mock('./LogEntry', () => ({ LogEntry: () => null }))

// Comments stripped BEFORE parsing (parseRules is brace-aware, not comment-aware) — which is
// also exactly what a browser does with the comment inside the broken selector. Resolved from
// the cwd: under jsdom `import.meta.url` is an http URL.
const RULES: Rule[] = ['styles.css', 'cockpit-panes.css'].flatMap((f) =>
  parseRules(readFileSync(resolve(process.cwd(), 'src', f), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '')),
)

/** Every rule of the sheet that really applies to `el`, by jsdom's own selector engine. A
 *  selector it cannot evaluate (a pseudo-element, say) cannot style a plain element anyway. */
function matching(el: Element): Rule[] {
  return RULES.filter((r) => {
    try {
      return el.matches(r.selector)
    } catch {
      return false
    }
  })
}

/** The winning declaration of `prop` on `el`: specificity, then source order (`reversed` flips
 *  the tie-break, i.e. asks what would win if the tied rules swapped places in the file). */
function winner(el: Element, prop: string, reversed = false): string | null {
  const tieWins = (a: Rule, b: Rule) => (reversed ? a.order <= b.order : a.order >= b.order)
  let best: { value: string; rule: Rule } | null = null
  for (const rule of matching(el)) {
    for (const d of rule.decls) {
      if (d.prop !== prop) continue
      const spec = best ? cmpSpec(rule.spec, best.rule.spec) : 1
      if (!best || spec > 0 || (spec === 0 && tieWins(rule, best.rule))) best = { value: d.value, rule }
    }
  }
  return best?.value ?? null
}

async function mount() {
  render(<RttyCockpit snap={null} />)
  await waitFor(() => expect(document.querySelector('.rtty-stream .cw-decode-text')?.textContent).toBe(RX + TX))
  return document.querySelector('.rtty-stream') as HTMLElement
}

beforeEach(() => {
  document.documentElement.dataset.theme = 'dark'
})
afterEach(cleanup)

describe('RTTY transcript — our own keyed text (#379)', () => {
  it('mounts what this guard reasons about: a framed stream holding a sent span and a received one', async () => {
    const stream = await mount()
    // The chain the selectors must reach through, read off the real render — not modelled.
    expect(stream.parentElement?.classList.contains('pane-body')).toBe(true)
    const sent = stream.querySelector('.cw-decode-text .rtty-sent')
    expect(sent?.textContent).toBe(TX)
    expect([...stream.querySelectorAll('.cw-decode-text span')].some((s) => !s.classList.contains('rtty-sent'))).toBe(true)
  })

  it('draws a character WE keyed in the accent colour with a left rule — the cascade reaches the span', async () => {
    const stream = await mount()
    const sent = stream.querySelector('.rtty-sent')!
    expect(winner(sent, 'color'), 'no rule gives our own text its colour').toBe('var(--accent)')
    expect(winner(sent, 'border-left'), 'no rule gives our own text its rule — colour alone').toBe(
      '2px solid var(--accent)',
    )
  })

  it('leaves received copy unmarked — the control that the sent style is not simply on every span', async () => {
    const stream = await mount()
    const received = [...stream.querySelectorAll('.cw-decode-text span')].find((s) => !s.classList.contains('rtty-sent'))!
    expect(received.textContent).toBe(RX)
    expect(winner(received, 'border-left')).toBeNull()
    expect(winner(received, 'color')).not.toBe('var(--accent)')
  })

  it('sheds the card look inside its frame by SPECIFICITY, so no reordering of the sheet brings it back', async () => {
    const stream = await mount()
    // `.cw-decode` gives every decode box a bordered card; inside a pane frame the frame IS the
    // card. The flatten must out-rank it outright — as `.pane-body > .panel` does — rather than
    // win a tie on where it happens to sit in the file.
    for (const reversed of [false, true]) {
      expect(winner(stream, 'border', reversed), `border (ties ${reversed ? 'reversed' : 'as written'})`).toBe('0')
      expect(winner(stream, 'padding', reversed), `padding (ties ${reversed ? 'reversed' : 'as written'})`).toBe('0')
      expect(winner(stream, 'background', reversed), `background (ties ${reversed ? 'reversed' : 'as written'})`).toBe(
        'transparent',
      )
    }
  })
})
