// @vitest-environment jsdom
//
// THE STATIONS CARD MUST NEVER PAINT UNDER ITS OWN SNR BADGE (2026-09-23).
//
// Measured in headless Chrome against the real sheet, inside the real Operate cockpit: at the
// 1024×768 floor the Classic Stations list is 252.9 px wide and the card's call column 37.5 px,
// so even a bare `W1AW` ran 13 px under the SNR badge; a card carrying the WATCH tile, three need
// chips, B4 and the ULTRA pill ran 441 px past its column, over the badge AND the Work button;
// and line 2 (country · grid · distance), whose own rule says `overflow: hidden; text-overflow:
// ellipsis`, is a plain <span> — inline — so both declarations were dead and it ran up to 240 px
// under the same controls. At 1920 the chip-heavy cards still overran by 137–259 px.
//
// The fix is five mechanisms, and each is pinned below as the value that WINS the cascade on the
// RENDERED card — never as the presence of a declaration, which is how two dead layout fixes
// shipped in this project:
//
//   1. line 1 wraps, so chips go to a second row instead of under the badge;
//   2. the badge drops below the call column when that column would fall under its floor, and
//      the floor is written to yield (`min(…)`), per the layout contract;
//   3. line 2 is a block, so the ellipsis its rule has always asked for takes effect;
//   4. a card never shrinks in its scroller — a flex column squeezed every card to its 44 px
//      min-height on a busy band, harmless for two lines of text and fatal for wrapped rows;
//   5. a chip wider than the whole line (only at the narrowest rails) clips inside itself.
//
// WHAT THIS CANNOT PROVE: geometry. jsdom lays nothing out, so "does not overlap" is not a claim
// this file can make; the Chrome measurement is that evidence, and this guard is what stops a
// later rule from silently undoing any one of its causes.
//
// HOW IT COMPUTES. The rules and their declarations come from the RAW sheet through the shared
// resolver (`cssCascade.ts`), and jsdom's real selector engine decides which of them match the
// rendered element; the winner is picked by importance, specificity, then source order. jsdom's
// own CSSOM cannot be the source of values: it silently DROPS a declaration it cannot parse, and
// `flex: 1 1 min(6.5em, 100% - 20px)` — the call column's floor — is one (checked: it reads back
// as ''). Its getComputedStyle also never expands `flex`, `flex-flow` or `overflow`.
import { describe, it, expect, afterEach } from 'vitest'
import { render, cleanup, screen } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { StationList } from './StationList'
import { saveWatchlist } from '../watchlist'
import type { NeedAlert, NeedTag, Station } from '../types'
import { NEED_TIER } from '../features/needs'
import { cmpSpec, parseRules } from '../cssCascade'

// jsdom leaves `import.meta.url` a non-file URL; vitest's cwd is the `ui` project root. Comment
// bodies are blanked first, so prose can never read as a declaration.
const RULES = parseRules(
  readFileSync(resolve(process.cwd(), 'src', 'styles.css'), 'utf8').replace(/\/\*[\s\S]*?\*\//g, ''),
)

afterEach(() => {
  cleanup()
  localStorage.clear()
})

/**
 * The value of a longhand that wins on `el`, where any of `props` (the longhand and the
 * shorthands that set it) may carry it; `pick` extracts the longhand from whichever won.
 * Null when nothing in the sheet sets it (the initial value applies).
 */
function winning(el: Element, props: string[], pick: (prop: string, value: string) => string): string | null {
  let win: { prop: string; value: string; important: boolean; spec: readonly number[]; order: number } | null = null
  for (const rule of RULES) {
    let hit = false
    try {
      hit = el.matches(rule.selector)
    } catch {
      continue // a selector this jsdom cannot parse (a pseudo-element) cannot be applying either
    }
    if (!hit) continue
    for (const d of rule.decls) {
      if (!props.includes(d.prop)) continue
      const important = /!important\s*$/.test(d.value)
      const cand = { prop: d.prop, value: d.value.replace(/\s*!important\s*$/, ''), important, spec: rule.spec, order: rule.order }
      const beats =
        !win ||
        (cand.important !== win.important
          ? cand.important
          : cmpSpec(cand.spec, win.spec) !== 0
            ? cmpSpec(cand.spec, win.spec) > 0
            : cand.order >= win.order)
      if (beats) win = cand
    }
  }
  return win ? pick(win.prop, win.value) : null
}

const tokens = (v: string) => v.split(/\s+(?![^(]*\))/) // split on spaces outside parentheses
const flexWrap = (el: Element) =>
  winning(el, ['flex-flow', 'flex-wrap'], (p, v) =>
    p === 'flex-wrap' ? v : (tokens(v).find((t) => /^(nowrap|wrap|wrap-reverse)$/.test(t)) ?? 'nowrap'),
  ) ?? 'nowrap'
/** `flex: none` → 0; `flex: <n>` / `auto` / `initial` → 1; `flex: <g> <s> …` → s. */
const flexShrink = (el: Element) =>
  winning(el, ['flex', 'flex-shrink'], (p, v) => {
    if (p === 'flex-shrink') return v
    if (v === 'none') return '0'
    const t = tokens(v)
    return t.length >= 2 && /^[\d.]+$/.test(t[1]) ? t[1] : '1'
  }) ?? '1'
/** `flex: <g> <s> <basis>` carries the basis third; a bare number sets it to 0%. */
const flexBasis = (el: Element) =>
  winning(el, ['flex', 'flex-basis'], (p, v) => {
    if (p === 'flex-basis') return v
    const t = tokens(v)
    return t.length >= 3 ? t[2] : t.length === 1 && /^[\d.]+$/.test(t[0]) ? '0%' : 'auto'
  }) ?? 'auto'
const overflowX = (el: Element) =>
  winning(el, ['overflow', 'overflow-x'], (p, v) => (p === 'overflow-x' ? v : tokens(v)[0])) ?? 'visible'
const display = (el: Element) => winning(el, ['display'], (_p, v) => v)

const station = (call: string, extra: Partial<Station> = {}): Station =>
  ({ call, grid: 'GD18', snr: -12, lastHeardSlot: 10, heardCount: 3, presence: 'active', worked: false, ...extra }) as Station
const alert = (call: string, tags: NeedTag[]): NeedAlert => ({
  call, entity: 'Falkland Islands', band: '20m', zone: 13, tags, priority: NEED_TIER[tags[0]], headline: '', mode: 'FT8', freqMhz: 14.074,
})

/** The card the defect was measured on: WATCH, three needs, B4, the ULTRA pill, a long call. */
function mountTheWorstCard(): HTMLElement {
  saveWatchlist([{ id: 'w', kind: 'call', value: 'VP8*' }])
  const call = 'VP8/G4ABC/P'
  render(
    <StationList
      stations={[station(call, { country: 'Falkland Islands', worked: true, workedBand: true, gridRarity: 'ultraRare' })]}
      myGrid="EN52"
      currentSlot={10}
      activePeer={null}
      unreadByPeer={{}}
      needByCall={new Map()}
      needAlertsByCall={new Map([[call, [alert(call, ['NewEntity', 'NewZone', 'NewGrid'])]]])}
      band="20m"
      feedMode="FT8"
      onSelect={() => {}}
      onCall={() => {}}
      conversations={[]}
      onArchive={() => {}}
      bandActive={false}
      bandUnread={0}
      onSelectBand={() => {}}
    />,
  )
  return screen.getByTitle(`Double-click to work ${call}`)
}

describe('the Stations card never paints under its SNR badge', () => {
  it('renders the card this guard is about (the fixture cannot silently thin out)', () => {
    const card = mountTheWorstCard()
    const line1 = card.querySelector('.station-line1')!
    const chips = [...line1.children].filter((c) => c.matches('.need-chip, .rarity-chip'))
    // WATCH + NEW + ZONE + GRID + ULTRA — the chips that overran the badge.
    expect(chips.map((c) => c.className)).toEqual([
      'need-chip need-watch',
      'need-chip need-entity',
      'need-chip need-zone',
      'need-chip need-grid',
      'rarity-chip ultra',
    ])
    expect(card.querySelector('.snr-badge')).not.toBeNull()
  })

  it('1. line 1 wraps, so a chip that does not fit starts a new row', () => {
    const card = mountTheWorstCard()
    expect(flexWrap(card.querySelector('.station-line1')!)).toBe('wrap')
  })

  it('2. the SNR badge can drop below a starved call column, whose floor yields', () => {
    const card = mountTheWorstCard()
    expect(flexWrap(card.querySelector('.station-open')!)).toBe('wrap')
    // A floor that must exist under a bounded parent is written to yield (the layout contract):
    // `min(<the call column's floor>, <what the row can give>)`, never a bare length.
    expect(flexBasis(card.querySelector('.station-main')!)).toMatch(/^min\(/)
  })

  it('3. line 2 is a block, so its ellipsis applies instead of running under the badge', () => {
    const line2 = mountTheWorstCard().querySelector('.station-line2')!
    expect(display(line2)).toBe('block')
    expect(overflowX(line2)).toBe('hidden')
  })

  it('4. a card keeps its rows in its scroller instead of being squeezed to its min-height', () => {
    expect(flexShrink(mountTheWorstCard())).toBe('0')
  })

  it('5. a chip wider than the whole line clips inside itself rather than overrunning it', () => {
    const line1 = mountTheWorstCard().querySelector('.station-line1')!
    for (const chip of [...line1.children].filter((c) => c.matches('.need-chip, .rarity-chip'))) {
      expect(overflowX(chip), chip.className).toBe('hidden')
    }
  })
})
