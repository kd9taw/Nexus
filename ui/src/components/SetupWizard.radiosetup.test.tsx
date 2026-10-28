// @vitest-environment jsdom
//
// THE FIRST-RUN SETUP AND "SET UP RADIOS" AGREE ON RADIO 1. The first-run setup's Finish writes its
// own choices for the radio it sets up (the radio in use) in one whole save, after anything the
// radio setup opened from it wrote. So a sound card or keying method changed for radio 1 in the
// radio setup must come back into the first-run page, or its Finish puts the page's older choice
// back over it. And the radio setup must open on the page's choices for radio 1, not on what was
// stored before them, or it shows a different sound card from the one the page shows. The backend
// here is a live one: a read returns what it holds now, the per-radio verb changes that radio.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor, within } from '@testing-library/react'
import { SetupWizard } from './SetupWizard'
import { ConfirmHost } from '../confirm'

vi.setConfig({ testTimeout: 15_000 })

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})
vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const mod: Record<string, unknown> = {}
  for (const name of Object.keys(actual)) {
    mod[name] = typeof actual[name] === 'function' ? api.get(name) : actual[name]
  }
  return mod
})

const RADIO = { enabled: true, baud: 38400, rigConn: 'serial', rigAddr: '', bands: [], pttMethod: 'vox', pttSerialPort: '' }
let station: Record<string, unknown> = {}

beforeEach(() => {
  for (const spy of Object.values(api.spies)) {
    spy.mockReset()
    spy.mockImplementation(() => Promise.resolve(null))
  }
  // Radio 1 stored with no sound cards and VOX: what a first run starts from.
  station = {
    mycall: 'KD9TAW',
    activeRadio: 0,
    audioIn: '',
    audioOut: '',
    pttMethod: 'vox',
    radios: [{ ...RADIO, id: 0, name: 'FTDX10', serialPort: 'COM5', rigModel: 1042, audioIn: '', audioOut: '' }],
    routingRules: [],
    defaultRadio: null,
    radioSwitchPolicy: 'act',
  }
  api.get('getSettings').mockImplementation(() => Promise.resolve(structuredClone(station)))
  api.get('addRadio').mockImplementation(() => {
    const radios = station.radios as Record<string, unknown>[]
    station = { ...station, radios: [...radios, { ...RADIO, id: 1, name: 'FT-991', serialPort: '', rigModel: 0, audioIn: '', audioOut: '' }] }
    return Promise.resolve(null)
  })
  api.get('updateRadioProfile').mockImplementation((id: number, patch: Record<string, unknown>) => {
    station = {
      ...station,
      radios: (station.radios as Record<string, unknown>[]).map((r) => (r.id === id ? { ...r, ...structuredClone(patch) } : r)),
    }
    return Promise.resolve(null)
  })
  api.get('getAudioDevices').mockImplementation(() =>
    Promise.resolve({ input: [{ name: 'CODEC A' }, { name: 'CODEC B' }], output: [{ name: 'CODEC A OUT' }, { name: 'CODEC B OUT' }] }),
  )
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('discoverFlex').mockImplementation(() => Promise.resolve([]))
  api.get('tciProbe').mockImplementation(() => Promise.resolve({ host: '127.0.0.1', resolved: true, tried: [], found: null }))
  api.get('detectRigs').mockImplementation(() =>
    Promise.resolve([
      { portName: 'COM5', suggestedModel: 1042, suggestedModelName: 'Yaesu FTDX10', suggestedAudio: 'CODEC A', suggestedAudioOut: 'CODEC A OUT', chip: 'CP210x', civSide: null, interfaceName: null, interfacePttMethod: null },
      { portName: 'COM7', suggestedModel: 1035, suggestedModelName: 'Yaesu FT-991', suggestedAudio: 'CODEC B', suggestedAudioOut: 'CODEC B OUT', chip: 'CP210x', civSide: null, interfaceName: null, interfacePttMethod: null },
    ]),
  )
  api.get('probeCatPorts').mockImplementation(() =>
    Promise.resolve({ found: true, portName: 'COM7', baud: 38400, model: 1035, modelName: 'Yaesu FT-991', freqMhz: 7.074, detail: 'FT-991 on COM7', modelSeeded: false }),
  )
})
afterEach(() => cleanup())

/** The first-run setup, on the rig step, with radio 1 picked (the FTDX10 row: CODEC A, CAT) and a
 *  second radio added; then "Set up radios" opened from it, at its Audio and keying step. */
async function openRadioSetupFromFirstRun(
  pick = async () => fireEvent.click(await screen.findByRole('button', { name: /FTDX10/ })),
  atNames?: (dialog: HTMLElement) => void,
) {
  const onApply = vi.fn()
  render(
    <>
      <SetupWizard settings={structuredClone(station) as never} onApply={onApply} onTestCat={vi.fn() as never} onSkip={vi.fn()} />
      <ConfirmHost />
    </>,
  )
  fireEvent.click(screen.getByRole('button', { name: /Next/ })) // → rig step
  await pick()
  fireEvent.click(await screen.findByRole('button', { name: /second radio/ }))
  fireEvent.click(await screen.findByRole('button', { name: /Set up which radio does what/ }))
  const dialog = await screen.findByRole('dialog', { name: 'Set up radios' })
  atNames?.(dialog)
  while (!within(dialog).queryByRole('heading', { name: 'Audio and keying' })) {
    fireEvent.click(within(dialog).getByRole('button', { name: 'Next' }))
  }
  return { onApply, dialog }
}
const radio1Tx = (dialog: HTMLElement) => within(dialog).getAllByRole('combobox', { name: 'Output Device (TX)' })[0] as HTMLSelectElement
const radio1Keying = (dialog: HTMLElement) => within(dialog).getAllByRole('combobox', { name: 'PTT Method' })[0] as HTMLSelectElement

describe("first run: radio 1's sound cards and keying in Set up radios", () => {
  it("the radio setup shows the first-run page's unsaved choices for radio 1", async () => {
    const { dialog } = await openRadioSetupFromFirstRun()
    // Stored: no sound card, VOX. The page has picked CODEC A OUT and CAT for it, unsaved.
    expect(radio1Tx(dialog).value).toBe('CODEC A OUT')
    expect(radio1Keying(dialog).value).toBe('cat')
  })

  it("a change made there to radio 1 is what the first-run Finish saves, not the page's older choice", async () => {
    const { onApply, dialog } = await openRadioSetupFromFirstRun()
    fireEvent.change(radio1Tx(dialog), { target: { value: 'CODEC B OUT' } })
    fireEvent.change(radio1Keying(dialog), { target: { value: 'vox' } })
    while (within(dialog).queryByRole('button', { name: 'Next' })) fireEvent.click(within(dialog).getByRole('button', { name: 'Next' }))
    fireEvent.click(within(dialog).getByRole('button', { name: 'Finish' }))
    await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Set up radios' })).toBeNull())
    const radio1 = (station.radios as Record<string, unknown>[])[0]
    expect([radio1.audioOut, radio1.pttMethod]).toEqual(['CODEC B OUT', 'vox'])
    // The first run's own Finish: its draft goes over the station whole (App's applyWizardDraft).
    fireEvent.click(screen.getByRole('button', { name: /Next/ })) // → log
    fireEvent.click(screen.getByRole('button', { name: /Next/ })) // → goals
    fireEvent.click(screen.getByRole('button', { name: /Finish — everything on/ }))
    const draft = onApply.mock.calls[0][4] as Record<string, unknown>
    expect(draft.audioOut).toBe('CODEC B OUT')
    // VOX is what the station held when the first run opened, so the draft leaves it out and the
    // stored VOX the radio setup wrote stays.
    expect(draft.pttMethod ?? station.pttMethod).toBe('vox')
    // Radio 1's receive card, not changed in the radio setup, is still the page's pick.
    expect(draft.audioIn).toBe('CODEC A')
  })

  // The page's unsaved rig model, port and connection for radio 1 are what the setup checks.
  const CAT_NO_MODEL = /PTT is set to CAT but the rig model is None\/VOX/
  const finishSetup = (dialog: HTMLElement) => {
    while (within(dialog).queryByRole('button', { name: 'Next' })) fireEvent.click(within(dialog).getByRole('button', { name: 'Next' }))
    fireEvent.click(within(dialog).getByRole('button', { name: 'Finish' }))
  }
  const storedRadio1 = (patch: Record<string, unknown>) => {
    station = { ...station, ...patch, radios: [{ ...(station.radios as Record<string, unknown>[])[0], ...patch }] }
  }

  it("CAT for radio 1 is not refused when the page has picked a model, though the stored one is None/VOX", async () => {
    // Stored: a port, model 0 and RTS keying (so picking the FTDX10 on the page keeps RTS).
    storedRadio1({ rigModel: 0, pttMethod: 'rts' })
    const { dialog } = await openRadioSetupFromFirstRun()
    expect(radio1Keying(dialog).value).toBe('rts')
    fireEvent.change(radio1Keying(dialog), { target: { value: 'cat' } })
    expect(within(dialog).queryByText(CAT_NO_MODEL)).toBeNull()
    finishSetup(dialog)
    await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Set up radios' })).toBeNull())
    expect((station.radios as Record<string, unknown>[])[0].pttMethod).toBe('cat')
  })

  it('CAT for radio 1 is refused when the page has taken its model off, though the stored one has a model', async () => {
    // Stored: TCI (its own model number) on COM5, RTS keying. The page goes back to USB / Serial,
    // which takes TCI's model number off, unsaved: model 0 on a serial port.
    storedRadio1({ rigConn: 'tci', rigModel: 7, serialPort: 'COM5', pttMethod: 'rts' })
    const { dialog } = await openRadioSetupFromFirstRun(async () =>
      fireEvent.click(await screen.findByRole('button', { name: /USB \/ Serial/ })),
    )
    fireEvent.change(radio1Keying(dialog), { target: { value: 'cat' } })
    finishSetup(dialog)
    expect((await within(dialog).findByRole('alert')).textContent).toMatch(CAT_NO_MODEL)
    expect((station.radios as Record<string, unknown>[])[0].pttMethod).toBe('rts')
    expect(api.spies.updateRadioProfile.mock.calls.some(([id]) => id === 0)).toBe(false)
  })

  it("a Finish refused before radio 1's write does not bring its change back into the page", async () => {
    // The rename of radio 2 lands, then radio 1's write is refused: its new sound card was not saved.
    const { onApply, dialog } = await openRadioSetupFromFirstRun(undefined, (d) =>
      fireEvent.change(within(d).getAllByRole('textbox')[1], { target: { value: 'FT-991 shack' } }),
    )
    api.get('updateRadioProfile').mockImplementation((id: number) =>
      id === 0 ? Promise.reject(new Error('the station refused it')) : Promise.resolve(null),
    )
    fireEvent.change(radio1Tx(dialog), { target: { value: 'CODEC B OUT' } })
    finishSetup(dialog)
    await within(dialog).findByText(/the station refused it/)
    expect(api.spies.renameRadio).toHaveBeenCalled()
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }))
    fireEvent.click(screen.getByRole('button', { name: /Next/ })) // → log
    fireEvent.click(screen.getByRole('button', { name: /Next/ })) // → goals
    fireEvent.click(screen.getByRole('button', { name: /Finish — everything on/ }))
    expect((onApply.mock.calls[0][4] as Record<string, unknown>).audioOut).toBe('CODEC A OUT')
  })

  it("positive control: a Finish refused after radio 1's write brings its saved change back into the page", async () => {
    const { onApply, dialog } = await openRadioSetupFromFirstRun()
    // Radio 1's write lands, then radio 2's is refused.
    api.get('updateRadioProfile').mockImplementation((id: number, patch: Record<string, unknown>) => {
      if (id !== 0) return Promise.reject(new Error('the station refused it'))
      station = {
        ...station,
        radios: (station.radios as Record<string, unknown>[]).map((r) => (r.id === id ? { ...r, ...structuredClone(patch) } : r)),
      }
      return Promise.resolve(null)
    })
    fireEvent.change(radio1Tx(dialog), { target: { value: 'CODEC B OUT' } })
    fireEvent.change(within(dialog).getAllByRole('combobox', { name: 'PTT Method' })[1], { target: { value: 'rts' } })
    finishSetup(dialog)
    await within(dialog).findByText(/the station refused it/)
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }))
    fireEvent.click(screen.getByRole('button', { name: /Next/ })) // → log
    fireEvent.click(screen.getByRole('button', { name: /Next/ })) // → goals
    fireEvent.click(screen.getByRole('button', { name: /Finish — everything on/ }))
    await waitFor(() => expect(onApply).toHaveBeenCalled())
    expect((onApply.mock.calls[0][4] as Record<string, unknown>).audioOut).toBe('CODEC B OUT')
  })
})
