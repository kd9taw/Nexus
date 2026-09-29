// @vitest-environment jsdom
//
// D#19 — an ERROR toast must stay long enough to be read. At the shared 4 s default a
// failure ("QRZ upload failed: …") was gone before an operator looking at the rig got back
// to the screen, and several call sites asked for even less (3 s). Errors now stay at least
// ERROR_MIN_TTL_MS, or until dismissed; info/success toasts keep their own timing.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import {
  pushToast,
  dismissToast,
  subscribeToasts,
  subscribePopups,
  setPopupNotifications,
  popsUpWhenOff,
  ERROR_MIN_TTL_MS,
  type Toast,
} from './toast'

let current: Toast[] = []
let unsub: () => void = () => {}

beforeEach(() => {
  vi.useFakeTimers()
  unsub = subscribeToasts((t) => {
    current = t
  })
})
afterEach(() => {
  for (const t of current) dismissToast(t.id)
  unsub()
  vi.useRealTimers()
})

const visible = (id: number) => current.some((t) => t.id === id)

describe('error toasts stay readable (D#19)', () => {
  it('the floor is at least 12 s', () => {
    expect(ERROR_MIN_TTL_MS).toBeGreaterThanOrEqual(12_000)
  })

  it('a default error toast is still up after 11.9 s and gone after the floor', () => {
    const id = pushToast('QRZ upload failed', 'error')
    vi.advanceTimersByTime(11_900)
    expect(visible(id)).toBe(true)
    vi.advanceTimersByTime(ERROR_MIN_TTL_MS)
    expect(visible(id)).toBe(false)
  })

  it('a call site asking for a SHORTER error ttl is raised to the floor', () => {
    const id = pushToast('No channel on 60m', 'error', 3000)
    vi.advanceTimersByTime(11_900)
    expect(visible(id)).toBe(true)
  })

  it('a longer error ttl is kept, and 0 still means "until dismissed"', () => {
    const long = pushToast('Storm alert', 'error', 30_000)
    const sticky = pushToast('Download failed', 'error', 0)
    vi.advanceTimersByTime(29_000)
    expect(visible(long)).toBe(true)
    vi.advanceTimersByTime(600_000)
    expect(visible(long)).toBe(false)
    expect(visible(sticky)).toBe(true)
    dismissToast(sticky)
    expect(visible(sticky)).toBe(false)
  })

  it('control: info and success toasts are unchanged (4 s default, explicit ttl honoured)', () => {
    const info = pushToast('QSY 20m', 'info')
    const ok = pushToast('Saved', 'success', 2000)
    vi.advanceTimersByTime(2100)
    expect(visible(ok)).toBe(false)
    expect(visible(info)).toBe(true)
    vi.advanceTimersByTime(2000)
    expect(visible(info)).toBe(false)
  })
})

// #391: WHAT STILL POPS UP WITH POP-UPS OFF. The rule case by case (`popsUpWhenOff`), then the two
// subscriptions: the corner's, which the switch filters, and the bus's, which it never touches.
describe('pop-ups off (#391)', () => {
  const noop = () => {}
  it.each([
    ['a confirmation', { kind: 'success' }, false],
    ['a decode alert, loud and with its Work button', { kind: 'success', alert: true, prominent: true, action: noop }, false],
    ['a quiet alert with a button (a new grid)', { kind: 'info', alert: true, action: noop }, false],
    ['an alert drawn red for loudness (a storm)', { kind: 'error', alert: true, prominent: true }, false],
    ['an error', { kind: 'error' }, true],
    ['a notice (a transmit refusal)', { kind: 'info' }, true],
    ['a prominent notice that is not an alert (the ISS auto-arm)', { kind: 'success', prominent: true }, true],
    ['a confirmation with a button that is a control (Undo)', { kind: 'success', action: noop }, true],
    ['a prominent notice (a contest start warning)', { kind: 'info', prominent: true }, true],
  ] as const)('%s', (_what, fields, shows) => {
    expect(popsUpWhenOff({ id: 1, message: 'm', ...fields } as Toast)).toBe(shows)
  })

  let shown: Toast[] = []
  let unsubPopups: () => void = () => {}
  beforeEach(() => {
    unsubPopups = subscribePopups((now) => {
      shown = now
    })
  })
  afterEach(() => {
    setPopupNotifications(true)
    unsubPopups()
  })
  const inCorner = (id: number) => shown.some((t) => t.id === id)

  it('takes a confirmation already up out of the corner at once, and brings it back when turned on', () => {
    const logged = pushToast('Logged W1AW', 'success')
    const failed = pushToast('Could not write the log', 'error')
    expect(inCorner(logged) && inCorner(failed), 'fixture: both up').toBe(true)
    setPopupNotifications(false)
    expect(inCorner(logged), 'the confirmation stayed in the corner').toBe(false)
    expect(inCorner(failed), 'the error left the corner').toBe(true)
    expect(visible(logged), 'the bus lost the confirmation').toBe(true)
    setPopupNotifications(true)
    expect(inCorner(logged)).toBe(true)
  })

  it('keeps a toast raised while off on the bus, out of the corner, and on its own timer', () => {
    setPopupNotifications(false)
    const saved = pushToast('Saved', 'success', 2000)
    expect(visible(saved)).toBe(true)
    expect(inCorner(saved)).toBe(false)
    vi.advanceTimersByTime(2100)
    expect(visible(saved), 'a hidden toast outlived its ttl').toBe(false)
  })
})
