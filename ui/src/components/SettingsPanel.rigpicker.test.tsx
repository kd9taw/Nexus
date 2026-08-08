// @vitest-environment jsdom
//
// The Rig-Model picker: the baud it may impose, and the screen-reader semantics that decided
// its design.
//
// FOUR ROUNDS OF THE BAUD RULE, and why the fifth is a different shape. Picking a rig imposes a
// CAT rate, and every round fixed the named radio and broke another: an FT-847 running 57,600
// clobbered to 4,800; the FX-4 (fixed 115,200) shipped unable to talk; three Kenwoods dead on a
// fresh install; and an IC-746 row reading `[9600, 19200]` against a manual offering
// 300/1200/4800/9600/19200/AUTO, which silently moved a 4,800 radio to 19,200 — the first bug
// again, created by the fix for the first bug. The cause was never the logic: it was 61 models
// of baud menus transcribed by hand out of hardware manuals. So the table now rests on one
// fact the backend states outright — `serial_rate_min == serial_rate_max` — and the first half
// of this file CHECKS IT AGAINST THAT FACT, from caps dumped by
// `scripts/gen-hamlib-serial-speeds.mjs` into `__fixtures__/hamlibSerialSpeeds.json`. A row
// transcribed from a manual cannot pass. Every rig that lost its entry keeps whatever baud is
// set, and `baud_ladder.rs` finds a working rate empirically instead.
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
import { SettingsPanel, RIG_FIXED_BAUD, baudForRig } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'
import hamlibSerialSpeeds from './__fixtures__/hamlibSerialSpeeds.json'

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
    'civDiagnosticLog', 'civDiagnosticStatus', 'allTxtLocation', 'revealAllTxt', 'recordingsLocation', 'revealRecordings', 'appVersion',
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

// The curated list the panel loads on mount, chosen to hold both sides of the rule at once:
// six fixed-4800 Yaesu backends and the FX-4 (115,200..115,200) are LISTED, so a pick moves the
// baud; the FT-847 (4800..57600), the TS-870S (1200..57600), the FTDX10/FT-710 and the IC-7300
// declare a range rather than a rate and so are UNLISTED, and a pick must leave the box alone.
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

/** Caps of every rig the picker offers, dumped from the BUNDLED Hamlib. The only admissible basis. */
const CAPS = new Map(hamlibSerialSpeeds.rigs.map((r) => [r.model, r]))
/** Rates the Baud picker offers — whatever a pick imposes has to be restorable by hand. */
const OFFERED = [1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200]

describe('the basis rule: a row exists only where the backend says there is ONE rate', () => {
  // ⭐ THE FIX FOR ALL FOUR ROUNDS, and it is a data rule, not a logic one. Everything the
  // table claims is checked here against `rigctl --dump-caps` from the Hamlib that ships in the
  // installer. Add a row transcribed from a radio's manual and these fail.
  const entries = [...RIG_FIXED_BAUD.entries()]

  it('the caps fixture is the bundled Hamlib and covers the catalog', () => {
    // If this ever reads as a different build, the facts below are about a different Hamlib
    // than the operator's copy — regenerate with `node scripts/gen-hamlib-serial-speeds.mjs`.
    expect(hamlibSerialSpeeds.hamlib).toMatch(/^Hamlib 4\.7\.1 /)
    expect(hamlibSerialSpeeds.rigs.length).toBeGreaterThan(100)
  })

  it('⭐ every listed model declares serial_rate_min == serial_rate_max', () => {
    // The rule itself. A range — however narrow, however low its max — is not evidence: Hamlib
    // does not enforce it (model 3085 opens clean at 115,200 against a declared max of 19,200)
    // and Nexus drives Icoms past it on purpose for the native CI-V scope.
    for (const [model] of entries) {
      const c = CAPS.get(model)
      expect({ model, declared: c ? `${c.min}..${c.max}` : 'not in the catalog caps' }).toEqual({
        model,
        declared: c ? `${c.min}..${c.min}` : 'not in the catalog caps',
      })
    }
  })

  it('⭐ every listed rate IS the rate its backend declares', () => {
    // The IC-746 failure mode pointed at the value rather than the row: `[9600, 19200]` where
    // the radio does 300..19,200. A transcribed number cannot survive this even in a row whose
    // model happens to be fixed-rate.
    for (const [model, rate] of entries) {
      expect({ model, rate }).toEqual({ model, rate: CAPS.get(model)?.min })
    }
  })

  it('⭐ every one-rate rig the picker offers HAS a row', () => {
    // The other direction, and it is the FX-4 bug: model 2053 runs at 115,200 and nothing else,
    // was added to the picker with no entry, and shipped unable to talk on the 38,400 default.
    // Any fixed-rate radio added to the catalog from now on fails here until it is listed.
    const fixed = hamlibSerialSpeeds.rigs.filter((r) => r.min === r.max)
    expect(fixed.length).toBeGreaterThan(20)
    for (const r of fixed) {
      expect({ model: r.model, name: r.name, listed: RIG_FIXED_BAUD.has(r.model) }).toEqual({
        model: r.model,
        name: r.name,
        listed: true,
      })
    }
  })

  it('every rate it can impose is on the Baud menu', () => {
    // Otherwise a pick forces a value the operator can neither see nor put back by hand.
    for (const [model, rate] of entries) {
      expect({ model, rate, offered: OFFERED.includes(rate) }).toEqual({ model, rate, offered: true })
    }
  })
})

describe('a listed rig gets its one rate, whatever the box said before', () => {
  // There is no judgement left to get wrong. On a one-rate rig every other value is a setting
  // that cannot work, so there is nothing to protect — which is why the clobber that broke
  // rounds one and four is now unrepresentable rather than guarded.
  const entries = [...RIG_FIXED_BAUD.entries()]

  it('there are rigs to check at all', () => {
    expect(entries.length).toBeGreaterThan(20)
  })

  it('the app default is replaced, and the rate itself is left alone', () => {
    for (const [model, rate] of entries) {
      expect({ model, fromDefault: baudForRig(model, 38400) }).toEqual({ model, fromDefault: rate })
      expect({ model, already: baudForRig(model, rate) }).toEqual({ model, already: null })
    }
  })

  it('…and so is every other rate on the menu, because none of them can work', () => {
    for (const [model, rate] of entries) {
      for (const other of OFFERED.filter((b) => b !== rate)) {
        expect({ model, other, imposed: baudForRig(model, other) }).toEqual({ model, other, imposed: rate })
      }
    }
  })

  it('the FX-4 lands on 115,200 — the only rate it has', async () => {
    // `rigctl -m 2053 --dump-caps`: `Serial speed: 115200..115200`. It reached the picker with
    // no entry at all, so picking it left the 38,400 default and CAT could not come up.
    await openRadioTab()
    expect(baudSelect().value).toBe('38400')
    fireEvent.change(rigSelect(), { target: { value: '2053' } })
    expect(baudSelect().value).toBe('115200')
  })

  const FIXED_4800: [number, string][] = [
    [1004, 'FT-1000MP Mark-V'],
    [1010, 'FT-736R'],
    [1014, 'FT-920'],
    [1016, 'FT-990'],
    [1021, 'FT-100 / FT-100D'],
    [1024, 'FT-1000MP'],
  ]

  it.each(FIXED_4800)('model %i — %s — lands on 4800 in the picker', async (model) => {
    await openRadioTab()
    fireEvent.change(rigSelect(), { target: { value: String(model) } })
    expect(baudSelect().value).toBe('4800')
  })

  it('re-picking the same rig is idempotent', async () => {
    await openRadioTab()
    for (let i = 0; i < 3; i++) fireEvent.change(rigSelect(), { target: { value: '1004' } })
    expect(baudSelect().value).toBe('4800')
  })
})

describe('an unlisted rig is never touched — the trade this round chose, and the ladder is its other half', () => {
  // ⭐ THE TRADE, written down. Every rig below had a row derived from its manual and lost it.
  // A radio with a real baud MENU has no fact behind it that this code can read, so Nexus now
  // keeps whatever is set and `baud_ladder.rs` finds a working rate empirically. The cost is
  // that a fresh install can sit on 38,400 against a radio that is not on it; the gain is that
  // no operator's working setting is ever overwritten by a transcription error.
  const LOST_ITS_ROW: [number, string][] = [
    [1001, 'Yaesu FT-847 — caps 4800..57600 (round one: 57,600 clobbered to 4,800)'],
    [2001, 'Kenwood TS-50S — caps 1200..57600'],
    [2002, 'Kenwood TS-440S — caps 1200..4800'],
    [2003, 'Kenwood TS-450S — caps 1200..4800'],
    [2004, 'Kenwood TS-570D — caps 1200..57600 (round three)'],
    [2010, 'Kenwood TS-870S — caps 1200..57600 (round three)'],
    [2011, 'Kenwood TS-940S — caps 1200..4800'],
    [2013, 'Kenwood TS-950SDX — caps 1200..4800'],
    [2016, 'Kenwood TS-570S — caps 1200..57600 (round three)'],
    [2025, 'Kenwood TS-140S — caps 4800..9600'],
    [3013, 'Icom IC-718 — caps 300..19200'],
    [3023, 'Icom IC-746 — caps 300..19200 (round four: the wrong row)'],
    [3044, 'Icom IC-910 — caps 300..19200'],
    [3046, 'Icom IC-746PRO — caps 300..19200'],
    [3057, 'Icom IC-756PROIII — caps 300..19200'],
    [3060, 'Icom IC-7000 — caps 300..19200'],
    [3070, 'Icom IC-7100 — caps 300..19200'],
    [3076, 'Xiegu X108G — caps 300..19200'],
    [3087, 'Xiegu X6100 — caps 300..19200'],
    [3089, 'Xiegu X5105 — caps 300..19200'],
    [3091, 'Xiegu X6200 — caps 300..19200'],
  ]

  it.each(LOST_ITS_ROW)('model %i is unlisted and left alone at every rate — %s', (model) => {
    expect(RIG_FIXED_BAUD.has(model)).toBe(false)
    for (const rate of OFFERED) {
      expect({ model, rate, imposed: baudForRig(model, rate) }).toEqual({ model, rate, imposed: null })
    }
  })

  it('the IC-746 at 4,800 survives the pick — the round-four bug, directly', () => {
    // The row said `rates: [9600, 19200]`, so 4,800 counted as a rate the radio "cannot run"
    // and was replaced by 19,200. Its manual (Set Mode item 27) offers 4,800.
    expect(baudForRig(3023, 4800)).toBeNull()
  })

  it('the FT-847 at 57,600 survives the pick — the round-one bug, directly', async () => {
    await openRadioTab()
    fireEvent.change(baudSelect(), { target: { value: '57600' } })
    fireEvent.change(rigSelect(), { target: { value: '1001' } })
    expect(baudSelect().value).toBe('57600')
  })

  it('…and at the 38,400 default it is left there too — that is the cost, and the ladder pays it', async () => {
    // Deliberate and load-bearing: 38,400 is NOT on the FT-847's Menu 37 dial, so this is a
    // radio that will not answer until the ladder finds its rate. Round one imposed 4,800 here
    // and round four proved that kind of guess wrong on a different radio.
    await openRadioTab()
    fireEvent.change(rigSelect(), { target: { value: '1001' } })
    expect(baudSelect().value).toBe('38400')
  })

  it('the two Icoms whose caps are stale stay unlisted, as they always had to', () => {
    // 3085 IC-705: caps 4800..19200, but 115,200 is its CI-V USB default AND what Nexus's own
    // native scope requires. 3092 IC-7760: caps 300..19200, menu goes to 115,200/Auto. Neither
    // is `min == max`, so the basis rule excludes them without needing to know any of that.
    for (const model of [3085, 3092]) {
      expect({ model, listed: RIG_FIXED_BAUD.has(model) }).toEqual({ model, listed: false })
      for (const rate of [9600, 19200, 38400, 115200]) expect(baudForRig(model, rate)).toBeNull()
    }
  })

  it('a rig with no row is never touched, including a model number typed by hand', () => {
    for (const rate of OFFERED) {
      expect(baudForRig(1042, rate)).toBeNull() // FTDX10 — caps 4800..38400
      expect(baudForRig(3073, rate)).toBeNull() // IC-7300 — caps 4800..115200
      expect(baudForRig(999999, rate)).toBeNull() // not a rig at all
    }
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
