// @vitest-environment jsdom
//
// The APRS-IS / iGate settings first shipped under Logging & Connectors ▸ Integrations & Feeds,
// alongside the other network feeds. That is where they belong by TYPE — and it is not where
// anybody looked for them. The first operator to go hunting went to the APRS settings, did not
// find them, and reported them missing (2026-07-29 ruling).
//
// They now live in Modes ▸ APRS, beside RTTY / CW / Phone, because a setting nobody can find is a
// setting that does not exist. These tests pin the placement so a later tidy-up cannot quietly
// file them back under the heading that reads better but does not get found.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Mock EVERY export of `../api`, derived from the real module rather than a hand-kept list.
//
// The list was the problem: a verb missing from it made the panel THROW ON MOUNT ("No export is
// defined on the mock"), which presents as a behaviour regression in whichever test happened to
// run -- not as the out-of-date mock it actually is. Reading the real module's export names makes
// that failure impossible by construction.
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
  for (const spy of Object.values(api.spies)) {
    spy.mockClear()
    spy.mockImplementation(() => Promise.resolve(null))
  }
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  // The Logging tab renders these two as lists; the default `null` throws there but nowhere else.
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([]))
  api.get('getConnectionLog').mockImplementation(() => Promise.resolve([]))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('0.21.2'))
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52' } as never),
  )
})
afterEach(cleanup)

const openTab = async (name: string) =>
  fireEvent.click(await screen.findByRole('tab', { name }))

/** The fieldset a control sits in, by its <legend>. */
const sectionOf = (el: HTMLElement) =>
  el.closest('fieldset')?.querySelector('legend')?.textContent ?? null

describe('APRS-IS settings live where operators look for them', () => {
  it('every one of the nine controls is on the Digital tab under APRS', async () => {
    // The whole group moves together — a half-move that stranded the iGate toggle behind the old
    // heading would be worse than not moving at all.
    renderPanel()
    await openTab('Digital')
    for (const label of [
      'APRS-IS feed',
      'Server',
      'Port',
      'Radius (km)',
      'Watched calls',
      'Weather stations',
      'Objects & items',
      'Messages',
      'Receive-only iGate',
    ]) {
      const el = await screen.findByText(label)
      expect(sectionOf(el), `"${label}" must sit under the APRS legend`).toBe('APRS')
    }
  })

  it('they are NOT under Logging & Connectors any more', async () => {
    renderPanel()
    await openTab('Logging & Connectors')
    // The old home. Finding the iGate toggle here again means the move was reverted.
    expect(screen.queryByText('Receive-only iGate')).toBeNull()
    expect(screen.queryByText('APRS-IS feed')).toBeNull()
  })

  it('the APRS section sits with the other per-mode settings, not off on its own', async () => {
    // Discoverability was the entire complaint: an operator scanning for APRS should meet it in
    // the same list as the modes they already know.
    //
    // ⚠️ AMENDED 2026-08-12, WITH THE OPERATOR'S SIGN-OFF, AND NOT AS COLLATERAL FROM A RENAME.
    // The original assertion named CW alongside RTTY, because at the time every mode shared one
    // `Modes` page. That page has been split into the three tabs the nav rail itself presents
    // (Phone · CW · Digital), so CW now has its own tab and can no longer co-render here.
    //
    // The 2026-07-29 ruling this test exists to protect is UNCHANGED and still enforced: APRS
    // keeps its own `<legend>APRS</legend>` beside other modes an operator already knows, rather
    // than being filed by TYPE under Logging & Connectors where the first operator to go looking
    // could not find it. A setting nobody can find is a setting that does not exist. What the
    // ruling forbids is re-filing APRS under a heading that reads better — moving CW to its own
    // tab does not do that, and the test below still proves APRS has mode neighbours.
    renderPanel()
    await openTab('Digital')
    const legends = [...document.querySelectorAll('fieldset legend')].map((l) => l.textContent)
    expect(legends).toContain('APRS')
    expect(legends).toContain('RTTY')
    // Still a list of modes, not APRS alone on a page of its own.
    expect(legends.filter((l) => l && /RTTY|APRS|Q65|MSK144|JT65|FST4|WSPR|Digital/.test(l)).length)
      .toBeGreaterThan(2)
  })

  it('the enable-gate survived the move — dependent controls stay disabled until the feed is on', async () => {
    // `disabled={!form.aprsIsEnabled}` on eight controls is easy to drop when re-indenting a
    // 180-line block. The default is feed-off, so every dependent control must start disabled.
    renderPanel()
    await openTab('Digital')
    const server = (await screen.findByText('Server')).closest('label')?.querySelector('input')
    expect(server).toBeTruthy()
    expect(server!.disabled).toBe(true)

    const igate = (await screen.findByText('Receive-only iGate'))
      .closest('label')
      ?.querySelector('button')
    expect(igate).toBeTruthy()
    expect(igate!.disabled).toBe(true)

    // ...and the master switch itself is never gated, or nothing could ever be turned on.
    const master = (await screen.findByText('APRS-IS feed'))
      .closest('label')
      ?.querySelector('button')
    expect(master!.disabled).toBe(false)
    expect(master!.getAttribute('aria-checked')).toBe('false')
  })
})

describe('the RF side — "Over the air"', () => {
  it('all five controls sit under the APRS legend', async () => {
    renderPanel()
    await openTab('Digital')
    for (const label of [
      'Channel (RF)',
      'Beacon symbol',
      'Beacon comment',
      'Digipeater path',
      'Beacon SSID',
    ]) {
      const el = await screen.findByText(label)
      expect(sectionOf(el), `"${label}" must sit under the APRS legend`).toBe('APRS')
    }
  })

  it('⭐ NONE of them is gated on the internet feed', async () => {
    // The neighbouring block's `disabled={!form.aprsIsEnabled}` is one careless re-indent away
    // from these five, and it would be backwards: RF APRS works with the feed off, which is how
    // most stations run it. The fixture has the feed OFF, so a copied gate shows up immediately.
    renderPanel()
    await openTab('Digital')
    for (const label of ['Channel (RF)', 'Beacon symbol', 'Beacon SSID']) {
      const el = (await screen.findByText(label)).closest('label')!.querySelector('select')
      expect(el!.disabled, `"${label}" must not be behind the internet feed`).toBe(false)
    }
    for (const label of ['Beacon comment', 'Digipeater path']) {
      const el = (await screen.findByText(label)).closest('label')!.querySelector('input')
      expect(el!.disabled, `"${label}" must not be behind the internet feed`).toBe(false)
    }
    // The positive control: the gate DOES still bite next door, so "not disabled" above means
    // something. Without this, a build where nothing is ever disabled passes the whole block.
    const server = (await screen.findByText('Server')).closest('label')?.querySelector('input')
    expect(server!.disabled, 'the APRS-IS gate must still be in force').toBe(true)
  })

  it('the channel and SSID both default to "follow me", and name what they resolved to', async () => {
    // Neither is "unset": null means follow my grid / follow my callsign, and the channel option
    // NAMES the number it derived — which is the whole mitigation for approximate boundaries.
    renderPanel()
    await openTab('Digital')
    const channel = (await screen.findByText('Channel (RF)'))
      .closest('label')!
      .querySelector('select') as HTMLSelectElement
    expect(channel.value).toBe('')
    // The fixture's grid is EN52 (Wisconsin) → 144.390.
    expect(channel.options[0].textContent).toContain('144.390')
    expect(channel.options[0].textContent).toContain('from your grid')

    const ssid = (await screen.findByText('Beacon SSID'))
      .closest('label')!
      .querySelector('select') as HTMLSelectElement
    expect(ssid.value).toBe('')
    expect(ssid.options[0].textContent).toBe('From my callsign')
  })

  it('the digipeater path shows the stored hops and can be emptied to mean "direct"', async () => {
    renderPanel()
    await openTab('Digital')
    const path = (await screen.findByText('Digipeater path'))
      .closest('label')!
      .querySelector('input') as HTMLInputElement
    expect(path.value).toBe('WIDE1-1, WIDE2-1')
    fireEvent.change(path, { target: { value: '' } })
    expect(path.value).toBe('')
  })

  it('the symbol picker offers the alternate-table identities, not just the primary eight', async () => {
    // Digipeater `\#` and iGate `\&` are the two a fixed station running as infrastructure
    // wants, and they are the only reason the symbol TABLE is a stored field at all.
    renderPanel()
    await openTab('Digital')
    const sym = (await screen.findByText('Beacon symbol'))
      .closest('label')!
      .querySelector('select') as HTMLSelectElement
    const values = [...sym.options].map((o) => o.value)
    expect(values).toContain('/>')
    expect(values).toContain('\\#')
    expect(values).toContain('\\&')
    expect(sym.value).toBe('/>') // Car, on the primary table — the shipped default
  })
})
