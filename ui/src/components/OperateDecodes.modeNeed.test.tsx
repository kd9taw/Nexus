// @vitest-environment jsdom
//
// BAND ACTIVITY MARKS A "NEW MODE" ONLY IN THE MODE THE NEED IS FOR (operator ruling 2026-10-07,
// "each mode separately").
//
// The report: C5R (The Gambia) calling on 20 m FT8, the log holding The Gambia on 20 m SSB and on
// 40 m FT4 only. FT8 is a mode never worked there, and the callsign card said so. The feed is
// keyed by callsign while a need names one mode, so the feed has to say which mode IT shows: it
// is the tier every row here is decoded in. Described as the class "Digital", it could not tell
// FT8 from FT4, and so a need for either one marked both or neither.
import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { OperateDecodes } from './OperateDecodes'
import type { DecodeRow, NeedAlert, Tier } from '../types'

vi.mock('../api', () => ({ openQrzPage: vi.fn() }))

afterEach(cleanup)

/** C5R calling CQ, decoded on `tier`. */
const c5r = (tier: Tier): DecodeRow => ({
  from: 'C5R',
  snr: -12,
  dtSec: 0.2,
  freqHz: 1200,
  message: 'CQ C5R IK13',
  isCq: true,
  directedToMe: false,
  worked: false,
  tier,
  rv: 0,
})

/** The needs engine's row for C5R: The Gambia never worked on `mode`, any band. */
const newMode = (mode: 'FT8' | 'FT4'): NeedAlert => ({
  call: 'C5R',
  entity: 'The Gambia',
  band: '20m',
  zone: 35,
  tags: ['NewMode'],
  priority: 30,
  headline: `New mode — ${mode} The Gambia (any band)`,
  mode,
  exactMode: mode,
  freqMhz: null,
})

/** The MODE chips on C5R's row, on a feed of `tier` holding `need`. */
function modeChips(tier: Tier, need: NeedAlert): number {
  const { container } = render(
    <OperateDecodes
      decodes={[c5r(tier)]}
      slot={100}
      rxOffsetHz={1200}
      band="20m"
      tier={tier}
      harqRescues={0}
      onCall={() => {}}
      needAlertsByCall={new Map([['C5R', [need]]])}
    />,
  )
  const row = [...container.querySelectorAll('.decode-row')].find((r) => r.textContent?.includes('C5R'))
  if (!row) throw new Error('no row for C5R')
  return row.querySelectorAll('.need-chip.need-mode').length
}

describe('Band Activity marks a new mode only in its own mode', () => {
  it('an FT8 need marks C5R on the FT8 feed', () => {
    expect(modeChips('FT8', newMode('FT8'))).toBe(1)
  })

  it('…and not on the FT4 feed, where working C5R would not close it', () => {
    expect(modeChips('FT4', newMode('FT8'))).toBe(0)
  })

  it('an FT4 need marks the FT4 feed and not the FT8 one', () => {
    expect(modeChips('FT4', newMode('FT4'))).toBe(1)
    cleanup()
    expect(modeChips('FT8', newMode('FT4'))).toBe(0)
  })
})
