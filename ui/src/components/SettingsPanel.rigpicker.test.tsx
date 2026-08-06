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
// Plus the FX-4, which is in the shipped verified tier and runs at one rate only, and the
// TS-870S, whose menu spans 1,200..57,600 while the radio ships on 9,600 — the one shape in
// which a rate the rig CAN run still has to be replaced.
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
  [2010, 'Kenwood TS-870S'],
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

  it('no entry ever replaces a rate its own rig can run — once someone has CHOSEN it', () => {
    // THE clobber guard. For every entry, every rate the rig is allowed = hands off.
    // 38,400 is excluded because it is the one rate that carries no evidence of a choice:
    // it is what a settings file that has never been touched already says. That half is the
    // sweep below.
    for (const [model, r] of entries) {
      for (const rate of r.rates.filter((b) => b !== 38400)) {
        expect({ model, rate, imposed: baudForRig(model, rate) }).toEqual({ model, rate, imposed: null })
      }
    }
  })

  it('every entry moves the app default onto the rate its radio ships on', () => {
    // ⭐ THE OTHER HALF, and the round-three regression. An entry exists because the radio is
    // NOT on 38,400 when it comes out of its box; leaving the default in place there is dead
    // CAT on a fresh install, which is what three Kenwoods shipped with. There is no entry in
    // this table for which "leave 38,400 alone" is the right answer — if there ever is, it
    // needs no entry at all.
    for (const [model, r] of entries) {
      expect({ model, imposed: baudForRig(model, 38400) }).toEqual({
        model,
        imposed: r.preferred === 38400 ? null : r.preferred,
      })
    }
  })

  it('…and never invents a rate the picker cannot show', () => {
    // Whatever an entry imposes has to be restorable by hand from the Baud menu.
    const offered = new Set([1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200])
    for (const [model] of entries) {
      const imposed = baudForRig(model, 38400)
      if (imposed !== null) expect({ model, imposed, offered: offered.has(imposed) }).toEqual({ model, imposed, offered: true })
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

  it('the cost of reading 38,400 as "never chosen" is bounded to three radios', () => {
    // ⭐ THE TRADE, written down rather than left implicit. Half 2 of the rule cannot tell a
    // deliberate 38,400 from an untouched one, so on a listed rig whose menu CONTAINS 38,400 it
    // will overwrite a choice. That is only possible where 38,400 is in `rates` at all — every
    // other entry already replaced it under half 1 — and it is exactly the three Kenwoods,
    // whose radios ship on 9,600. If this list grows, the trade grew with it: check the new
    // rig's manual and decide deliberately.
    const canOverwriteAChoice = entries.filter(([, r]) => r.rates.includes(38400) && r.preferred !== 38400)
    expect(canOverwriteAChoice.map(([m]) => m).sort((a, b) => a - b)).toEqual([2004, 2010, 2016])
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

describe('a radio that SHIPS on a rate gets it — the app default is not a choice', () => {
  // ⭐ THE ROUND-THREE REGRESSION, and the whole reason `baudForRig` has two halves.
  //
  // These three were given `rateRange(1200, 57600, 9600)` — Hamlib's DECLARED range — under
  // the rule "never clobber a valid choice". 38,400 is inside 1,200..57,600, so it counted as
  // a valid choice, `baudForRig` imposed nothing, and a fresh install that picked "Kenwood
  // TS-870S" saved 38,400 against a radio sitting on its factory 9,600: dead CAT out of the
  // box, on three popular radios, where the plain `model → baud` map this replaced had worked.
  //
  // The declared range is what the BACKEND can drive. It is not what the RADIO is set to.
  const SHIPS_ON_9600: [number, string][] = [
    [2010, 'Kenwood TS-870S'],
    [2004, 'Kenwood TS-570D'],
    [2016, 'Kenwood TS-570S'],
  ]

  it.each(SHIPS_ON_9600)('model %i comes out of its box on 9,600 — %s', (model) => {
    expect(baudForRig(model, 38400)).toBe(9600)
  })

  it.each(SHIPS_ON_9600)('…and model %i leaves every rate the operator chose alone — %s', (model) => {
    // The radio HAS a baud menu, so all of these are real settings someone may be running.
    for (const chosen of [1200, 2400, 4800, 9600, 19200, 57600]) {
      expect({ model, chosen, imposed: baudForRig(model, chosen) }).toEqual({ model, chosen, imposed: null })
    }
  })

  it('the TS-870S picker lands on 9,600, and a 57,600 the operator set survives it', async () => {
    await openRadioTab()
    expect(baudSelect().value).toBe('38400')
    fireEvent.change(rigSelect(), { target: { value: '2010' } })
    expect(baudSelect().value).toBe('9600')
    fireEvent.change(baudSelect(), { target: { value: '57600' } })
    fireEvent.change(rigSelect(), { target: { value: '2010' } })
    expect(baudSelect().value).toBe('57600')
  })
})

describe('the verified tier: the Icom CI-V rigs whose RADIO tops out below 38,400', () => {
  // The audit finished for the verified tier. `rigctl -m <model> --dump-caps` (bundled Hamlib
  // 4.7.1) says nine verified-tier radios declare a range that excludes the 38,400 default —
  // and all nine are Icom CI-V, the one family where a low declared max is NOT evidence
  // (measured last round: `rig_open` on 3085 at 115,200 returns 0 against a declared max of
  // 19,200, and Nexus drives Icoms past it on purpose for the native scope).
  //
  // So each of the nine was settled against the RADIO's own CI-V BAUD menu, not the backend's
  // caps. Seven top out at 19,200 and earn an entry; two do 115,200 and must not have one.
  const TOPS_AT_19200: [number, string][] = [
    [3013, 'IC-718 — set mode CI-V baud: 300/1200/4800/9600/19200 ("3/12/48/96/HI")'],
    [3023, 'IC-746 — set mode CI-V baud: 9600/19200/Auto'],
    [3046, 'IC-746PRO — set mode CI-V baud: 300/1200/4800/9600/19200/Auto'],
    [3057, 'IC-756PROIII — set mode CI-V baud: 300/1200/4800/9600/19200/Auto'],
    [3044, 'IC-910H — set mode CI-V baud: 300/1200/4800/9600/19200/Auto'],
    [3060, 'IC-7000 — set mode CI-V baud: 4800/9600/19200/Auto'],
    [3070, 'IC-7100 — Connectors ▸ CI-V baud: 300/1200/4800/9600/19200/Auto'],
  ]

  it.each(TOPS_AT_19200)('model %i is set to 19,200 out of the box — %s', (model) => {
    expect(baudForRig(model, 38400)).toBe(19200)
  })

  it.each(TOPS_AT_19200)('…and model %i leaves a slower rate the operator chose alone', (model) => {
    expect(baudForRig(model, 9600)).toBeNull()
  })

  it('the two Icoms that really do 115,200 get NO entry, whatever their caps say', () => {
    // THE EXCEPTION, and it survives because it was checked per rig rather than by make.
    // 3085 IC-705: caps 4800..19200, but 115,200 is its CI-V USB default AND what Nexus's
    //   own native scope requires — an entry here would break the feature it was added for.
    // 3092 IC-7760: caps 300..19200, but its CI-V baud menu goes to 115,200/Auto and the USB
    //   spectrum path runs there. The backend's range is stale, not narrow.
    for (const model of [3085, 3092]) {
      expect({ model, listed: RIG_CAT_RATES.has(model) }).toEqual({ model, listed: false })
      for (const rate of [9600, 19200, 38400, 115200]) expect(baudForRig(model, rate)).toBeNull()
    }
  })

  it('no OTHER verified-tier rig is left on a rate its backend excludes', () => {
    // The sweep that closes the audit: every verified-tier model, its declared
    // `Serial speed: min..max` from the bundled 4.7.1, and what Nexus sets on a fresh install.
    // Models with no serial port at all (Dummy/NET/FLRig/Flex-native) print no such line.
    const CAPS: [number, number, number][] = [
      [3073, 4800, 115200], [3085, 4800, 19200], [3078, 300, 115200], [3081, 4800, 38400],
      [3092, 300, 19200], [3094, 4800, 115200], [3070, 300, 19200], [3013, 300, 19200],
      [3060, 300, 19200], [3023, 300, 19200], [3046, 300, 19200], [3057, 300, 19200],
      [3044, 300, 19200], [3090, 4800, 230400], [1035, 4800, 38400], [1049, 4800, 115200],
      [1051, 4800, 115200], [1042, 4800, 38400], [1040, 4800, 38400], [1044, 4800, 38400],
      [1036, 4800, 38400], [1022, 4800, 38400], [1043, 4800, 38400], [1020, 4800, 38400],
      [1041, 4800, 38400], [1046, 4800, 38400], [1028, 4800, 38400], [1029, 4800, 38400],
      [1034, 4800, 38400], [1037, 4800, 38400], [1032, 4800, 38400], [1024, 4800, 4800],
      [2031, 4800, 115200], [2037, 4800, 115200], [2041, 4800, 115200], [2039, 4800, 115200],
      [2028, 4800, 115200], [2014, 1200, 115200], [2010, 1200, 57600], [2009, 4800, 4800],
      [2029, 4800, 38400], [2043, 4800, 38400], [2047, 4800, 115200], [2044, 4800, 38400],
      [2045, 4800, 38400], [2054, 300, 115200], [2048, 300, 115200], [2040, 4800, 38400],
      [2056, 1200, 115200], [16013, 57600, 57600], [16008, 57600, 57600], [16011, 57600, 57600],
      [3088, 19200, 19200], [3087, 300, 19200], [3091, 300, 19200], [3089, 300, 19200],
      [2057, 9600, 230400], [2052, 4800, 115200], [2053, 115200, 115200], [2055, 38400, 115200],
      [17002, 9600, 9600],
    ]
    // The Icom pair is the documented exception: their caps are stale, so being outside the
    // declared range is the correct, deliberate outcome for them and only for them.
    const EXEMPT = new Set([3085, 3092])
    for (const [model, min, max] of CAPS) {
      const set = baudForRig(model, 38400) ?? 38400
      const within = min <= set && set <= max
      expect({ model, set, within: within || EXEMPT.has(model) }).toEqual({ model, set, within: true })
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
