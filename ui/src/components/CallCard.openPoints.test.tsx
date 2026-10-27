// @vitest-environment jsdom
//
// WHERE THE STATION CARD OPENS FROM (CallCard.tsx), each on the real surface with the window's card
// host mounted, as App mounts it:
//   (A) the Logbook's call and the Spots board's call open it (the card's header has the QRZ page);
//   (B) the decode list's, the roster's and the station card's QRZ/↗ link opens it, while the row's
//       own click, double-click and Enter keep their FT behaviour;
//   (C) the log strip's narrow card button beside Lookup, which never takes the keyboard from the call
//       field and is not in the contest strip;
//   (D) the call in the map's path readout.
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { AppSnapshot, DecodeRow, FieldDayStatus, LoggedQso, SpotRow, Station } from '../types'
import type { LogQuestion } from '../features/logAnswers'

const engineLog = vi.hoisted(() => ({ rows: [] as unknown[] }))
vi.mock('../api', async (importOriginal) => {
  const real = await importOriginal<typeof import('../api')>()
  return {
    ...real,
    askLog: vi.fn(async (q: LogQuestion) =>
      (await import('../features/logAnswers.testkit')).answerAs(q, engineLog.rows as LoggedQso[]),
    ),
    getCallStats: vi.fn(async () => ({ newOne: false, newBand: false, newMode: false })),
    rotatorBearingToCall: vi.fn(async () => {
      throw new Error('no bearing here')
    }),
    qrzLookup: vi.fn(async () => null),
    openQrzPage: vi.fn(async () => {}),
    getDeclination: vi.fn(async () => 0),
    getAurora: vi.fn(async () => null),
    getPca: vi.fn(async () => null),
    getSatellites: vi.fn(async () => null),
    getLogStats: vi.fn(async () => null),
    getOtaMapSpots: vi.fn(async () => []),
    getPotaActivator: vi.fn(async () => ({ calls: [], parks: [] })),
    logOperators: vi.fn(async () => []),
    logActivations: vi.fn(async () => []),
    lotwSatNames: vi.fn(async () => []),
    resolveEntity: vi.fn(async () => null),
    lookupPark: vi.fn(async () => null),
    lookupParkLive: vi.fn(async () => null),
    searchParks: vi.fn(async () => []),
    setCwPeerInfo: vi.fn(async () => {}),
    setLogFormGrid: vi.fn(async () => {}),
  }
})
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn((fn: () => unknown) => fn()) }))

import * as api from '../api'
import { CallCardHost, closeCallCard } from './CallCard'
import { SpotsPanel } from './SpotsPanel'
import { OperateDecodes } from './OperateDecodes'
import { OperateRoster } from './OperateRoster'
import { StationCard } from './StationCard'
import { LogEntry } from './LogEntry'
import { Logbook } from './Logbook'
import { MapView } from './MapView'

vi.setConfig({ testTimeout: 15_000 })

const SNAP = {
  radio: { band: '20m', dialMhz: 14.074, operatingMode: 'digital', sideband: 'USB' },
  link: { tier: 'FT8' },
  logTick: 1,
  hunt: null,
  mygrid: 'EN52',
} as unknown as AppSnapshot

/** The surface beside the window's one card host, as App mounts it. */
function withHost(surface: React.ReactNode) {
  return render(
    <>
      <CallCardHost snap={SNAP} spots={[]} needAlertsByCall={new Map()} />
      {surface}
    </>,
  )
}

const card = (call: string) => screen.queryByRole('dialog', { name: `Station card for ${call}` })

class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}

beforeAll(() => {
  ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RO
  // The Logbook's list draws rows into the height it measures.
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 900 })
})
beforeEach(() => {
  engineLog.rows = []
  localStorage.clear()
  // jsdom lays nothing out, and a card with no room is never shown: give every box a size.
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue(
    { left: 10, top: 10, right: 70, bottom: 30, width: 60, height: 20, x: 10, y: 10, toJSON: () => ({}) } as DOMRect,
  )
})
afterEach(() => {
  act(() => closeCallCard())
  cleanup()
  vi.restoreAllMocks()
  vi.clearAllMocks()
})

describe('(A) the Logbook and the Spots board', () => {
  it("the Logbook's call opens the card, not the QRZ page", async () => {
    engineLog.rows = [
      { call: 'K1ABC', grid: 'FN31', band: '40m', freqMhz: 7.074, mode: 'FT8', rstSent: '-10', rstRcvd: '-12', whenUnix: 1_700_000_000, confirmed: false, awardConfirmed: false },
    ]
    const { container } = withHost(<Logbook defaultBand="40m" defaultFreqMhz={7.074} defaultMode="FT8" />)
    const call = await waitFor(() => {
      const b = container.querySelector('.logbook-row:not(.head) .qrz-link-call') as HTMLElement | null
      expect(b).not.toBeNull()
      return b!
    })
    fireEvent.click(call)
    expect(card('K1ABC')).not.toBeNull()
    expect(api.openQrzPage).not.toHaveBeenCalled()
    // The card's header carries the page.
    fireEvent.click(card('K1ABC')!.querySelector('.callcard-qrz')!)
    expect(api.openQrzPage).toHaveBeenCalledWith('K1ABC')
  })

  it("the Spots board's call opens the card, and the row's own click (tune) does not fire", () => {
    const onSelect = vi.fn()
    const spot = { call: 'EC1DD', entity: 'Spain', zone: 14, state: null, band: '20m', freqMhz: 14.025, mode: 'CW', submode: 'CW', spotter: 'W3LPL', corroborators: [], ageSecs: 60, comment: '', licensed: true, spotterLocal: true } as unknown as SpotRow
    withHost(<SpotsPanel spots={[spot]} bandPlan={[]} selectedCall={null} onSelect={onSelect} onWork={() => {}} />)
    fireEvent.click(screen.getByRole('button', { name: 'EC1DD' }))
    expect(card('EC1DD')).not.toBeNull()
    expect(onSelect).not.toHaveBeenCalled()
  })
})

describe('(B) the decode list, the roster and the station card: their QRZ/↗ link', () => {
  const decode = { from: 'W1AW', snr: -12, dtSec: 0.2, freqHz: 1200, message: 'CQ W1AW FN31', isCq: true, directedToMe: false, worked: false, tier: 'FT8', rv: 0 } as DecodeRow

  it('decodes: the link opens the card; double-click on the row still works the station', () => {
    const onCall = vi.fn()
    const { container } = withHost(
      <OperateDecodes decodes={[decode]} slot={100} rxOffsetHz={1200} band="20m" tier="FT8" harqRescues={0} onCall={onCall} />,
    )
    const link = container.querySelector('.decode-qrz') as HTMLElement
    fireEvent.click(link)
    expect(card('W1AW')).not.toBeNull()
    expect(api.openQrzPage).not.toHaveBeenCalled()
    fireEvent.dblClick(link)
    expect(onCall, 'a double-click on the link is not a work').not.toHaveBeenCalled()
    fireEvent.dblClick(screen.getByText('CQ W1AW FN31'))
    expect(onCall).toHaveBeenCalledTimes(1)
  })

  const station = { call: 'W1AW', grid: 'FN31', snr: -10, lastHeardSlot: 100, heardCount: 2, presence: 'active', worked: false } as unknown as Station

  it('roster: ↗ opens the card; click, Enter and double-click on the row keep their meaning', () => {
    const onSelect = vi.fn()
    const onCall = vi.fn()
    const { container } = withHost(
      <OperateRoster stations={[station]} myGrid="EN52" currentSlot={100} needByCall={new Map()} selectedCall="W1AW" onSelect={onSelect} onCall={onCall} />,
    )
    fireEvent.click(container.querySelector('.qrz-link')!)
    expect(card('W1AW')).not.toBeNull()
    expect(onSelect).not.toHaveBeenCalled()
    const row = container.querySelector('.or-row[aria-selected]')!
    fireEvent.click(row)
    fireEvent.keyDown(row, { key: 'Enter' })
    expect(onSelect).toHaveBeenCalledTimes(2)
    fireEvent.dblClick(row)
    expect(onCall).toHaveBeenCalledTimes(1)
  })

  it('station card: QRZ opens the card; a double-click on it is not a work', () => {
    const onCall = vi.fn()
    const onSelect = vi.fn()
    const { container } = withHost(
      <StationCard station={station} myGrid="EN52" currentSlot={100} selected={false} unread={0} need={null} needAll={[]} onSelect={onSelect} onCall={onCall} />,
    )
    const qrz = container.querySelector('.station-qrz') as HTMLElement
    fireEvent.click(qrz)
    expect(card('W1AW')).not.toBeNull()
    expect(onSelect).not.toHaveBeenCalled()
    fireEvent.dblClick(qrz)
    expect(onCall).not.toHaveBeenCalled()
  })
})

describe('(C) the log strip', () => {
  it('a narrow card button beside Lookup opens the card for the typed call and leaves the keyboard in the call field', () => {
    withHost(<LogEntry snap={SNAP} mode="SSB" defaultRst="59" exchange="terrestrial" titled={false} />)
    const field = screen.getByPlaceholderText('Call') as HTMLInputElement
    const button = screen.getByRole('button', { name: 'Card' })
    expect(button.hasAttribute('disabled'), 'nothing to show with no call').toBe(true)
    fireEvent.change(field, { target: { value: 'ec1dd' } })
    field.focus()
    expect(button.tabIndex, 'not a Tab stop between the call and the report').toBe(-1)
    const down = fireEvent.mouseDown(button)
    expect(down, 'the press must not move focus out of the call field').toBe(false)
    fireEvent.click(button, { detail: 1 }) // a mouse click: the card opens without taking the keyboard
    expect(card('EC1DD')).not.toBeNull()
    expect(document.activeElement).toBe(field)
  })

  it('the contest strip has no card button', () => {
    const fieldDay = {
      running: true, state: 'Idle', qsoCount: 0, sections: 0, points: 0, log: [], role: '',
      receives: [{ key: 'CLASS', kind: 'pattern', required: true }, { key: 'SECTION', kind: 'enum', required: true, domain: 'fd_sections' }],
      composing: [{ key: 'CLASS', raw: '3A' }, { key: 'SECTION', raw: 'WI', domain: 'fd_sections' }],
    } as unknown as FieldDayStatus
    withHost(<LogEntry snap={SNAP} mode="SSB" defaultRst="59" exchange="terrestrial" titled={false} fieldDay={fieldDay} fdMode="PH" />)
    // The control: the contest strip did render.
    expect(document.querySelector('.log-entry-fd')).not.toBeNull()
    expect(screen.queryByRole('button', { name: 'Card' })).toBeNull()
  })
})

describe('(D) the map', () => {
  it("the call in the path readout opens the card", async () => {
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(
      () =>
        new Proxy({} as Record<string | symbol, unknown>, {
          get: (_t, k) => {
            if (k === 'measureText') return () => ({ width: 0 })
            return () => ({ addColorStop() {} })
          },
        }) as unknown as RenderingContext,
    )
    vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockImplementation(() => 600)
    vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockImplementation(() => 400)
    const station = { call: 'W9NEAR', grid: 'EN61', snr: -10, lastHeardSlot: 1, heardCount: 2, presence: 'active', worked: false } as unknown as Station
    await act(async () => {
      withHost(
        <MapView myGrid="EN52" theme={'dark' as never} stations={[station]} prop={null} selectedCall="W9NEAR" onSelectCall={() => {}} needByCall={new Map()} intent="dx" projection="globe" />,
      )
    })
    fireEvent.click(screen.getByRole('button', { name: 'W9NEAR' }))
    expect(card('W9NEAR')).not.toBeNull()
  })
})
