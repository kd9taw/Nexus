// @vitest-environment jsdom
//
// The rig controls every cockpit shares — the band picker, the tuning strip (nudge, typed dial,
// VFO), a scope click or drag and the dial wheel — used to drop a failed call without a word. At
// the station that stays so (the next read-back shows what the rig took). On a computer driving
// a station the refusal is usually the link's or the station's, so there it is shown, in the
// words it came with, and a burst shows it once.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import type { RefObject } from 'react'
import { render, cleanup, fireEvent, screen, renderHook, act } from '@testing-library/react'
import type { AppSnapshot } from './types'

const driving = vi.hoisted(() => ({ on: false }))
vi.mock('./features/durableStore', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./features/durableStore')>()),
  drivesAnotherStation: () => driving.on,
}))
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./toast')>()),
  pushToast: vi.fn(),
}))
// Each case is refused in words of its own, so the burst guard (the same words within a few
// seconds show once) never carries over from the case before.
const refusal = vi.hoisted(() => ({ n: 0, text: () => `Not connected to the station. (${refusal.n})` }))
vi.mock('./api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./api')>()),
  getLicensedBandPlan: vi.fn(async () => [{ band: '20m', dialMhz: 14.074, mode: 'USB' }]),
  pickBand: vi.fn(() => Promise.reject(refusal.text())),
  setFrequency: vi.fn(() => Promise.reject(refusal.text())),
  setVfo: vi.fn(() => Promise.reject(refusal.text())),
}))
// The real menu is a Radix portal that jsdom cannot open; a button per band is the same onPick.
vi.mock('./components/BandMenu', () => ({
  BandMenu: ({ items, onPick }: { items: { value: string }[]; onPick: (b: string) => void }) => (
    <div>
      {items.map((it) => (
        <button key={it.value} type="button" onClick={() => onPick(it.value)}>
          {it.value}
        </button>
      ))}
    </div>
  ),
}))

import { pushToast } from './toast'
import { BandPicker } from './components/BandPicker'
import { TuningStrip } from './components/TuningStrip'
import { useScopeTune } from './useScopeTune'
import { useWheelTune } from './useWheelTune'

const toast = pushToast as unknown as ReturnType<typeof vi.fn>
const snap = {
  radio: { dialMhz: 14.074, band: '20m', sideband: 'USB', txAllowed: true, catOk: true, activeVfo: 'A' },
} as unknown as AppSnapshot
const settle = () => act(async () => { await new Promise((r) => setTimeout(r, 0)) })

beforeEach(() => {
  toast.mockClear()
  vi.useRealTimers()
})
afterEach(() => {
  cleanup()
  driving.on = false
})

describe.each([
  ['driving a station', true],
  ['at the station', false],
])('a refused rig control, %s', (_where, on) => {
  const shown = (n: number) => (on ? expect(toast).toHaveBeenCalledTimes(n) : expect(toast).not.toHaveBeenCalled())
  beforeEach(() => {
    driving.on = on
    refusal.n++
  })

  it('a band pick', async () => {
    render(<BandPicker snap={snap} mode="phone" />)
    await settle()
    fireEvent.click(await screen.findByRole('button', { name: '20m' }))
    await settle()
    shown(1)
    if (on) expect(toast).toHaveBeenCalledWith(refusal.text(), 'error')
  })

  it('a tuning-strip nudge and a VFO pick', async () => {
    render(<TuningStrip snap={snap} />)
    fireEvent.click(screen.getAllByRole('button', { name: /^Tune up/ })[0])
    await settle()
    shown(1)
    refusal.n++
    fireEvent.click(screen.getByRole('button', { name: 'B' }))
    await settle()
    shown(on ? 2 : 0)
  })

  it('a scope click', async () => {
    const tune = renderHook(() => useScopeTune({ sideband: 'USB', enabled: true })).result
    tune.current({ dialHz: 14_100_000, kind: 'click' })
    await settle()
    shown(1)
  })

  it('a wheel burst says it once', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'performance'] })
    const el = document.createElement('div')
    const ref = { current: el } as RefObject<HTMLElement | null>
    const tuneBy = renderHook(() =>
      useWheelTune(ref, { dialMhz: 14.1, sideband: 'USB', enabled: true, stepHz: 100 }),
    ).result
    for (let i = 0; i < 3; i++) {
      tuneBy.current(100)
      vi.advanceTimersByTime(150)
      await act(async () => { await Promise.resolve(); await Promise.resolve() })
    }
    shown(1)
  })
})
