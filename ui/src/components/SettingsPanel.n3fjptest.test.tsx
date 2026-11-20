// @vitest-environment jsdom
//
// Settings ▸ Services ▸ N3FJP Integration — the connection Test's failures. The backend answers
// the failures it can explain with a code (`n3fjp_test_connection` in src-tauri, from
// tempo-net's `ConnectFailure`), and the panel words each one with what to do next; anything
// else is shown as the backend said it.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor, fireEvent } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import { EN } from '../i18n/en'
import defaultSettings from './__fixtures__/defaultSettings.json'

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Every export of `../api`, read from the real module, so a new verb cannot throw on mount.
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
    <SettingsPanel
      target="n3fjp"
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
    Promise.resolve({ ...defaultSettings, n3fjpHost: '192.168.1.20', n3fjpPort: 1100 } as never),
  )
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

/** Press Test N3FJP and read the result line once it has failed. */
async function failedTestSays(): Promise<string> {
  const button = await waitFor(() => {
    const b = [...document.querySelectorAll<HTMLButtonElement>('#settings-n3fjp button')].find(
      (el) => el.textContent === EN['settings.n3fjp.test.action'],
    )
    if (!b || b.disabled) throw new Error('no enabled Test N3FJP button yet')
    return b
  })
  fireEvent.click(button)
  return await waitFor(() => {
    const text = document.querySelector('#settings-n3fjp .cat-result.fail')?.textContent
    if (!text) throw new Error('no failed result yet')
    return text
  })
}

describe('the N3FJP connection Test', () => {
  it.each([
    ['n3fjpRefused', 'settings.n3fjp.test.refused'],
    ['n3fjpUnreachable', 'settings.n3fjp.test.unreachable'],
    ['n3fjpHostNotFound', 'settings.n3fjp.test.hostNotFound'],
    ['n3fjpNoHost', 'settings.n3fjp.test.noHost'],
  ] as const)('words %s with what to do next', async (code, key) => {
    api.get('n3fjpTestConnection').mockImplementation(() => Promise.reject(code))
    renderPanel()
    expect(await failedTestSays()).toBe(`✗ ${EN[key]}`)
  })

  it("shows any other failure in the backend's own words", async () => {
    api.get('n3fjpTestConnection').mockImplementation(() => Promise.reject('send failed: broken pipe'))
    renderPanel()
    expect(await failedTestSays()).toBe('✗ send failed: broken pipe')
  })

  it('tests the host and port in the form, unsaved, and saves nothing', async () => {
    api.get('n3fjpTestConnection').mockImplementation(() => Promise.resolve("N3FJP's Test Log v6.6"))
    renderPanel()
    const host = await waitFor(() => {
      const el = document.querySelector<HTMLInputElement>('#settings-n3fjp input[value="192.168.1.20"]')
      if (!el) throw new Error('no host box yet')
      return el
    })
    const port = document.querySelector<HTMLInputElement>('#settings-n3fjp input[value="1100"]')!
    fireEvent.change(host, { target: { value: ' 192.168.1.30 ' } })
    fireEvent.change(port, { target: { value: '1234' } })
    const button = [...document.querySelectorAll<HTMLButtonElement>('#settings-n3fjp button')].find(
      (el) => el.textContent === EN['settings.n3fjp.test.action'],
    )!
    fireEvent.click(button)
    await waitFor(() => expect(document.querySelector('#settings-n3fjp .cat-result.ok')).not.toBeNull())
    expect(api.get('n3fjpTestConnection')).toHaveBeenCalledWith('192.168.1.30', 1234)
    expect(api.get('setSettings')).not.toHaveBeenCalled()
  })

  it('the English names the right place and the next step', () => {
    expect(EN['settings.n3fjp.test.noHost']).toContain('Settings ▸ Services')
    expect(EN['settings.n3fjp.test.refused']).toContain('Application Program Interface ▸ TCP API')
    expect(EN['settings.n3fjp.test.unreachable']).toContain('Windows Firewall')
  })
})
