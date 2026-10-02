// @vitest-environment jsdom
//
// The Clock box and the Space Wx trends as the grid renders them — through the real PaneFrame and
// registry, so a component that works on its own but was never wired into its pane fails here.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup, waitFor } from '@testing-library/react'
import type { DailySolarIndices } from '../../types'

const api = vi.hoisted(() => ({
  getSolarIndices: vi.fn(
    (): Promise<DailySolarIndices> =>
      Promise.resolve({ days: [{ dayUnix: Math.floor(Date.now() / 86_400_000) * 86_400 - 86_400, sfi: 95, ssn: 46 }] }),
  ),
}))
vi.mock('../../api', async (importOriginal) => ({ ...(await importOriginal<object>()), ...api }))

import { PaneFrame } from './PaneFrame'
import { paneById } from './panes'
import { PANE_IDS } from '../../features/connectConfig'
import type { PaneContext } from './paneContext'

afterEach(cleanup)

const ctx = (over: Partial<PaneContext> = {}): PaneContext =>
  ({
    myGrid: 'EN52',
    prop: null,
    scales: null,
    alerts: [],
    muf: [],
    getout: null,
    selectedCall: null,
    pathOpen: [],
    outlookOpen: [],
    ...over,
  }) as unknown as PaneContext

describe('the Clock box', () => {
  it('is a pickable pane that needs no feed — it keeps time with the station offline', () => {
    expect(PANE_IDS).toContain('clock')
    expect(paneById('clock')!.title.length).toBeGreaterThan(0)
    const { container } = render(<PaneFrame slotId="left1" paneId="clock" ctx={ctx()} onAssign={() => {}} />)
    // Its own body, not the one-line fallback PaneFrame shows when a pane has nothing.
    expect(container.querySelector('.clock-pane')).not.toBeNull()
    expect(container.querySelector('.pane-basic')).toBeNull()
  })
})

describe('the Space Wx box', () => {
  it('carries the 30-day trends under its gauges', async () => {
    const live = ctx({
      prop: {
        source: 'live',
        spaceWx: { sfi: 97, kp: 2, aIndex: 7, xrayClass: 'B5', flare: false },
      } as PaneContext['prop'],
    })
    const { container } = render(<PaneFrame slotId="bottom2" paneId="spacewx" ctx={live} onAssign={() => {}} />)
    expect(container.querySelector('.swx-strip')).not.toBeNull()
    await waitFor(() => expect(container.querySelector('.swx-trend[data-index="SSN"]')).not.toBeNull())
    expect(api.getSolarIndices).toHaveBeenCalled()
  })

  it('stays honest offline: no trends and no gauges drawn from a modelled snapshot', () => {
    const off = ctx({
      prop: { source: 'offline', spaceWx: { sfi: 120, kp: 2, aIndex: 8, xrayClass: 'A0', flare: false } } as PaneContext['prop'],
    })
    const { container } = render(<PaneFrame slotId="bottom2" paneId="spacewx" ctx={off} onAssign={() => {}} />)
    expect(container.querySelector('.swx-strip')).toBeNull()
    expect(container.querySelector('.swx-trends')).toBeNull()
    expect(container.querySelector('.pane-basic')).not.toBeNull()
  })
})
