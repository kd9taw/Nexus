import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'

// #478: THE SATELLITE ALARM'S NOTICE. Closing it with ✕ hid the notice and left the alarm beeping
// for up to a minute, and its one button only stopped the beep. ✕ now stops the alarm too, and the
// button opens that bird on the Satellites screen (stopping the alarm), where the screen exists.
//
// The window shim delegates to the globals at CALL time, so vitest's fake timers drive the loop.
class MemoryStorage {
  private m = new Map<string, string>()
  get length() { return this.m.size }
  clear() { this.m.clear() }
  getItem(k: string) { return this.m.has(k) ? (this.m.get(k) as string) : null }
  key(i: number) { return [...this.m.keys()][i] ?? null }
  removeItem(k: string) { this.m.delete(k) }
  setItem(k: string, v: string) { this.m.set(k, String(v)) }
}
const memStore = new MemoryStorage() as unknown as Storage
globalThis.localStorage = memStore
vi.stubGlobal('window', {
  localStorage: memStore,
  setTimeout: (...a: Parameters<typeof setTimeout>) => setTimeout(...a),
  clearTimeout: (h: ReturnType<typeof setTimeout>) => clearTimeout(h),
  setInterval: (...a: Parameters<typeof setInterval>) => setInterval(...a),
  clearInterval: (h: ReturnType<typeof setInterval>) => clearInterval(h),
} as unknown as Window & typeof globalThis)

vi.mock('../toast', () => ({ pushToast: vi.fn() }))
vi.mock('../alerts', () => ({ doubleBeep: vi.fn() }))
import { pushToast } from '../toast'
import { doubleBeep } from '../alerts'
import { t } from '../i18n'
import { checkSatAlarms, resetSatAlarms, toggleSatAlarm } from './satAlarm'
import type { SatPass } from '../types'

const toasts = vi.mocked(pushToast)
const beeps = vi.mocked(doubleBeep)
const NOW = 1_800_000_000_000
const pass = (name: string): SatPass =>
  ({ name, aosUnix: NOW / 1000 + 60, losUnix: NOW / 1000 + 660, maxElDeg: 40, aosAzDeg: 200, losAzDeg: 40 }) as SatPass
const opts = () => toasts.mock.calls[0][3]!

beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(NOW)
  memStore.clear()
  resetSatAlarms()
  toasts.mockClear()
  beeps.mockClear()
  toggleSatAlarm('RS-44')
})
afterEach(() => {
  resetSatAlarms()
  vi.useRealTimers()
})

/** Beeps heard over the next ten seconds. */
const beepsOver10s = () => {
  const before = beeps.mock.calls.length
  vi.advanceTimersByTime(10_000)
  return beeps.mock.calls.length - before
}

describe('the satellite alarm notice (#478)', () => {
  it('CONTROL: left alone, the alarm keeps sounding', () => {
    checkSatAlarms([pass('RS-44')], NOW)
    expect(beepsOver10s()).toBeGreaterThan(0)
  })

  it('closing the notice with ✕ stops the alarm', () => {
    checkSatAlarms([pass('RS-44')], NOW)
    opts().onClose!()
    expect(beepsOver10s()).toBe(0)
  })

  it('with the Satellites screen there, the button opens that bird on it and stops the alarm', () => {
    const open = vi.fn()
    checkSatAlarms([pass('RS-44')], NOW, open)
    expect(opts().actionLabel).toBe(t('sat.alarm.open'))
    opts().action!()
    expect(open).toHaveBeenCalledWith('RS-44')
    expect(beepsOver10s()).toBe(0)
  })

  it('without it, the button is Stop alarm, as before', () => {
    checkSatAlarms([pass('RS-44')], NOW)
    expect(opts().actionLabel).toBe(t('sat.alarm.stop'))
    opts().action!()
    expect(beepsOver10s()).toBe(0)
  })
})
