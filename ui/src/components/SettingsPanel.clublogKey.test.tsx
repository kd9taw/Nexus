// @vitest-environment jsdom
//
// Settings ▸ Confirmations ▸ ClubLog API key. The station's screen may be streamed, and screens get
// shared: the key shows as •••• until Reveal, at the station's own screen only. Through a stream it
// can be neither revealed nor changed. Read from a computer driving the station it is not sent at
// all: the field says whether the station has one saved, and takes nothing.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, act, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import { pushToast } from '../toast'
import { t } from '../i18n'
import defaultSettings from './__fixtures__/defaultSettings.json'
import { STREAM_WATCHED_EVENT } from '../remote-native/stream-watched'

vi.setConfig({ testTimeout: 15_000 })

const KEY = 'FAKE-OVERRIDE-KEY-0001'

/** Whether the event being handled came through a stream, as `isStreamInput` answers it. */
const stream = vi.hoisted(() => ({ on: false }))
vi.mock('../remote-native/stream-input', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  isStreamInput: () => stream.on,
}))

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Every export of `../api`, derived from the real module (see SettingsPanel.lotwupload.test.tsx
// for why a hand-kept list is the trap).
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

beforeEach(() => {
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([]))
  api.get('getConnectionLog').mockImplementation(() => Promise.resolve([]))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.2.6'))
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  stream.on = false
  watching.on = false
  delete (window as { __TAURI__?: unknown }).__TAURI__
})

/** Whether a stream watches the station's screen, as the station answers and tells it. */
const watching = { on: false, tell: undefined as undefined | ((e: { payload: boolean }) => void) }
function station(settings: Record<string, unknown>) {
  ;(window as { __TAURI__?: unknown }).__TAURI__ = {
    event: {
      listen: (name: string, handler: (e: { payload: boolean }) => void) => {
        if (name === STREAM_WATCHED_EVENT) watching.tell = handler
        return Promise.resolve(() => {})
      },
    },
  }
  api.get('getStreamWatched').mockImplementation(() => Promise.resolve(watching.on))
  // The station's read: while a stream watches, the key blank and whether one is saved.
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({
      ...defaultSettings, mycall: 'KD9TAW', ...settings,
      ...(watching.on ? {
        clublogApiKey: '', secretsSavedAtStation: { clublogApiKey: true, cloudlogKey: false },
        stationOnlySettings: ['cloudlogUrl', 'fdJoinAddr', 'icomLanHost', 'icomLanPort'],
      } : {}),
    } as never),
  )
}
async function tell(on: boolean) {
  watching.on = on
  await act(async () => { watching.tell?.({ payload: on }) })
}


async function open(settings: Record<string, unknown>, watched?: boolean, target = 'confirmations') {
  if (watched === undefined) {
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW', ...settings } as never),
    )
  } else {
    watching.on = watched
    station(settings)
  }
  render(
    <SettingsPanel
      target={target}
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
    />,
  )
  if (target !== 'confirmations') return { input: null as never, reveal: () => null }
  const label = await screen.findByText(t('settings.confirmations.clublog.apiKey.label'))
  const field = label.closest('label') as HTMLLabelElement
  const input = field.querySelector('input') as HTMLInputElement
  const reveal = () =>
    field.querySelector('button') as HTMLButtonElement | null
  return { input, reveal }
}

const refused = () => vi.mocked(pushToast).mock.calls.filter((c) => c[0] === t('remote.lan.atShackOnly')).length

describe('the ClubLog key at the station', () => {
  it('shows as •••• until Reveal, and Hide masks it again', async () => {
    const { input, reveal } = await open({ clublogApiKey: KEY })
    expect([input.type, input.value]).toEqual(['password', KEY])
    expect(reveal()?.textContent).toBe(t('settings.confirmations.clublog.apiKey.reveal'))
    fireEvent.click(reveal()!)
    expect(input.type).toBe('text')
    expect(reveal()?.textContent).toBe(t('settings.confirmations.clublog.apiKey.hide'))
    fireEvent.click(reveal()!)
    expect(input.type).toBe('password')
  })

  it('is neither revealed nor changed through a stream, and is changed at the screen', async () => {
    const { input, reveal } = await open({ clublogApiKey: KEY })
    stream.on = true
    fireEvent.click(reveal()!)
    expect(input.type).toBe('password')
    fireEvent.change(input, { target: { value: 'FAKE-STREAMED-KEY-0002' } })
    expect(input.value).toBe(KEY)
    expect(refused()).toBe(2)
    stream.on = false
    fireEvent.change(input, { target: { value: 'FAKE-TYPED-KEY-0003' } })
    expect(input.value).toBe('FAKE-TYPED-KEY-0003')
    expect(refused()).toBe(2)
  })

  it('masks again at the first press or key that comes through a stream', async () => {
    const { input, reveal } = await open({ clublogApiKey: KEY })
    fireEvent.click(reveal()!)
    fireEvent.pointerDown(document.body)
    expect(input.type, 'CONTROL: a press at the screen').toBe('text')
    stream.on = true
    fireEvent.pointerDown(document.body)
    expect(input.type).toBe('password')
    stream.on = false
    fireEvent.click(reveal()!)
    stream.on = true
    fireEvent.keyDown(document.body, { key: 'a' })
    expect(input.type).toBe('password')
  })
})

describe('the ClubLog key read from afar', () => {
  it('says the station has one saved, and takes nothing', async () => {
    const { input, reveal } = await open({ clublogApiKey: '', secretsSavedAtStation: { clublogApiKey: true } })
    expect(input.disabled).toBe(true)
    expect(input.value).toBe('')
    expect(input.placeholder).toBe(t('settings.confirmations.clublog.apiKey.savedAtStation'))
    expect(reveal()).toBeNull()
  })

  it('says the station has none saved', async () => {
    const { input } = await open({ clublogApiKey: '', secretsSavedAtStation: { clublogApiKey: false } })
    expect(input.disabled).toBe(true)
    expect(input.placeholder).toBe(t('settings.confirmations.clublog.apiKey.notSavedAtStation'))
  })
})

describe('the ClubLog key while a stream watches the station', () => {
  const reason = () => t('settings.confirmations.clublog.apiKey.watched')

  it('hides a revealed key at once, holds none, and disables Reveal with the reason', async () => {
    const { input, reveal } = await open({ clublogApiKey: KEY }, false)
    fireEvent.click(reveal()!)
    expect([input.type, input.value]).toEqual(['text', KEY])
    // The station's read of the keys answers later: the screen hides the key before it does.
    let answer: (s: unknown) => void = () => {}
    api.get('getSettings').mockImplementationOnce(() => new Promise((r) => { answer = r }))
    await tell(true)
    expect(input.type).toBe('password')
    expect(input.value).toBe('')
    expect(document.body.innerHTML).not.toContain(KEY)
    expect(input.disabled).toBe(true)
    expect(input.placeholder).toBe(reason())
    expect(reveal()?.disabled).toBe(true)
    expect(reveal()?.title).toBe(reason())
    fireEvent.click(reveal()!)
    expect(input.type).toBe('password')
    expect(api.get('getSettings')).toHaveBeenCalledTimes(2)
    await act(async () => answer({ ...defaultSettings, mycall: 'KD9TAW', clublogApiKey: '', secretsSavedAtStation: { clublogApiKey: true } }))
    expect(input.value).toBe('')
    expect(document.body.innerHTML).not.toContain(KEY)
  })

  it('offers Reveal again once the stream has ended, with the key read back and still masked', async () => {
    const { input, reveal } = await open({ clublogApiKey: KEY }, false)
    fireEvent.click(reveal()!)
    await tell(true)
    await waitFor(() => expect(api.get('getSettings')).toHaveBeenCalledTimes(2))
    await tell(false)
    await waitFor(() => expect(input.value).toBe(KEY))
    expect(input.type, 'revealed before the stream, masked after it').toBe('password')
    expect(input.disabled).toBe(false)
    expect(reveal()?.disabled).toBe(false)
    fireEvent.click(reveal()!)
    expect(input.type).toBe('text')
  })

  it('opened while a stream already watches, shows no key and no Reveal to press', async () => {
    const { input, reveal } = await open({ clublogApiKey: KEY }, true)
    await waitFor(() => expect(reveal()?.disabled).toBe(true))
    expect(input.value).toBe('')
    expect(input.placeholder).toBe(reason())
    expect(document.body.innerHTML).not.toContain(KEY)
  })

  it('locks the Cloudlog address while watched, and frees it once the stream has ended', async () => {
    await open({ clublogApiKey: KEY, cloudlogUrl: 'https://log.shack.example' }, false)
    const url = () => document.querySelector('[data-setting="cloudlogUrl"]') as HTMLInputElement
    expect(url().disabled, 'CONTROL: at the station, unwatched').toBe(false)
    await tell(true)
    await waitFor(() => expect(url().disabled).toBe(true))
    expect(document.querySelector('[data-set-at-station="cloudlogUrl"]')).not.toBeNull()
    await tell(false)
    await waitFor(() => expect(url().disabled).toBe(false))
  })

  it('a save shows the station\'s Cloudlog address when the stream locked it after it was typed', async () => {
    await open({ clublogApiKey: KEY, cloudlogUrl: 'https://log.shack.example' }, false)
    const url = () => document.querySelector('[data-setting="cloudlogUrl"]') as HTMLInputElement
    fireEvent.change(url(), { target: { value: 'https://collector.example' } })
    await tell(true)
    await waitFor(() => expect(url().disabled).toBe(true))
    expect(url().value, 'typed before the lock').toBe('https://collector.example')
    const save = (await screen.findAllByRole('button', { name: t('settings.panel.save') })).find(
      (b) => (b as HTMLButtonElement).type === 'submit',
    ) as HTMLButtonElement
    fireEvent.click(save)
    // The station kept its own; the form shows it after the save, and says so.
    await waitFor(() => expect(url().value).toBe('https://log.shack.example'))
    expect(vi.mocked(pushToast)).toHaveBeenCalledWith(t('settings.stationOnly.kept'), 'info')
  })

  it('CONTROL: unwatched, a typed Cloudlog address stays as typed after a save', async () => {
    await open({ clublogApiKey: KEY, cloudlogUrl: 'https://log.shack.example' }, false)
    const url = () => document.querySelector('[data-setting="cloudlogUrl"]') as HTMLInputElement
    fireEvent.change(url(), { target: { value: 'https://collector.example' } })
    const save = (await screen.findAllByRole('button', { name: t('settings.panel.save') })).find(
      (b) => (b as HTMLButtonElement).type === 'submit',
    ) as HTMLButtonElement
    fireEvent.click(save)
    await waitFor(() => expect(api.get('getSettings').mock.calls.length).toBeGreaterThan(1))
    expect(url().value).toBe('https://collector.example')
    expect(vi.mocked(pushToast)).not.toHaveBeenCalledWith(t('settings.stationOnly.kept'), 'info')
  })

  it('hides the Connections log while watched, and shows it again once the stream has ended', async () => {
    const line = { tsUnix: 1_700_000_000, connector: 'Cloudlog', level: 'info', message: 'FAKE-LINE-0001' }
    api.get('getConnectionLog').mockImplementation(() =>
      watching.on ? Promise.reject(new Error('refused')) : Promise.resolve([line]),
    )
    await open({}, false, 'services')
    await waitFor(() => expect(document.body.innerHTML).toContain('FAKE-LINE-0001'))
    await tell(true)
    await waitFor(() => expect(document.body.innerHTML).not.toContain('FAKE-LINE-0001'))
    expect(screen.getByText(t('settings.connections.log.watched'))).toBeTruthy()
  })

  it('CONTROL: unwatched, the station screen reveals as before', async () => {
    const { input, reveal } = await open({ clublogApiKey: KEY }, false)
    await waitFor(() => expect(api.get('getStreamWatched')).toHaveBeenCalled())
    expect(reveal()?.disabled).toBe(false)
    fireEvent.click(reveal()!)
    expect([input.type, input.value]).toEqual(['text', KEY])
  })
})
