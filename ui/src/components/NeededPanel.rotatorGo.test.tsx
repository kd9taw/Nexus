// @vitest-environment jsdom
//
// THE NEEDED BOARD'S ROTATOR GO ENDS WHATEVER TRACKER HELD THE MAST. A Moon follow (or a satellite
// pass) left running after a manual turn keeps its badge and, once the Moon has moved past the
// deadband, swings the antenna back minutes later. So Go stops the tracker first, as the Rotor
// pane's own moves do, and only then turns the antenna — never the other way round, or the stop's
// halt could land after the turn.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, waitFor } from '@testing-library/react'
import { NeededPanel } from './NeededPanel'
import { t } from '../i18n'

vi.setConfig({ testTimeout: 15_000 })

const order = vi.hoisted(() => [] as string[])
const status = vi.hoisted(() => ({ sat: null as unknown, moon: null as unknown }))
vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  readRotator: vi.fn(async () => 120),
  getSatTrackStatus: vi.fn(async () => status.sat),
  getMoonStatus: vi.fn(async () => status.moon),
  stopSatTrack: vi.fn(async () => {
    order.push('stop')
  }),
  pointRotator: vi.fn(async (deg: number) => {
    order.push(`point ${deg}`)
  }),
}))
vi.mock('../toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
}))

beforeEach(() => {
  order.length = 0
  status.sat = null
  status.moon = null
  localStorage.clear()
})
afterEach(cleanup)

const go = async (deg: string) => {
  const { container } = render(
    <NeededPanel alerts={[]} bandPlan={[]} selectedCall={null} onQsy={() => {}} onSelect={() => {}} onPoint={() => {}} />,
  )
  fireEvent.change(container.querySelector<HTMLInputElement>('.np-rotator-input')!, { target: { value: deg } })
  fireEvent.click(container.querySelector('.np-rotator-go')!)
  await waitFor(() => expect(order.some((o) => o.startsWith('point'))).toBe(true))
  expect(container.querySelector('.np-rotator-go')!.getAttribute('title')).toBe(t('needed.rotator.go.title'))
  cleanup()
  return [...order]
}

describe('the Needed board rotator Go', () => {
  it('a Moon follow, or a pass under way, is stopped first, then the antenna turns', async () => {
    status.moon = { azDeg: 100, elDeg: 20, track: { state: 'following' }, window: null }
    expect(await go('90')).toEqual(['stop', 'point 90'])
    order.length = 0
    status.moon = null
    status.sat = { name: 'SO-50', state: 'tracking' }
    expect(await go('45')).toEqual(['stop', 'point 45'])
  })

  it('an armed pass waiting for AOS stays armed: the rotor is yours for HF until then', async () => {
    status.sat = { name: 'SO-50', state: 'armed' }
    expect(await go('90')).toEqual(['point 90'])
  })
})
