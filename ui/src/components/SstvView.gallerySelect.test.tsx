// @vitest-environment jsdom
//
// CLEARING OUT THE GALLERY: tick several pictures and delete them together, after ONE question that
// says how many. Pictures that could not be removed stay ticked, and a message says how many and why.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { SstvView } from './SstvView'
import { ConfirmHost } from '../confirm'
import { getSstvState, sstvDeleteImages } from '../api'
import { pushToast } from '../toast'
import type { AppSnapshot, SstvGalleryEntry, SstvState } from '../types'

// The house budget for a SstvView render suite on a loaded box (SstvView.deleteFocus.test.tsx).
vi.setConfig({ testTimeout: 15_000 })

vi.mock('./Waterfall', () => ({ Waterfall: () => null }))
vi.mock('./RotorStrip', () => ({ RotorStrip: () => null }))
globalThis.ResizeObserver ??= class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
vi.mock('../api', () => ({
  getSstvState: vi.fn(),
  sstvArm: vi.fn(),
  sstvAutoArm: vi.fn(async () => null),
  getLicensedBandPlan: vi.fn(async () => []),
  sstvSend: vi.fn(),
  sstvStop: vi.fn(),
  setOperatingMode: vi.fn(),
  setRfPower: vi.fn(async () => {}),
  revealSstvGallery: vi.fn(async () => {}),
  sstvManualRx: vi.fn(),
  openPanelWindow: vi.fn(async () => {}),
  sstvDeleteImage: vi.fn(async () => {}),
  sstvDeleteImages: vi.fn(async () => []),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

const snap = { mycall: 'N0CALL', radio: { dialMhz: 14.23, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: true, tuning: false, txAllowed: true } } as unknown as AppSnapshot

const picture = (hh: string, mode: string): SstvGalleryEntry => ({
  path: `/pictures/Nexus SSTV/20261030T${hh}0000Z.png`, mode, finishedUtc: `2026-10-30T${hh}:00:00Z`, freqMhz: 14.23, lines: 256,
})
let gallery: SstvGalleryEntry[] = []
const idle = () => ({ armed: false, mode: null, linesDone: 0, linesTotal: 0, previewRgbBase64: null, previewWidth: 0, previewHeight: 0, hedrShiftHz: 0,
  gallery, health: { armed: false, audioPeak: 0, lastAudioUnix: null, drains: 0, visSeen: 0, lastVisUnix: null, unknownVis: 0, lastUnknownVisCode: null, lastUnknownVisUnix: null, images: 0, lastImageUnix: null },
  sending: false, txMode: null, txProgress: 0, txElapsedSecs: 0, txTotalSecs: 0 }) as unknown as SstvState

beforeEach(() => {
  vi.mocked(getSstvState).mockImplementation(async () => idle())
  vi.mocked(sstvDeleteImages).mockReset()
  vi.mocked(sstvDeleteImages).mockImplementation(async (paths: string[]) => {
    gallery = gallery.filter((g) => !paths.includes(g.path))
    return []
  })
  vi.mocked(pushToast).mockClear()
})
afterEach(() => cleanup())

const tickFor = (mode: string) => screen.getByRole('checkbox', { name: new RegExp(mode) }) as HTMLInputElement
const ticked = () => screen.queryAllByRole('checkbox').filter((c) => (c as HTMLInputElement).checked).map((c) => c.closest('.sstv-thumb')!.querySelector('.sstv-thumb-mode')!.textContent)

describe('the gallery’s tick boxes', () => {
  beforeEach(() => {
    gallery = [picture('10', 'Robot 36'), picture('11', 'Scottie 1'), picture('12', 'PD-120'), picture('13', 'Martin 1')]
  })

  it('ticks one picture from the keyboard, and Select all / Clear selection do what they say', async () => {
    render(<><SstvView snap={snap} /><ConfirmHost /></>)
    await waitFor(() => expect(screen.getAllByRole('checkbox')).toHaveLength(4))
    expect(screen.queryByRole('button', { name: /Delete selected/ }), 'nothing ticked, nothing to delete').toBeNull()

    const box = tickFor('Martin 1')
    act(() => box.focus())
    fireEvent.click(box) // Space on a focused checkbox is a click
    expect(ticked()).toEqual(['Martin 1'])
    screen.getByRole('button', { name: 'Delete selected (1)' })

    fireEvent.click(screen.getByRole('button', { name: 'Select all' }))
    expect(ticked()).toHaveLength(4)
    screen.getByRole('button', { name: 'Delete selected (4)' })

    fireEvent.click(screen.getByRole('button', { name: 'Clear selection' }))
    expect(ticked()).toEqual([])
  })

  it('there is no Select incomplete', async () => {
    render(<><SstvView snap={snap} /><ConfirmHost /></>)
    await waitFor(() => expect(screen.getAllByRole('checkbox')).toHaveLength(4))
    expect(screen.queryByRole('button', { name: /incomplete/i })).toBeNull()
  })

  it('Delete selected asks once, naming the count, and deletes exactly the ticked pictures', async () => {
    render(<><SstvView snap={snap} /><ConfirmHost /></>)
    await waitFor(() => expect(screen.getAllByRole('checkbox')).toHaveLength(4))
    fireEvent.click(tickFor('Robot 36'))
    fireEvent.click(tickFor('Martin 1'))
    fireEvent.click(screen.getByRole('button', { name: 'Delete selected (2)' }))

    const dialog = await screen.findByRole('dialog')
    expect(dialog.textContent).toContain('Delete 2 pictures?')
    fireEvent.click(within(dialog).getByRole('button', { name: 'Delete 2 pictures' }))

    await waitFor(() => expect(sstvDeleteImages).toHaveBeenCalledTimes(1))
    expect(vi.mocked(sstvDeleteImages).mock.calls[0][0].sort()).toEqual([
      '/pictures/Nexus SSTV/20261030T100000Z.png',
      '/pictures/Nexus SSTV/20261030T130000Z.png',
    ])
    await waitFor(() => expect(screen.getAllByRole('checkbox')).toHaveLength(2))
    expect(ticked()).toEqual([])
  })

  it('a No deletes nothing and keeps the ticks', async () => {
    render(<><SstvView snap={snap} /><ConfirmHost /></>)
    await waitFor(() => expect(screen.getAllByRole('checkbox')).toHaveLength(4))
    fireEvent.click(tickFor('Robot 36'))
    fireEvent.click(screen.getByRole('button', { name: 'Delete selected (1)' }))
    const dialog = await screen.findByRole('dialog')
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    expect(sstvDeleteImages).not.toHaveBeenCalled()
    expect(ticked()).toEqual(['Robot 36'])
  })

  it('a partial failure says how many stayed and why, and leaves just those ticked', async () => {
    vi.mocked(sstvDeleteImages).mockImplementation(async (paths: string[]) => {
      const stuck = paths.find((p) => p.includes('T11'))!
      gallery = gallery.filter((g) => !paths.includes(g.path) || g.path === stuck)
      return [{ path: stuck, reason: 'Could not delete it: in use' }]
    })
    render(<><SstvView snap={snap} /><ConfirmHost /></>)
    await waitFor(() => expect(screen.getAllByRole('checkbox')).toHaveLength(4))
    fireEvent.click(tickFor('Robot 36'))
    fireEvent.click(tickFor('Scottie 1'))
    fireEvent.click(screen.getByRole('button', { name: 'Delete selected (2)' }))
    const dialog = await screen.findByRole('dialog')
    fireEvent.click(within(dialog).getByRole('button', { name: 'Delete 2 pictures' }))

    await waitFor(() => expect(screen.getAllByRole('checkbox')).toHaveLength(3))
    expect(pushToast).toHaveBeenCalledWith('1 picture was not deleted: Could not delete it: in use', 'error')
    expect(ticked()).toEqual(['Scottie 1'])
  })
})
