// @vitest-environment jsdom
// The station's alerts, as this window shows them: the toast's words and button, the sound, once
// per alert, and nothing for an alert that is not the station's. Which alert a decode raises is
// the station's (crates/tempo-app/src/needed_alert.rs) and held to the shared table by
// `alerts.agreement.test.ts`; this file holds the window's half in the operator's own words.

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import type { NeededAlert } from './alerts'
import { __resetStationAlertsForTest, showRaisedAlert, useStationAlerts } from './useStationAlerts'
import type { RaisedAlert } from './usePounce'
import { pushToast } from './toast'
import { renderHook } from '@testing-library/react'

vi.mock('./toast', () => ({ pushToast: vi.fn() }))
const toasts = vi.mocked(pushToast)

const tones: number[] = []
class Tones {
  state = 'running'
  currentTime = 0
  destination = {}
  createOscillator() {
    const osc = {
      type: 'sine',
      frequency: { value: 0, setValueAtTime() {}, exponentialRampToValueAtTime() {} },
      connect() {},
      start: () => tones.push(osc.frequency.value),
      stop() {},
    }
    return osc
  }
  createGain() {
    return { gain: { setValueAtTime() {}, exponentialRampToValueAtTime() {} }, connect() {} }
  }
}

function alert(over: Partial<NeededAlert>): NeededAlert {
  return {
    kind: 'myCall',
    call: 'K1ABC',
    entity: null,
    grid: null,
    tier: null,
    watch: null,
    work: { message: 'KD9TAW K1ABC FN42', snr: -9, freqHz: 1234, tier: 'FT8' },
    ...over,
  }
}

let ids = 1
function raised(a: unknown, kind = 'needed', id = ids++): RaisedAlert {
  return { v: 1, id, kind, atMs: 0, alert: a }
}

beforeEach(() => {
  vi.useFakeTimers()
  vi.stubGlobal('AudioContext', Tones)
  toasts.mockClear()
  tones.length = 0
  __resetStationAlertsForTest()
})
afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

describe('a needed-station alert, in the operator’s words', () => {
  it('someone calling you: loud, long, and an Answer button that works the decode', () => {
    const onWork = vi.fn()
    const a = alert({})
    showRaisedAlert(raised(a), onWork)
    vi.runAllTimers()
    const [text, tone, ms, opts] = toasts.mock.calls[0]
    expect(text).toContain('K1ABC')
    expect(text).toContain('calling you')
    expect([tone, ms]).toEqual(['success', 20000])
    expect(opts).toMatchObject({ alert: true, prominent: true, actionLabel: 'Answer' })
    expect(tones).toEqual([880])
    opts!.action!()
    expect(onWork).toHaveBeenCalledWith(a)
  })

  it('a new DXCC names its entity, double-beeps and offers Work', () => {
    showRaisedAlert(raised(alert({ kind: 'newDxcc', call: 'VP8PJ', entity: 'Falkland Islands' })), () => {})
    vi.runAllTimers()
    const [text, tone, ms, opts] = toasts.mock.calls[0]
    expect(text).toContain('NEW DXCC')
    expect(text).toContain('Falkland Islands')
    expect([tone, ms]).toEqual(['success', 15000])
    expect(opts).toMatchObject({ alert: true, prominent: true, actionLabel: 'Work' })
    expect(tones).toEqual([520, 780])
  })

  it('a rare grid names its tier and square; a plain new grid is quiet', () => {
    showRaisedAlert(raised(alert({ kind: 'rareGrid', call: 'W1AW', grid: 'RR73', tier: 'ultraRare' })), () => {})
    showRaisedAlert(raised(alert({ kind: 'newGrid', call: 'W2AW' })), () => {})
    vi.runAllTimers()
    expect(toasts.mock.calls[0][0]).toContain('ULTRA-RARE grid RR73')
    expect(toasts.mock.calls[0][3]).toMatchObject({ prominent: true, actionLabel: 'Work' })
    expect(toasts.mock.calls[1][0]).toContain('New grid')
    expect(toasts.mock.calls[1].slice(1, 3)).toEqual(['info', 6000])
    expect(toasts.mock.calls[1][3]?.prominent).toBeUndefined()
    expect(tones).toEqual([740, 1110])
  })

  it('a watch-list station names the entry as labelled; an opted-in CQ is a short info toast', () => {
    showRaisedAlert(raised(alert({ kind: 'watch', call: 'W1AW', watch: 'grid EM7*' })), () => {})
    showRaisedAlert(raised(alert({ kind: 'cq', call: 'F5CQ', entity: 'France' })), () => {})
    vi.runAllTimers()
    expect(toasts.mock.calls[0][0]).toContain('⭐ Watch')
    expect(toasts.mock.calls[0][0]).toContain('grid EM7*')
    expect(toasts.mock.calls[0][3]).toMatchObject({ alert: true, prominent: true, actionLabel: 'Work' })
    expect(toasts.mock.calls[1][0]).toContain('F5CQ')
    expect(toasts.mock.calls[1].slice(1, 3)).toEqual(['info', 6000])
    expect(toasts.mock.calls[1][3]).toMatchObject({ actionLabel: 'Answer' })
    expect(tones).toEqual([520, 620, 780])
  })

  it('a station not parsed is "a station", with no button to work it', () => {
    showRaisedAlert(raised(alert({ kind: 'newDxcc', call: null, work: null })), () => {})
    const [text, , , opts] = toasts.mock.calls[0]
    expect(text).not.toContain('null')
    expect(opts?.action).toBeUndefined()
  })
})

describe('each alert shows once, and only the station’s', () => {
  it('one alert raised is one toast; the same alert delivered twice is still one', () => {
    const one = raised(alert({}))
    showRaisedAlert(one, () => {})
    showRaisedAlert(one, () => {})
    expect(toasts).toHaveBeenCalledTimes(1)
    // CONTROL: another alert about the same station is its own toast.
    showRaisedAlert(raised(alert({})), () => {})
    expect(toasts).toHaveBeenCalledTimes(2)
  })

  it('a Pounce alert, a later shape or an empty one is not shown here', () => {
    // Shaped as a needed alert, so only its kind keeps it out.
    showRaisedAlert(raised(alert({}), 'pounce'), () => {})
    showRaisedAlert({ ...raised(alert({})), v: 2 }, () => {})
    showRaisedAlert(raised(null), () => {})
    showRaisedAlert(null, () => {})
    vi.runAllTimers()
    expect(toasts).not.toHaveBeenCalled()
    expect(tones).toEqual([])
  })

  it('a park newly on the air is one double beep, whatever the count, and no toast', () => {
    showRaisedAlert(raised({ references: ['US-0001', 'US-0002', 'US-0003'] }, 'parks'), () => {})
    vi.runAllTimers()
    expect(tones).toEqual([880, 1320])
    expect(toasts).not.toHaveBeenCalled()
  })
})

describe('the window hears the station', () => {
  it('shows each alert the station raises on the `alert` event, and stops listening on unmount', async () => {
    let heard: ((e: { payload: RaisedAlert }) => void) | null = null
    const unlisten = vi.fn()
    const listen = vi.fn(async (name: string, cb: (e: { payload: RaisedAlert }) => void) => {
      expect(name).toBe('alert')
      heard = cb
      return unlisten
    })
    vi.stubGlobal('__TAURI__', { event: { listen } })
    const onWork = vi.fn()
    const { unmount } = renderHook(() => useStationAlerts(onWork))
    await vi.waitFor(() => expect(heard).not.toBeNull())
    const a = alert({ kind: 'cq', call: 'JA1XYZ' })
    heard!({ payload: raised(a) })
    expect(toasts).toHaveBeenCalledTimes(1)
    toasts.mock.calls[0][3]!.action!()
    expect(onWork).toHaveBeenCalledWith(a)
    unmount()
    expect(unlisten).toHaveBeenCalled()
  })
})
