// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
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

describe('OperateRoster works a station where they were heard', () => {
  it('passes the last-heard offset in the freq slot on a work double-click', () => {
    // The field report: a roster click started the QSO but never moved the waterfall
    // marks, unlike a Band Activity double-click. The roster row carries the station's
    // last decode offset, and the work gesture must hand it to the shared handler in
    // the SAME positional slot Band Activity uses (freq is 5th; message/snr stay
    // undefined — there is no clicked line here).
    const onCall = vi.fn()
    render(
      <OperateRoster
        stations={[{ ...station('RF9C', 100), freqHz: 1512 }]}
        myGrid="EN52"
        currentSlot={100}
        needByCall={new Map()}
        selectedCall={null}
        onSelect={() => {}}
        onCall={onCall}
      />,
    )
    fireEvent.dblClick(screen.getByText('RF9C'))
    expect(onCall).toHaveBeenCalledWith('RF9C', 'EN52', undefined, undefined, 1512)
  })

  it('passes undefined freq for a station heard only by free-text attribution', () => {
    const onCall = vi.fn()
    render(
      <OperateRoster
        stations={[station('K2DEF', 100)]}
        myGrid="EN52"
        currentSlot={100}
        needByCall={new Map()}
        selectedCall={null}
        onSelect={() => {}}
        onCall={onCall}
      />,
    )
    fireEvent.dblClick(screen.getByText('K2DEF'))
    expect(onCall).toHaveBeenCalledWith('K2DEF', 'EN52', undefined, undefined, undefined)
  })
})

describe('OperateRoster hide-blocked (blocklist display half)', () => {
  it('the checkbox drops blocked rows; the working station always stays', () => {
    localStorage.setItem(
      'nexus.roster.filters',
      JSON.stringify({ neededOnly: false, hideWorked: false, hideBlocked: true }),
    )
    render(
      <OperateRoster
        stations={[station('PD2BS', 100), station('K1ABC', 100), station('W5XYZ', 100)]}
        myGrid="EN52"
        currentSlot={100}
        needByCall={new Map()}
        selectedCall={null}
        workingCall="W5XYZ"
        ignoredCalls={new Set(['PD2BS', 'W5XYZ'])}
        onSelect={() => {}}
        onCall={() => {}}
      />,
    )
    expect(screen.queryByText('PD2BS')).toBeNull() // blocked + hidden
    expect(screen.queryByText('K1ABC')).not.toBeNull()
    expect(screen.queryByText('W5XYZ')).not.toBeNull() // blocked BUT being worked — stays
    localStorage.clear()
  })

  it('unchecked (default): blocked rows stay visible (dimmed)', () => {
    render(
      <OperateRoster
        stations={[station('PD2BS', 100)]}
        myGrid="EN52"
        currentSlot={100}
        needByCall={new Map()}
        selectedCall={null}
        ignoredCalls={new Set(['PD2BS'])}
        onSelect={() => {}}
        onCall={() => {}}
      />,
    )
    expect(screen.queryByText('PD2BS')).not.toBeNull()
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

describe('the station being worked is visible in the roster', () => {
  // ISSUE #16, asked for by two operators: "Cannot easily see what station is being worked in
  // Call Roster." There was nothing to see, and not because the highlight was subtle — the roster
  // was never told. App binds `selectedCall` to `activePeer`, which is the Tempo CHAT peer and is
  // null for the whole of an FT8 session, so nothing ever matched.
  function renderWith(props: Record<string, unknown>) {
    return render(
      <OperateRoster
        stations={[station('W1AW', 100), station('K1ABC', 100)]}
        myGrid="EN52"
        currentSlot={100}
        needByCall={new Map()}
        needAlertsByCall={new Map()}
        band="20m"
        feedMode="FT8"
        selectedCall={null}
        onSelect={() => {}}
        onCall={() => {}}
        {...props}
      />,
    )
  }
  const row = (call: string) =>
    screen.getByRole('row', { name: new RegExp(`^${call}`) })

  it('marks the worked station and only that one', () => {
    renderWith({ workingCall: 'W1AW' })
    expect(row('W1AW').className).toContain('working')
    expect(row('K1ABC').className).not.toContain('working')
  })

  it('says so for a screen reader too — the colour is not the only carrier', () => {
    renderWith({ workingCall: 'W1AW' })
    expect(row('W1AW').getAttribute('aria-label')).toContain('working now')
    expect(row('K1ABC').getAttribute('aria-label')).not.toContain('working now')
  })

  it('is not dimmed by the age fade — the contact in progress must stay legible', () => {
    // Rows fade with age; the one you are working must not, however long the QSO runs.
    renderWith({ workingCall: 'W1AW', currentSlot: 102 })
    expect(row('W1AW').style.opacity).toBe('1')
  })

  it('is independent of selection, and a row can be both', () => {
    renderWith({ workingCall: 'W1AW', selectedCall: 'K1ABC' })
    expect(row('W1AW').className).toContain('working')
    expect(row('W1AW').className).not.toContain('selected')
    expect(row('K1ABC').className).toContain('selected')
    cleanup()
    renderWith({ workingCall: 'W1AW', selectedCall: 'W1AW' })
    expect(row('W1AW').className).toContain('working')
    expect(row('W1AW').className).toContain('selected')
  })

  it('omitted behaves exactly as before — no row is marked', () => {
    renderWith({})
    expect(row('W1AW').className).not.toContain('working')
    expect(row('K1ABC').className).not.toContain('working')
  })
})

describe('the roster says who each station is calling, and what state they are in', () => {
  // ISSUE #40: GridTracker's roster shows both, and an operator scanning a busy band wants to
  // know who is already engaged (and with whom) before double-clicking a row. Both facts ride
  // the decode itself — the addressee of the frame, and the FCC callsign→state hint the needed
  // board already resolves. Neither is a callbook lookup.
  const withCols = (extra: Partial<Station>[]) =>
    render(
      <OperateRoster
        stations={extra.map((e, i) => ({ ...station(`W${i}CAL`, 100), ...e }))}
        myGrid="EN52"
        currentSlot={100}
        needByCall={new Map()}
        selectedCall={null}
        onSelect={() => {}}
        onCall={() => {}}
      />,
    )
  const cell = (call: string, cls: string) =>
    screen.getByRole('row', { name: new RegExp(`^${call}`) }).querySelector(cls)?.textContent

  it('shows the call being worked, and CQ when the station is calling nobody', () => {
    withCols([
      { call: 'BUSY', calling: 'K1ABC' },
      { call: 'FREE', calling: null },
    ])
    expect(cell('BUSY', '.or-calling')).toBe('K1ABC')
    expect(cell('FREE', '.or-calling')).toBe('CQ')
  })

  it('shows the state OR province as a pill, and a bare em dash when there is neither', () => {
    withCols([
      { call: 'USSTA', state: 'VT' },
      { call: 'CANSTA', state: 'ON' },
      { call: 'DXSTA', state: null },
    ])
    // A PILL, not plain text (operator, 2026-08-14: "display the actual state or province as a
    // pill icon like you do the rest of them"). The chip element itself carries the code, so a
    // regression back to a bare text node fails here rather than passing on the cell's text.
    expect(cell('USSTA', '.or-state .or-subdiv')).toBe('VT')
    expect(cell('CANSTA', '.or-state .or-subdiv')).toBe('ON')
    // Nothing to say → say nothing, quietly. A pill around an em dash would be chrome
    // advertising an absence, on most rows of a DX-heavy roster.
    expect(cell('DXSTA', '.or-state')).toBe('—')
    expect(cell('DXSTA', '.or-state .or-subdiv')).toBeUndefined()
  })

  it('sorts by state, parking the state-less at the end', () => {
    withCols([
      { call: 'AAA', state: 'VT' },
      { call: 'BBB', state: null },
      { call: 'CCC', state: 'CT' },
    ])
    fireEvent.click(screen.getByRole('button', { name: /^State/ }))
    const calls = screen
      .getAllByRole('row')
      .slice(1)
      .map((r) => r.querySelector('.or-call')?.textContent?.replace('↗', ''))
    expect(calls).toEqual(['CCC', 'AAA', 'BBB'])
  })
})
