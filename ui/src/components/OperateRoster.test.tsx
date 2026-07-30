// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import { OperateRoster, freshness } from './OperateRoster'
import type { NeedAlert, NeedTag, Station } from '../types'

// The declination fetch is the only mount-time engine call; stub it away.
vi.mock('../api', () => ({
  getDeclination: vi.fn(() => Promise.resolve(0)),
}))

// This project runs vitest WITHOUT auto-cleanup (no setupFiles), so renders otherwise pile
// up in the same document and a "chip is absent" assertion can pass on a leftover from an
// earlier test — or fail on one. Every roster test here renders the same callsign, so the
// teardown is what makes these assertions mean anything.
afterEach(cleanup)

function station(call: string, lastHeardSlot: number): Station {
  return {
    call,
    grid: 'EN52',
    snr: -10,
    lastHeardSlot,
    heardCount: 1,
    presence: 'heard' as Station['presence'],
    worked: false,
  }
}

describe('OperateRoster recency window', () => {
  it('shows only stations heard within the last 3 cycles', () => {
    const currentSlot = 100
    const stations = [
      station('FRESH0', 100), // age 0 — this cycle
      station('FRESH3', 97), // age 3 — the window edge, still shown
      station('STALE4', 96), // age 4 — dropped
      station('STALE99', 1), // long gone — dropped
    ]
    render(
      <OperateRoster
        stations={stations}
        myGrid="EN52"
        currentSlot={currentSlot}
        needByCall={new Map()}
        selectedCall={null}
        onSelect={() => {}}
        onCall={() => {}}
      />,
    )
    expect(screen.queryByText('FRESH0')).not.toBeNull()
    expect(screen.queryByText('FRESH3')).not.toBeNull()
    expect(screen.queryByText('STALE4')).toBeNull()
    expect(screen.queryByText('STALE99')).toBeNull()
  })
})

describe('OperateRoster freshness fade', () => {
  it('dims rows as they age toward the drop-off (full when just heard)', () => {
    expect(freshness(0)).toBe(1)
    expect(freshness(3)).toBeCloseTo(0.5) // window edge → floor
    expect(freshness(1)).toBeGreaterThan(freshness(2)) // monotonically dimmer with age
    expect(freshness(99)).toBe(0.5) // never below the readable floor
  })
})

// Field report 2026-07-29: the roster showed a MODE chip for Asiatic Russia on 30m FT8
// against an operator with six 30m FT8 contacts there. The backend need was real but it
// was a CW need — the roster's per-call alert union carried no band/mode gate, so a chip
// the operator could not close on this surface read as a false "new mode on 30m".
describe('OperateRoster need chips are scoped to the surface', () => {
  const arAlert = (mode: string, tags: NeedTag[] = ['NewMode']): NeedAlert => ({
    call: 'RF9C',
    entity: 'Asiatic Russia',
    band: '30m',
    zone: 17,
    tags,
    priority: 30,
    headline: `New mode — ${mode} Asiatic Russia (any band)`,
    mode,
    freqMhz: null,
  })

  function renderRoster(alert: NeedAlert, band = '30m', feedMode = 'FT8') {
    render(
      <OperateRoster
        stations={[station('RF9C', 100)]}
        myGrid="EN52"
        currentSlot={100}
        needByCall={new Map([['RF9C', alert.tags[0]]])}
        needAlertsByCall={new Map([['RF9C', [alert]]])}
        band={band}
        feedMode={feedMode}
        selectedCall={null}
        onSelect={() => {}}
        onCall={() => {}}
      />,
    )
  }

  it('hides a CW new-mode chip on a 30m FT8 roster', () => {
    renderRoster(arAlert('CW'))
    expect(screen.queryByText('RF9C')).not.toBeNull() // the station still lists
    expect(screen.queryByText('MODE')).toBeNull()
  })

  it('still shows a digital new-mode chip on a 30m FT8 roster', () => {
    renderRoster(arAlert('FT8'))
    expect(screen.queryByText('MODE')).not.toBeNull()
  })

  it('keeps an all-time-new entity chip even from a CW alert (band/mode agnostic)', () => {
    renderRoster(arAlert('CW', ['NewEntity']))
    expect(screen.queryAllByTitle('NEW ONE').length).toBeGreaterThan(0)
  })
})

// The two need systems meeting in the component: the surface gate decides what is
// eligible, then the strongest-need ranking picks from what is left. Neither alone is
// enough — ungated, the CW need is the "strongest" and colours the row MODE (the field
// report); gate-only would lose the strongest-of-several-alerts resolution.
describe('OperateRoster composes the surface gate with strongest-need ranking', () => {
  const twoAlerts: NeedAlert[] = [
    // Real, higher priority, but a CW need the operator cannot close on an FT8 roster.
    {
      call: 'RF9C',
      entity: 'Asiatic Russia',
      band: '30m',
      zone: 17,
      tags: ['NewMode'],
      priority: 30,
      headline: 'New mode — CW Asiatic Russia (any band)',
      mode: 'CW',
      freqMhz: null,
    },
    // Lower priority, but genuinely closable right here.
    {
      call: 'RF9C',
      entity: 'Asiatic Russia',
      band: '30m',
      zone: 17,
      tags: ['Confirm'],
      priority: 10,
      headline: 'Confirm — Asiatic Russia',
      mode: 'FT8',
      freqMhz: null,
    },
  ]

  function renderWith(band?: string, feedMode?: string) {
    render(
      <OperateRoster
        stations={[station('RF9C', 100)]}
        myGrid="EN52"
        currentSlot={100}
        needByCall={new Map([['RF9C', 'NewMode' as NeedTag]])}
        needAlertsByCall={new Map([['RF9C', twoAlerts]])}
        band={band}
        feedMode={feedMode}
        selectedCall={null}
        onSelect={() => {}}
        onCall={() => {}}
      />,
    )
  }

  it('ranks from the strongest CLOSABLE need, not the strongest overall', () => {
    renderWith('30m', 'FT8')
    expect(screen.queryByText('MODE')).toBeNull() // the CW need is withheld here
    expect(screen.queryByText('CNF')).not.toBeNull() // the closable one survives
    // The row names the need it actually ranked from.
    expect(screen.queryByLabelText(/needed Confirm/)).not.toBeNull()
    expect(screen.queryByLabelText(/needed NewMode/)).toBeNull()
  })

  it('leaves hosts that declare no surface on the ungated strongest-need path', () => {
    renderWith(undefined, undefined)
    expect(screen.queryByText('MODE')).not.toBeNull()
    expect(screen.queryByLabelText(/needed NewMode/)).not.toBeNull()
  })
})
