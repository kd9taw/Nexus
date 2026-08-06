// @vitest-environment jsdom
//
// Field report 2026-08 (FT-847 owner, CAT works in WSJT-X, dead in Nexus): the app's default
// baud is 38400, a rate the FT-847 cannot select — its CAT RATE menu (Menu 37) offers only
// 4800 / 9600 / 57600, factory 4800. `BAUD_BY_MODEL` exists precisely to auto-set a rate the
// radio can answer on when a rig is picked, and it carried Xiegu and Kenwood entries only:
// no Yaesu at all. Every rate asserted here is read off the model's Hamlib backend caps
// (`serial_rate_min`/`serial_rate_max` in rigs/yaesu/*.c), never from a manual's prose.
//
// The second half guards the rig picker's ACCESSIBILITY, which decided its design: the QA
// pass wanted the `<select>` replaced by an input + `<datalist>` so a name could be typed.
// JAWS does not announce a datalist-backed input the way it announces a `<select>` (the
// suggestion popup is browser chrome, not DOM — Chromium/WebView2 never puts it on the IA2
// bridge JAWS reads), and a11y here is always-on, never a mode. So the control of record
// stays a native `<select>` and searchability arrives as a filter field beside it. These
// tests are what stops that being quietly undone.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { SettingsPanel, RIG_CAT_RATES, baudForRig } from './SettingsPanel'
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
    'civDiagnosticLog', 'civDiagnosticStatus', 'allTxtLocation', 'revealAllTxt', 'appVersion',
    'getSpectrumRow', 'setFrequency', 'getWatchlist', 'setWatchlist', 'openPanelWindow',
    'getAssistanceJournal', 'setUnassistedMode',
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

/** A serial-CAT radio sitting on the app's 38400 default — the state the field report starts in. */
function serialRadio() {
  const radio = {
    id: 0,
    name: 'Radio 1',
    enabled: true,
    serialPort: 'COM3',
    baud: 38400,
    rigModel: 0,
    rigModelName: '',
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
  return {
    ...defaultSettings,
    ...radio,
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    activeRadio: 0,
    radios: [radio],
  } as never
}

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

// The curated list the panel loads on mount. Yaesu spread across the eras that matter:
// three fixed-4800 backends, the FT-847 (4800..57600), and two that really do 38400.
// Plus the FX-4, which is in the shipped verified tier and runs at one rate only.
const CURATED: [number, string][] = [
  [1001, 'Yaesu FT-847'],
  [1004, 'Yaesu FT-1000MP Mark-V'],
  [1010, 'Yaesu FT-736R'],
  [1014, 'Yaesu FT-920'],
  [1016, 'Yaesu FT-990'],
  [1021, 'Yaesu FT-100 / FT-100D'],
  [1024, 'Yaesu FT-1000MP'],
  [1042, 'Yaesu FTDX10'],
  [1049, 'Yaesu FT-710'],
  [2053, 'BG2FX FX-4 (C/CR/L)'],
  [3073, 'Icom IC-7300'],
]

beforeEach(() => {
  for (const spy of Object.values(api.spies)) {
    spy.mockClear()
    spy.mockImplementation(() => Promise.resolve(null))
  }
  api.get('getRigModels').mockImplementation(() => Promise.resolve(CURATED))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve(CURATED))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve({}))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.0.1'))
})
afterEach(cleanup)

async function openRadioTab() {
  api.get('getSettings').mockImplementation(() => Promise.resolve(serialRadio()))
  renderPanel()
  fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
  await screen.findByText('Connection')
}

const rigSelect = () => screen.getByRole('combobox', { name: 'Rig Model' }) as HTMLSelectElement
const baudSelect = () =>
  screen.getByText('Baud').closest('label')?.querySelector('select') as HTMLSelectElement

describe('picking a Yaesu sets a baud the radio can actually run', () => {
  // Hamlib rigs/yaesu/*.c, `serial_rate_min`/`serial_rate_max` per caps struct. Every one of
  // these tops out BELOW the app's 38400 default, so the default is guaranteed silence.
  const FIXED_4800: [number, string][] = [
    [1004, 'FT-1000MP Mark-V (ft1000mp.c, 4800/4800)'],
    [1010, 'FT-736R (ft736.c, 4800/4800)'],
    [1014, 'FT-920 (ft920.c, 4800/4800)'],
    [1016, 'FT-990 (ft990.c, 4800/4800)'],
    [1021, 'FT-100 / FT-100D (ft100.c, 4800/4800)'],
    [1024, 'FT-1000MP (ft1000mp.c, 4800/4800)'],
  ]

  it('the FT-847 — the reported radio — lands on its factory CAT rate, not 38400', async () => {
    await openRadioTab()
    expect(baudSelect().value).toBe('38400')
    fireEvent.change(rigSelect(), { target: { value: '1001' } })
    // ft847.c: serial_rate_min 4800 / max 57600. The radio's Menu 37 enumerates
    // 4800 / 9600 / 57600 and ships on 4800 — the rate an untouched radio answers on.
    expect(baudSelect().value).toBe('4800')
  })

  it.each(FIXED_4800)('model %i — %s — lands on 4800', async (model) => {
    await openRadioTab()
    fireEvent.change(rigSelect(), { target: { value: String(model) } })
    expect(baudSelect().value).toBe('4800')
  })

  it('a Yaesu that really does 38400 is left alone', async () => {
    // ftdx10.c and ft710.c: serial_rate_max 38400 and 115200. The default already answers,
    // so overriding it here would be the same bug pointed the other way.
    await openRadioTab()
    fireEvent.change(rigSelect(), { target: { value: '1042' } })
    expect(baudSelect().value).toBe('38400')
    fireEvent.change(rigSelect(), { target: { value: '1049' } })
    expect(baudSelect().value).toBe('38400')
  })
})

describe('picking a rig never overwrites a rate the radio can already run', () => {
  // The regression the first cut of the fix above shipped: the baud was applied on EVERY
  // pick, unconditionally. Six of the seven Yaesu entries were safe only by accident — their
  // backends declare `serial_rate_min == serial_rate_max`, so there was no other rate to
  // lose. The FT-847 is not one of them (`rigctl -m 1001 --dump-caps`: 4800..57600), and the
  // operator whose report started all this runs his at 57600: selecting his own radio in the
  // picker rewrote the setting that had just been made to work.
  const setBaud = (rate: number) => fireEvent.change(baudSelect(), { target: { value: String(rate) } })

  it('the FT-847 at 57,600 — the rate the reporter’s radio is on — survives the pick', async () => {
    await openRadioTab()
    setBaud(57600)
    fireEvent.change(rigSelect(), { target: { value: '1001' } })
    expect(baudSelect().value).toBe('57600')
  })

  it('…and at 9,600, the other rate on its Menu 37 dial', async () => {
    await openRadioTab()
    setBaud(9600)
    fireEvent.change(rigSelect(), { target: { value: '1001' } })
    expect(baudSelect().value).toBe('9600')
  })

  it('…while 38,400, which its dial does not offer, still becomes 4,800', async () => {
    // The original field report, unchanged: the app default is not a rate this radio can be
    // set to, so it is not a choice to protect.
    await openRadioTab()
    expect(baudSelect().value).toBe('38400')
    fireEvent.change(rigSelect(), { target: { value: '1001' } })
    expect(baudSelect().value).toBe('4800')
  })

  it('re-picking the same rig is idempotent — it never walks the baud back', async () => {
    await openRadioTab()
    setBaud(57600)
    for (let i = 0; i < 3; i++) fireEvent.change(rigSelect(), { target: { value: '1001' } })
    expect(baudSelect().value).toBe('57600')
  })
})

describe('the rule, not the table — a future entry cannot reintroduce the clobber', () => {
  // These are about `baudForRig` itself, because a table is only ever as safe as the rule it
  // is read through. Any entry added later is swept by all four.
  const entries = [...RIG_CAT_RATES.entries()]

  it('there are entries to check at all', () => {
    expect(entries.length).toBeGreaterThan(20)
  })

  it('every entry’s fallback rate is one of its own legal rates', () => {
    // Otherwise the very act of imposing a rate would set an illegal one.
    for (const [model, r] of entries) {
      expect({ model, ok: r.rates.includes(r.preferred) }).toEqual({ model, ok: true })
    }
  })

  it('every entry has at least one legal rate, and they are rates the picker offers', () => {
    // An entry whose rates are all off the Baud menu would force a value the operator cannot
    // see or restore by hand.
    const offered = new Set([1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200])
    for (const [model, r] of entries) {
      expect({ model, n: r.rates.length }).not.toEqual({ model, n: 0 })
      for (const rate of r.rates) expect({ model, rate, offered: offered.has(rate) }).toEqual({ model, rate, offered: true })
    }
  })

  it('no entry ever replaces a rate its own rig can run', () => {
    // THE clobber guard. For every entry, every rate the rig is allowed = hands off.
    for (const [model, r] of entries) {
      for (const rate of r.rates) {
        expect({ model, rate, imposed: baudForRig(model, rate) }).toEqual({ model, rate, imposed: null })
      }
    }
  })

  it('…and always replaces one it cannot', () => {
    const all = [1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200]
    for (const [model, r] of entries) {
      for (const rate of all.filter((b) => !r.rates.includes(b))) {
        expect({ model, rate, imposed: baudForRig(model, rate) }).toEqual({
          model,
          rate,
          imposed: r.preferred,
        })
      }
    }
  })

  it('a rig with no entry is never touched, whatever it is set to', () => {
    // Absent means "no evidence" — the branch that must stay a no-op, because it is every
    // rig in the catalog that is not listed, including every model added in future.
    for (const rate of [1200, 4800, 9600, 38400, 57600, 115200]) {
      expect(baudForRig(1042, rate)).toBeNull() // FTDX10 — genuinely does 38400
      expect(baudForRig(3073, rate)).toBeNull() // IC-7300
      expect(baudForRig(999999, rate)).toBeNull() // a raw model number typed by hand
    }
  })
})

describe('a rig whose backend declares ONE rate cannot be left on the app default', () => {
  // `rigctl -m <model> --dump-caps` against the BUNDLED Hamlib 4.7.1, field
  // `Serial speed: <min>..<max>`, for every model in either catalog tier that declares
  // min == max. Each is a radio the picker offers on which the 38,400 default cannot work.
  const ONE_RATE: [number, number, string][] = [
    [2053, 115200, 'BG2FX FX-4/C/CR/L — added to the verified tier this batch'],
    [2021, 4800, 'Elecraft K2'],
    [2050, 9600, 'Lab599 Discovery TX-500'],
    [16013, 57600, 'Ten-Tec Eagle (599)'],
    [16008, 57600, 'Ten-Tec Orion (565)'],
    [16011, 57600, 'Ten-Tec Omni VII (588)'],
    [16001, 57600, 'Ten-Tec TT-550 Pegasus'],
    [16002, 57600, 'Ten-Tec TT-538 Jupiter'],
    [16007, 1200, 'Ten-Tec TT-516 Argonaut V'],
    [16009, 1200, 'Ten-Tec TT-585 Paragon'],
    [17002, 9600, 'Alinco DX-SR8'],
    [17001, 9600, 'Alinco DX-77'],
  ]

  it.each(ONE_RATE)('model %i runs at %i only — %s', (model, rate) => {
    expect(baudForRig(model, 38400)).toBe(rate)
    expect(baudForRig(model, rate)).toBeNull()
  })

  it('the four other radios added this batch really do 38,400, so they get no entry', () => {
    // Measured the same way, and this half matters as much: 3094 4800..115200,
    // 1051 4800..115200, 2052 4800..115200, 2055 38400..115200. Listing a rig that does not
    // need listing is how the clobber gets back in.
    for (const model of [3094, 1051, 2052, 2055]) {
      expect({ model, listed: RIG_CAT_RATES.has(model) }).toEqual({ model, listed: false })
      expect(baudForRig(model, 38400)).toBeNull()
    }
  })

  it('the FX-4 lands on 115,200 — the only rate it has', async () => {
    // `rigctl -m 2053 --dump-caps` (bundled Hamlib 4.7.1): `Serial speed: 115200..115200`.
    // The FX-4 was added to the verified tier with no rate entry at all, so picking it left
    // the 38400 default in place and CAT could not come up on a radio the picker offers.
    await openRadioTab()
    expect(baudSelect().value).toBe('38400')
    fireEvent.change(rigSelect(), { target: { value: '2053' } })
    expect(baudSelect().value).toBe('115200')
  })
})

describe('the rig picker is searchable without losing its screen-reader semantics', () => {
  it('stays a native <select> — not a datalist-backed input', async () => {
    // The a11y decision, pinned. A `<select>` is a first-class combobox on the IA2 bridge:
    // JAWS speaks its name, its value and "n of m", and its own first-letter type-ahead
    // works. An `<input list>` is announced as a plain edit field and its popup is silent.
    await openRadioTab()
    const sel = rigSelect()
    expect(sel.tagName).toBe('SELECT')
    expect(sel.getAttribute('list')).toBeNull()
    // Every model reachable as a real option element, i.e. present in the a11y tree.
    expect(screen.getAllByRole('option', { name: /Yaesu FT-847/ }).length).toBe(1)
  })

  it('typing a name narrows the list', async () => {
    await openRadioTab()
    const filter = screen.getByRole('textbox', { name: /filter the rig model list/i })
    fireEvent.change(filter, { target: { value: 'ft847' } })
    // Hyphen/space-insensitive: "ft847" finds "Yaesu FT-847".
    expect(screen.queryByRole('option', { name: /Yaesu FT-847/ })).not.toBeNull()
    expect(screen.queryByRole('option', { name: /IC-7300/ })).toBeNull()
    expect(screen.queryByRole('option', { name: /FTDX10/ })).toBeNull()
  })

  it('a model number finds its rig', async () => {
    await openRadioTab()
    fireEvent.change(screen.getByRole('textbox', { name: /filter the rig model list/i }), {
      target: { value: '3073' },
    })
    expect(screen.queryByRole('option', { name: /IC-7300/ })).not.toBeNull()
    expect(screen.queryByRole('option', { name: /Yaesu FT-847/ })).toBeNull()
  })

  it('several words all have to match', async () => {
    await openRadioTab()
    fireEvent.change(screen.getByRole('textbox', { name: /filter the rig model list/i }), {
      target: { value: 'yaesu 1000mp' },
    })
    expect(screen.getAllByRole('option', { name: /FT-1000MP/ }).length).toBe(2)
    expect(screen.queryByRole('option', { name: /FT-736R/ })).toBeNull()
  })

  it('the chosen rig never filters itself out of its own picker', async () => {
    // A `<select>` whose value matches no option displays the first one instead — the form
    // would read as a different radio than it holds.
    await openRadioTab()
    fireEvent.change(rigSelect(), { target: { value: '1001' } })
    fireEvent.change(screen.getByRole('textbox', { name: /filter the rig model list/i }), {
      target: { value: 'icom' },
    })
    expect(rigSelect().value).toBe('1001')
    expect(screen.queryByRole('option', { name: /Yaesu FT-847/ })).not.toBeNull()
  })

  it('keeps the live region mounted before there is anything to announce', async () => {
    // A region created in the same tick as its first content is a coin-flip with a screen
    // reader. It has to be there, empty, waiting.
    await openRadioTab()
    const filter = screen.getByRole('textbox', { name: /filter the rig model list/i })
    const field = filter.closest('label') as HTMLElement
    const region = field.querySelector('[role="status"]')
    expect(region).not.toBeNull()
    expect(region?.textContent).toBe('')
  })

  it('says how many matched, out loud', async () => {
    // Politely, in a live region: a sighted operator sees the list shrink; a blind one
    // otherwise gets no feedback at all until they arrow into the closed combobox.
    await openRadioTab()
    fireEvent.change(screen.getByRole('textbox', { name: /filter the rig model list/i }), {
      target: { value: 'yaesu' },
    })
    const status = screen.getByText(/9 models match/i)
    expect(status.getAttribute('role')).toBe('status')
    fireEvent.change(screen.getByRole('textbox', { name: /filter the rig model list/i }), {
      target: { value: 'zzz' },
    })
    expect(screen.getByText(/No model matches/i).getAttribute('role')).toBe('status')
  })

  it('an empty filter shows everything again', async () => {
    await openRadioTab()
    const filter = screen.getByRole('textbox', { name: /filter the rig model list/i })
    fireEvent.change(filter, { target: { value: 'icom' } })
    expect(screen.queryByRole('option', { name: /FTDX10/ })).toBeNull()
    fireEvent.change(filter, { target: { value: '' } })
    expect(screen.queryByRole('option', { name: /FTDX10/ })).not.toBeNull()
    // …and stops announcing, so the region is silent when nothing is being filtered.
    expect(screen.queryByText(/models? match/i)).toBeNull()
  })
})
