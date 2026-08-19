// @vitest-environment jsdom
//
// OPERATOR REPORT, raised TWICE (2026-08-18): "I selected grids, vhf/uhf 6m and up in the
// settings and its still showing the grid icons in ft8 in both roster and classic mode when
// on hf bands."
//
// Settings ▸ Spots & Alerts carries a per-type BAND SCOPE (alertDxccBands / alertGridBands /
// alertRareGridBands). It gated the SOUND and the TOAST (alerts.ts) and nothing else, so the
// GRID icon kept painting HF rows on both Operate surfaces. The scope now governs the icons
// through the same `bandScopeOk`, and this file pins the two surfaces he named — the unit
// gates live in features/needs.test.ts and features/decodeNeeds.test.ts.
import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import { OperateRoster } from './OperateRoster'
import { OperateDecodes } from './OperateDecodes'
import { visibleNeeds } from '../features/needs'
import type { DecodeRow, NeedAlert, NeedTag, Station } from '../types'

vi.mock('../api', () => ({
  getDeclination: vi.fn(() => Promise.resolve(0)),
  openQrzPage: vi.fn(),
}))

const SLOT = 100
/** The shipped defaults the operator is running: grids VHF+ only, DXCC everywhere. */
const SCOPES = { dxcc: 'all', grid: 'vhf', rareGrid: 'vhf' }
const MODES = { cw: true, phone: true }

function station(call: string): Station {
  return {
    call,
    grid: 'EN52',
    snr: -10,
    lastHeardSlot: SLOT,
    heardCount: 1,
    presence: 'heard' as Station['presence'],
    worked: false,
  }
}

function alert(tags: NeedTag[], band: string, over: Partial<NeedAlert> = {}): NeedAlert {
  return {
    call: 'K1GRID',
    entity: 'United States',
    band,
    zone: 5,
    tags,
    priority: 55,
    headline: 'New grid',
    mode: 'Digital',
    freqMhz: null,
    ...over,
  }
}

function decode(over: Partial<DecodeRow> = {}): DecodeRow {
  return {
    from: 'K1GRID',
    snr: -10,
    dtSec: 0.1,
    freqHz: 1200,
    message: 'CQ K1GRID FN42',
    isCq: true,
    directedToMe: false,
    worked: false,
    tier: 'FT8',
    rv: 0,
    ...over,
  } as DecodeRow
}

/** The roster as App builds it: alerts through the shared gate, then grouped per call. */
function roster(alerts: NeedAlert[], band: string, scopes?: typeof SCOPES) {
  const gated = visibleNeeds(alerts, MODES, scopes)
  const byCall = new Map<string, NeedAlert[]>()
  for (const a of gated) byCall.set(a.call.toUpperCase(), [...(byCall.get(a.call.toUpperCase()) ?? []), a])
  return render(
    <OperateRoster
      stations={[station('K1GRID')]}
      myGrid="EN52"
      currentSlot={SLOT}
      needByCall={new Map()}
      needAlertsByCall={byCall}
      band={band}
      feedMode="Digital"
      selectedCall={null}
      onSelect={() => {}}
      onCall={() => {}}
    />,
  )
}

/** The Band Activity feed as Classic renders it. */
function feed(d: DecodeRow, band: string, scopes?: typeof SCOPES) {
  return render(
    <OperateDecodes
      decodes={[d]}
      slot={SLOT}
      rxOffsetHz={1200}
      band={band}
      tier="FT8"
      harqRescues={0}
      onCall={() => {}}
      needScopes={scopes}
    />,
  )
}

afterEach(() => {
  cleanup()
  localStorage.clear()
})

describe('the grid band scope reaches the ROSTER (his "roster mode")', () => {
  it('HF: a station whose only need is a new grid shows NO grid chip', () => {
    roster([alert(['NewGrid'], '20m')], '20m', SCOPES)
    expect(screen.queryByText('GRID')).toBeNull()
  })

  it('POSITIVE CONTROL — the same station on 6 m still shows it', () => {
    roster([alert(['NewGrid'], '6m')], '6m', SCOPES)
    expect(screen.getByText('GRID')).toBeTruthy()
  })

  it('and with no scopes at all the chip is there on HF (the gate, not the fixture)', () => {
    roster([alert(['NewGrid'], '20m')], '20m')
    expect(screen.getByText('GRID')).toBeTruthy()
  })

  it('tag-level: a new BAND + new GRID on HF keeps BAND, loses only GRID', () => {
    roster([alert(['NewBand', 'NewGrid'], '20m')], '20m', SCOPES)
    expect(screen.getByText('BAND')).toBeTruthy()
    expect(screen.queryByText('GRID')).toBeNull()
  })

  it('a WANTED watch-list entry still tags on HF', () => {
    roster([alert(['Wanted', 'NewGrid'], '20m')], '20m', {
      dxcc: 'off',
      grid: 'off',
      rareGrid: 'off',
    })
    // The roster's dense column renders NEED_CHIP.short — 'WANT', not the board's 'WANTED'.
    expect(screen.getByText('WANT')).toBeTruthy()
  })
})

describe('the grid band scope reaches the DECODE FEED (his "classic mode")', () => {
  // This half is the one the alert-side gate could never reach: the feed's GRID icon comes
  // from the decode's own engine flag, not from a NeedAlert.
  it('HF: a decode carrying an unworked grid shows NO grid icon', () => {
    feed(decode({ newGrid: true }), '20m', SCOPES)
    expect(screen.queryByText('GRID')).toBeNull()
  })

  it('POSITIVE CONTROL — the same decode on 6 m still shows it', () => {
    feed(decode({ newGrid: true }), '6m', SCOPES)
    expect(screen.getByText('GRID')).toBeTruthy()
  })

  it('and with no scopes the icon is there on HF (the gate, not the fixture)', () => {
    feed(decode({ newGrid: true }), '20m')
    expect(screen.getByText('GRID')).toBeTruthy()
  })

  it('a rare 💎 grid follows the rare scope, and comes back when it is widened', () => {
    feed(decode({ newGrid: true, gridRarity: 'ultraRare' }), '20m', SCOPES)
    expect(screen.queryByText('GRID')).toBeNull()
    cleanup()
    feed(decode({ newGrid: true, gridRarity: 'ultraRare' }), '20m', { ...SCOPES, rareGrid: 'all' })
    expect(screen.getByText('GRID')).toBeTruthy()
  })

  it('NEW ONE is untouched on HF at its default scope, and gated when set to VHF+', () => {
    feed(decode({ newDxcc: true }), '20m', SCOPES)
    expect(screen.getByText('NEW ONE')).toBeTruthy()
    cleanup()
    feed(decode({ newDxcc: true }), '20m', { ...SCOPES, dxcc: 'vhf' })
    expect(screen.queryByText('NEW ONE')).toBeNull()
  })
})
