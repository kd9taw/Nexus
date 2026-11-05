// @vitest-environment jsdom
//
// Two radios that each need their OWN serial port, baud and sound card — an FT-710 and an
// FT-991A on one Mac, the station this was found on (field report, 2026-10-14: "they both like
// different baud rates … Nexus wants them to be the same for both radios", and the FT-991A "is
// not grabbing" its own USB Audio CODEC). Every test here runs a whole round trip through a
// backend that keeps state the way the engine does, and asserts what each radio's profile holds
// afterwards, by value.
//
// The rig form edits ONE radio at a time, and it holds that radio's port, baud and audio in its
// flat fields. Three moves pointed the form at a different radio, or trusted an old copy of the
// roster, without its fields following:
//   - "+ Add radio" chose the new radio for editing and left the operating radio's fields in the
//     form, so the first Save copied them onto the new radio;
//   - Edit read a radio from the roster as it was when Settings opened, so a radio saved since came
//     back with its old values, and the next Save wrote them back — and saving one radio wrote the
//     other's old values back the same way;
//   - removing the radio being edited handed the form to the operating radio still holding the
//     removed radio's fields, and the next Save stamped them onto the operating radio.
//
// Tx Power and RX Gain are per radio as well, and their sliders act on the radio in use as they
// move (`set_tx_level` / `set_rx_gain` take no radio). While the form edited a radio that is not
// the one in use, they changed the radio in use anyway, and the radio in use's live drive was
// mirrored into the edited radio's form, so Save gave the edited radio that drive.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { SettingsPanel, radioPatch } from './SettingsPanel'
import { confirmDialog } from '../confirm'
import { setRxGain, setTxLevel, type RadioProfilePatch } from '../api'
import type { RadioStatus } from '../types'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'
import { EN } from '../i18n/en'

// The house budget (see SettingsPanel.radiorouting.test.tsx): a loaded full suite runs cases far
// slower than one core does.
vi.setConfig({ testTimeout: 15_000 })

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Every export of `../api`, derived from the real module (the radiorouting test says why).
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
vi.mock('../confirm', () => ({ confirmDialog: vi.fn(async () => true) }))

// The patch fields typed, so `radioPatch` accepts both; anything else rides along untyped.
type Profile = Partial<RadioProfilePatch> & Record<string, unknown> & { id: number }
type Station = Partial<RadioProfilePatch> &
  Record<string, unknown> & { radios: Profile[]; activeRadio: number }

const FT710: Profile = {
  id: 0,
  name: 'FT-710',
  enabled: true,
  serialPort: '/dev/cu.usbserial-01A10000',
  baud: 38400,
  rigModel: 1049,
  rigModelName: 'Yaesu FT-710',
  rigConn: 'serial',
  rigAddr: '',
  rigctldPort: 4534,
  rotctldPort: 4535,
  icomNativeCat: false,
  audioIn: 'USB Audio Device',
  audioOut: 'USB Audio Device',
  txLevel: 0.5,
  rxGain: 1,
  pttMethod: 'cat',
  rotatorModel: 0,
  rotatorPort: '',
  rotatorBaud: 9600,
  rotatorHost: '',
  nativeScope: 'auto',
  bands: [],
}
const FT991A: Profile = {
  ...FT710,
  id: 1,
  name: 'FT-991A',
  serialPort: '/dev/cu.usbserial-00A20000',
  baud: 9600,
  rigModel: 1035,
  rigModelName: 'Yaesu FT-991',
  rigctldPort: 4536,
  rotctldPort: 4537,
  audioIn: 'USB Audio CODEC',
  audioOut: 'USB Audio CODEC',
}

/** What the engine holds. Read and written only through the mocked verbs below. */
let station: Station

const copy = <T,>(v: T): T => JSON.parse(JSON.stringify(v)) as T

/** `Settings::sync_flat_from_active`: the flat rig fields describe the operating radio. */
function mirrorActive(s: Station): Station {
  const active = s.radios.find((r) => r.id === s.activeRadio)
  return active ? { ...s, ...radioPatch(active) } : s
}

function profile(id: number): Profile {
  const p = station.radios.find((r) => r.id === id)
  if (!p) throw new Error(`no radio ${id}`)
  return p
}

/** The stateful half of the engine these flows go through, with the engine's own rules:
 *  `apply_settings` folds a payload's flat rig fields into the radio its `activeRadio` names and
 *  keeps the live roster and active radio; `update_radio_profile` patches one profile in place;
 *  `add_radio` appends `RadioProfile::default()` with fresh ports; neither add nor remove switches
 *  the operating radio. */
function installStation(radios: Profile[], activeRadio = 0) {
  station = mirrorActive(
    copy({ ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52', radios, activeRadio } as Station),
  )
  api.get('getSettings').mockImplementation(async () => copy(station))
  api.get('setSettings').mockImplementation(async (s: Station) => {
    const formActive = s.radios?.length ? s.activeRadio : station.activeRadio
    const radios = station.radios.map((r) => (r.id === formActive ? { ...r, ...radioPatch(s) } : r))
    station = mirrorActive({ ...copy(s), radios, activeRadio: station.activeRadio })
    return {}
  })
  api.get('updateRadioProfile').mockImplementation(async (id: number, patch: Profile) => {
    station = mirrorActive({
      ...station,
      radios: station.radios.map((r) => (r.id === id ? { ...r, ...copy(patch), id } : r)),
    })
    return {}
  })
  api.get('addRadio').mockImplementation(async () => {
    const id = Math.max(...station.radios.map((r) => r.id)) + 1
    const used = station.radios.flatMap((r) => [r.rigctldPort, r.rotctldPort])
    let port = 4534
    while (used.includes(port)) port += 1
    station = {
      ...station,
      radios: [
        ...station.radios,
        {
          id,
          name: `Radio ${station.radios.length + 1}`,
          enabled: true,
          pttMethod: 'vox',
          rigModel: 0,
          rigModelName: 'None / VOX',
          serialPort: '',
          baud: 38400,
          rigConn: 'serial',
          rigAddr: '',
          rigctldPort: port,
          rotctldPort: port + 1,
          icomNativeCat: false,
          audioIn: '',
          audioOut: '',
          txLevel: 0.9,
          rxGain: 1,
          rotatorModel: 0,
          rotatorPort: '',
          rotatorBaud: 9600,
          rotatorHost: '',
          nativeScope: 'auto',
          bands: [],
        },
      ],
    }
    return {}
  })
  api.get('removeRadio').mockImplementation(async (id: number) => {
    station = { ...station, radios: station.radios.filter((r) => r.id !== id) }
    return {}
  })
  // `set_tx_level` / `set_rx_gain` set the live drive and gain of the radio in use: the flat field
  // and that radio's profile together.
  const inUse = (edit: Partial<Profile>) =>
    station.radios.map((r) => (r.id === station.activeRadio ? { ...r, ...edit } : r))
  api.get('setTxLevel').mockImplementation(async (level: number) => {
    station = { ...station, txLevel: level, radios: inUse({ txLevel: level }) }
    return {}
  })
  api.get('setRxGain').mockImplementation(async (gain: number) => {
    station = { ...station, rxGain: gain, radios: inUse({ rxGain: gain }) }
    return {}
  })
}

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

/** The panel with radio 0 in use; `radio` is the live status the app hands it (the cockpit's Pwr). */
function panel(radio?: Partial<RadioStatus>) {
  return (
    <SettingsPanel
      activeRadioId={0}
      radio={radio as RadioStatus | undefined}
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
  )
}

function renderPanel(radio?: Partial<RadioStatus>) {
  return render(panel(radio))
}

beforeEach(() => {
  for (const spy of Object.values(api.spies)) {
    spy.mockClear()
    spy.mockImplementation(() => Promise.resolve(null))
  }
  // Reset, not clear: a queued once-answer a test did not consume must not answer the next test.
  vi.mocked(confirmDialog).mockReset()
  vi.mocked(confirmDialog).mockImplementation(async () => true)
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  const cards = ['USB Audio CODEC', 'USB Audio Device'].map((name) => ({ name, label: name }))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: cards, output: cards }))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve({}))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.18.0'))
})
afterEach(cleanup)

/** The control beside a rig-form label: the select or input inside the same `<label>`. */
function control(label: string): HTMLInputElement | HTMLSelectElement {
  const span = screen
    .getAllByText(label)
    .find((n) => n.classList.contains('settings-label') && n.closest('label'))
  const el = span?.closest('label')?.querySelector('select, input')
  if (!el) throw new Error(`no control labelled ${label}`)
  return el as HTMLInputElement | HTMLSelectElement
}

/** What the rig form shows for the radio it is editing. */
function shown() {
  return {
    baud: Number(control('Baud').value),
    serialPort: control('Serial Port').value,
    audioIn: control('Input Device (RX)').value,
    audioOut: control('Output Device (TX)').value,
  }
}

/** The CAT and audio fields this report is about, as a profile holds them. */
function held(p: Profile) {
  return { baud: p.baud, serialPort: p.serialPort, audioIn: p.audioIn, audioOut: p.audioOut }
}

async function openRadioTab() {
  fireEvent.click(await screen.findByRole('tab', { name: 'Radio' }))
  await screen.findByRole('button', { name: '+ Add radio' })
}

/** Press Save and wait for the save to finish. The click itself takes the status to "saving"
 *  (synchronously, in the submit handler), so the "Saved" waited for is this save's own. */
async function save() {
  const writes = () =>
    api.get('setSettings').mock.calls.length + api.get('updateRadioProfile').mock.calls.length
  const before = writes()
  fireEvent.click(
    screen.getAllByRole('button', { name: 'Save' }).find((b) => (b as HTMLButtonElement).type === 'submit')!,
  )
  await waitFor(() => {
    const refused = document.querySelector('.settings-error')?.textContent
    if (refused) throw new Error(`the save was refused: ${refused}`)
    expect(writes()).toBeGreaterThan(before)
    expect(document.querySelector('.settings-ok')?.textContent).toBe('Saved')
  })
}

/** Click the one Edit button there is — on the card of the radio the form is not editing. */
async function editTheOtherRadio(name: string) {
  fireEvent.click(await screen.findByRole('button', { name: 'Edit' }))
  await screen.findByText((_, el) => el?.classList.contains('radio-card') === true && el.classList.contains('editing') && (el.querySelector('input') as HTMLInputElement | null)?.defaultValue === name)
}

describe('each radio keeps its own port, baud and sound card', () => {
  it('Add radio loads the NEW radio into the rig form, so Save cannot copy the operating radio onto it', async () => {
    installStation([FT991A], 1)
    renderPanel()
    await openRadioTab()
    await waitFor(() => expect(shown()).toEqual(held(FT991A)))

    fireEvent.click(screen.getByRole('button', { name: '+ Add radio' }))
    await waitFor(() => expect(station.radios).toHaveLength(2))
    const added = station.radios[1]
    // The form now edits the new radio, and shows ITS fields: its default 38400, no port, the
    // system default sound card — not the FT-991A's 9600, port and USB Audio CODEC.
    await waitFor(() => expect(shown()).toEqual(held(added)))

    await save()
    expect(held(profile(added.id))).toEqual(held(added))
    // …and the operating radio is exactly as it was.
    expect(held(profile(1))).toEqual(held(FT991A))
  })

  it('Add radio over unsaved edits asks first, and a No keeps the edits and adds nothing', async () => {
    installStation([FT710])
    renderPanel()
    await openRadioTab()
    await waitFor(() => expect(shown()).toEqual(held(FT710)))
    fireEvent.change(control('Baud'), { target: { value: '19200' } })

    vi.mocked(confirmDialog).mockResolvedValueOnce(false)
    fireEvent.click(screen.getByRole('button', { name: '+ Add radio' }))
    await waitFor(() => expect(confirmDialog).toHaveBeenCalled())

    expect(api.get('addRadio')).not.toHaveBeenCalled()
    expect(station.radios).toHaveLength(1)
    expect(shown().baud).toBe(19200)
  })

  it('a radio opened for Edit again after a Save shows what was saved, and saving it again keeps it', async () => {
    installStation([FT710, { ...FT991A, baud: 4800 }])
    renderPanel()
    await openRadioTab()

    await editTheOtherRadio('FT-991A')
    expect(shown().baud).toBe(4800)
    fireEvent.change(control('Baud'), { target: { value: '9600' } })
    await save()
    expect(profile(1).baud).toBe(9600)

    await editTheOtherRadio('FT-710')
    expect(shown()).toEqual(held(FT710))
    await editTheOtherRadio('FT-991A')
    // What was SAVED, not what the roster held when Settings opened.
    expect(shown().baud).toBe(9600)

    await save()
    expect(held(profile(1))).toEqual(held(FT991A))
    expect(held(profile(0))).toEqual(held(FT710))
  })

  it('saving one radio never puts back an older value of the other radio', async () => {
    installStation([FT710, FT991A])
    renderPanel()
    await openRadioTab()

    // The operating FT-710, edited and saved first…
    await waitFor(() => expect(shown()).toEqual(held(FT710)))
    fireEvent.change(control('Baud'), { target: { value: '19200' } })
    fireEvent.change(control('Input Device (RX)'), { target: { value: 'USB Audio CODEC' } })
    await save()
    expect(held(profile(0))).toEqual({ ...held(FT710), baud: 19200, audioIn: 'USB Audio CODEC' })

    // …then the FT-991A. Its Save must not hand the FT-710 the values it had before.
    await editTheOtherRadio('FT-991A')
    fireEvent.change(control('Output Device (TX)'), { target: { value: 'USB Audio Device' } })
    await save()
    expect(held(profile(0))).toEqual({ ...held(FT710), baud: 19200, audioIn: 'USB Audio CODEC' })
    expect(held(profile(1))).toEqual({ ...held(FT991A), audioOut: 'USB Audio Device' })
  })

  it('removing the radio being edited puts the operating radio back in the form before anything can save it', async () => {
    installStation([FT710, FT991A])
    renderPanel()
    await openRadioTab()

    await editTheOtherRadio('FT-991A')
    expect(shown()).toEqual(held(FT991A))
    const remove = screen
      .getAllByRole('button', { name: 'Remove' })
      .find((b) => !(b as HTMLButtonElement).disabled)!
    fireEvent.click(remove)
    await waitFor(() => expect(station.radios).toHaveLength(1))
    await waitFor(() => expect(shown()).toEqual(held(FT710)))

    await save()
    expect(held(profile(0))).toEqual(held(FT710))
  })
})

/** The Audio section's sliders. Tx Power's POSITION is √level (its square-law curve). */
const txPower = () => screen.getByLabelText('Transmit drive level') as HTMLInputElement
const rxGain = () => screen.getByLabelText('RX capture gain') as HTMLInputElement

/** Move a slider and let go of it, by pointer and by key: every handler the slider has. */
function move(slider: HTMLInputElement, value: string) {
  fireEvent.change(slider, { target: { value } })
  fireEvent.pointerUp(slider, { target: { value } } as never)
  fireEvent.keyUp(slider, { target: { value } } as never)
}

/** The radio in use (the FT-710) at drive 0.25 and gain ×1.5; the FT-991A at 0.36 and ×2. */
const IN_USE: Profile = { ...FT710, txLevel: 0.25, rxGain: 1.5 }
const OTHER: Profile = { ...FT991A, txLevel: 0.36, rxGain: 2 }
const levels = (p: Profile | Station) => ({ txLevel: p.txLevel, rxGain: p.rxGain })

describe('Tx Power and RX Gain belong to the radio being edited', () => {
  it('Tx Power, moved while editing a radio not in use, changes nothing live and Save stores it on that radio', async () => {
    installStation([IN_USE, OTHER])
    renderPanel({ txLevel: 0.25 })
    await openRadioTab()
    await editTheOtherRadio('FT-991A')
    expect(txPower().value).toBe('0.6')

    move(txPower(), '0.8')
    // The FT-710's live drive, which its cockpit Pwr slider shows, did not move.
    expect(levels(station)).toEqual(levels(IN_USE))
    expect(api.get('setTxLevel')).not.toHaveBeenCalled()

    await save()
    expect(levels(profile(1))).toEqual({ txLevel: 0.8 ** 2, rxGain: 2 })
    expect(levels(profile(0))).toEqual(levels(IN_USE))
  })

  it('RX Gain, moved while editing a radio not in use, changes nothing live and Save stores it on that radio', async () => {
    installStation([IN_USE, OTHER])
    renderPanel({ txLevel: 0.25 })
    await openRadioTab()
    await editTheOtherRadio('FT-991A')
    expect(rxGain().value).toBe('2')

    move(rxGain(), '3.5')
    expect(levels(station)).toEqual(levels(IN_USE))
    expect(api.get('setRxGain')).not.toHaveBeenCalled()

    await save()
    expect(levels(profile(1))).toEqual({ txLevel: 0.36, rxGain: 3.5 })
    expect(levels(profile(0))).toEqual(levels(IN_USE))
  })

  it("the drive of the radio in use does not follow into the form of the radio being edited", async () => {
    installStation([IN_USE, OTHER])
    const { rerender } = renderPanel({ txLevel: 0.25 })
    await openRadioTab()
    await editTheOtherRadio('FT-991A')
    expect(txPower().value).toBe('0.6')

    // The cockpit's Pwr slider moves the FT-710 to 0.81 while the FT-991A is open for Edit.
    await setTxLevel(0.81)
    rerender(panel({ txLevel: 0.81 }))
    await save()
    expect(profile(1).txLevel).toBe(0.36)
    expect(txPower().value).toBe('0.6')
  })

  it('a second Save of a radio not in use keeps a cockpit Pwr and RX gain change made after the first', async () => {
    installStation([IN_USE, OTHER])
    const { rerender } = renderPanel({ txLevel: 0.25 })
    await openRadioTab()
    await editTheOtherRadio('FT-991A')
    await save()

    // The FT-710's cockpit moves its drive to 0.81 and its RX gain to ×3.5 while the FT-991A is open.
    await setTxLevel(0.81)
    await setRxGain(3.5)
    rerender(panel({ txLevel: 0.81 }))
    move(txPower(), '0.7')
    await save()
    expect({ live: levels(station), inUse: levels(profile(0)), edited: levels(profile(1)) }).toEqual({
      live: { txLevel: 0.81, rxGain: 3.5 },
      inUse: { txLevel: 0.81, rxGain: 3.5 },
      edited: { txLevel: 0.7 ** 2, rxGain: 2 },
    })
  })

  it('after a Save of a radio not in use, a Save of the radio in use keeps a cockpit RX gain change made between', async () => {
    installStation([IN_USE, OTHER])
    renderPanel({ txLevel: 0.25 })
    await openRadioTab()
    await editTheOtherRadio('FT-991A')
    await save()

    // The FT-710's cockpit moves its RX gain to ×3.5; then the FT-710's own Edit, and Save.
    await setRxGain(3.5)
    await editTheOtherRadio('FT-710')
    await save()
    expect({ live: levels(station), inUse: levels(profile(0)) }).toEqual({
      live: { txLevel: 0.25, rxGain: 3.5 },
      inUse: { txLevel: 0.25, rxGain: 3.5 },
    })
  })

  it('a Save after Test CAT on a radio not in use keeps a cockpit Pwr change made after the test', async () => {
    installStation([IN_USE, OTHER])
    renderPanel({ txLevel: 0.25 })
    await openRadioTab()
    await editTheOtherRadio('FT-991A')
    await setTxLevel(0.81)
    // Test CAT saves the radio being edited first (it cannot test a radio that is not in use).
    fireEvent.click(screen.getByRole('button', { name: 'Test CAT' }))
    await waitFor(() => expect(api.get('updateRadioProfile')).toHaveBeenCalled())
    await waitFor(() =>
      expect((screen.getByRole('button', { name: 'Test CAT' }) as HTMLButtonElement).disabled).toBe(false),
    )

    await setTxLevel(0.9)
    await save()
    expect({ live: levels(station), inUse: levels(profile(0)) }).toEqual({
      live: { txLevel: 0.9, rxGain: 1.5 },
      inUse: { txLevel: 0.9, rxGain: 1.5 },
    })
  })

  it('control: editing the radio in use, both sliders act on it live and Tx Power follows its cockpit Pwr', async () => {
    installStation([IN_USE, OTHER])
    const { rerender } = renderPanel({ txLevel: 0.25 })
    await openRadioTab()
    await waitFor(() => expect(txPower().value).toBe('0.5'))

    move(txPower(), '0.8')
    expect(api.get('setTxLevel')).toHaveBeenLastCalledWith(0.8 ** 2)
    move(rxGain(), '3.5')
    expect(api.get('setRxGain')).toHaveBeenLastCalledWith(3.5)
    expect(levels(station)).toEqual({ txLevel: 0.8 ** 2, rxGain: 3.5 })

    rerender(panel({ txLevel: 0.81 }))
    expect(txPower().value).toBe('0.9')
  })
})

/** The hint under a slider: the `.settings-hint` in the slider's own `<label>`, as it reads. */
const hint = (slider: HTMLInputElement) =>
  slider.closest('label')?.querySelector('.settings-hint')?.textContent

/** A catalog sentence as it reads on screen, its markers rendered as elements. */
const reads = (key: 'settings.audio.txPower.hint' | 'settings.audio.rxGain.hint') =>
  EN[key].replace(/<\/?[a-z]+>/g, '')

describe('the hints under Tx Power and RX Gain', () => {
  it('while a radio not in use is open for Edit, both say that Save stores the level for that radio', async () => {
    installStation([IN_USE, OTHER])
    renderPanel({ txLevel: 0.25 })
    await openRadioTab()
    await editTheOtherRadio('FT-991A')
    const note =
      'Save stores this for FT-991A, and it takes effect when that radio is in use; the radio in use now does not change.'
    expect({ txPower: hint(txPower()), rxGain: hint(rxGain()) }).toEqual({ txPower: note, rxGain: note })
  })

  it('control: on the radio in use, each slider keeps its own hint', async () => {
    installStation([IN_USE, OTHER])
    renderPanel({ txLevel: 0.25 })
    await openRadioTab()
    await waitFor(() => expect(txPower().value).toBe('0.5'))
    expect({ txPower: hint(txPower()), rxGain: hint(rxGain()) }).toEqual({
      txPower: reads('settings.audio.txPower.hint'),
      rxGain: reads('settings.audio.rxGain.hint'),
    })
  })
})

// The transmit meters beside the drive slider: set the drive against the radio's ALC without leaving
// Settings. Read-only (the cockpits' TxMeters, nothing to press), and only for the radio in use, which
// is the one they measure: under another radio's Drive they would be a different radio's ALC.
/** The meters block that follows the drive slider's own field. */
const metersBesideDrive = () => {
  const field = txPower().closest('.settings-field')!
  const next = field.nextElementSibling
  return next?.querySelector('.ph-txmeters') ?? null
}

describe('the TX meters beside the drive slider', () => {
  it('on the radio in use, they follow the slider, ALC among them, live while it transmits', async () => {
    installStation([IN_USE, OTHER])
    renderPanel({ txLevel: 0.25, transmitting: true, txAlc: 0.4, txSwr: 1.2 })
    await openRadioTab()
    await waitFor(() => expect(metersBesideDrive()).not.toBeNull())
    const meters = metersBesideDrive()!
    const labels = [...meters.querySelectorAll('.ph-txmeter-label')].map((l) => l.textContent)
    expect(labels).toContain('ALC')
    expect(meters.textContent).toContain('40%')
    expect(meters.querySelector('button, input, select'), 'a readout: nothing in it to press').toBeNull()
  })

  it('while a radio not in use is open for Edit, they are not shown', async () => {
    installStation([IN_USE, OTHER])
    renderPanel({ txLevel: 0.25, transmitting: true, txAlc: 0.4 })
    await openRadioTab()
    await editTheOtherRadio('FT-991A')
    expect(metersBesideDrive()).toBeNull()
  })
})
