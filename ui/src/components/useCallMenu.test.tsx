// @vitest-environment jsdom
//
// One call menu everywhere (useCallMenu): a right-click, the Menu key or Shift+F10 on a call opens the
// Call Roster's menu with Best times first; opening it and choosing Best times never reaches the row
// (no select, no tune, no work). On the Spots board, the Chase box and a band map spot label; on the
// band map's track, between labels, a right-click still marks a frequency and opens no menu. The station
// card's Best times button hands its place to the Best times card.
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { NeedAlert, SpotRow } from '../types'
import type { PaneContext } from './connect/paneContext'

vi.mock('../api', async (importOriginal) => {
  const real = await importOriginal<typeof import('../api')>()
  return {
    ...real,
    rotatorBearingToCall: vi.fn(() => Promise.reject(new Error('unset'))),
    getBestTimes: vi.fn(() => new Promise(() => {})),
    getCallStats: vi.fn(() => new Promise(() => {})),
  }
})

import { useCallMenu } from './useCallMenu'
import { SpotsPanel } from './SpotsPanel'
import { BandMap } from './BandMap'
import { ChasePane } from './prop/ChasePane'
import { BestTimesCardHost, closeBestTimesCard } from './BestTimesCard'
import { CallCardHost, closeCallCard, openCallCard } from './CallCard'

beforeAll(() => {
  Element.prototype.hasPointerCapture = () => false
  Element.prototype.setPointerCapture = () => {}
  Element.prototype.releasePointerCapture = () => {}
  Element.prototype.scrollIntoView = () => {}
})
afterEach(() => {
  closeBestTimesCard()
  closeCallCard()
  cleanup()
})

const firstItem = async () => (await screen.findAllByRole('menuitem'))[0].textContent
const card = () => document.querySelector('.bt-card')

function Row({ onRow }: { onRow: () => void }) {
  const menu = useCallMenu()
  return (
    <div role="row" tabIndex={0} onClick={onRow} {...menu.on('r', 'EA8ABC', { band: '20m' })}>
      <span>{menu.at('r')}EA8ABC</span>
    </div>
  )
}

describe('the shared call menu', () => {
  it('opens on a right-click, the Menu key and Shift+F10, Best times first, and reaches no row', async () => {
    const onRow = vi.fn()
    render(
      <>
        <BestTimesCardHost />
        <Row onRow={onRow} />
      </>,
    )
    const row = screen.getByRole('row')
    fireEvent.contextMenu(row)
    expect(await firstItem()).toBe('Best times')
    fireEvent.click((await screen.findAllByRole('menuitem'))[0])
    await waitFor(() => expect(card()).not.toBeNull())
    expect(onRow).not.toHaveBeenCalled()
    for (const key of [{ key: 'ContextMenu', shiftKey: false }, { key: 'F10', shiftKey: true }]) {
      cleanup()
      render(<Row onRow={onRow} />)
      fireEvent.keyDown(screen.getByRole('row'), key)
      expect(await firstItem(), key.key).toBe('Best times')
    }
    expect(onRow).not.toHaveBeenCalled()
  })
})

const spot = (over: Partial<SpotRow>): SpotRow =>
  ({ call: 'W1AW', entity: 'United States', zone: 5, band: '20m', freqMhz: 14.025, mode: 'CW', spotter: 'K3LR', corroborators: [], ageSecs: 30, comment: '', licensed: true, ...over }) as SpotRow

describe('surfaces that had no call menu', () => {
  it('the Spots board: a right-click on a row opens it, and the row neither selects nor works', async () => {
    const onSelect = vi.fn()
    const onWork = vi.fn()
    const { container } = render(<SpotsPanel spots={[spot({ call: 'DL1ABC' })]} bandPlan={[]} selectedCall={null} onSelect={onSelect} onWork={onWork} />)
    fireEvent.contextMenu(container.querySelector('.sp-row')!)
    expect(await firstItem()).toBe('Best times')
    expect(onSelect).not.toHaveBeenCalled()
    expect(onWork).not.toHaveBeenCalled()
  })

  it('the Chase box: a right-click on a row opens it, and the row selects nothing', async () => {
    const onSelectCall = vi.fn()
    const alert = { call: 'EC1DD', entity: 'Spain', band: '20m', zone: 14, tags: ['NewEntity'], priority: 90, headline: 'EC1DD needed', mode: 'CW', freqMhz: 14.025, admittedAt: Math.floor(Date.now() / 1000) - 120 } as unknown as NeedAlert
    const ctx = { myGrid: 'JO21EV', entityCentroids: new Map(), needAlerts: [alert], bandOutlook: null, prop: null, dxpedWindows: new Map(), onSelectCall } as unknown as PaneContext
    const { container } = render(<ChasePane ctx={ctx} />)
    fireEvent.contextMenu(container.querySelector('.chase-row')!)
    expect(await firstItem()).toBe('Best times')
    expect(onSelectCall).not.toHaveBeenCalled()
  })

  it('the band map: a spot label opens it and marks nothing; the track marks as before and opens nothing', async () => {
    const onMarkHere = vi.fn()
    const onWorkSpot = vi.fn()
    const { container } = render(
      <BandMap band="20m" dialMhz={14.025} txAllowed spots={[spot({ call: 'K1ABC', freqMhz: 14.03 })]} spotMode="CW" onWorkSpot={onWorkSpot} tuneEnabled onMarkHere={onMarkHere} />,
    )
    fireEvent.contextMenu(container.querySelector('.bandmap-spot')!)
    expect(await firstItem()).toBe('Best times')
    expect(onMarkHere).not.toHaveBeenCalled()
    expect(onWorkSpot).not.toHaveBeenCalled()
    cleanup()
    const again = render(
      <BandMap band="20m" dialMhz={14.025} txAllowed spots={[spot({ call: 'K1ABC', freqMhz: 14.03 })]} spotMode="CW" onWorkSpot={onWorkSpot} tuneEnabled onMarkHere={onMarkHere} />,
    )
    fireEvent.contextMenu(again.container.querySelector('.bandmap-track')!)
    expect(onMarkHere).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole('menu')).toBeNull()
  })
})

describe('the station card', () => {
  it('has a Best times button that puts the Best times card in its place', async () => {
    const anchor = document.createElement('button')
    document.body.appendChild(anchor)
    const snap = { radio: { operatingMode: 'digital' }, link: { tier: 'FT8' } } as never
    render(
      <>
        <CallCardHost snap={snap} spots={[]} needAlertsByCall={new Map()} />
        <BestTimesCardHost />
      </>,
    )
    act(() => openCallCard('EA8ABC', anchor))
    const btn = document.querySelector<HTMLButtonElement>('.callcard-besttimes')!
    expect(btn.textContent).toBe('Best times')
    fireEvent.click(btn)
    await waitFor(() => expect(card()).not.toBeNull())
    expect(document.querySelectorAll('.callcard:not(.bt-card)')).toHaveLength(0)
    anchor.remove()
  })
})
