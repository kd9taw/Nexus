// @vitest-environment jsdom
//
// THE TRANSMITTER ALARM CELL (operator rulings 2026-10-04 "Sticky until dismissed", 2026-11-07 "One line
// inside the strip" and "Never wraps, alarms also shown by Stop"), as the TX strip draws it for every
// operating screen: one line in the strip's row beside the TX state, always drawn at one minimum width
// (as the header's last row, every alarm moved Stop TX 26 to 38 px; real Chrome). It holds the
// station's transmitter alarms and every critical status-lane item, which the top bar may cut. The station sends the alarms the
// operator has not dismissed, oldest first; the banner shows the oldest with its radio, its UTC
// time and the station's own words, says how many wait, and its Dismiss names exactly the alarm on
// screen. It locks nothing: the strip's controls render exactly as without it.
//
// The cascade half is computed over the real sheets, never grepped: one constant width to the strip's
// line breaker, and in flow. Whether anything moves is geometry, which jsdom never lays out (the Chrome
// sweep, tasks/scratch/stop-steady/probe.mjs).
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen, fireEvent, act } from '@testing-library/react'
import { CockpitHeader } from './CockpitHeader'
import { CockpitTxStrip } from './CockpitTxStrip'
import { TxAlarmBanner } from './TxAlarmBanner'
import { css, loadSheets } from '../cssCascade.testkit'
import { dismissTxAlarm } from '../api'
import { setStatus } from '../status'
import { audioErrorLane } from '../features/audioError'
import { pcMicLane } from '../features/pcMic'
import { pttRefusedLane } from '../features/pttRefused'
import { slotKeyRefusedLane } from '../features/slotKeyRefused'
import { slotAudioLostLane } from '../features/slotAudioLost'
import { slotUnkeyFailedLane } from '../features/slotUnkeyFailed'
import { t } from '../i18n'
import type { AppSnapshot, TxAlarm } from '../types'

vi.mock('../api', () => ({
  setFrequency: vi.fn(() => Promise.resolve(null)),
  dismissTxAlarm: vi.fn(),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: async <T,>(action: () => Promise<T>) => {
    try {
      return await action()
    } catch {
      return null
    }
  },
}))
vi.mock('../useWheelTune', () => ({ useWheelTune: () => undefined }))

const UNCONFIRMED = 'the radio did not confirm the unkey — it may still be transmitting. Check the radio now.'
const HELD =
  'an earlier Nexus session (0x7A3C0001) still holds the transmitter; Nexus will not key until it is released'
/** 2026-09-30 14:22:33 UTC. */
const AT = Date.UTC(2026, 8, 30, 14, 22, 33)

const alarm = (id: number, text: string, atMs: number, radioName = 'FLEX-6600'): TxAlarm => ({
  id,
  text,
  radioId: 1,
  radioName,
  atMs,
})

const snapWith = (txAlarms?: TxAlarm[]) =>
  ({
    radio: {
      dialMhz: 14.074, catOk: true, sideband: 'USB', transmitting: false, txEnabled: true,
      tuning: false, txAllowed: true,
    },
    txAlarms,
  }) as unknown as AppSnapshot

const mount = (snap: AppSnapshot, onSnap = vi.fn()) =>
  render(
    <CockpitTxStrip
      radio={snap.radio}
      onStopTx={() => {}}
      onTune={() => {}}
      alarm={<TxAlarmBanner alarms={snap.txAlarms} onSnap={onSnap} />}
    />,
  )
const banner = () => document.querySelector<HTMLElement>('.ch-txalarm')
const words = () => document.querySelector<HTMLElement>('.ch-txalarm-text')

beforeAll(() => loadSheets(['styles.css', 'cockpit-panes.css']))
beforeEach(() => vi.mocked(dismissTxAlarm).mockReset())
afterEach(() => {
  cleanup()
  for (const id of LANE_IDS) setStatus(id, null)
})

/** Every lane id this file sets, cleared after each test (the lane store outlives a render). */
const LANE_IDS = ['probe-critical', 'probe-warning', 'audio', 'pcMic', 'pttRefused', 'slotKeyRefused', 'slotAudioLost', 'slotUnkeyFailed', 'txWatchdog']

describe('the transmitter alarm cell on the TX strip', () => {
  it('no alarm: the cell is still drawn, empty, with no Dismiss', () => {
    mount(snapWith(undefined))
    expect(banner(), 'always drawn, so an alarm arriving adds no box').not.toBeNull()
    expect(banner()!.textContent).toBe('')
    expect(screen.queryByRole('button', { name: 'Dismiss' })).toBeNull()
  })

  it('shows the oldest alarm on one line with its radio, its UTC time and its own words, the whole on hover, and how many wait', () => {
    mount(snapWith([alarm(7, UNCONFIRMED, AT), alarm(8, HELD, AT + 60_000)]))
    const lead = `Transmitter alarm, FLEX-6600, 14:22:33 UTC: ${UNCONFIRMED}`
    expect(words()!.textContent).toContain(lead)
    expect(words()!.title, 'the whole alarm on hover').toBe(lead)
    expect(words()!.textContent, 'a newer alarm waits behind it, never in its place').not.toContain(HELD)
    const queue = document.querySelector<HTMLElement>('.ch-txalarm-queue')!
    expect(queue.textContent).toBe('1 of 2')
    expect(queue.title, 'the waiting alarm is reachable on hover').toContain(HELD)
  })

  it('a radio the station does not name: its time and its words', () => {
    mount(snapWith([alarm(7, UNCONFIRMED, AT, '')]))
    expect(words()!.textContent).toContain(`Transmitter alarm, 14:22:33 UTC: ${UNCONFIRMED}`)
    expect(document.querySelector('.ch-txalarm-queue'), 'one alarm: no count').toBeNull()
  })

  it('Dismiss clears exactly the alarm on screen, and takes the snapshot it answers with', async () => {
    const after = snapWith([alarm(8, HELD, AT + 60_000)])
    vi.mocked(dismissTxAlarm).mockResolvedValue(after)
    const onSnap = vi.fn()
    mount(snapWith([alarm(7, UNCONFIRMED, AT), alarm(8, HELD, AT + 60_000)]), onSnap)
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }))
    })
    expect(dismissTxAlarm).toHaveBeenCalledTimes(1)
    expect(dismissTxAlarm).toHaveBeenCalledWith(7)
    expect(onSnap).toHaveBeenCalledWith(after)
  })

  it('locks nothing: every control of the strip renders exactly as without it', () => {
    // React's generated ids differ from one mount to the next; nothing else may.
    const controls = () =>
      [...document.querySelectorAll('.cockpit-txstrip > :not(.ch-txalarm)')].map((e) => e.outerHTML.replace(/[:«]r[0-9a-z]+[:»]/g, 'id'))
    mount(snapWith(undefined))
    const before = controls()
    cleanup()
    mount(snapWith([alarm(7, UNCONFIRMED, AT)]))
    expect(words(), 'premise: the alarm is shown').not.toBeNull()
    expect(controls()).toEqual(before)
  })

  it('is in the strip’s row beside the TX state, after Stop TX, in flow', () => {
    mount(snapWith([alarm(7, UNCONFIRMED, AT)]))
    const strip = document.querySelector('.cockpit-txstrip')!
    expect(banner()!.parentElement).toBe(strip)
    expect(banner()!.previousElementSibling?.classList.contains('cq-statecap'), 'right after the TX state').toBe(true)
    const stop = screen.getByRole('button', { name: /^stop tx$/i })
    expect(stop.compareDocumentPosition(banner()!) & Node.DOCUMENT_POSITION_FOLLOWING, 'never before Stop TX').toBeTruthy()
    expect(css(banner()!, 'position'), 'positioned, it could sit over a control').toBeNull()
  })

  // THE CASCADE (one constant width to the strip's line breaker) is computed in cockpit-panes.test.ts,
  // "THE TRANSMITTER ALARM CELL", over the raw declarations of both sheets.
})

// THE TOP BAR MAY CUT ITS LANE NOW (operator pick "Never wraps, alarms also shown by Stop"), so every
// CRITICAL lane item must also be in this cell. Each critical producer the app has, fed what makes it
// critical, and the TX watchdog's halt (App's own item), must reach it; a warning must not.
describe('every critical status-lane item is shown by Stop TX too', () => {
  const at = 1_790_000_000
  const CRITICAL: Array<[string, ReturnType<typeof audioErrorLane>]> = [
    ['audio: engine stopped', audioErrorLane('loop ended', 'engineStopped')],
    ['audio: sound card', audioErrorLane('unplugged', 'soundCard')],
    ['audio: no receive audio', audioErrorLane('silent', 'noReceiveAudio')],
    ['audio: PTT refused', audioErrorLane('rig refused', 'ptt')],
    ['audio: a kind this page does not know', audioErrorLane('new', null)],
    ['computer microphone', pcMicLane({ at, cause: 'noDevice' })],
    ['PTT kept off the air', pttRefusedLane({ at, mode: 'USB' })],
    ['slot key refused', slotKeyRefusedLane({ at, why: 'busy' } as never)],
    ['slot audio lost', slotAudioLostLane({ at } as never)],
    ['slot unkey failed', slotUnkeyFailedLane({ at, why: 'no answer' } as never)],
    ['TX watchdog (App)', { tier: 'critical', message: t('remote.stream.mic.ended.watchdog') }],
  ]
  it.each(CRITICAL)('%s', (_name, item) => {
    expect(item?.tier, 'premise: the producer calls this critical').toBe('critical')
    mount(snapWith(undefined))
    act(() => setStatus('probe-critical', item))
    expect(words()?.textContent).toContain(item!.message)
    expect(words()!.title, 'the whole item on hover').toContain(item!.message)
  })

  it('a warning stays in the lane only', () => {
    mount(snapWith(undefined))
    act(() => setStatus('probe-warning', { tier: 'warning', message: 'NO RF SCOPE' }))
    expect(banner()!.textContent).toBe('')
  })

  it('a transmitter alarm comes first, with its Dismiss; the critical item waits in the count, whole on hover', () => {
    mount(snapWith([alarm(7, UNCONFIRMED, AT)]))
    act(() => setStatus('probe-critical', { tier: 'critical', message: 'PTT OFF FAILED — TX STOPPED' }))
    expect(words()!.textContent).toContain(UNCONFIRMED)
    const queue = document.querySelector<HTMLElement>('.ch-txalarm-queue')!
    expect(queue.textContent).toBe('1 of 2')
    expect(queue.title).toContain('PTT OFF FAILED — TX STOPPED')
    expect(screen.getByRole('button', { name: 'Dismiss' })).toBeTruthy()
  })

  it('with no transmitter alarm, a critical item is the line, and there is nothing to dismiss', () => {
    mount(snapWith(undefined))
    act(() => setStatus('probe-critical', { tier: 'critical', message: 'PTT OFF FAILED — TX STOPPED' }))
    expect(words()!.textContent).toContain('PTT OFF FAILED — TX STOPPED')
    expect(screen.queryByRole('button', { name: 'Dismiss' })).toBeNull()
  })
})

// STOP TX STAYS PUT: the header stands above the strip, so the alarm may not be drawn there.
describe('the cockpit header', () => {
  it('draws no alarm, whatever the station sends', () => {
    render(<CockpitHeader snap={snapWith([alarm(7, UNCONFIRMED, AT)])} modeIndicator={<span>USB</span>} bandControl={<span>20m</span>} />)
    expect(banner()).toBeNull()
  })
})
