// @vitest-environment jsdom
//
// Saving the FIRST login for QRZ Logbook, ClubLog or eQSL turns that service's upload on, and its
// catch-up sends every contact with no record of being sent there — an imported log included,
// because an import marks nothing as sent. An operator who imported 10,000 contacts from another
// program and then saved a QRZ key started re-sending the whole old log. The first login now asks
// "Send N contacts not yet sent to <service>?", and the default — the focused button, Escape, a
// dialog that cannot open — is "Only contacts made from now on". A login saved over a stored one
// asks nothing and sends no answer, so a station that had its logins before sees no change.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'
import { ConfirmHost } from '../confirm'
import { t } from '../i18n'

vi.setConfig({ testTimeout: 15_000 })

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const mod: Record<string, unknown> = {}
  for (const name of Object.keys(actual)) {
    mod[name] = typeof actual[name] === 'function' ? api.get(name) : actual[name]
  }
  return mod
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (fn: () => Promise<unknown>) => fn()),
}))

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

function renderPanel() {
  return render(
    <>
      <ConfirmHost />
      <SettingsPanel
        target="confirmations"
        activeRadioId={0}
        scale={1 as never}
        scaleMode={'auto' as never}
        scaleCap={1 as never}
        onScaleModeChange={() => {}}
        onScaleCapChange={() => {}}
        density={'comfortable' as never}
        onDensityChange={() => {}}
        onResetLayout={() => {}}
        features={features}
      />
    </>,
  )
}

beforeEach(() => {
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getConnectionLog').mockImplementation(() => Promise.resolve([]))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([]))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.2.6'))
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52' } as never),
  )
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(() => {
  cleanup()
  for (const spy of Object.values(api.spies)) spy.mockReset()
})

/** The three logins, by the placeholder of their field, the setter they save through and the
 *  service name and wire id the question uses. */
const LOGINS = [
  ['qrz', 'QRZ Logbook', 'settings.confirmations.qrz.apiKey.placeholder', 'setQrzLogbookKey'],
  ['clublog', 'ClubLog', 'settings.confirmations.clublog.password.placeholder', 'setClublogPassword'],
  ['eqsl', 'eQSL', 'settings.confirmations.eqsl.password.placeholder', 'setEqslPassword'],
] as const

/** Type a login into its field and press the Save beside it. */
async function save(placeholder: string) {
  const field = (await screen.findByPlaceholderText(t(placeholder as never))) as HTMLInputElement
  fireEvent.change(field, { target: { value: 'FAKE-LOGIN' } })
  const button = field.parentElement!.querySelector('button') as HTMLButtonElement
  fireEvent.click(button)
}

const question = (count: number, service: string) =>
  t('settings.connections.backlog.title', { count, service })

describe('the first login saved for an upload service asks about the contacts never sent there', () => {
  it.each(LOGINS)(
    '%s: the default answer saves with "only from now on" (sendOld false)',
    async (wire, name, placeholder, setter) => {
      api.get('uploadBacklog').mockImplementation(() => Promise.resolve(10_000))
      renderPanel()
      await save(placeholder)
      expect(await screen.findByText(question(10_000, name))).toBeTruthy()
      expect(api.get('uploadBacklog')).toHaveBeenCalledWith(wire)
      expect(api.get(setter)).not.toHaveBeenCalled()
      // The default button has the focus, and is "Only contacts made from now on".
      const fromNow = screen.getByRole('button', { name: t('settings.connections.backlog.fromNow') })
      expect(document.activeElement).toBe(fromNow)
      fireEvent.click(fromNow)
      await waitFor(() => expect(api.get(setter)).toHaveBeenCalledTimes(1))
      expect(api.get(setter).mock.calls[0]).toEqual(['FAKE-LOGIN', false])
    },
  )

  it.each(LOGINS)('%s: "Send them" saves with sendOld true', async (_wire, name, placeholder, setter) => {
    api.get('uploadBacklog').mockImplementation(() => Promise.resolve(3))
    renderPanel()
    await save(placeholder)
    await screen.findByText(question(3, name))
    fireEvent.click(screen.getByRole('button', { name: t('settings.connections.backlog.send') }))
    await waitFor(() => expect(api.get(setter)).toHaveBeenCalledTimes(1))
    expect(api.get(setter).mock.calls[0]).toEqual(['FAKE-LOGIN', true])
  })

  it('Escape is the default answer too', async () => {
    api.get('uploadBacklog').mockImplementation(() => Promise.resolve(5))
    renderPanel()
    await save('settings.confirmations.qrz.apiKey.placeholder')
    const title = await screen.findByText(question(5, 'QRZ Logbook'))
    fireEvent.keyDown(title, { key: 'Escape' })
    await waitFor(() => expect(api.get('setQrzLogbookKey')).toHaveBeenCalledTimes(1))
    expect(api.get('setQrzLogbookKey').mock.calls[0]).toEqual(['FAKE-LOGIN', false])
  })

  it.each(LOGINS)(
    '%s: a login saved over a stored one asks nothing and sends no answer',
    async (_wire, name, placeholder, setter) => {
      api.get('uploadBacklog').mockImplementation(() => Promise.resolve(null))
      renderPanel()
      await save(placeholder)
      await waitFor(() => expect(api.get(setter)).toHaveBeenCalledTimes(1))
      expect(api.get(setter).mock.calls[0]).toEqual(['FAKE-LOGIN', undefined])
      expect(screen.queryByText(question(0, name), { exact: false })).toBeNull()
      expect(document.querySelector('.confirm-actions')).toBeNull()
    },
  )

  // Operator ruling "Set it silently": with nothing waiting nothing is asked, and the answer is
  // "from now on", so a log imported afterwards is not sent by a later save of the login.
  it('with nothing waiting nothing is asked, and uploads start from now', async () => {
    api.get('uploadBacklog').mockImplementation(() => Promise.resolve(0))
    renderPanel()
    await save('settings.confirmations.clublog.password.placeholder')
    await waitFor(() => expect(api.get('setClublogPassword')).toHaveBeenCalledTimes(1))
    expect(api.get('setClublogPassword').mock.calls[0]).toEqual(['FAKE-LOGIN', false])
    expect(document.querySelector('.confirm-actions')).toBeNull()
  })

  it('a count that cannot be read is no answer, which the station takes as the default', async () => {
    api.get('uploadBacklog').mockImplementation(() => Promise.reject(new Error('refused')))
    renderPanel()
    await save('settings.confirmations.eqsl.password.placeholder')
    await waitFor(() => expect(api.get('setEqslPassword')).toHaveBeenCalledTimes(1))
    expect(api.get('setEqslPassword').mock.calls[0]).toEqual(['FAKE-LOGIN', undefined])
  })
})
