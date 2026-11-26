// @vitest-environment jsdom
//
// THE WIPE ON EVERY LOG LINE, AND THE OPTIONAL WIPE ON A QSY (operator request: "CTRL-W for the wipe
// feature to clear the log. Say you are not going to get someone worked and move up the band").
//
//  · Ctrl+W joins Alt+W, N1MM Logger+'s two wipe keys, and both reach every log line, not only a
//    contest strip: the call and every box go. As in N1MM, a second Alt+W right after puts them
//    back, and Ctrl+W cannot be undone. Ctrl+W typed in a text box that is not the log line's is
//    left to that box.
//  · "Wipe an unlogged call when I QSY" (Settings ▸ Logbook ▸ Log line, off by default) sets the
//    un-logged line aside, with Restore, when the dial moves off the contact's frequency and stays
//    off for QSY_HOLD_MS: more than 3 kHz on phone, more than 1 kHz on CW and the digital modes.
//    Never while transmitting, nor on a dial read taken during an over: a keyed rig in split reads
//    back its TX VFO, and the rig's own PTT is polled only about once a second, so an over is seen
//    late and the dial read during it is not where the contact is. A transmission on the new
//    frequency makes it the contact's, once the dial has settled after the over.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, act } from '@testing-library/react'
import type { ComponentProps } from 'react'
import { LogEntry } from './LogEntry'
import { logQso } from '../api'
import { setQsyWipe } from '../features/qsyWipe'
import type { AppSnapshot } from '../types'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => null) : actual[k]
  }
  return {
    ...auto,
    qrzLookup: vi.fn(async () => null),
    resolveEntity: vi.fn(async () => null),
    lookupPark: vi.fn(async () => null),
    lookupParkLive: vi.fn(async () => null),
    searchParks: vi.fn(async () => []),
    setCwPeerInfo: vi.fn(async () => {}),
    setLogFormGrid: vi.fn(async () => {}),
    logQso: vi.fn(async (rec: unknown) => rec),
    askLog: vi.fn(() => new Promise(() => {})),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action().catch(() => null)),
}))

const mockedLog = vi.mocked(logQso)

/** The station on `dialMhz` (20 m unless said), transmitting or not. */
const at = (dialMhz = 14.25, transmitting = false, band = '20m') =>
  ({ mycall: 'KD9TAW', mygrid: 'EN52', radio: { band, dialMhz, transmitting }, hunt: null, bandChangeTick: 1 }) as unknown as AppSnapshot

type StripProps = Partial<ComponentProps<typeof LogEntry>>
/** The Phone cockpit's strip, as PhoneCockpit renders it. */
const phone = (snap: AppSnapshot, props: StripProps = {}) => (
  <LogEntry snap={snap} mode="SSB" defaultRst="59" exchange="terrestrial" titled={false} draft="phone" fdMode="PH" clearCallOnBandChange {...props} />
)
/** The CW cockpit's strip. */
const cw = (snap: AppSnapshot, props: StripProps = {}) => (
  <LogEntry snap={snap} mode="CW" defaultRst="599" exchange="terrestrial" titled={false} draft="cw" fdMode="CW" clearCallOnBandChange {...props} />
)
/** The Satellites section's strip: no band-change clear, and the dial follows Doppler. */
const sat = (snap: AppSnapshot) => <LogEntry snap={snap} mode="FM" defaultRst="59" exchange="satellite" titled={false} draft="satellites" />

const input = (cls: string) => document.querySelector<HTMLInputElement>(`input.${cls}`)!
const call = () => input('le-call')
const comment = () => document.querySelector<HTMLTextAreaElement>('textarea.le-comment')!
const rsts = () => [...document.querySelectorAll<HTMLInputElement>('input.le-rst')]
const type = (el: HTMLInputElement | HTMLTextAreaElement, value: string) => fireEvent.change(el, { target: { value } })
const notice = () => document.querySelector('.le-band-cleared')
const settle = () => act(async () => { for (let i = 0; i < 8; i++) await Promise.resolve() })
/** A key on `target` (the window unless said), as the browser delivers it; true when nothing held it. */
const press = async (init: KeyboardEventInit, target: Window | Element = window) => {
  let free = true
  await act(async () => {
    free = fireEvent.keyDown(target, init)
  })
  return free
}
const CTRL_W = { key: 'w', code: 'KeyW', ctrlKey: true }
const ALT_W = { key: 'w', code: 'KeyW', altKey: true }

/** EA8XX in the line, with a name, a comment and both reports. */
function fillEa8xx() {
  type(call(), 'EA8XX')
  type(input('le-name'), 'Pedro')
  type(comment(), 'first QSO')
  type(rsts()[0], '57')
  type(rsts()[1], '55')
}
const lineNow = () => [call().value, input('le-name').value, comment().value, ...rsts().map((r) => r.value)]
const FILLED = ['EA8XX', 'Pedro', 'first QSO', '57', '55']
const BLANK = ['', '', '', '59', '59']

beforeEach(() => {
  mockedLog.mockReset().mockImplementation(async (rec: unknown) => rec as never)
  setQsyWipe(false)
})

describe('Ctrl+W and Alt+W wipe every log line, not only a contest strip', () => {
  it('Alt+W wipes the Phone line, every box of it, and a second Alt+W right after puts it back', async () => {
    render(phone(at()))
    fillEa8xx()
    expect(lineNow()).toEqual(FILLED)
    expect(await press(ALT_W, call()), 'Alt+W is the line\'s: the webview gets nothing of it').toBe(false)
    expect(lineNow()).toEqual(BLANK)
    expect(await press(ALT_W, call())).toBe(false)
    expect(lineNow(), 'put back').toEqual(FILLED)
  })

  it('Ctrl+W wipes the Phone line for good, as in N1MM: a second Ctrl+W, or an Alt+W, puts nothing back', async () => {
    render(phone(at()))
    fillEa8xx()
    expect(await press(CTRL_W, call()), 'Ctrl+W is the line\'s: the webview gets nothing of it').toBe(false)
    expect(lineNow()).toEqual(BLANK)
    expect(await press(CTRL_W, call())).toBe(false)
    expect(lineNow(), 'irreversible').toEqual(BLANK)
    expect(await press(ALT_W, call())).toBe(false)
    expect(lineNow(), 'Alt+W cannot undo a Ctrl+W').toEqual(BLANK)
  })

  it('Ctrl+W after an Alt+W ends the chance to put the Alt+W\'s line back', async () => {
    render(phone(at()))
    fillEa8xx()
    await press(ALT_W)
    await press(CTRL_W)
    expect(lineNow(), 'Ctrl+W never puts back').toEqual(BLANK)
    await press(ALT_W)
    expect(lineNow()).toEqual(BLANK)
  })

  it('any other key between the two presses ends the chance to put it back', async () => {
    render(phone(at()))
    fillEa8xx()
    await press(ALT_W)
    await press({ key: 'a', code: 'KeyA' })
    await press(ALT_W)
    expect(lineNow()).toEqual(BLANK)
  })

  it('Ctrl+W pressed outside any text box wipes the line too', async () => {
    render(phone(at()))
    fillEa8xx()
    expect(await press(CTRL_W, document.body)).toBe(false)
    expect(lineNow()).toEqual(BLANK)
  })

  it('Ctrl+W in a text box that is not the log line is that box\'s: the line is untouched', async () => {
    render(
      <>
        <textarea className="cw-send-box" />
        {phone(at())}
      </>,
    )
    fillEa8xx()
    const other = document.querySelector<HTMLTextAreaElement>('textarea.cw-send-box')!
    expect(await press(CTRL_W, other)).toBe(true)
    expect(lineNow()).toEqual(FILLED)
    // …while Alt+W fires from there, as it always has on a contest strip.
    expect(await press(ALT_W, other)).toBe(false)
    expect(lineNow()).toEqual(BLANK)
  })

  it('Ctrl with Alt, Shift or Cmd is not the wipe', async () => {
    render(phone(at()))
    fillEa8xx()
    for (const extra of [{ altKey: true }, { shiftKey: true }, { metaKey: true }]) {
      expect(await press({ ...CTRL_W, ...extra }), JSON.stringify(extra)).toBe(true)
    }
    expect(lineNow()).toEqual(FILLED)
  })

  it('the Satellites line wipes too', async () => {
    render(sat(at(145.8)))
    type(call(), 'EA8XX')
    expect(await press(CTRL_W)).toBe(false)
    expect(call().value).toBe('')
  })

  it('a strip whose cockpit is not on screen leaves the key alone', async () => {
    render(phone(at(), { active: false }))
    type(call(), 'EA8XX')
    expect(await press(CTRL_W)).toBe(true)
    expect(call().value).toBe('EA8XX')
  })

  it('a wipe never logs anything', async () => {
    render(phone(at()))
    fillEa8xx()
    await press(ALT_W)
    await press(ALT_W)
    await press(CTRL_W)
    await settle()
    expect(mockedLog).not.toHaveBeenCalled()
  })
})

describe('"Wipe an unlogged call when I QSY"', () => {
  beforeEach(() => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'Date'] })
  })
  afterEach(() => {
    vi.useRealTimers()
  })
  /** Time passing with the dial where it is: the hold, or an over. */
  const wait = (ms: number) => act(async () => { vi.advanceTimersByTime(ms) })
  /** The station on `dialMhz`, keyed (the engine's TX owner or the rig's own PTT read back) or not. */
  const rx = (dialMhz: number) => at(dialMhz)
  const keyedByRig = (dialMhz: number) =>
    ({ ...at(dialMhz), radio: { band: '20m', dialMhz, transmitting: false, rigKeyed: true } }) as unknown as AppSnapshot
  const keyedByNexus = (dialMhz: number) =>
    ({ ...at(dialMhz), radio: { band: '20m', dialMhz, transmitting: false, txBusyReason: 'cw' } }) as unknown as AppSnapshot

  it('is off by default: a QSY leaves the call in the line', async () => {
    const view = render(phone(rx(14.25)))
    type(call(), 'EA8XX')
    view.rerender(phone(rx(14.27)))
    await wait(5000)
    expect(call().value).toBe('EA8XX')
  })

  it('on phone, a move of 3 kHz or less keeps the line, and more, held, sets it aside, with Restore', async () => {
    setQsyWipe(true)
    const view = render(phone(rx(14.25)))
    fillEa8xx()
    view.rerender(phone(rx(14.253)))
    await wait(5000)
    expect(lineNow(), 'tuning onto the voice is not a QSY').toEqual(FILLED)
    view.rerender(phone(rx(14.2535)))
    await settle()
    expect(lineNow(), 'not before the move has held').toEqual(FILLED)
    await wait(2500)
    expect(lineNow()).toEqual(BLANK)
    expect(notice()?.textContent).toContain('EA8XX')
    expect(notice()?.textContent, 'the notice says where it will log').toContain('14.250')
    fireEvent.click(screen.getByRole('button', { name: 'Restore' }))
    await settle()
    expect(lineNow()).toEqual(FILLED)
    // Put back on the new frequency, it is not wiped again for being there.
    view.rerender(phone(rx(14.2536)))
    await wait(5000)
    expect(lineNow()).toEqual(FILLED)
    // …and logs on the frequency it was made on.
    fireEvent.click(screen.getByRole('button', { name: /^log$/i }))
    await settle()
    expect((mockedLog.mock.calls[0][0] as unknown as { freqMhz: number }).freqMhz).toBeCloseTo(14.25, 6)
  })

  it('on CW, more than 1 kHz, held, is a QSY', async () => {
    setQsyWipe(true)
    const view = render(cw(rx(14.025)))
    type(call(), 'EA8XX')
    view.rerender(cw(rx(14.0259)))
    await wait(5000)
    expect(call().value).toBe('EA8XX')
    view.rerender(cw(rx(14.0262)))
    await wait(2500)
    expect(call().value).toBe('')
  })

  it('a quick spin out and back is not a QSY', async () => {
    setQsyWipe(true)
    const view = render(cw(rx(14.025)))
    type(call(), 'EA8XX')
    view.rerender(cw(rx(14.03)))
    await wait(1000)
    view.rerender(cw(rx(14.025)))
    await wait(5000)
    expect(call().value).toBe('EA8XX')
  })

  it('SPLIT, keyed at the rig\'s own mic: the TX VFO read before the PTT poll sees the key never wipes', async () => {
    setQsyWipe(true)
    const view = render(phone(rx(14.195)))
    type(call(), 'EA8XX')
    // Keyed at the mic: the rig reads back its TX VFO 5 kHz up, and the PTT read is a second late.
    view.rerender(phone(rx(14.2)))
    await wait(1000)
    view.rerender(phone(keyedByRig(14.2)))
    await wait(4000)
    // Unkeyed: the dial is the RX VFO again, the PTT read still a second late.
    view.rerender(phone(keyedByRig(14.195)))
    await wait(1000)
    view.rerender(phone(rx(14.195)))
    await wait(5000)
    expect(call().value).toBe('EA8XX')
  })

  it('SPLIT, a pile-up: an over read at the TX VFO, then the RX VFO again, never wipes, call after call', async () => {
    setQsyWipe(true)
    const view = render(cw(rx(14.025)))
    type(call(), 'EA8XX')
    for (let over = 0; over < 3; over++) {
      view.rerender(cw(keyedByNexus(14.027)))
      await wait(3000)
      view.rerender(cw(rx(14.025)))
      await wait(5000)
      expect(call().value, `after over ${over + 1}`).toBe('EA8XX')
    }
  })

  it('never during a transmission, and a transmission on the new frequency makes it the contact\'s', async () => {
    setQsyWipe(true)
    const view = render(phone(rx(14.25)))
    type(call(), 'EA8XX')
    view.rerender(phone(rx(14.26)))
    await settle()
    view.rerender(phone(keyedByNexus(14.26)))
    await wait(5000)
    expect(call().value, 'mid-over: untouched').toBe('EA8XX')
    view.rerender(phone(rx(14.26)))
    await wait(5000)
    expect(call().value, 'called on 14.260: the contact is there now').toBe('EA8XX')
  })

  it('a move made while the other-radio override is open never wipes, then or when it closes', async () => {
    setQsyWipe(true)
    const view = render(phone(rx(14.25)))
    type(call(), 'EA8XX')
    const toggle = screen.getByRole('button', { name: /Log a contact from another radio/ })
    fireEvent.click(toggle)
    view.rerender(phone(rx(14.27)))
    await wait(5000)
    expect(call().value, 'override open').toBe('EA8XX')
    fireEvent.click(toggle)
    await wait(5000)
    expect(call().value, 'override closed: the line is where the dial is now').toBe('EA8XX')
  })

  it('an empty line is never set aside, and a call typed after a QSY is anchored where it was typed', async () => {
    setQsyWipe(true)
    const view = render(phone(rx(14.25)))
    view.rerender(phone(rx(14.3)))
    await wait(5000)
    expect(notice()).toBeNull()
    type(call(), 'EA8XX')
    view.rerender(phone(rx(14.301)))
    await wait(5000)
    expect(call().value).toBe('EA8XX')
  })

  it('the Satellites line is never touched: its dial follows Doppler', async () => {
    setQsyWipe(true)
    const view = render(sat(rx(145.8)))
    type(call(), 'EA8XX')
    view.rerender(sat(rx(145.81)))
    await wait(5000)
    expect(call().value).toBe('EA8XX')
  })

  it('turned on with a call already in the line, the call is anchored where the dial is then', async () => {
    const view = render(phone(rx(14.25)))
    type(call(), 'EA8XX')
    view.rerender(phone(rx(14.26)))
    await settle()
    act(() => setQsyWipe(true))
    await wait(5000)
    expect(call().value).toBe('EA8XX')
    view.rerender(phone(rx(14.27)))
    await wait(2500)
    expect(call().value).toBe('')
  })
})
