// @vitest-environment jsdom
//
// A login is saved at the station's own screen only, never through a stream of it. The stream's
// press is marked only while its event is being handled (`isStreamInput`), and the QRZ Logbook,
// ClubLog and eQSL saves first ask how many contacts were never sent there: the save itself goes
// out after that answer comes back, when the press is long over and no mark is left to refuse it
// by. So the question is refused through a stream too, and a refused question stops the save.
// At the station the save goes out as before.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'
import { ConfirmHost } from '../confirm'
import { t } from '../i18n'

vi.setConfig({ testTimeout: 15_000 })

const stream = vi.hoisted(() => ({ on: false }))
vi.mock('../remote-native/stream-input', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  isStreamInput: () => stream.on,
}))

/** The login saves and their question run as shipped, through the real bridge below; everything
 *  else the panel asks is a stand-in. */
const REAL = vi.hoisted(
  () => new Set(['uploadBacklog', 'setQrzLogbookKey', 'setClublogPassword', 'setEqslPassword']),
)
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
    mod[name] = typeof actual[name] === 'function' && !REAL.has(name) ? api.get(name) : actual[name]
  }
  return mod
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (fn: () => Promise<unknown>) => {
    try {
      return await fn()
    } catch {
      return null
    }
  }),
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

/** What reached the station's bridge, by command. */
let sent: string[] = []
beforeEach(() => {
  sent = []
  ;(window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
    // A login is already stored, so the question asks nothing and the save follows at once.
    invoke: async (cmd: string) => {
      sent.push(cmd)
      return null
    },
  }
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
    Promise.resolve({ ...defaultSettings, mycall: 'W1AW', mygrid: 'FN31' } as never),
  )
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(() => {
  cleanup()
  stream.on = false
  for (const spy of Object.values(api.spies)) spy.mockReset()
  delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
})

const LOGINS = [
  ['set_qrz_logbook_key', 'settings.confirmations.qrz.apiKey.placeholder'],
  ['set_clublog_password', 'settings.confirmations.clublog.password.placeholder'],
  ['set_eqsl_password', 'settings.confirmations.eqsl.password.placeholder'],
] as const

/** Type a login into its field and press the Save beside it; `throughStream` marks the press as
 *  the stream's for exactly as long as its event is handled, as the stream's dispatcher does. */
async function save(placeholder: string, throughStream: boolean) {
  const field = (await screen.findByPlaceholderText(t(placeholder as never))) as HTMLInputElement
  fireEvent.change(field, { target: { value: 'FAKE-LOGIN' } })
  const button = field.parentElement!.querySelector('button') as HTMLButtonElement
  stream.on = throughStream
  fireEvent.click(button)
  stream.on = false
}

describe('a login whose save asks a question first is saved at the station only', () => {
  it.each(LOGINS)('%s through a stream is never sent', async (cmd, placeholder) => {
    renderPanel()
    await save(placeholder, true)
    // Long enough for the question's answer and a save behind it to have gone out.
    await new Promise((r) => setTimeout(r, 50))
    expect(sent).not.toContain(cmd)
    expect(sent).not.toContain('upload_backlog')
  })

  it.each(LOGINS)('%s at the station is sent', async (cmd, placeholder) => {
    renderPanel()
    await save(placeholder, false)
    await waitFor(() => expect(sent).toContain(cmd))
    expect(sent).toContain('upload_backlog')
  })
})
