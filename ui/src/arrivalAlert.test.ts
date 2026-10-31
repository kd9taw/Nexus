// @vitest-environment jsdom
//
// The NOAA storm-watch heads-up: one quiet toast per watch serial number, never repeated — not on
// the next poll, not after a restart (the serials are kept in localStorage) — and a new serial is
// news.
import { describe, it, expect, beforeEach, vi } from 'vitest'

const toast = vi.hoisted(() => ({ push: vi.fn() }))
const beep = vi.hoisted(() => ({ double: vi.fn() }))
vi.mock('./toast', () => ({ pushToast: toast.push }))
vi.mock('./alerts', () => ({ doubleBeep: beep.double }))

import { processArrivalWatch, resetArrivalAlerts, SEEN_KEY } from './arrivalAlert'
import type { ArrivalOutlook, ArrivalWatch } from './types'
import { dayLabel } from './components/prop/SolarTrends'

const DAY = 86_400
const T0 = 1_793_404_800 // 2026-10-31 00:00 UTC

function outlook(serial: number, window: [number, number] | null = [T0, T0 + 2 * DAY], maxG = 1): ArrivalOutlook {
  const watch: ArrivalWatch = { serial, code: 'WATA20', issued: T0 - 5 * 3600, days: [], comment: null, window, maxG }
  return { watch, scales: null, model: null, l1: null, rationale: null }
}

beforeEach(() => {
  toast.push.mockClear()
  beep.double.mockClear()
  localStorage.clear()
  resetArrivalAlerts()
})

describe('the storm watch toast', () => {
  it('announces a watch once, quietly, naming its level and days', () => {
    processArrivalWatch(outlook(1134), T0 - 3600)
    expect(toast.push).toHaveBeenCalledTimes(1)
    const [msg, kind, , opts] = toast.push.mock.calls[0]
    expect(msg).toContain('G1')
    expect(msg).toContain(`${dayLabel(T0)}–${dayLabel(T0 + DAY)}`)
    expect(kind).toBe('info')
    expect(opts).toEqual({ alert: true })
    expect(beep.double, 'a watch is quiet: no beep').not.toHaveBeenCalled()
    processArrivalWatch(outlook(1134), T0 - 3000)
    processArrivalWatch(outlook(1134), T0 + 3600)
    expect(toast.push, 'the same serial announced twice').toHaveBeenCalledTimes(1)
  })

  it('stays quiet after a restart for a serial already announced', () => {
    processArrivalWatch(outlook(1134), T0 - 3600)
    resetArrivalAlerts() // a new window/session: only the stored serials remain
    processArrivalWatch(outlook(1134), T0 - 3000)
    expect(toast.push).toHaveBeenCalledTimes(1)
    expect(JSON.parse(localStorage.getItem(SEEN_KEY) ?? '[]')).toEqual([1134])
  })

  it('announces a new serial: a reissued watch is news', () => {
    processArrivalWatch(outlook(1134), T0 - 3600)
    processArrivalWatch(outlook(1135, [T0, T0 + DAY], 2), T0 - 1800)
    expect(toast.push).toHaveBeenCalledTimes(2)
    expect(toast.push.mock.calls[1][0]).toContain('G2')
  })

  it('says nothing with no watch, a watch whose window is over, or no feed', () => {
    processArrivalWatch(null, T0)
    processArrivalWatch({ watch: null, scales: null, model: null, l1: null, rationale: null }, T0)
    processArrivalWatch(outlook(1134), T0 + 2 * DAY)
    processArrivalWatch(outlook(1136, null), T0)
    expect(toast.push).not.toHaveBeenCalled()
  })

  it('still announces once per serial when storage refuses', () => {
    const set = vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('quota')
    })
    try {
      processArrivalWatch(outlook(1200), T0 - 3600)
      processArrivalWatch(outlook(1200), T0 - 3000)
      expect(toast.push).toHaveBeenCalledTimes(1)
    } finally {
      set.mockRestore()
    }
  })
})
