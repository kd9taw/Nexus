// @vitest-environment jsdom
//
// DELETING A RECEIVED PICTURE, FROM THE KEYBOARD: once it is gone, the keyboard is on the same
// control of the picture that takes its place — the next one, else the one before — and on the
// gallery itself when none is left. The ✕ it was on goes with the picture, and the question
// (confirmDialog) used to leave the keyboard on the page itself.

import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { SstvView } from './SstvView'
import { ConfirmHost } from '../confirm'
import { getSstvState, sstvDeleteImage } from '../api'
import type { AppSnapshot, SstvState } from '../types'

vi.mock('./Waterfall', () => ({ Waterfall: () => null }))
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
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

const snap = { mycall: 'KD9TAW', radio: { dialMhz: 14.23, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: true, tuning: false, txAllowed: true } } as unknown as AppSnapshot
const picture = (hh: string, mode: string) => ({
  path: `/data/sstv-gallery/20260717T${hh}0000Z.bmp`, mode, finishedUtc: `2026-07-17T${hh}:00:00Z`, freqMhz: 14.23, lines: 256,
})
let gallery = [picture('10', 'Robot 36'), picture('11', 'Scottie 1'), picture('12', 'PD120')]
const idle = () => ({ armed: false, mode: null, linesDone: 0, linesTotal: 0, previewRgbBase64: null, previewWidth: 0, previewHeight: 0, hedrShiftHz: 0,
  gallery, health: { armed: false, audioPeak: 0, lastAudioUnix: null, drains: 0, visSeen: 0, lastVisUnix: null, unknownVis: 0, lastUnknownVisCode: null, lastUnknownVisUnix: null, images: 0, lastImageUnix: null },
  sending: false, txMode: null, txProgress: 0, txElapsedSecs: 0, txTotalSecs: 0 }) as unknown as SstvState

beforeEach(() => {
  vi.mocked(getSstvState).mockImplementation(async () => idle())
  vi.mocked(sstvDeleteImage).mockImplementation(async (path: string) => {
    gallery = gallery.filter((g) => g.path !== path)
    return undefined as never
  })
})
afterEach(() => cleanup())

const deletes = () => [...document.querySelectorAll<HTMLButtonElement>('.sstv-thumb-del')]
/** The ✕ of the picture shown in `mode`, focused and pressed, and the question answered yes. */
async function deleteByKeyboard(mode: string) {
  const del = deletes().find((b) => b.closest('.sstv-thumb')?.textContent?.includes(mode))!
  act(() => del.focus())
  fireEvent.click(del)
  fireEvent.click(await screen.findByRole('button', { name: /delete/i }))
  await waitFor(() => expect(del.isConnected).toBe(false))
}
const modeOf = (el: Element | null) => el?.closest('.sstv-thumb')?.querySelector('.sstv-thumb-mode')?.textContent

it('the next picture’s ✕, the one before when it was the last, and the gallery when none is left', async () => {
  gallery = [picture('10', 'Robot 36'), picture('11', 'Scottie 1'), picture('12', 'PD120')]
  render(<><SstvView snap={snap} /><ConfirmHost /></>)
  await waitFor(() => expect(deletes()).toHaveLength(3)) // newest first: PD120, Scottie 1, Robot 36

  await deleteByKeyboard('Scottie 1')
  await waitFor(() => expect(document.activeElement?.className).toBe('sstv-thumb-del'))
  expect(modeOf(document.activeElement), 'the picture that took its place').toBe('Robot 36')

  await deleteByKeyboard('Robot 36')
  await waitFor(() => expect(modeOf(document.activeElement), 'the one before it').toBe('PD120'))

  await deleteByKeyboard('PD120')
  await waitFor(() => expect(document.activeElement).not.toBe(document.body))
  expect(document.activeElement?.classList.contains('sstv-gallery-grid'), 'the gallery itself').toBe(true)
})
