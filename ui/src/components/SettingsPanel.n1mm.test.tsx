// @vitest-environment jsdom
//
// The N1MM contact broadcast only spoke during a Field Day event, which made it useless for what a
// tester actually wanted it for: a map app listening for N1MM contact datagrams and plotting
// every QSO on its map as it is logged (2026-07-29 request). "Broadcast every QSO" is that standing
// switch. These tests pin the two things that decide whether an operator can find and use it: it
// lives in the N1MM+ section on the tab where they went looking, and it starts OFF.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'

// THE BUDGET (2026-10-09). The slowest case here, "the switch sits with the N1MM address, on the tab the…", takes
// 0.21 s and 0.27 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

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
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([]))
  api.get('getConnectionLog').mockImplementation(() => Promise.resolve([]))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('0.21.3'))
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52' } as never),
  )
})
afterEach(cleanup)

const openLogging = async () =>
  fireEvent.click(await screen.findByRole('tab', { name: 'Services' }))

/** The fieldset a control sits in, by its <legend>. */
const sectionOf = (el: HTMLElement) =>
  el.closest('fieldset')?.querySelector('legend')?.textContent ?? null

const switchFor = async (label: string) =>
  (await screen.findByText(label)).closest('label')?.querySelector('button') as HTMLButtonElement

const addressField = async () =>
  (await screen.findByText('N1MM contact broadcast address'))
    .closest('label')
    ?.querySelector('input') as HTMLInputElement

describe('the N1MM broadcast can be switched on independently of Field Day', () => {
  it('the switch sits with the N1MM address, on the tab the tester went to', async () => {
    // Filed anywhere else it is a feature nobody finds — the whole point of the request.
    renderPanel()
    await openLogging()
    const el = await screen.findByText('Broadcast every QSO')
    expect(sectionOf(el)).toBe('N1MM+ Integration')
  })

  it('starts OFF — an upgrade never begins putting an operator on the network', async () => {
    renderPanel()
    await openLogging()
    expect((await switchFor('Broadcast every QSO')).getAttribute('aria-checked')).toBe('false')
    expect((await addressField()).value).toBe('')
  })

  it('turning it on with a blank address fills in the local target', async () => {
    // Otherwise the switch reads as broken: on, and silently sending nowhere.
    renderPanel()
    await openLogging()
    fireEvent.click(await switchFor('Broadcast every QSO'))
    expect((await switchFor('Broadcast every QSO')).getAttribute('aria-checked')).toBe('true')
    expect((await addressField()).value).toBe('127.0.0.1:12060')
  })

  it('says plainly that an address alone sends only the contacts logged in Contest mode', async () => {
    // The reported bug was a SILENT failure: the address sat there looking like a standing
    // integration beside HRD, which worked, and emitted nothing. A configured-but-silent
    // integration has to say so on the screen where it was configured.
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve({
        ...defaultSettings,
        mycall: 'KD9TAW',
        n1mmAddr: '127.0.0.1:12061',
        n1mmUpload: false,
      } as never),
    )
    renderPanel()
    await openLogging()
    const hint = (await screen.findByText('N1MM contact broadcast address'))
      .closest('label')
      ?.querySelector('.settings-hint')
    // Every contest's contacts go out (the service loop sends them for any running contest
    // session); it said "nothing outside a Field Day event".
    expect(hint?.textContent).toContain(
      'An address alone sends only the contacts you log in Contest mode — turn on Broadcast every QSO below for everyday logging.',
    )

    // ...and once it IS a standing output, it says that instead.
    fireEvent.click(await switchFor('Broadcast every QSO'))
    expect(hint?.textContent).toMatch(/Sending for every logged QSO/i)
  })

  // The service loop pushes every NEW contest contact to N3FJP and to the N1MM address while
  // any contest session runs, not only Field Day's; what it fills in at N3FJP is the Field Day
  // Contest Log's form (call, band, mode, class, section), plus the exchange of the three other
  // programs whose boxes N3FJP documents (CQ WPX, CQ WW RTTY, VHF). The groups said "Each FD
  // contact" and "packets go out only while a Field Day event is running".
  it('the N3FJP and N1MM groups say what goes out in every contest', async () => {
    renderPanel()
    await openLogging()
    await screen.findByText('N1MM contact broadcast address')
    expect(document.querySelector('#settings-n3fjp .settings-note')!.textContent).toBe(
      "Each contact you log in Contest mode reaches the club's master N3FJP log the moment you log it. Nexus fills in the form of the N3FJP Field Day Contest Log (call, band, mode, class and section), so at Field Day the whole club's score updates in real time. CQ WPX, CQ WW RTTY and the ARRL VHF contests carry their exchange too; a QSO party, Sweepstakes or CQ WW CW/SSB contact reaches N3FJP without its exchange. Run N3FJP on the master computer; point Nexus at its IP + port (default 1100).",
    )
    const hintBeside = async (label: string) =>
      (await screen.findByText(label)).closest('.settings-field')?.querySelector('.settings-hint')?.textContent
    expect(await hintBeside('Use ENTER for contest scoring')).toBe(
      "Log each contest contact with N3FJP's ENTER sequence, which scores the contest — the correct path. Turn off to fall back to a plain ADDDIRECT insert (may not score). On by default.",
    )
    expect(await hintBeside('Forward every QSO')).toBe(
      "Also push every logged QSO (not just contest contacts) to N3FJP ACLog on the host above — everyday general logging. N3FJP dedupes, so it's safe to run alongside the contest push.",
    )
    expect(await hintBeside('Broadcast every QSO')).toBe(
      'Send the contact packet for every logged QSO, not just contest contacts — point a map app such as GridTracker at the address above and each contact plots on its map as you log it. One packet per QSO: this never doubles up with the contest broadcast, so it is safe to leave on through a contest. Off by default; with it off, packets go out only for the contacts you log in Contest mode.',
    )
  })

  it("never overwrites an address the operator already set", async () => {
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve({
        ...defaultSettings,
        mycall: 'KD9TAW',
        n1mmAddr: '192.168.1.50:12060',
      } as never),
    )
    renderPanel()
    await openLogging()
    fireEvent.click(await switchFor('Broadcast every QSO'))
    expect((await addressField()).value).toBe('192.168.1.50:12060')
  })
})
