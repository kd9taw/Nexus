// @vitest-environment jsdom
// Remote browser alerts for rare DX (the station's Pounce) and new POTA activations. Both are
// opt-in, silent on their first read, notify each item once, and never reach the station with
// anything but their read.
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, renderHook } from '@testing-library/react'

vi.mock('../alerts', async (original) => ({ ...(await original<typeof import('../alerts')>()), doubleBeep: vi.fn() }))
vi.mock('../api', async (original) => ({ ...(await original<typeof import('../api')>()), osNotify: vi.fn(async () => {}) }))

import { doubleBeep } from '../alerts'
import { osNotify } from '../api'
import { processPotaAlert, resetPotaAlertsForTest } from '../features/potaAlert'
import { usePounce, type PounceAlert } from '../usePounce'
import type { OtaMapSpot, OtaSpot } from '../types'
import { SessionStatus } from './SessionStatus'
import { RemoteCollections } from './collections'
import type { ApplicationClient } from './application-client'
import type { QueryPage } from './application-query-protocol'
import { useNeedAlerts } from './useNeedAlerts'
import { RARE_ALERT_POLL_MS, useRareDxAlerts } from './useRareDxAlerts'
import { POTA_ALERT_POLL_MS, usePotaAlerts } from './usePotaAlerts'
import fixture from './__fixtures__/ota.json'

class FakeNotification {
  static permission: NotificationPermission = 'default'
  static answer: NotificationPermission = 'granted'
  static requestPermission = vi.fn(async () => { FakeNotification.permission = FakeNotification.answer; return FakeNotification.answer })
  static shown: FakeNotification[] = []
  onclick: (() => void) | null = null
  close = vi.fn()
  constructor(readonly title: string, readonly options: NotificationOptions) { FakeNotification.shown.push(this) }
}

// ── The station, as the browser sees it: two reads behind a transport spy ───────────────────
let ring: PounceAlert[] = []
let threshold = 'atno'
let potaSpots: OtaSpot[] = []
let potaStatus: 'ready' | 'expired' = 'ready'
const sent: string[] = []
const pounce = (call: string, atUnix: number, over: Partial<PounceAlert> = {}): PounceAlert => ({ call, band: '20m', mode: 'CW',
  freqMhz: 14.025, tags: ['NewEntity'], entity: 'Bouvet Island', atUnix, ...over })
const spot = (reference: string, activator = 'K3XYZ', name = `Park ${reference}`): OtaSpot => {
  // The station's hunter-feed row also carries spotTimeUnix, which the shared OtaSpot type omits.
  const row = { program: 'POTA', reference, name, activator, freqKhz: 14062, mode: 'CW', spotter: null, comment: null, grid: null,
    lat: 41.7, lon: -72.7, spotTimeUnix: 1000, newPark: true, bandOpen: false }
  return row
}
const mapSpot = (s: OtaSpot): OtaMapSpot => ({ program: s.program, reference: s.reference, name: s.name, activator: s.activator,
  freqMhz: s.freqKhz / 1000, mode: s.mode, lat: s.lat ?? 0, lon: s.lon ?? 0, approx: false, ageSecs: 0, newRef: true })
const base = { type: 'applicationPage', offset: 0, nextCursor: null, ageMs: 0 } as const
function pouncePage(): QueryPage {
  return { ...base, requestId: crypto.randomUUID(), snapshotId: crypto.randomUUID(), collection: 'pounce' as QueryPage['collection'],
    total: ring.length, retained: ring.length, rows: structuredClone(ring) as unknown as QueryPage['rows'], meta: { capturedAgeMs: 0, source: { threshold } } }
}
function otaPage(): QueryPage {
  const source = structuredClone(fixture)
  source.feeds[0] = { program: 'POTA', status: potaStatus, sourceAgeMs: potaStatus === 'ready' ? 1000 : 900_000,
    spots: potaStatus === 'ready' ? structuredClone(potaSpots) as typeof source.feeds[0]['spots'] : [] }
  return { ...base, requestId: crypto.randomUUID(), snapshotId: crypto.randomUUID(), collection: 'ota', total: 0, retained: 0, rows: [],
    meta: { capturedAgeMs: 0, source: source as unknown as QueryPage['meta'] } }
}
const invoke = vi.fn(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
  sent.push(command)
  if (command === 'get_remote_pounce' && args?.collection === 'pounce') return pouncePage()
  if (command === 'get_remote_ota' && args?.collection === 'ota') return otaPage()
  throw new Error('applicationUnsupported')
})
const client = { invoke, supports: () => true, getPhase: () => 'ready' } as unknown as ApplicationClient
let source: RemoteCollections

function Harness({ ready = true, offered = true }: { ready?: boolean; offered?: boolean }) {
  return <SessionStatus stale={false} disconnect={() => {}} alerts={useNeedAlerts(null, false, null)}
    rareAlerts={useRareDxAlerts(source, ready, offered)} potaAlerts={usePotaAlerts(source, ready, offered)} />
}
const button = (container: HTMLElement, label: string) => [...container.querySelectorAll('button')].find(b => b.textContent === label)
const panel = (container: HTMLElement) => container.querySelector('.remote-need-alerts')?.textContent ?? ''
const shown = () => FakeNotification.shown.map(n => ({ title: n.title, body: n.options.body, tag: n.options.tag }))
const titles = () => FakeNotification.shown.map(n => n.title)
const flush = () => act(async () => { await vi.advanceTimersByTimeAsync(0) })
const poll = (ms: number) => act(async () => { await vi.advanceTimersByTimeAsync(ms) })

beforeEach(() => {
  vi.useFakeTimers()
  localStorage.clear(); sent.length = 0; invoke.mockClear()
  ring = []; threshold = 'atno'; potaSpots = []; potaStatus = 'ready'
  Object.assign(FakeNotification, { permission: 'default', answer: 'granted', shown: [] }); FakeNotification.requestPermission.mockClear()
  vi.stubGlobal('Notification', FakeNotification)
  vi.mocked(doubleBeep).mockClear(); vi.mocked(osNotify).mockClear()
  source = new RemoteCollections(client)
})
afterEach(() => { cleanup(); source.dispose(); vi.useRealTimers(); vi.unstubAllGlobals(); vi.restoreAllMocks() })

it('rare DX: off until clicked, silent on the first read and on replay, once per raised alert, and the click only focuses', async () => {
  ring = [pounce('VP8PJ', 100, { entity: 'South Orkney Islands' })]
  const view = render(<Harness />)
  await poll(RARE_ALERT_POLL_MS)
  expect(sent).toEqual([])
  expect(FakeNotification.requestPermission).not.toHaveBeenCalled()

  const rare = button(view.container, 'Alert me about rare DX')!
  await act(async () => { fireEvent.click(rare) })
  expect(FakeNotification.requestPermission).toHaveBeenCalledOnce()
  await flush()
  expect(sent).toEqual(['get_remote_pounce'])
  expect(titles()).toEqual([]) // what the station raised before this browser asked is not news
  expect(rare.getAttribute('aria-pressed')).toBe('true')

  ring = [...ring, pounce('3Y0J', 200)]
  await poll(RARE_ALERT_POLL_MS)
  expect(shown()).toEqual([{ title: 'New one: Bouvet Island', body: '3Y0J — 14.025 MHz CW', tag: 'pounce-3Y0J-20m-CW' }])
  await poll(RARE_ALERT_POLL_MS)
  expect(titles()).toHaveLength(1)

  const focus = vi.spyOn(window, 'focus').mockImplementation(() => {})
  FakeNotification.shown[0].onclick!()
  expect(focus).toHaveBeenCalledOnce()
  expect(FakeNotification.shown[0].close).toHaveBeenCalledOnce()

  // The station link drops; an alert is raised meanwhile. The first read back is a baseline.
  view.rerender(<Harness ready={false} />)
  ring = [...ring, pounce('FT5ZM', 300, { entity: 'Amsterdam & St. Paul Is.', band: '15m', freqMhz: 21.074, mode: 'FT8' })]
  await poll(RARE_ALERT_POLL_MS)
  view.rerender(<Harness ready />)
  await flush()
  expect(titles()).toHaveLength(1)
  // Positive control for that silence: an alert raised after the baseline still notifies.
  ring = [...ring, pounce('BS7H', 400, { entity: 'Scarborough Reef', band: '17m', freqMhz: null })]
  await poll(RARE_ALERT_POLL_MS)
  expect(titles()).toEqual(['New one: Bouvet Island', 'New one: Scarborough Reef'])
  expect(shown()[1].body).toBe('BS7H — 17m CW')
  expect(localStorage.getItem('nexus.remote.rareDxAlerts')).toBe('on')

  // Notify only: nothing but the read ever reached the station, clicks included.
  expect(new Set(sent)).toEqual(new Set(['get_remote_pounce']))
  // Positive control for the spy: a command sent through this transport IS recorded.
  await source.invoke('set_frequency', { hz: 14_025_000 }).catch(() => {})
  expect(sent[sent.length - 1]).toBe('set_frequency')

  await act(async () => { fireEvent.click(rare) })
  const reads = sent.length
  ring = [...ring, pounce('P5DX', 500, { entity: 'North Korea' })]
  await poll(RARE_ALERT_POLL_MS); await poll(RARE_ALERT_POLL_MS)
  expect(sent.length).toBe(reads)
  expect(titles()).toHaveLength(2)
  expect(localStorage.getItem('nexus.remote.rareDxAlerts')).toBe('off')
})

it('rare DX: notifies for exactly the alerts the desktop Pounce raised, worded as the desktop words them', async () => {
  let deliver: ((event: { payload: PounceAlert }) => void) | null = null
  const bridge = window as unknown as { __TAURI__?: unknown }
  bridge.__TAURI__ = { event: { listen: async (_name: string, handler: (event: { payload: PounceAlert }) => void) => { deliver = handler; return () => {} } } }
  try {
    renderHook(() => usePounce())
    await flush()
    expect(deliver).not.toBeNull()
    FakeNotification.permission = 'granted'
    localStorage.setItem('nexus.remote.rareDxAlerts', 'on')
    ring = [pounce('VP8PJ', 100, { entity: 'South Orkney Islands' })]
    render(<Harness />)
    await flush()
    const raised = [
      pounce('3Y0J', 200),
      pounce('BS7H', 260, { band: '17m', freqMhz: null, entity: 'Scarborough Reef', tags: ['NewEntity', 'NewZone'] }),
      pounce('3Y0J', 3900), // the same slot again once the desktop's hour-long cooldown has passed
    ]
    for (const alert of raised) {
      ring = [...ring, alert]
      act(() => deliver!({ payload: alert }))
      await poll(RARE_ALERT_POLL_MS)
    }
    const desktop = vi.mocked(osNotify).mock.calls.map(([title, body]) => ({ title, body }))
    expect(desktop).toHaveLength(3)
    expect(vi.mocked(doubleBeep)).toHaveBeenCalled() // the desktop path really ran
    expect(shown().map(({ title, body }) => ({ title, body }))).toEqual(desktop)
  } finally { delete bridge.__TAURI__ }
})

it('rare DX: explains a station whose Pounce is off, and reads nothing from a station too old to offer it', async () => {
  FakeNotification.permission = 'granted'
  localStorage.setItem('nexus.remote.rareDxAlerts', 'on')
  threshold = 'off'
  const view = render(<Harness />)
  await flush()
  expect(panel(view.container)).toContain('Pounce is off at the station.')
  threshold = 'atno'
  await poll(RARE_ALERT_POLL_MS)
  expect(panel(view.container)).not.toContain('Pounce is off at the station.')
  cleanup(); sent.length = 0
  const old = render(<Harness offered={false} />)
  await poll(RARE_ALERT_POLL_MS)
  expect(button(old.container, 'Stop rare-DX alerts')).toBeUndefined()
  expect(button(old.container, 'Alert me about rare DX')).toBeUndefined()
  expect(panel(old.container)).toContain('Rare-DX alerts need a newer Nexus at the station.')
  expect(panel(old.container)).toContain('POTA activation alerts need a newer Nexus at the station.')
  expect(sent).toEqual([])
})

it('POTA: never for parks already on the air when turned on, once per new activation, silent after a stale feed, a reconnect or a re-enable', async () => {
  potaSpots = [spot('US-0002'), spot('US-0003')]
  const view = render(<Harness />)
  await poll(POTA_ALERT_POLL_MS)
  expect(sent).toEqual([])
  expect(FakeNotification.requestPermission).not.toHaveBeenCalled()

  const pota = button(view.container, 'Alert me about new POTA activations')!
  await act(async () => { fireEvent.click(pota) })
  expect(FakeNotification.requestPermission).toHaveBeenCalledOnce()
  await flush()
  expect(sent).toEqual(['get_remote_ota'])
  await poll(POTA_ALERT_POLL_MS)
  expect(titles()).toEqual([]) // the parks already active at turn-on never alert

  potaSpots = [...potaSpots, spot('US-0005', 'K3XYZ', 'Fresh Park')]
  await poll(POTA_ALERT_POLL_MS)
  expect(shown()).toEqual([{ title: 'New POTA activation: US-0005', body: 'K3XYZ at Fresh Park, 14.062 MHz CW', tag: 'nexus-pota-US-0005' }])
  await poll(POTA_ALERT_POLL_MS)
  expect(titles()).toHaveLength(1)
  const focus = vi.spyOn(window, 'focus').mockImplementation(() => {})
  FakeNotification.shown[0].onclick!()
  expect(focus).toHaveBeenCalledOnce()

  // The station stops refreshing its POTA spots. When fresh spots return, what is on the air
  // then is a new baseline, not a burst.
  potaStatus = 'expired'
  await poll(POTA_ALERT_POLL_MS)
  expect(panel(view.container)).toContain('no fresh POTA spots')
  potaStatus = 'ready'; potaSpots = [...potaSpots, spot('US-0006')]
  await poll(POTA_ALERT_POLL_MS)
  expect(panel(view.container)).not.toContain('no fresh POTA spots')
  expect(titles()).toHaveLength(1)

  view.rerender(<Harness ready={false} />)
  potaSpots = [...potaSpots, spot('US-0007')]
  await poll(POTA_ALERT_POLL_MS)
  view.rerender(<Harness ready />)
  await flush()
  expect(titles()).toHaveLength(1)

  // Positive control for every silence above.
  potaSpots = [...potaSpots, spot('US-0008')]
  await poll(POTA_ALERT_POLL_MS)
  expect(titles()).toEqual(['New POTA activation: US-0005', 'New POTA activation: US-0008'])

  // Turned off and on again: parks that came on the air meanwhile are already active at turn-on.
  await act(async () => { fireEvent.click(pota) })
  potaSpots = [...potaSpots, spot('US-0009')]
  await poll(POTA_ALERT_POLL_MS)
  await act(async () => { fireEvent.click(pota) })
  await flush(); await poll(POTA_ALERT_POLL_MS)
  expect(titles()).toHaveLength(2)
  expect(new Set(sent)).toEqual(new Set(['get_remote_ota']))
})

it('POTA: the browser notifies on exactly the ticks the desktop alert beeps, for the same feed', async () => {
  FakeNotification.permission = 'granted'
  localStorage.setItem('nexus.remote.potaAlerts', 'on')
  resetPotaAlertsForTest()
  const ticks = [
    ['US-0002', 'US-0003'], // on the air when the alert is turned on
    ['US-0002', 'US-0003'],
    ['US-0002', 'US-0003', 'US-0005'], // a park comes on the air
    ['US-0002', 'US-0005'], // one goes off the air
    ['US-0002', 'US-0005', 'US-0003'], // and is spotted again: a new spot on both
    ['US-0002', 'US-0005', 'US-0003', 'US-0006', 'US-0007'], // two at once: one desktop beep
  ]
  const desktop: boolean[] = [], browser: boolean[] = []
  let before = 0
  for (const [i, refs] of ticks.entries()) {
    const spots = refs.map(r => spot(r))
    const beeps = vi.mocked(doubleBeep).mock.calls.length
    processPotaAlert(spots.map(mapSpot))
    desktop.push(vi.mocked(doubleBeep).mock.calls.length > beeps)
    potaSpots = spots
    if (i === 0) render(<Harness />)
    await (i === 0 ? flush() : poll(POTA_ALERT_POLL_MS))
    browser.push(FakeNotification.shown.length > before); before = FakeNotification.shown.length
  }
  expect(desktop).toEqual([false, false, true, false, true, true]) // guards the fixture itself
  expect(browser).toEqual(desktop)
  expect(titles()).toEqual(['New POTA activation: US-0005', 'New POTA activation: US-0003', 'New POTA activation: US-0006', 'New POTA activation: US-0007'])
})

it('a refusal on any alert toggle is explained once and hides every alert toggle', async () => {
  FakeNotification.answer = 'denied'
  const view = render(<Harness />)
  await act(async () => { fireEvent.click(button(view.container, 'Alert me about new POTA activations')!) })
  expect(panel(view.container)).toContain('Notifications are blocked for this site')
  expect(view.container.querySelectorAll('.remote-need-alerts button')).toHaveLength(0)
  await poll(POTA_ALERT_POLL_MS)
  expect(sent).toEqual([])
})
