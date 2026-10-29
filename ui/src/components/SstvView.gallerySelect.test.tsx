// @vitest-environment jsdom
//
// CLEARING OUT THE GALLERY: tick several pictures and delete them together, after ONE question that
// says how many. "Select incomplete" ticks every picture that came in short of its mode's height and
// deletes nothing: the operator sees what is ticked and presses Delete. A picture whose mode is not
// in the table, or whose line count was never recorded, is never ticked by it.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { isIncompletePicture, SstvView } from './SstvView'
import { ConfirmHost } from '../confirm'
import { getSstvState, sstvDeleteImages } from '../api'
import { pushToast } from '../toast'
import { SSTV_RX_MODES } from '../sstvModes'
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

// Full heights from the mode table the tests do not restate (sstv-modes.test.ts holds that table to
// the crate's own `image_lines`).
const height = (name: string) => SSTV_RX_MODES.find((m) => m.name === name)!.height
const picture = (hh: string, mode: string, lines: number): SstvGalleryEntry => ({
  path: `/pictures/Nexus SSTV/20261030T${hh}0000Z.png`, mode, finishedUtc: `2026-10-30T${hh}:00:00Z`, freqMhz: 14.23, lines,
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

describe('isIncompletePicture', () => {
  it('a picture short of its mode’s height is incomplete; a full one is not', () => {
    expect(isIncompletePicture(picture('10', 'Scottie 1', height('Scottie 1') - 40))).toBe(true)
    expect(isIncompletePicture(picture('10', 'Scottie 1', height('Scottie 1')))).toBe(false)
    expect(isIncompletePicture(picture('10', 'PD-120', height('PD-120') - 1))).toBe(true)
  })
  it('finds the mode by its filename slug too, as a picture adopted from the folder carries it', () => {
    expect(isIncompletePicture(picture('10', 'robot36', 100))).toBe(true)
  })
  it('never an unknown mode, and never a picture whose line count was not recorded', () => {
    expect(isIncompletePicture(picture('10', 'Mystery 9', 10))).toBe(false)
    expect(isIncompletePicture(picture('10', '', 10))).toBe(false)
    expect(isIncompletePicture(picture('10', 'Scottie 1', 0))).toBe(false)
  })
})

describe('the gallery’s tick boxes', () => {
  beforeEach(() => {
    gallery = [
      picture('10', 'Robot 36', height('Robot 36')),
      picture('11', 'Scottie 1', 120),
      picture('12', 'Mystery 9', 5),
      picture('13', 'Martin 1', height('Martin 1')),
    ]
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

  it('Select incomplete ticks the short picture only: not the full ones, not the unknown mode, and deletes nothing', async () => {
    render(<><SstvView snap={snap} /><ConfirmHost /></>)
    await waitFor(() => expect(screen.getAllByRole('checkbox')).toHaveLength(4))
    fireEvent.click(screen.getByRole('button', { name: 'Select incomplete' }))
    expect(ticked()).toEqual(['Scottie 1'])
    expect(sstvDeleteImages).not.toHaveBeenCalled()
    expect(screen.queryByRole('dialog')).toBeNull()
  })

  it('Select incomplete with none to find says so and ticks nothing', async () => {
    gallery = [picture('10', 'Robot 36', height('Robot 36')), picture('12', 'Mystery 9', 5)]
    render(<><SstvView snap={snap} /><ConfirmHost /></>)
    await waitFor(() => expect(screen.getAllByRole('checkbox')).toHaveLength(2))
    fireEvent.click(screen.getByRole('button', { name: 'Select incomplete' }))
    expect(ticked()).toEqual([])
    expect(pushToast).toHaveBeenCalledWith('No incomplete pictures in the gallery', 'info')
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
