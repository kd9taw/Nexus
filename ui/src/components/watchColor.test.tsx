// @vitest-environment jsdom
//
// #299 — A COLOUR PER WATCHED CALL. An operator asked to set one watched call "bright pink" so it is
// obvious the moment it shows on the Call Roster. Every watched station wore the same lime.
//
// What this holds:
//   · the Settings manager sets an entry's colour from a small fixed palette, and clears it again;
//   · the list keeps it, and drops a colour that is not one of the palette's (a damaged file);
//   · wherever a watched station is marked — the WATCH tile on the Call Roster (and the Stations
//     cards and Spots, which draw the same tile) and the Band Activity row — the mark paints in that
//     colour, by re-pointing the one token every watch mark reads (`--need-watch`);
//   · an entry with no colour keeps the lime (the control), and the station is never sent a colour;
//   · every palette colour is defined in BOTH themes (computed from the sheet, not a regex), and its
//     chip and row tint read in every mode the contrast guards sweep.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { OperateRoster } from './OperateRoster'
import { OperateDecodes } from './OperateDecodes'
import { WatchlistPanel } from './WatchlistPanel'
import { WATCH_COLORS, loadWatchlist, saveWatchlist, type WatchFilter } from '../watchlist'
import { NEED_TIER } from '../features/needs'
import { MODES, contrast, expandWith, isNight, parseRules, rootTokensFrom, toRgb, type Rgb } from '../cssCascade'
import type { DecodeRow, NeedAlert, Station } from '../types'

vi.mock('../api', () => ({
  getDeclination: vi.fn(() => Promise.resolve(0)),
  openQrzPage: vi.fn(),
}))

const PINK: WatchFilter = { id: 'w-pink', kind: 'call', value: 'W6E', color: 'pink' }
const PLAIN: WatchFilter = { id: 'w-plain', kind: 'call', value: 'K1ABC' }

const SLOT = 100
const station = (call: string): Station => ({
  call,
  grid: 'EN52',
  snr: -10,
  lastHeardSlot: SLOT,
  heardCount: 1,
  presence: 'active',
  worked: false,
  country: 'United States',
})

beforeEach(() => {
  localStorage.clear()
  saveWatchlist([PINK, PLAIN])
})
afterEach(cleanup)

const needWatchOf = (el: Element | null | undefined) => (el as HTMLElement | null)?.style.getPropertyValue('--need-watch') ?? null

describe('#299 the watch list keeps a colour per entry', () => {
  it('loads a palette colour and drops one that is not on the palette, keeping the entry', () => {
    saveWatchlist([PINK, { ...PLAIN, color: 'chartreuse' as never }])
    const [pink, plain] = loadWatchlist()
    expect(pink.color).toBe('pink')
    expect(plain.id).toBe('w-plain')
    expect('color' in plain).toBe(false)
  })

  it('the Settings manager sets an entry’s colour, and Default takes it off', () => {
    render(<WatchlistPanel />)
    const pick = screen.getByRole('combobox', { name: /Colour for K1ABC/i })
    fireEvent.change(pick, { target: { value: 'cyan' } })
    expect(loadWatchlist().find((f) => f.id === 'w-plain')?.color).toBe('cyan')
    fireEvent.change(pick, { target: { value: '' } })
    expect('color' in loadWatchlist().find((f) => f.id === 'w-plain')!).toBe(false)
    // The other entry is untouched by either edit.
    expect(loadWatchlist().find((f) => f.id === 'w-pink')?.color).toBe('pink')
  })
})

describe('#299 a watched call is marked in its colour', () => {
  it('the Call Roster’s WATCH tile wears the entry’s colour; an uncoloured entry keeps the lime', () => {
    render(
      <OperateRoster
        stations={[station('W6E'), station('K1ABC')]}
        myGrid="EN52"
        currentSlot={SLOT}
        needByCall={new Map()}
        needAlertsByCall={new Map()}
        band="20m"
        feedMode="FT8"
        selectedCall={null}
        onSelect={() => {}}
        onCall={() => {}}
      />,
    )
    const tile = (call: string) => screen.getByText(call).closest('[role="row"]')!.querySelector('.need-chip.need-watch')
    expect(needWatchOf(tile('W6E'))).toBe('var(--watch-pink)')
    expect(needWatchOf(tile('K1ABC'))).toBe('')
  })

  it('a Band Activity row of a watched call wears the entry’s colour; the control keeps the lime', () => {
    const decode = (from: string, freqHz: number): DecodeRow => ({
      from,
      snr: -12,
      dtSec: 0.2,
      freqHz,
      message: `CQ ${from} EN52`,
      isCq: true,
      directedToMe: false,
      worked: false,
      tier: 'FT8',
      rv: 0,
    })
    const wanted = (call: string): NeedAlert => ({
      call,
      entity: 'United States',
      band: '20m',
      zone: 4,
      tags: ['Wanted'],
      priority: NEED_TIER.Wanted,
      headline: '',
      mode: 'Digital',
      freqMhz: null,
    })
    render(
      <OperateDecodes
        decodes={[decode('W6E', 1000), decode('K1ABC', 1500)]}
        slot={100}
        rxOffsetHz={1200}
        band="20m"
        tier="FT8"
        harqRescues={0}
        onCall={() => {}}
        needAlertsByCall={new Map([['W6E', [wanted('W6E')]], ['K1ABC', [wanted('K1ABC')]]])}
      />,
    )
    const row = (call: string) => screen.getAllByText(new RegExp(call))[0].closest('.decode-row') as HTMLElement
    expect(row('W6E').classList.contains('need-watch')).toBe(true)
    expect(needWatchOf(row('W6E'))).toBe('var(--watch-pink)')
    expect(row('K1ABC').classList.contains('need-watch')).toBe(true)
    expect(needWatchOf(row('K1ABC'))).toBe('')
  })
})

describe('#299 the palette reads in both themes', () => {
  const css = readFileSync(resolve(process.cwd(), 'src', 'styles.css'), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '')
  const rules = parseRules(css)

  it('every palette colour resolves to a different value in the dark and the light theme', () => {
    const dark = rootTokensFrom(rules, 'dark')
    const light = rootTokensFrom(rules, 'light')
    for (const c of WATCH_COLORS) {
      const d = dark.get(`--watch-${c}`)
      const l = light.get(`--watch-${c}`)
      expect(d, `--watch-${c} has no dark value`).toBeTruthy()
      expect(l, `--watch-${c} has no light value`).toBeTruthy()
      expect(d, `--watch-${c} is one value for both themes`).not.toBe(l)
    }
  })

  // The tile letters its word in the theme's ink on a tint of its colour (24 %, 10 % at night) and
  // draws its border in the colour at full strength; a Band Activity row is a 24 % tint under the
  // same ink. Measured on every surface a list sits on, in every swept mode. The lime is the control:
  // whatever floor the palette has to meet, the shipped colour meets it too.
  const SURFACES = ['--bg', '--panel', '--bg-elev', '--bg-elev-2']
  for (const name of ['need-watch', ...WATCH_COLORS.map((c) => `watch-${c}`)]) {
    it(`--${name}: the word reads 4.5:1 on its tint and the border shows 3:1, in every mode`, () => {
      const fails: string[] = []
      for (const mode of MODES) {
        const tokens = rootTokensFrom(rules, mode)
        const at = (v: string, under: Rgb) => toRgb(expandWith(tokens, v), under)
        const tint = isNight(mode) ? 10 : 24
        for (const s of SURFACES) {
          const surface = at(`var(${s})`, [0, 0, 0])
          if (!surface) continue
          const colour = at(`var(--${name})`, surface)!
          const ink = at('var(--text)', surface)!
          const chip = at(`color-mix(in srgb, var(--${name}) ${tint}%, transparent)`, surface)!
          const word = contrast(ink, chip)
          const border = contrast(colour, surface)
          if (word < 4.5) fails.push(`${mode} on ${s}: word ${word.toFixed(2)}:1`)
          if (border < 3) fails.push(`${mode} on ${s}: border ${border.toFixed(2)}:1`)
        }
      }
      expect(fails).toEqual([])
    })
  }
})
