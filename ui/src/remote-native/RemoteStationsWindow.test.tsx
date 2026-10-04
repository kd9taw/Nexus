// @vitest-environment jsdom
//
// The Stations on this network window's entry beside the Remote stations window's, in Settings ▸
// Station ▸ Remote access: one click asks Rust to open it once, in the language the app shows now,
// asking no service for anything; a failure says so in words. The Remote stations entry beside it is
// unchanged (SettingsPanel.remotestations.test.tsx).
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { EN } from '../i18n/en'

const api = vi.hoisted(() => ({ openLan: vi.fn(), openRemote: vi.fn() }))
vi.mock('../api', () => ({ openLanStationsWindow: api.openLan, openRemoteStationsWindow: api.openRemote }))
const locale = vi.hoisted(() => ({ now: 'de' }))
vi.mock('../i18n', async (importOriginal) => ({ ...(await importOriginal<Record<string, unknown>>()), getLocale: () => locale.now }))
import { RemoteStationsWindow } from './RemoteStationsWindow'

beforeEach(() => {
  api.openLan.mockReset().mockResolvedValue(undefined)
  api.openRemote.mockReset().mockResolvedValue(undefined)
})
afterEach(cleanup)

const theButton = () => screen.getByRole('button', { name: EN['settings.lanStations.open'] }) as HTMLButtonElement

it('opens the Stations on this network window once per click, in the language the app shows', async () => {
  let finish: () => void = () => {}
  api.openLan.mockImplementation(() => new Promise<void>(resolve => { finish = resolve }))
  render(<RemoteStationsWindow />)
  expect(screen.getByText(EN['settings.lanStations.hint']).closest('.settings-field')?.contains(theButton())).toBe(true)
  fireEvent.click(theButton())
  expect(api.openLan).toHaveBeenCalledTimes(1)
  expect(api.openLan.mock.calls[0]).toEqual(['de'])
  expect(api.openRemote).not.toHaveBeenCalled()
  await waitFor(() => expect(theButton().disabled).toBe(true))
  finish()
  await waitFor(() => expect(theButton().disabled).toBe(false))
  expect(screen.queryByRole('alert')).toBeNull()
})

it('says so in words when the window could not be opened, and lets the operator try again', async () => {
  api.openLan.mockRejectedValue(new Error('remoteWindowFailed'))
  render(<RemoteStationsWindow />)
  fireEvent.click(theButton())
  expect((await screen.findByRole('alert')).textContent).toBe(EN['settings.lanStations.failed'])
  api.openLan.mockResolvedValue(undefined)
  fireEvent.click(theButton())
  await waitFor(() => expect(screen.queryByRole('alert')).toBeNull())
  expect(api.openLan).toHaveBeenCalledTimes(2)
})
