// @vitest-environment jsdom
//
// Settings deep links — the panel can be TOLD where to go.
//
// Before this, Settings always opened on the Station tab and took no target: the component had
// no prop for it, and there was not one `id` anchor or `scrollIntoView` call in its 8,000 lines.
// Meanwhile the app hard-coded ~228 "Settings ▸ …" pointers across the UI, the Rust toasts and
// the docs — prose that told an operator the path and then made them walk it. Setup Health's own
// RX-audio dot said "check the audio device below" and pointed 1,870 lines down a scroller.
//
// What is pinned here:
//  - a target opens the right TAB, including for a section that moved tabs in the nine-tab split;
//  - it SCROLLS the section into view (the anchor exists and is asked for);
//  - a target inside a COLLAPSED disclosure expands it — a control the operator still cannot see
//    has not been found, which is the Issue #62 failure exactly;
//  - a pointer written against a tab that no longer exists still lands (the alias resolver), so
//    the sweep of 228 prose pointers is not a prerequisite for shipping the split;
//  - an unrecognised target degrades to the default landing rather than blanking the panel.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'

const api = vi.hoisted(() => {
  const VERBS = [
    'clearCloudlogKey', 'clearClublogPassword', 'clearEqslPassword', 'clearHamqthPassword',
    'clearHrdlogCode', 'clearLotwPassword', 'clearQrzLogbookKey', 'clearQrzPassword', 'detectRigs',
    'downloadEqslReport', 'downloadLotwReport', 'getAllRigModels', 'getAudioDevices', 'getBandPlan',
    'getRigModels', 'getSerialPortsDetailed', 'getSettings', 'setCloudlogKey', 'setClublogPassword',
    'setEqslPassword', 'setHamqthPassword', 'setHrdlogCode', 'setLotwPassword', 'setQrzLogbookKey',
    'setQrzPassword', 'setRepeaterbookToken', 'setRxGain', 'setSettings', 'setTxLevel', 'addRadio',
    'removeRadio', 'renameRadio', 'setActiveRadio', 'setRadioBands', 'updateRadioProfile', 'testCat',
    'probeCatPorts', 'qrzTestConnection', 'syncQrz', 'n3fjpTestConnection', 'getConnectionLog',
    'getCredentialsStatus', 'fetchLotwUsers', 'getLotwUsersStatus', 'fetchFccStates',
    'getFccStatesStatus', 'getTleStatus', 'fetchTlesNow', 'importTles', 'discoverFlex',
    'civDiagnosticLog', 'civDiagnosticStatus', 'allTxtLocation', 'revealAllTxt',
    'recordingsLocation', 'revealRecordings', 'appVersion', 'getSpectrumRow', 'setFrequency',
    'getWatchlist', 'setWatchlist', 'openPanelWindow', 'getAssistanceJournal', 'setUnassistedMode',
  ]
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get, VERBS }
})

vi.mock('../api', () => {
  const mod: Record<string, unknown> = {}
  for (const v of api.VERBS) mod[v] = api.get(v)
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

function renderPanel(target?: string) {
  return render(
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
}

/** Which tab is selected, by the rail's own aria state. */
const selectedTab = () =>
  screen.getAllByRole('tab').find((t) => t.getAttribute('aria-selected') === 'true')?.textContent

beforeEach(() => {
  // The list-returning verbs must hand back real shapes: the panel maps over them on first
  // render, so a bare `null` from the default spy crashes FrequencyControl before a single tab
  // exists to assert on.
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([]))
  api.get('getConnectionLog').mockImplementation(() => Promise.resolve([]))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.2.6'))
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52' } as never),
  )
  // jsdom has no layout engine, so scrollIntoView is not implemented. Spy on the prototype so
  // the assertion is "the panel ASKED to scroll the right element", which is the behaviour that
  // matters — the browser owns the rest.
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('a deep link lands where it was pointed', () => {
  it('opens on Station with no target, exactly as before', async () => {
    renderPanel()
    await screen.findByRole('tab', { name: 'Station' })
    expect(selectedTab()).toBe('Station')
  })

  it('opens the tab a section now lives on, not the one it used to', async () => {
    // `audio` is on Radio; this is the maintainer's own complaint made navigable.
    renderPanel('audio')
    await waitFor(() => expect(selectedTab()).toBe('Radio'))
  })

  it('follows a section that MOVED in the nine-tab split', async () => {
    // `cw` was on the old shared Modes page and now has its own tab. The caller passes a section
    // id and does not need to know that — which is why pointers target ids, not tab names.
    renderPanel('cw')
    await waitFor(() => expect(selectedTab()).toBe('CW'))
  })

  it('scrolls the targeted section into view', async () => {
    renderPanel('audio')
    await waitFor(() => expect(selectedTab()).toBe('Radio'))
    await waitFor(() => {
      const el = document.getElementById('settings-audio')
      expect(el, 'the Audio fieldset must carry its registry anchor').toBeTruthy()
      expect(el && (el.scrollIntoView as unknown as ReturnType<typeof vi.fn>).mock).toBeTruthy()
    })
    await waitFor(() => expect(Element.prototype.scrollIntoView).toHaveBeenCalled())
  })

  it('EXPANDS a collapsed disclosure it points into', async () => {
    // The Advanced group under Rig & CAT ships collapsed, and that is where the toggle from
    // Issue #62 lives — an operator filed a feature request for a control that had already
    // shipped, because it was folded away and the disclosure did not read as a control. A deep
    // link that leaves it folded has not found anything.
    renderPanel('rig-advanced')
    await waitFor(() => expect(selectedTab()).toBe('Radio'))
    await waitFor(() => {
      const toggle = screen
        .getAllByRole('button')
        .find((b) => b.textContent?.trim() === 'Advanced')
      expect(toggle, 'the Advanced disclosure must be on the Radio tab').toBeTruthy()
      expect(toggle?.getAttribute('aria-expanded')).toBe('true')
    })
  })

  it('leaves the Advanced disclosure collapsed when nothing points at it', async () => {
    // The control for the control: the previous test would pass just as well if the group were
    // simply always open, which would put a zero-RF toggle in front of every operator.
    renderPanel('audio')
    await waitFor(() => expect(selectedTab()).toBe('Radio'))
    const toggle = screen.getAllByRole('button').find((b) => b.textContent?.trim() === 'Advanced')
    expect(toggle?.getAttribute('aria-expanded')).toBe('false')
  })
})

describe('a pointer written against the old layout still lands', () => {
  it('follows a prose path naming a tab that no longer exists', async () => {
    // Hundreds of these are still in the tree. "Modes" was deleted by the split; the section it
    // names is what the operator actually asked for, so that wins.
    renderPanel('Settings ▸ Modes ▸ CW')
    await waitFor(() => expect(selectedTab()).toBe('CW'))
  })

  it('follows a legacy tab name from the 0.17 consolidation', async () => {
    // "Logbook & QSL" has been dead since 0.17 and is STILL cited in six places in the tree.
    renderPanel('Settings ▸ Logbook & QSL')
    await waitFor(() => expect(selectedTab()).toBe('Logging & Connectors'))
  })

  it('degrades to the default landing when it cannot resolve the target', async () => {
    // Never a blank panel and never a dead end: unresolvable is exactly as good as the old
    // behaviour, which is the floor this whole mechanism has to clear.
    renderPanel('Settings ▸ Something That Never Existed')
    await screen.findByRole('tab', { name: 'Station' })
    expect(selectedTab()).toBe('Station')
  })
})
