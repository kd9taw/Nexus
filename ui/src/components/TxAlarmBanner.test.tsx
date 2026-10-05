// @vitest-environment jsdom
//
// THE TRANSMITTER ALARM BANNER (operator ruling 2026-10-04, "Sticky until dismissed"), as the
// cockpit header draws it for every cockpit that has one. The station sends the alarms the
// operator has not dismissed, oldest first; the banner shows the oldest with its radio, its UTC
// time and the station's own words, says how many wait, and its Dismiss names exactly the alarm on
// screen. It locks nothing: the dial and the CAT verdict render exactly as without it.
//
// The cascade half is computed over the real sheets, never grepped: its own row, and in flow.
// Whether it covers Stop TX or moves the dock is geometry, which jsdom never lays out — that is
// the compiled browser's check (remote/test/browser.test.mjs, the Quick layout scenario).
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen, fireEvent, act } from '@testing-library/react'
import { CockpitHeader } from './CockpitHeader'
import { StationControlContext } from '../stationAccess'
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

const mount = (snap: AppSnapshot, onSnap = vi.fn(), control = true) =>
  render(
    <StationControlContext.Provider value={control}>
      <CockpitHeader snap={snap} onSnap={onSnap} modeIndicator={<span>USB</span>} bandControl={<span>20m</span>} />
    </StationControlContext.Provider>,
  )
const banner = () => document.querySelector<HTMLElement>('.ch-txalarm')

beforeAll(() => loadSheets(['styles.css', 'cockpit-panes.css', 'remote-web/application.css']))
beforeEach(() => vi.mocked(dismissTxAlarm).mockReset())
afterEach(cleanup)

describe('the transmitter alarm in the cockpit header', () => {
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

  it('the Remote shows it without the button: the dismissal is the station’s', () => {
    mount(snapWith([alarm(7, UNCONFIRMED, AT)]), vi.fn(), false)
    expect(banner()!.textContent).toContain(UNCONFIRMED)
    expect(screen.queryByRole('button', { name: 'Dismiss' })).toBeNull()
  })

  it('locks nothing: the dial and the CAT verdict render exactly as without it', () => {
    // React's generated ids differ from one mount to the next; nothing else may.
    const part = (sel: string) => document.querySelector(sel)!.outerHTML.replace(/[:«]r[0-9a-z]+[:»]/g, 'id')
    mount(snapWith(undefined))
    const before = [part('.ch-freq'), part('.ch-actions')]
    cleanup()
    mount(snapWith([alarm(7, UNCONFIRMED, AT)]))
    expect(banner(), 'premise: the alarm is shown').not.toBeNull()
    expect([part('.ch-freq'), part('.ch-actions')]).toEqual(before)
    expect(document.querySelector('.cockpit-cat')!.textContent).toBe('CAT ✓')
  })

  it('is the header’s last row, its own full-width line, and in flow', () => {
    mount(snapWith([alarm(7, UNCONFIRMED, AT)]))
    const header = document.querySelector('.cockpit-header')!
    expect(header.lastElementChild, 'the last row, so the controls above never move').toBe(banner())
    expect(css(banner()!, 'flex-basis'), 'a line of its own').toBe('100%')
    expect(css(banner()!, 'margin-bottom'), 'the alert box’s own margin is not a gap here').toBe('0px')
    expect(css(banner()!, 'position'), 'positioned, it could sit over Stop TX').toBeNull()
  })

  it('spans the Remote’s compact header grid', () => {
    // The Quick header (`.cockpit-header--quick`) is a two-column grid: without a span the banner
    // would take one cell of a row and squeeze beside nothing.
    mount(snapWith([alarm(7, UNCONFIRMED, AT)]))
    document.querySelector('.cockpit-header')!.classList.add('cockpit-header--quick')
    expect(css(banner()!, 'grid-column')).toBe('1 / -1')
  })
})
