// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { AppSnapshot } from '../types'
import { ConfirmHost } from '../confirm'
import { t } from '../i18n'

// The desktop's own self-spot: nothing here reaches the backend, pota.app or a cluster. `selfSpot`
// is the one door to both, and it is a mock.
const api = vi.hoisted(() => ({
  selfSpot: vi.fn(),
  clearActivation: vi.fn(),
  getOtaSpots: vi.fn(async () => []),
  getActivation: vi.fn(async () => ({ program: 'POTA', reference: 'US-0001', qsoCount: 3 })),
  parksCount: vi.fn(async () => 0),
  huntedParksCount: vi.fn(async () => 0),
  logActivations: vi.fn(async () => []),
}))
vi.mock('../api', () => ({
  ...api,
  clearHuntTarget: vi.fn(), openPanelWindow: vi.fn(), setHuntTarget: vi.fn(), setActivation: vi.fn(),
  downloadParks: vi.fn(), importParksCsv: vi.fn(), importHuntedParksCsv: vi.fn(),
}))
const toast = vi.hoisted(() => ({ pushToast: vi.fn() }))
vi.mock('../toast', async (original) => ({ ...(await original<typeof import('../toast')>()), pushToast: toast.pushToast }))

import { PotaSotaView } from './PotaSotaView'
import { RESPOT_DWELL_MS, respotStep, respotThresholdHz, respotWatch } from '../selfSpot'

// THE BUDGET (2026-10-09). The slowest case here, "posts nothing until the operator presses Spot me and…", takes
// 0.36 s and 0.39 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const snap = { hunt: null, radio: { dialMhz: 14.285 } } as unknown as AppSnapshot
const tuned = (dialMhz: number, operatingMode = 'cw') =>
  ({ hunt: null, radio: { dialMhz, operatingMode } }) as unknown as AppSnapshot
beforeEach(() => {
  api.selfSpot.mockReset()
  toast.pushToast.mockReset()
  api.clearActivation.mockReset()
  api.clearActivation.mockResolvedValue({ program: null, reference: null, qsoCount: 0 })
})
afterEach(() => { cleanup(); localStorage.clear(); vi.useRealTimers() })

async function view() {
  render(<><PotaSotaView snap={snap} /><ConfirmHost /></>)
  return screen.findByRole('button', { name: t('ota.selfSpot.button') })
}

it('posts nothing until the operator presses Spot me and confirms, then posts once', async () => {
  api.selfSpot.mockResolvedValue({ parks: [{ reference: 'US-0001', pota: 'posted' }], cluster: 'queued' })
  const button = await view()
  expect(api.selfSpot).not.toHaveBeenCalled()
  fireEvent.click(button)
  expect(await screen.findByText(t('ota.selfSpot.confirm.body', { reference: 'US-0001', freq: '14.2850', comment: 'QRV' }))).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
  await waitFor(() => expect(screen.queryByRole('button', { name: t('ota.selfSpot.confirm.post') })).toBeNull())
  expect(api.selfSpot).not.toHaveBeenCalled()
  // A second press asks again; the earlier answer is never remembered.
  fireEvent.click(button)
  fireEvent.click(await screen.findByRole('button', { name: t('ota.selfSpot.confirm.post') }))
  await waitFor(() => expect(api.selfSpot).toHaveBeenCalledTimes(1))
  expect(api.selfSpot).toHaveBeenCalledWith('US-0001', 14_285_000, 'QRV')
  await waitFor(() => expect(toast.pushToast).toHaveBeenCalledWith(t('ota.selfSpot.both'), 'success'))
})

it('a two-fer: one confirm names both parks, one press, and each park’s answer is said by name', async () => {
  api.getActivation.mockResolvedValueOnce({ program: 'POTA', reference: 'US-1234,US-5678', qsoCount: 0 })
  api.selfSpot.mockResolvedValue({
    parks: [{ reference: 'US-1234', pota: 'posted' }, { reference: 'US-5678', pota: 'failed' }],
    cluster: 'queued',
  })
  fireEvent.click(await view())
  expect(await screen.findByText(t('ota.selfSpot.confirm.body', { reference: 'US-1234, US-5678', freq: '14.2850', comment: 'QRV' }))).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: t('ota.selfSpot.confirm.post') }))
  await waitFor(() => expect(toast.pushToast).toHaveBeenCalledTimes(3))
  expect(api.selfSpot).toHaveBeenCalledTimes(1)
  expect(api.selfSpot).toHaveBeenCalledWith('US-1234,US-5678', 14_285_000, 'QRV')
  expect(toast.pushToast.mock.calls).toEqual([
    [t('ota.selfSpot.cluster.queued'), 'success'],
    [t('ota.selfSpot.park', { reference: 'US-1234', message: t('ota.selfSpot.pota.posted') }), 'success'],
    [t('ota.selfSpot.park', { reference: 'US-5678', message: t('ota.selfSpot.pota.failed') }), 'error', 8000],
  ])
  // Every park taken and the cluster's line queued: the one message.
  toast.pushToast.mockClear()
  api.getActivation.mockResolvedValueOnce({ program: 'POTA', reference: 'US-1234,US-5678', qsoCount: 0 })
  api.selfSpot.mockResolvedValue({
    parks: [{ reference: 'US-1234', pota: 'posted' }, { reference: 'US-5678', pota: 'posted' }],
    cluster: 'queued',
  })
  cleanup()
  fireEvent.click(await view())
  fireEvent.click(await screen.findByRole('button', { name: t('ota.selfSpot.confirm.post') }))
  await waitFor(() => expect(toast.pushToast).toHaveBeenCalledWith(t('ota.selfSpot.both'), 'success'))
  expect(toast.pushToast).toHaveBeenCalledTimes(1)
})

it('reports each target on its own when they differ, so one failure never hides the other', async () => {
  api.selfSpot.mockResolvedValue({ parks: [{ reference: 'US-0001', pota: 'loginRequired' }], cluster: 'queued' })
  fireEvent.click(await view())
  fireEvent.click(await screen.findByRole('button', { name: t('ota.selfSpot.confirm.post') }))
  await waitFor(() => expect(toast.pushToast).toHaveBeenCalledTimes(2))
  expect(toast.pushToast).toHaveBeenCalledWith(t('ota.selfSpot.cluster.queued'), 'success')
  expect(toast.pushToast).toHaveBeenCalledWith(t('ota.selfSpot.pota.loginRequired'), 'error', 8000)
  expect(toast.pushToast).not.toHaveBeenCalledWith(t('ota.selfSpot.both'), 'success')
})

it('says so when the park or dial moved after the press', async () => {
  api.selfSpot.mockRejectedValue('contextChanged')
  fireEvent.click(await view())
  fireEvent.click(await screen.findByRole('button', { name: t('ota.selfSpot.confirm.post') }))
  await waitFor(() => expect(toast.pushToast).toHaveBeenCalledWith(t('ota.selfSpot.moved'), 'error', 6000))
})

// While a contest that allows spotting only over amateur RF runs (Winter Field Day 2027: "You may
// spot yourself and others only via amateur RF."), the station refuses the self-spot with a token,
// and the toast says why in words rather than as a failure with no reason.
it('says why when a contest that allows spotting only over RF is running', async () => {
  api.selfSpot.mockRejectedValue('spotRfOnly')
  fireEvent.click(await view())
  fireEvent.click(await screen.findByRole('button', { name: t('ota.selfSpot.confirm.post') }))
  await waitFor(() => expect(toast.pushToast).toHaveBeenCalledWith(t('ota.selfSpot.rfOnly'), 'error', 6000))
  expect(toast.pushToast).not.toHaveBeenCalledWith(t('ota.selfSpot.failed'), 'error', 6000)
})

it('gives the keyboard back to Spot me when the question closes, answered either way', async () => {
  // The question is opened by code, and closed with the keyboard left on the page itself.
  api.selfSpot.mockResolvedValue({ parks: [{ reference: 'US-0001', pota: 'posted' }], cluster: 'queued' })
  const button = await view()
  act(() => button.focus())
  fireEvent.click(button)
  fireEvent.click(await screen.findByRole('button', { name: 'Cancel' }))
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  await waitFor(() => expect(document.activeElement, 'after no').toBe(button))
  fireEvent.click(button)
  fireEvent.click(await screen.findByRole('button', { name: t('ota.selfSpot.confirm.post') }))
  await waitFor(() => expect(api.selfSpot).toHaveBeenCalledTimes(1))
  await waitFor(() => expect(document.activeElement, 'after yes').toBe(button))
})

// ---- The comment, the QSY prompt and QRT at Stop ----

/** Press Spot me and confirm it, once. */
async function spotOnce() {
  fireEvent.click(await screen.findByRole('button', { name: t('ota.selfSpot.button') }))
  fireEvent.click(await screen.findByRole('button', { name: t('ota.selfSpot.confirm.post') }))
}

/** The re-spot prompts pushed so far. */
const prompts = () => toast.pushToast.mock.calls.filter((c) => c[3]?.actionLabel === t('ota.respot.button'))

it('carries the operator’s comment to the confirm and the post, upper case', async () => {
  api.selfSpot.mockResolvedValue({ parks: [{ reference: 'US-0001', pota: 'posted' }], cluster: 'queued' })
  await view()
  const comment = screen.getByRole('textbox', { name: t('ota.selfSpot.comment.label') }) as HTMLInputElement
  expect(comment.value, 'QRV by default').toBe('QRV')
  fireEvent.change(comment, { target: { value: 'qrv cw  20wpm' } })
  fireEvent.click(screen.getByRole('button', { name: t('ota.selfSpot.button') }))
  expect(await screen.findByText(t('ota.selfSpot.confirm.body', { reference: 'US-0001', freq: '14.2850', comment: 'QRV CW 20WPM' }))).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: t('ota.selfSpot.confirm.post') }))
  await waitFor(() => expect(api.selfSpot).toHaveBeenCalledWith('US-0001', 14_285_000, 'QRV CW  20WPM'))
})

it('the QSY prompt waits for the move and the dwell, never posts by itself, and its button is Spot me', async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true })
  api.selfSpot.mockResolvedValue({ parks: [{ reference: 'US-0001', pota: 'posted' }], cluster: 'queued' })
  const { rerender } = render(<><PotaSotaView snap={tuned(14.062)} /><ConfirmHost /></>)
  await spotOnce()
  await waitFor(() => expect(api.selfSpot).toHaveBeenCalledTimes(1))
  // 900 Hz on CW is the same frequency, however long it sits there.
  rerender(<><PotaSotaView snap={tuned(14.0629)} /><ConfirmHost /></>)
  await act(async () => { vi.advanceTimersByTime(RESPOT_DWELL_MS + 5000) })
  expect(prompts()).toHaveLength(0)
  // A real move: nothing until it has sat there for the dwell.
  rerender(<><PotaSotaView snap={tuned(14.065)} /><ConfirmHost /></>)
  await act(async () => { vi.advanceTimersByTime(RESPOT_DWELL_MS - 3000) })
  expect(prompts()).toHaveLength(0)
  await act(async () => { vi.advanceTimersByTime(5000) })
  expect(prompts()).toHaveLength(1)
  const [message, kind, , opts] = prompts()[0]
  expect(message).toBe(t('ota.respot.prompt', { freq: '14.065' }))
  expect(kind).toBe('info')
  expect(opts.actionLabel).toBe(t('ota.respot.button'))
  // Offered once for this move, and still nothing posted.
  await act(async () => { vi.advanceTimersByTime(RESPOT_DWELL_MS * 3) })
  expect(prompts()).toHaveLength(1)
  expect(api.selfSpot).toHaveBeenCalledTimes(1)
  expect(screen.queryByRole('dialog'), 'nothing asked until the button is pressed').toBeNull()
  // The button is Spot me: it asks, and posts once on yes, at the new dial.
  act(() => opts.action())
  fireEvent.click(await screen.findByRole('button', { name: t('ota.selfSpot.confirm.post') }))
  await waitFor(() => expect(api.selfSpot).toHaveBeenCalledTimes(2))
  expect(api.selfSpot).toHaveBeenLastCalledWith('US-0001', 14_065_000, 'QRV')
})

it('phone needs 3 kHz before it is a new frequency', () => {
  expect(respotThresholdHz('phone')).toBe(3000)
  expect(respotThresholdHz('cw')).toBe(1000)
  expect(respotThresholdHz('digital')).toBe(1000)
  expect(respotThresholdHz(undefined)).toBe(1000)
  let w = respotWatch(14_285_000)
  for (const at of [0, RESPOT_DWELL_MS, RESPOT_DWELL_MS * 2]) {
    const step = respotStep(w, 14_287_500, at, respotThresholdHz('phone'))
    expect(step.prompt, `2.5 kHz at ${at} ms`).toBe(false)
    w = step.watch
  }
  w = respotWatch(14_285_000)
  w = respotStep(w, 14_288_000, 0, 3000).watch
  expect(respotStep(w, 14_288_100, RESPOT_DWELL_MS, 3000).prompt, 'a nudge keeps the dwell').toBe(true)
})

/** Press Stop: the "Activation over?" checklist opens, and nothing has ended yet. */
async function stop() {
  fireEvent.click(screen.getByRole('button', { name: new RegExp(t('ota.activation.stop.label')) }))
  await screen.findByRole('dialog', { name: t('activationOver.title') })
}
const endButton = () => screen.getByRole('button', { name: t('activationOver.end') })

it('Stop asks first; its checklist offers QRT only once spotted, unticked; End alone posts nothing', async () => {
  api.selfSpot.mockResolvedValue({ parks: [{ reference: 'US-0001', pota: 'posted' }], cluster: 'queued' })
  await view()
  await stop()
  expect(api.clearActivation, 'Stop alone ends nothing').not.toHaveBeenCalled()
  expect(screen.queryByRole('checkbox', { name: t('ota.selfSpot.qrt.label') }), 'not spotted yet').toBeNull()
  fireEvent.click(screen.getByRole('button', { name: t('activationOver.keep') }))
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  expect(api.clearActivation).not.toHaveBeenCalled()
  await spotOnce()
  await waitFor(() => expect(api.selfSpot).toHaveBeenCalledTimes(1))
  await stop()
  const qrt = (await screen.findByRole('checkbox', { name: t('ota.selfSpot.qrt.label') })) as HTMLInputElement
  expect(qrt.checked, 'unticked').toBe(false)
  fireEvent.click(endButton())
  await waitFor(() => expect(api.clearActivation).toHaveBeenCalledTimes(1))
  expect(api.selfSpot).toHaveBeenCalledTimes(1)
  expect(screen.queryByRole('dialog')).toBeNull()
})

it('posts QRT at the end only when ticked in the checklist and confirmed, then ends the activation', async () => {
  api.selfSpot.mockResolvedValue({ parks: [{ reference: 'US-0001', pota: 'posted' }], cluster: 'queued' })
  await view()
  await spotOnce()
  await waitFor(() => expect(api.selfSpot).toHaveBeenCalledTimes(1))
  // Ticked, but declined at the confirm: nothing posted, the activation still ends.
  await stop()
  fireEvent.click(await screen.findByRole('checkbox', { name: t('ota.selfSpot.qrt.label') }))
  fireEvent.click(endButton())
  expect(await screen.findByText(t('ota.selfSpot.confirm.body', { reference: 'US-0001', freq: '14.2850', comment: 'QRT' }))).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
  await waitFor(() => expect(api.clearActivation).toHaveBeenCalledTimes(1))
  expect(api.selfSpot).toHaveBeenCalledTimes(1)
  cleanup()
  api.selfSpot.mockClear()
  // Ticked and confirmed: the QRT goes first, while the activation still runs.
  await view()
  await spotOnce()
  await waitFor(() => expect(api.selfSpot).toHaveBeenCalledTimes(1))
  await stop()
  fireEvent.click(await screen.findByRole('checkbox', { name: t('ota.selfSpot.qrt.label') }))
  fireEvent.click(endButton())
  fireEvent.click(await screen.findByRole('button', { name: t('ota.selfSpot.confirm.post') }))
  await waitFor(() => expect(api.clearActivation).toHaveBeenCalledTimes(2))
  expect(api.selfSpot).toHaveBeenCalledTimes(2)
  expect(api.selfSpot).toHaveBeenLastCalledWith('US-0001', 14_285_000, 'QRT')
  expect(api.selfSpot.mock.invocationCallOrder[1]).toBeLessThan(api.clearActivation.mock.invocationCallOrder[1])
})

it('after a QSY the QRT goes on the frequency the spot stands on, not the new dial', async () => {
  api.selfSpot.mockResolvedValue({ parks: [{ reference: 'US-0001', pota: 'posted' }], cluster: 'queued' })
  const { rerender } = render(<><PotaSotaView snap={tuned(14.285, 'phone')} /><ConfirmHost /></>)
  await spotOnce()
  await waitFor(() => expect(api.selfSpot).toHaveBeenCalledWith('US-0001', 14_285_000, 'QRV'))
  rerender(<><PotaSotaView snap={tuned(14.31, 'phone')} /><ConfirmHost /></>)
  await stop()
  fireEvent.click(await screen.findByRole('checkbox', { name: t('ota.selfSpot.qrt.label') }))
  fireEvent.click(endButton())
  expect(await screen.findByText(t('ota.selfSpot.confirm.body', { reference: 'US-0001', freq: '14.2850', comment: 'QRT' }))).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: t('ota.selfSpot.confirm.post') }))
  await waitFor(() => expect(api.clearActivation).toHaveBeenCalledTimes(1))
  expect(api.selfSpot).toHaveBeenLastCalledWith('US-0001', 14_285_000, 'QRT')
})

it('says why when the comment is one the station refuses', async () => {
  api.selfSpot.mockRejectedValue('invalidComment')
  await view()
  await spotOnce()
  await waitFor(() => expect(toast.pushToast).toHaveBeenCalledWith(t('ota.selfSpot.badComment', { max: 30 }), 'error', 6000))
})
