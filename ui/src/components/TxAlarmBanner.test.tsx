// @vitest-environment jsdom
//
// THE TRANSMITTER ALARM BANNER (operator ruling 2026-10-04, "Sticky until dismissed"), as the TX
// strip draws it for every operating screen, its last line under Stop TX (2026-11-07: as the cockpit
// header's last row, every alarm that came or went moved Stop TX 26 to 38 px; real Chrome). The station sends the alarms the
// operator has not dismissed, oldest first; the banner shows the oldest with its radio, its UTC
// time and the station's own words, says how many wait, and its Dismiss names exactly the alarm on
// screen. It locks nothing: the strip's controls render exactly as without it.
//
// The cascade half is computed over the real sheets, never grepped: its own line, and in flow.
// Whether it covers Stop TX or moves the dock is geometry, which jsdom never lays out (the Chrome sweep).
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen, fireEvent, act } from '@testing-library/react'
import { CockpitHeader } from './CockpitHeader'
import { CockpitTxStrip } from './CockpitTxStrip'
import { TxAlarmBanner } from './TxAlarmBanner'
import { css, loadSheets } from '../cssCascade.testkit'
import { dismissTxAlarm } from '../api'
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

beforeAll(() => loadSheets(['styles.css', 'cockpit-panes.css']))
beforeEach(() => vi.mocked(dismissTxAlarm).mockReset())
afterEach(cleanup)

describe('the transmitter alarm on the TX strip', () => {
  it('no alarm, no banner', () => {
    mount(snapWith(undefined))
    expect(banner()).toBeNull()
    cleanup()
    mount(snapWith([]))
    expect(banner()).toBeNull()
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('shows the oldest alarm with its radio, its UTC time and its own words, and how many wait', () => {
    mount(snapWith([alarm(7, UNCONFIRMED, AT), alarm(8, HELD, AT + 60_000)]))
    const shown = screen.getByRole('alert')
    expect(shown).toBe(banner())
    expect(shown.textContent).toContain(`Transmitter alarm, FLEX-6600, 14:22:33 UTC: ${UNCONFIRMED}`)
    expect(shown.textContent, 'a newer alarm waits behind it, never in its place').not.toContain(HELD)
    expect(shown.textContent).toContain('1 of 2')
  })

  it('a radio the station does not name: its time and its words', () => {
    mount(snapWith([alarm(7, UNCONFIRMED, AT, '')]))
    expect(banner()!.textContent).toContain(`Transmitter alarm, 14:22:33 UTC: ${UNCONFIRMED}`)
    expect(banner()!.textContent, 'one alarm: no count').not.toContain('1 of')
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
    expect(banner(), 'premise: the alarm is shown').not.toBeNull()
    expect(controls()).toEqual(before)
  })

  it('is the strip’s last line, after Stop TX, its own full-width line, and in flow', () => {
    mount(snapWith([alarm(7, UNCONFIRMED, AT)]))
    const strip = document.querySelector('.cockpit-txstrip')!
    expect(strip.lastElementChild, 'the last line, so nothing on the strip moves').toBe(banner())
    const stop = screen.getByRole('button', { name: /^stop tx$/i })
    expect(stop.compareDocumentPosition(banner()!) & Node.DOCUMENT_POSITION_FOLLOWING, 'under Stop TX, never before it').toBeTruthy()
    expect(css(banner()!, 'flex-basis'), 'a line of its own').toBe('100%')
    expect(css(banner()!, 'margin-bottom'), 'the alert box’s own margin is not a gap here').toBe('0px')
    expect(css(banner()!, 'position'), 'positioned, it could sit over Stop TX').toBeNull()
  })


  // STOP TX STAYS PUT: the header stands above the strip, so the alarm may not be drawn there.
  it('the cockpit header draws no alarm, whatever the station sends', () => {
    render(<CockpitHeader snap={snapWith([alarm(7, UNCONFIRMED, AT)])} modeIndicator={<span>USB</span>} bandControl={<span>20m</span>} />)
    expect(banner()).toBeNull()
  })
})
