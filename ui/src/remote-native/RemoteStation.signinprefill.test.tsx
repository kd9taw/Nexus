// @vitest-environment jsdom
// B5: the sign-in / pairing buttons are dead until a station name is typed, and the field starts
// empty — so there was nothing to do on arrival and no hint that there should be. The field now
// prefills with the operator's callsign (the first-hour setting every station has), so the buttons
// are live at once; with no callsign, an inline hint asks for a name.
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import { RemoteStation } from './RemoteStation'
import type { AccountStatus, RemoteStationStatus } from './types'
import { EN } from '../i18n/en'

vi.setConfig({ testTimeout: 15_000 })
afterEach(() => { cleanup(); delete window.__TAURI_INTERNALS__ })

const noAccount: AccountStatus = { signingIn: false, added: [], unconfirmed: {}, removals: [], people: [] }
const unpaired: RemoteStationStatus = {
  phase: 'unpaired', origin: 'https://remote.hamradiotools.io', stationId: null, accountId: null,
  pairingId: null, pairingCode: null, expiresAt: null, devices: [], error: null, account: noAccount,
}

/** The card over a station that has not signed in, with the settings `get_settings` answers. */
function shack(settings: unknown) {
  window.__TAURI_INTERNALS__ = { invoke: (async (command: string) => {
    if (command === 'get_remote_station_status') return unpaired
    if (command === 'get_settings') return settings
    if (command === 'remote_station_action') return unpaired
    throw new Error('unexpectedCommand')
  }) as NonNullable<Window['__TAURI_INTERNALS__']>['invoke'] }
  render(<RemoteStation />)
}

it('prefills the station name with the callsign, so Sign in is live on mount', async () => {
  shack({ mycall: 'KD9TAW' })
  const name = await screen.findByLabelText(EN['remote.stationName']) as HTMLInputElement
  await waitFor(() => expect(name.value).toBe('KD9TAW'))
  const signIn = screen.getByRole('button', { name: EN['remote.account.signIn'] }) as HTMLButtonElement
  expect(signIn.disabled).toBe(false)
  expect(screen.queryByText(EN['remote.stationNameHint'])).toBeNull()
})

it('with no callsign, leaves the field empty, keeps the buttons dead, and shows the hint', async () => {
  shack({ mycall: '' })
  const signIn = await screen.findByRole('button', { name: EN['remote.account.signIn'] }) as HTMLButtonElement
  // The hint stands in for a default: it tells the operator what the dead button needs.
  await screen.findByText(EN['remote.stationNameHint'])
  const name = screen.getByLabelText(EN['remote.stationName']) as HTMLInputElement
  expect(name.value).toBe('')
  expect(signIn.disabled).toBe(true)
})
