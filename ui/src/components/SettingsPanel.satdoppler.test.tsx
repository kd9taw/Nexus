// @vitest-environment jsdom
//
// Satellite Doppler + rotator manners (Settings ▸ Radio). Two things are pinned here, and both
// are safety, not polish:
//
//  1. THE DEFAULTS. Doppler off, VFO mapping Off, no flip, post-pass Stop. A wrong VFO mapping
//     transmits on your own downlink and a flip breaks a rotator that cannot go past 90°, so an
//     upgrade must never arrive with either one already chosen for the operator.
//  2. THE WORDING. The mapping warning, the flip warning and the "Stop leaves it where the pass
//     ended" line are what stop an operator from finding out the hard way. They are part of the
//     feature, so they are tested like the rest of it.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
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
    'getFccStatesStatus', 'getTleStatus', 'fetchTlesNow', 'importTles', 'discoverFlex', 'civDiagnosticLog', 'civDiagnosticStatus',
    'allTxtLocation', 'revealAllTxt', 'recordingsLocation', 'revealRecordings', 'appVersion', 'getSpectrumRow', 'setFrequency',
    'getWatchlist', 'setWatchlist', 'openPanelWindow', 'getAssistanceJournal',
    'setUnassistedMode', 'confirmSatUplink',
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

const openRadio = async () => fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))

/** The fieldset a control sits in, by its <legend>. */
const sectionOf = (el: HTMLElement) =>
  el.closest('fieldset')?.querySelector('legend')?.textContent ?? null

const hintFor = async (label: string) =>
  (await screen.findByLabelText(label)).closest('.settings-field')?.querySelector('.settings-hint')
    ?.textContent ?? ''

describe('satellite Doppler settings', () => {
  it('ships correcting, with no VFO mapping chosen for the operator', async () => {
    // Correction is ON out of the box — it only moves the receive dial, and
    // only for a pass the operator armed. The MAPPING is still unchosen:
    // nothing reaches a transmit VFO until the operator says how their radio
    // is wired, because a wrong answer transmits on their own downlink.
    renderPanel()
    await openRadio()
    const enable = (await screen.findByLabelText(
      'Enable satellite Doppler correction',
    )) as HTMLInputElement
    expect(enable.checked).toBe(true)
    expect(sectionOf(enable)).toBe('Satellite Doppler')
    expect(((await screen.findByLabelText('Satellite VFO mapping')) as HTMLSelectElement).value).toBe(
      'off',
    )
  })

  it('the switch is an OFF switch — clearing it writes satDopplerOff', async () => {
    renderPanel()
    await openRadio()
    fireEvent.click(await screen.findByLabelText('Enable satellite Doppler correction'))
    expect(
      ((await screen.findByLabelText('Enable satellite Doppler correction')) as HTMLInputElement)
        .checked,
    ).toBe(false)
    fireEvent.click(document.querySelector('button.settings-save[type="submit"]') as HTMLButtonElement)
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    const saved = api.get('setSettings').mock.calls[0][0] as Record<string, unknown>
    expect(saved.satDopplerOff).toBe(true)
  })

  it('offers every mapping the operator might have wired, Off first', async () => {
    renderPanel()
    await openRadio()
    const sel = (await screen.findByLabelText('Satellite VFO mapping')) as HTMLSelectElement
    expect([...sel.options].map((o) => o.value)).toEqual([
      'off',
      'downlink-only',
      'uplink-only',
      'a-down-b-up',
      'a-up-b-down',
      'main-down-sub-up',
      'main-up-sub-down',
    ])
    // The IC-9700 full-duplex layout is named, because it is the one most operators want.
    expect(sel.textContent).toMatch(/Main = downlink, Sub = uplink \(IC-9700 full duplex\)/)
  })

  it('says on screen that a wrong mapping transmits on your own downlink', async () => {
    // The single most expensive mistake in satellite operating, and it is silent from the
    // shack: you hear yourself fine while sitting on top of the transponder output.
    renderPanel()
    await openRadio()
    expect(await hintFor('Satellite VFO mapping')).toMatch(
      /wrong mapping transmits on your own downlink/i,
    )
  })

  it('keeps both rate limits visible with their working defaults', async () => {
    renderPanel()
    await openRadio()
    const shift = (await screen.findByLabelText(
      'Minimum Doppler shift before retuning (Hz)',
    )) as HTMLInputElement
    const interval = (await screen.findByLabelText(
      'Doppler update interval (milliseconds)',
    )) as HTMLInputElement
    expect(shift.value).toBe('20')
    expect(interval.value).toBe('1000')
  })
})

// Round 2, defect 6b (rationale updated in round 3). The Satellite Doppler
// mapping is a flat (station-level) field, and picking one is a LIVE write
// confirming the uplink for the radio being OPERATED (the backend resolves
// it at write time) — but the fieldset lives on the Radio tab, the same tab
// as the per-radio Edit flow. While editing a non-active radio, a mapping
// pick would consent the operating rig while the panel shows another
// radio's card. The control refuses with the reason instead.
describe('the mapping select and the per-radio edit flow', () => {
  const RADIO0 = {
    id: 0,
    name: 'FTDX10',
    enabled: true,
    serialPort: 'COM3',
    baud: 38400,
    rigModel: 1042,
    rigModelName: 'Yaesu FTDX10',
    rigConn: 'serial',
    rigAddr: '',
    rigctldPort: 4532,
    rotctldPort: 4533,
    icomNativeCat: false,
    audioIn: 'in-0',
    audioOut: 'out-0',
    txLevel: 1,
    rxGain: 1,
    pttMethod: 'cat',
    rotatorModel: 0,
    rotatorPort: '',
    rotatorBaud: 9600,
    rotatorHost: '',
    nativeScope: 'auto',
    bands: [],
  }
  const RADIO1 = {
    ...RADIO0,
    id: 1,
    name: 'IC-9700',
    serialPort: 'COM7',
    baud: 115200,
    rigModel: 3081,
    rigModelName: 'Icom IC-9700',
    rigctldPort: 4534,
    rotctldPort: 4535,
    icomNativeCat: true,
  }
  const twoRadios = () =>
    Promise.resolve({
      ...defaultSettings,
      ...RADIO0,
      mycall: 'KD9TAW',
      mygrid: 'EN52',
      activeRadio: 0,
      radios: [RADIO0, RADIO1],
    } as never)

  it('is disabled, with the reason, while editing a radio you are not operating', async () => {
    api.get('getSettings').mockImplementation(twoRadios)
    renderPanel()
    await openRadio()
    // Only the NON-active radio's card offers Edit.
    fireEvent.click(await screen.findByRole('button', { name: 'Edit' }))
    const sel = (await screen.findByLabelText('Satellite VFO mapping')) as HTMLSelectElement
    expect(sel.disabled).toBe(true)
    expect(sel.title).toMatch(/confirmed per radio/i)
    expect(sel.title).toMatch(/pass rail/i)
  })

  it('stays live for the radio you are operating', async () => {
    api.get('getSettings').mockImplementation(twoRadios)
    renderPanel()
    await openRadio()
    const sel = (await screen.findByLabelText('Satellite VFO mapping')) as HTMLSelectElement
    expect(sel.disabled).toBe(false)
  })

  it('the edit-flow guard keys on the LIVE active radio, not the form snapshot (round 4)', async () => {
    // Residual 4: the disable guard compared editingRadioId against the FORM
    // SNAPSHOT's activeRadio. The snapshot goes stale while the panel is open
    // (the live switch is the activeRadioId prop — the same live state the
    // confirm verb resolves against), and a stale compare disabled the select
    // for the very radio the operator is operating AND editing — with a hint
    // telling them to "make it the active radio first". It already is.
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve({
        ...defaultSettings,
        ...RADIO0,
        mycall: 'KD9TAW',
        mygrid: 'EN52',
        activeRadio: 0, // the STALE snapshot
        radios: [RADIO0, RADIO1],
      } as never),
    )
    render(
      <SettingsPanel
        activeRadioId={1} // the LIVE operating radio — the panel opens editing it
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
    await openRadio()
    const sel = (await screen.findByLabelText('Satellite VFO mapping')) as HTMLSelectElement
    expect(sel.disabled).toBe(false)
  })

  it('the hint names the operating radio — the only rig a pick here confirms', async () => {
    renderPanel()
    await openRadio()
    const hint = await hintFor('Satellite VFO mapping')
    expect(hint).not.toMatch(/radio you have selected/i)
    expect(hint).toMatch(/radio you are operating/i)
  })

  it('a pick writes through the live confirm verb, resolved against the LIVE active radio (round 3)', async () => {
    // Defect 3: the old handler recorded consent for the FORM SNAPSHOT's
    // activeRadio, which goes stale if the active radio changes elsewhere
    // while the panel sits open. The pick now calls the backend verb with NO
    // radio id — the backend records the radio that is active at write time —
    // and the consent pair never rides the form's Save payload at all (the
    // pair is engine-owned live state; a stale snapshot cannot resurrect it).
    renderPanel()
    await openRadio()
    fireEvent.change(await screen.findByLabelText('Satellite VFO mapping'), {
      target: { value: 'main-down-sub-up' },
    })
    await waitFor(() =>
      expect(api.get('confirmSatUplink')).toHaveBeenCalledWith('main-down-sub-up'),
    )
    // Write-through, not form-buffered: no whole-settings Save was involved.
    expect(api.get('setSettings')).not.toHaveBeenCalled()
  })
})

describe('rotator settings', () => {
  it('post-pass defaults to Stop, and says Stop leaves the antenna where the pass ended', async () => {
    // Moving a mast nobody asked to move is the one unrecoverable surprise here.
    renderPanel()
    await openRadio()
    const sel = (await screen.findByLabelText(
      'What the rotator does after a pass',
    )) as HTMLSelectElement
    expect(sel.value).toBe('stop')
    expect([...sel.options].map((o) => o.value)).toEqual(['stop', 'park', 'ready'])
    expect(sel.options[0].textContent).toMatch(/leave the antenna where the pass ended/i)
    expect(sectionOf(sel)).toBe('Rotator')
  })

  it('flip is off and warns that many rotators cannot pass 90° elevation', async () => {
    renderPanel()
    await openRadio()
    const flip = (await screen.findByLabelText(
      'Allow the rotator to flip past 90 degrees elevation',
    )) as HTMLInputElement
    expect(flip.checked).toBe(false)
    expect(
      flip.closest('.settings-field')?.querySelector('.settings-hint')?.textContent ?? '',
    ).toMatch(/cannot mechanically go past 90° elevation/i)
  })

  it('carries park, ready, tolerance and calibration for both axes', async () => {
    renderPanel()
    await openRadio()
    for (const label of [
      'Park azimuth (degrees)',
      'Park elevation (degrees)',
      'Ready azimuth (degrees)',
      'Ready elevation (degrees)',
      'Azimuth calibration trim (degrees)',
      'Elevation calibration trim (degrees)',
    ]) {
      expect(((await screen.findByLabelText(label)) as HTMLInputElement).value).toBe('0')
    }
    // A deadband of 0 would let the rotator hunt for the whole pass, so it defaults to 2°.
    expect(
      ((await screen.findByLabelText('Azimuth tolerance (degrees)')) as HTMLInputElement).value,
    ).toBe('2')
    expect(
      ((await screen.findByLabelText('Elevation tolerance (degrees)')) as HTMLInputElement).value,
    ).toBe('2')
  })

  it('edits reach the save payload under the names the backend reads', async () => {
    // The form is saved through the existing whole-settings path; a renamed field would persist
    // nothing and fail silently. (The VFO mapping is deliberately NOT here:
    // the consent pair is backend-owned live state written by the
    // confirmSatUplink verb — see the round-3 test above — and a Save
    // payload cannot carry it.)
    renderPanel()
    await openRadio()
    fireEvent.change(await screen.findByLabelText('What the rotator does after a pass'), {
      target: { value: 'park' },
    })
    fireEvent.change(await screen.findByLabelText('Park azimuth (degrees)'), {
      target: { value: '180' },
    })
    fireEvent.click(await screen.findByLabelText('Allow the rotator to flip past 90 degrees elevation'))
    // The panel's own Save (the profile editor has a Save of its own).
    fireEvent.click(document.querySelector('button.settings-save[type="submit"]') as HTMLButtonElement)

    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    const saved = api.get('setSettings').mock.calls[0][0] as Record<string, unknown>
    expect(saved.rotPostPass).toBe('park')
    expect(saved.rotParkAz).toBe(180)
    expect(saved.rotAllowFlip).toBe(true)
  })
})
