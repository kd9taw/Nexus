// @vitest-environment jsdom
//
// THE STATION CARD (CallCard.tsx): what it shows, how it opens and closes, that opening it asks
// nothing of the network, that hover-to-open is off until the operator turns it on, and the rule that
// keeps it off every transmit control. The station is answered at the IPC bridge, as the desktop
// probes do, so every command the card sends is seen; jsdom never lays out, so the geometry is tested
// on `placeCallCard` itself here and in real Chrome by `scripts/browser-probe --desktop callcard`.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import {
  CallCardHost,
  CallCardView,
  callCardTrigger,
  closeCallCard,
  historyGrid,
  liveMode,
  localTime,
  placeCallCard,
  type Box,
  type CallCardData,
} from './CallCard'
import { callCardHoverOn, setCallCardHover } from '../features/callCardHover'
import { qrzLookup } from '../api'
import type { AppSnapshot, LoggedQso, QrzLookup, SpotRow } from '../types'
import type { CallHistory } from '../features/callHistory'

const BOOK: QrzLookup = {
  call: 'EC1DD',
  name: 'Pedro Gomez',
  nickname: null,
  qth: 'Vigo',
  grid: 'IN52TK',
  state: null,
  country: 'Spain',
  dxcc: 281,
  cqZone: 14,
  ituZone: 37,
  image: 'https://example.invalid/ec1dd.jpg',
  county: 'Pontevedra',
  iota: 'EU-080',
  email: 'ec1dd@example.invalid',
  web: 'example.invalid/ec1dd',
  qslVia: 'EA1RCI',
  lotw: true,
  eqsl: false,
  paperQsl: true,
  utcOffset: 1,
}

const qso = (over: Partial<LoggedQso>): LoggedQso =>
  ({
    call: 'EC1DD',
    grid: 'IN52',
    band: '20m',
    freqMhz: 14.074,
    mode: 'FT8',
    rstSent: '-10',
    rstRcvd: '-12',
    whenUnix: 1_700_000_000,
    confirmed: false,
    awardConfirmed: false,
    ...over,
  }) as LoggedQso

const LOG = [
  qso({ band: '20m', mode: 'FT8', confirmed: true, whenUnix: 1_700_000_000 }),
  qso({ band: '40m', mode: 'CW', whenUnix: 1_760_000_000, name: 'Pedro', qth: 'Vigo', country: 'Spain' }),
]

const HIST: CallHistory = {
  qsos: LOG,
  count: 2,
  workedBefore: true,
  dupeThisBand: true,
  lastUnix: 1_760_000_000,
  confirmedCount: 1,
  bands: ['20m', '40m'],
  modes: ['FT8', 'CW'],
}

const SPOT = { call: 'EC1DD', freqMhz: 14.025, spotter: 'W3LPL', ageSecs: 180, band: '20m' } as SpotRow

const data = (over: Partial<CallCardData> = {}): CallCardData => ({
  call: 'EC1DD',
  book: BOOK,
  looking: false,
  hist: HIST,
  stats: { newOne: false, newBand: true, newMode: false } as CallCardData['stats'],
  needs: ['NewBand'],
  spot: SPOT,
  aim: { bearing: { pointed: { bearing: 227.4, to: 'grid', grid: 'IN52TK', country: null }, km: 1530.6 } } as CallCardData['aim'],
  band: '20m',
  mode: 'FT8',
  nowMs: Date.UTC(2026, 9, 24, 12, 30),
  ...over,
})

describe('the card shows what Nexus knows about the call', () => {
  afterEach(cleanup)

  it('callbook, path, local time, QSL, needs, the band × mode grid, the last contact and the spot', () => {
    const { container } = render(<CallCardView data={data()} onClose={() => {}} />)
    const text = container.textContent ?? ''
    for (const s of ['EC1DD', 'Pedro Gomez', 'Vigo', 'Pontevedra', 'Spain', 'IN52TK', 'CQ 14', 'ITU 37', 'IOTA EU-080',
      'ec1dd@example.invalid', 'example.invalid/ec1dd', 'QSL via EA1RCI', 'LoTW', 'card', 'NEW BAND',
      'Their time 13:30 (UTC+1)', '2 contacts in the log', 'Last 2025-10-09 on 40m CW', 'Spotted on 14.025 by W3LPL, 3 min ago']) {
      expect(text, s).toContain(s)
    }
    expect(text).toMatch(/SP .*· 227° · LP .*· 47°/)
    expect(container.querySelector('img.callcard-photo')?.getAttribute('src')).toBe(BOOK.image)
    expect(container.querySelector('[data-slot="confirmed"]')).not.toBeNull()
    expect(container.querySelector('[data-slot="worked"]')).not.toBeNull()
    expect(screen.getByRole('button', { name: /QRZ/ })).toBeTruthy()
  })

  it('before the callbook is asked, who and where come from the newest contact, with Look up to ask it', () => {
    const lookup = vi.fn()
    const { container } = render(<CallCardView data={data({ book: null, onLookup: lookup })} onClose={() => {}} />)
    expect(container.textContent).toContain('Pedro')
    expect(container.textContent).toContain('Vigo · Spain')
    expect(container.querySelector('img')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Lookup' }))
    expect(lookup).toHaveBeenCalledTimes(1)
  })

  it('a callbook answer with no photo says why the initials show, instead of saying nothing', () => {
    const { container } = render(<CallCardView data={data({ book: { ...BOOK, image: null } })} onClose={() => {}} />)
    expect(container.querySelector('img')).toBeNull()
    expect(container.querySelector('.callcard-avatar')?.getAttribute('title')).toBe(
      'The callbook sent no photo. QRZ sends photos only to XML subscribers, and only when the station posted one.',
    )
    cleanup()
    // Not asked yet: nothing to explain, the Look up is there.
    const before = render(<CallCardView data={data({ book: null, onLookup: () => {} })} onClose={() => {}} />)
    expect(before.container.querySelector('.callcard-avatar')?.hasAttribute('title')).toBe(false)
  })

  it('the grid orders bands by wavelength and a confirmed contact wins its cell', () => {
    const g = historyGrid({ ...HIST, qsos: [...LOG, qso({ band: '20m', mode: 'FT8', confirmed: false })] })
    expect(g.bands).toEqual(['40m', '20m'])
    expect(g.cell('20m', 'FT8')).toBe('confirmed')
    expect(g.cell('40m', 'CW')).toBe('worked')
    expect(g.cell('40m', 'FT8')).toBeNull()
  })

  it('local time from a half-hour offset', () => {
    expect(localTime(Date.UTC(2026, 0, 1, 0, 0), 5.5)).toEqual({ time: '05:30', offset: 'UTC+5:30' })
    expect(localTime(Date.UTC(2026, 0, 1, 0, 0), -5)).toEqual({ time: '19:00', offset: 'UTC−5' })
  })
})

describe('it never covers a transmit control', () => {
  const VIEW = { w: 1366, h: 768 }
  const SIZE = { w: 320, h: 300 }
  const box = (left: number, top: number, right: number, bottom: number): Box => ({ left, top, right, bottom })
  const hits = (p: { left: number; top: number; width: number; maxHeight: number }, b: Box) =>
    p.left < b.right && p.left + p.width > b.left && p.top < b.bottom && p.top + p.maxHeight > b.top

  it('a call just above the TX strip and dock: the card goes above the call, clear of both', () => {
    const strip = box(0, 600, 1366, 640)
    const dock = box(0, 700, 1366, 768)
    const call = box(100, 570, 160, 590)
    const p = placeCallCard(call, SIZE, VIEW, [strip, dock])!
    expect(p).not.toBeNull()
    expect(hits(p, strip)).toBe(false)
    expect(hits(p, dock)).toBe(false)
    expect(hits(p, call)).toBe(false)
    expect(p.top + p.maxHeight).toBeLessThanOrEqual(call.top)
  })

  it('the control: with no transmit boxes given, the same call puts the card over where the strip is', () => {
    const p = placeCallCard(box(100, 570, 160, 590), SIZE, VIEW, [])!
    expect(hits(p, box(0, 600, 1366, 640))).toBe(true)
  })

  it('a Stop button in every column: the card is shorter and scrolls, never over Stop', () => {
    const stops = [box(0, 300, 1366, 330)]
    const p = placeCallCard(box(600, 100, 660, 120), { w: 320, h: 600 }, VIEW, stops)!
    expect(hits(p, stops[0])).toBe(false)
    expect(p.maxHeight).toBeLessThan(600)
  })

  it('a card shorter than the least worth drawing still shows whole', () => {
    expect(placeCallCard(box(100, 100, 160, 120), { w: 200, h: 40 }, VIEW, [])).toMatchObject({ maxHeight: 40 })
  })

  it('no room anywhere: it does not show', () => {
    const wall = [box(0, 0, 1366, 380), box(0, 420, 1366, 768)]
    expect(placeCallCard(box(10, 385, 60, 415), SIZE, VIEW, wall)).toBeNull()
  })
})

// ---- the card in the window, with the station answered at the IPC bridge ----

const sent: string[] = []
const answers: Record<string, (args: Record<string, unknown>) => unknown> = {
  ask_log: () => {
    throw new Error('not answered')
  },
  get_call_stats: () => ({ call: 'EC1DD', bands: [], modes: [], newOne: false, newBand: true, newMode: false, dxcc: null, wasFamily: false, was: null, waz: null, countryQsl: null, callQsl: {} }),
  rotator_bearing_to_call: () => ({ pointed: { bearing: 227.4, to: 'grid', grid: 'IN52TK', country: null }, km: 1530.6 }),
  qrz_lookup: () => BOOK,
  open_qrz_page: () => null,
}

const SNAP = {
  radio: { band: '20m', operatingMode: 'digital', sideband: 'USB' },
  link: { tier: 'FT8' },
  logTick: 1,
} as unknown as AppSnapshot

function Window({ fallback }: { fallback?: () => void }) {
  return (
    <>
      <CallCardHost snap={SNAP} spots={[SPOT]} needAlertsByCall={new Map()} />
      <button type="button" {...callCardTrigger('ec1dd', fallback)}>EC1DD</button>
      <button type="button">elsewhere</button>
    </>
  )
}

beforeEach(() => {
  sent.length = 0
  localStorage.clear()
  // jsdom lays nothing out, and a card with no room is never shown: give every box a size.
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue(
    { left: 10, top: 10, right: 70, bottom: 30, width: 60, height: 20, x: 10, y: 10, toJSON: () => ({}) } as DOMRect,
  )
  window.__TAURI_INTERNALS__ = {
    invoke: (cmd: string, args?: Record<string, unknown>) => {
      sent.push(cmd)
      const a = answers[cmd]
      if (!a) return Promise.reject(new Error(`${cmd} is not answered here`))
      try {
        return Promise.resolve(a(args ?? {}))
      } catch (e) {
        return Promise.reject(e)
      }
    },
  } as never
})
afterEach(() => {
  act(() => closeCallCard())
  cleanup()
  vi.useRealTimers()
  vi.restoreAllMocks()
})

const card = () => screen.queryByRole('dialog', { name: 'Station card for EC1DD' })

describe('opening and closing', () => {
  it('a click opens it, and nothing leaves the computer: no callbook request on open', async () => {
    render(<Window />)
    fireEvent.click(screen.getByRole('button', { name: 'EC1DD' }))
    expect(card()).not.toBeNull()
    await waitFor(() => expect(sent).toContain('get_call_stats'))
    expect(sent).not.toContain('qrz_lookup')
    expect(sent.every((c) => ['ask_log', 'get_call_stats', 'rotator_bearing_to_call'].includes(c)), sent.join()).toBe(true)
  })

  it("the header's Look up asks the callbook once, and its QRZ button opens the call's page", async () => {
    render(<Window />)
    fireEvent.click(screen.getByRole('button', { name: 'EC1DD' }))
    fireEvent.click(screen.getByRole('button', { name: 'Lookup' }))
    await waitFor(() => expect(card()!.textContent).toContain('Pedro Gomez'))
    expect(sent.filter((c) => c === 'qrz_lookup')).toHaveLength(1)
    fireEvent.click(screen.getByRole('button', { name: /QRZ/ }))
    await waitFor(() => expect(sent).toContain('open_qrz_page'))
  })

  it("a call the window already looked up (Phone's log strip) opens with that answer and its photo, asking nothing more", async () => {
    // The log strip's lookup, before its Card button is pressed. A call no other test has looked up.
    await act(async () => {
      await qrzLookup('k1abc')
    })
    expect(sent.filter((c) => c === 'qrz_lookup')).toHaveLength(1)
    render(
      <>
        <CallCardHost snap={SNAP} spots={[]} needAlertsByCall={new Map()} />
        <button type="button" {...callCardTrigger('K1ABC')}>K1ABC</button>
      </>,
    )
    fireEvent.click(screen.getByRole('button', { name: 'K1ABC' }))
    const open = screen.getByRole('dialog', { name: 'Station card for K1ABC' })
    expect(open.textContent).toContain('Pedro Gomez')
    expect(open.querySelector('img.callcard-photo')?.getAttribute('src')).toBe(BOOK.image)
    expect(screen.queryByRole('button', { name: 'Lookup' })).toBeNull()
    await waitFor(() => expect(sent).toContain('get_call_stats'))
    expect(sent.filter((c) => c === 'qrz_lookup')).toHaveLength(1)
  })

  it('Esc closes it, and the key is not swallowed (it is the stop key too)', () => {
    render(<Window />)
    fireEvent.click(screen.getByRole('button', { name: 'EC1DD' }))
    const esc = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })
    act(() => {
      window.dispatchEvent(esc)
    })
    expect(card()).toBeNull()
    expect(esc.defaultPrevented).toBe(false)
  })

  it('a press outside closes it; a press inside does not', () => {
    render(<Window />)
    fireEvent.click(screen.getByRole('button', { name: 'EC1DD' }))
    fireEvent.pointerDown(card()!)
    expect(card()).not.toBeNull()
    fireEvent.pointerDown(screen.getByRole('button', { name: 'elsewhere' }))
    expect(card()).toBeNull()
  })

  it('Enter on the call (a keyboard click) opens it with the keyboard inside, and Esc gives it back', async () => {
    render(<Window />)
    const call = screen.getByRole('button', { name: 'EC1DD' })
    call.focus()
    fireEvent.click(call, { detail: 0 })
    await waitFor(() => expect(card()!.contains(document.activeElement)).toBe(true))
    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    })
    expect(card()).toBeNull()
    expect(document.activeElement).toBe(call)
  })

  it('with no card in the window (a pop-out), the call does what it did before', () => {
    const fallback = vi.fn()
    render(<button type="button" {...callCardTrigger('EC1DD', fallback)}>EC1DD</button>)
    fireEvent.click(screen.getByRole('button', { name: 'EC1DD' }))
    expect(fallback).toHaveBeenCalledTimes(1)
  })
})

describe('hover to open', () => {
  const fine = () =>
    vi.spyOn(window, 'matchMedia').mockReturnValue({ matches: true } as MediaQueryList)

  it('is off by default: resting on a call opens nothing', () => {
    if (typeof window.matchMedia !== 'function') window.matchMedia = (() => ({ matches: true })) as never
    fine()
    vi.useFakeTimers()
    expect(callCardHoverOn()).toBe(false)
    render(<Window />)
    fireEvent.mouseEnter(screen.getByRole('button', { name: 'EC1DD' }))
    act(() => {
      vi.advanceTimersByTime(2000)
    })
    expect(card()).toBeNull()
  })

  it('turned on, resting half a second opens it and leaving closes it', () => {
    if (typeof window.matchMedia !== 'function') window.matchMedia = (() => ({ matches: true })) as never
    fine()
    vi.useFakeTimers()
    setCallCardHover(true)
    render(<Window />)
    const call = screen.getByRole('button', { name: 'EC1DD' })
    fireEvent.mouseEnter(call)
    act(() => {
      vi.advanceTimersByTime(400)
    })
    expect(card()).toBeNull()
    act(() => {
      vi.advanceTimersByTime(200)
    })
    expect(card()).not.toBeNull()
    fireEvent.mouseLeave(call)
    act(() => {
      vi.advanceTimersByTime(400)
    })
    expect(card()).toBeNull()
  })
})

// NEW MODE is new against the mode the radio is working. PSK and JTTY are one operating section, so
// the section alone named a JTTY station PSK31; the snapshot's `keyboardMode` says which.
describe('the mode the radio is working', () => {
  const at = (radio: Partial<AppSnapshot['radio']>) =>
    liveMode({ radio: { band: '20m', sideband: 'USB', ...radio } as AppSnapshot['radio'], link: { tier: 'FT8' } as AppSnapshot['link'] })
  it('names the keyboard mode on the air, and PSK31 from a station too old to say', () => {
    expect(at({ operatingMode: 'keyboard', keyboardMode: 'JTTY' })).toBe('JTTY')
    expect(at({ operatingMode: 'keyboard', keyboardMode: 'QPSK31' })).toBe('QPSK31')
    expect(at({ operatingMode: 'keyboard' })).toBe('PSK31')
    expect(at({ operatingMode: 'rtty', keyboardMode: 'JTTY' }), 'another section: its own mode').toBe('RTTY')
  })
})
