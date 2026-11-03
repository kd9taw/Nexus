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
vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  readRotator: vi.fn(async () => 120),
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
  localStorage.clear()
})
afterEach(cleanup)

describe('the Needed board rotator Go', () => {
  it('stops a tracker first, then turns the antenna', async () => {
    const { container } = render(
      <NeededPanel alerts={[]} bandPlan={[]} selectedCall={null} onQsy={() => {}} onSelect={() => {}} onPoint={() => {}} />,
    )
    const input = container.querySelector<HTMLInputElement>('.np-rotator-input')!
    fireEvent.change(input, { target: { value: '90' } })
    fireEvent.click(container.querySelector('.np-rotator-go')!)
    await waitFor(() => expect(order).toEqual(['stop', 'point 90']))
    expect(container.querySelector('.np-rotator-go')!.getAttribute('title')).toBe(t('needed.rotator.go.title'))
  })
})
