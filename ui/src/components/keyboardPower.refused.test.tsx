// @vitest-environment jsdom
//
// RTTY'S AND PSK'S RF POWER, REFUSED: as Phone's (rigControls.driving.test.tsx), the slider went on
// showing a level the rig never took, with no catch at all. A refused level now goes back to what
// the rig reports, and on a computer driving a station the refusal is shown.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { cleanup, fireEvent, act, screen, render } from '@testing-library/react'
import type { AppSnapshot } from '../types'

const h = vi.hoisted(() => ({ driving: false, n: 0, text: () => `Not connected to the station. (${h.n})` }))
vi.mock('../api', async (importOriginal) => ({
  ...(await import('./stop-line.api.testkit')).stopLineApi(await importOriginal<Record<string, unknown>>()),
  setRfPower: vi.fn(() => Promise.reject(h.text())),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('../features/durableStore', async (actual) => ({
  ...(await actual<typeof import('../features/durableStore')>()),
  drivesAnotherStation: () => h.driving,
}))
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: ({ power }: { power?: { value: number; onChange: (v: number) => void } }) =>
    power ? (
      <input aria-label="power" type="number" value={power.value} onChange={(e) => power.onChange(Number(e.target.value))} />
    ) : null,
}))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))

import { pushToast } from '../toast'
import { panelsWith, settle, snap as base } from './stop-line.testkit'
import { RttyCockpit } from './RttyCockpit'
import { PskCockpit } from './PskCockpit'

// The rig reports 50 %.
const snap = { ...base, radio: { ...base.radio, rfPower: 0.5 } } as AppSnapshot
const COCKPITS = [
  ['RTTY', () => render(<RttyCockpit snap={snap} panels={panelsWith([]) as never} onSetTxEnabled={() => {}} />)],
  ['PSK', () => render(<PskCockpit snap={snap} panels={panelsWith([]) as never} onSetTxEnabled={() => {}} />)],
] as const

beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
  ;(pushToast as unknown as ReturnType<typeof vi.fn>).mockClear()
  h.n++
})
afterEach(() => {
  cleanup()
  h.driving = false
})

describe.each([
  ['driving a station', true],
  ['at the station', false],
])('a refused RF power, %s', (_where, driving) => {
  it.each(COCKPITS)('%s goes back to what the rig reports', async (_n, mount) => {
    h.driving = driving
    mount()
    await settle()
    const power = screen.getByLabelText('power') as HTMLInputElement
    const was = '50'
    expect(power.value, 'control: the slider shows the rig').toBe(was)
    fireEvent.change(power, { target: { value: '7' } })
    expect(power.value, 'control: the slider moved').toBe('7')
    await act(async () => { await new Promise((r) => setTimeout(r, 0)) })
    expect(power.value).toBe(was)
    if (driving) expect(pushToast).toHaveBeenCalledWith(h.text(), 'error')
    else expect(pushToast).not.toHaveBeenCalled()
  })
})
