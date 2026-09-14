// @vitest-environment jsdom
//
// D#19 — an ERROR toast must stay long enough to be read. At the shared 4 s default a
// failure ("QRZ upload failed: …") was gone before an operator looking at the rig got back
// to the screen, and several call sites asked for even less (3 s). Errors now stay at least
// ERROR_MIN_TTL_MS, or until dismissed; info/success toasts keep their own timing.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { pushToast, dismissToast, subscribeToasts, ERROR_MIN_TTL_MS, type Toast } from './toast'

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
