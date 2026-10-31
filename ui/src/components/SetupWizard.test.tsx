// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { act, render, screen, fireEvent, waitFor, cleanup, within } from '@testing-library/react'
import { useState } from 'react'
import { SetupWizard } from './SetupWizard'
import { ConfirmHost } from '../confirm'
import * as api from '../api'
import { memoriesStore, emptyBank, addMemory } from '../features/memories'

vi.mock('../api', () => ({
  importAdif: vi.fn(),
  previewImport: vi.fn(),
  importLog: vi.fn(),
  detectRigs: vi.fn(() => Promise.resolve([])),
  discoverFlex: vi.fn(() => Promise.resolve([])),
  tciProbe: vi.fn(() => Promise.resolve({ host: '127.0.0.1', resolved: true, tried: [], found: null })),
  getAudioDevices: vi.fn(() => Promise.resolve({ input: [], output: [] })),
  getRigModels: vi.fn(() => Promise.resolve([])),
  getAllRigModels: vi.fn(() => Promise.resolve([])),
  probeCatPorts: vi.fn(() => Promise.resolve({ found: false, detail: 'no rig answered' })),
  addRadio: vi.fn(() => Promise.resolve(null)),
  getSettings: vi.fn(() => Promise.resolve({ radios: [] })),
  updateRadioProfile: vi.fn(() => Promise.resolve(null)),
}))

const importAdif = api.importAdif as ReturnType<typeof vi.fn>
const previewImport = api.previewImport as ReturnType<typeof vi.fn>
const importLog = api.importLog as ReturnType<typeof vi.fn>
/** What the import preview reads off a file of `contacts` contacts, none in the log yet. */
const preview = (contacts: number) => ({
  format: 'adif', template: null, records: contacts, contacts, firstUnix: null, lastUnix: null,
  new: contacts, duplicates: 0, updates: 0, notes: [], coverage: [], unmapped: [],
})
/** What the import answers: its counts, and a report that accounts for every field. */
const done = (added: number, skipped: number) => ({
  stats: { added, skipped, updated: 0, total: added + skipped, run: 'r1' },
  report: { ...preview(added + skipped), added, duplicates: skipped, updated: 0, newOnly: false, fields: [], accounted: true },
})
/** Import, then Close, in the preview the picked file opened. */
async function importInPreview() {
  fireEvent.click(await screen.findByRole('button', { name: /^Import$/ }))
  fireEvent.click(await screen.findByRole('button', { name: /^Close$/ }))
}

/** `guide: true` supplies onOpenGuide, which is what makes the last step offer
 *  the Getting started walkthrough. Omitted, the offer must not appear at all.
 *  `settings` is what the wizard prefills from and diffs its draft against.
 *  `onTestCat` / `onProveTx` stand in for App's; the confirm host is App's too, so Prove TX
 *  can ask its question here as it does there. */
function renderWizard(
  opts: {
    guide?: boolean
    settings?: Record<string, unknown>
    onTestCat?: ReturnType<typeof vi.fn>
    onProveTx?: () => void
  } = {},
) {
  const onApply = vi.fn()
  const onSkip = vi.fn()
  const onOpenGuide = vi.fn()
  const onTestCat = opts.onTestCat ?? vi.fn(() => Promise.resolve({} as never))
  render(
    <>
      <SetupWizard
        settings={(opts.settings ?? null) as never}
        onApply={onApply}
        onTestCat={onTestCat as never}
        onProveTx={opts.onProveTx}
        onSkip={onSkip}
        onOpenGuide={opts.guide ? onOpenGuide : undefined}
      />
      <ConfirmHost />
    </>,
  )
  return { onApply, onSkip, onOpenGuide, onTestCat }
}

const clickNext = () => fireEvent.click(screen.getByRole('button', { name: /Next/ }))
function gotoLogStep() {
  clickNext() // 0 station → 1 rig
  clickNext() // 1 rig → 2 log
}
function fireImport(content: string) {
  const input = document.querySelector('input[type="file"]') as HTMLInputElement
  fireEvent.change(input, {
    target: { files: [new File([content], 'log.adi', { type: 'text/plain' })] },
  })
}

describe('SetupWizard ADIF import step', () => {
  // Braces are load-bearing: a concise arrow RETURNS the mock, and vitest calls a
  // hook's return value as its teardown — an unhandled rejection the moment it rejects.
  beforeEach(() => {
    importAdif.mockReset()
    previewImport.mockReset()
    importLog.mockReset()
  })

  it('renders the optional log step and imports a log through the preview, reporting the count', async () => {
    previewImport.mockResolvedValue(preview(6))
    importLog.mockResolvedValue(done(5, 1))
    renderWizard()
    gotoLogStep()
    expect(screen.getByText(/Bring in your existing log/)).toBeTruthy()
    fireImport('<call:5>K1ABC<eor>')
    await importInPreview()
    expect(importLog).toHaveBeenCalledTimes(1)
    const result = await screen.findByText(/Imported/)
    expect(result.textContent).toContain('5')
    expect(result.textContent).toMatch(/seeded/)
  })

  it('treats a 0-QSO import as a warning, not a false "seeded" success', async () => {
    previewImport.mockResolvedValue(preview(1))
    importLog.mockResolvedValue(done(0, 0))
    renderWizard()
    gotoLogStep()
    fireImport('this is not an ADIF file')
    await importInPreview()
    expect(await screen.findByText(/No QSOs found/)).toBeTruthy()
    expect(screen.queryByText(/now seeded/)).toBeNull()
  })

  it('is skippable — Next advances to goals without importing', () => {
    renderWizard()
    gotoLogStep()
    clickNext() // 2 log → 3 goals, no file chosen
    expect(screen.getByText(/You get everything/)).toBeTruthy()
    expect(importAdif).not.toHaveBeenCalled()
    expect(importLog).not.toHaveBeenCalled()
  })
})

describe('SetupWizard rig-step pipeline (setup re-envisioning, 2026-08-09)', () => {
  beforeEach(() => {
    cleanup()
    vi.mocked(api.detectRigs).mockClear()
    vi.mocked(api.probeCatPorts).mockReset()
    vi.mocked(api.probeCatPorts).mockResolvedValue({
      found: false,
      detail: 'no rig answered',
    } as never)
  })

  it('scans on entering the rig step — the operator presses nothing', async () => {
    // "Nothing is working" was the literal output of an empty rig screen whose only
    // affordance was a button the operator had to know to press.
    renderWizard()
    expect(api.detectRigs).not.toHaveBeenCalled()
    clickNext() // 0 station → 1 rig
    await waitFor(() => expect(api.detectRigs).toHaveBeenCalledTimes(1))
  })

  it('a seeded Auto-test hit applies port+baud but DEMANDS the exact model', async () => {
    // An FT-991A answers the FTDX10 seed: the port and baud are proven, the model is a
    // guess — persisting the guess silently is what Settings refuses; the wizard asks.
    vi.mocked(api.probeCatPorts).mockResolvedValue({
      found: true,
      portName: 'COM5',
      baud: 38400,
      model: 1042,
      modelName: 'Yaesu FTDX10',
      freqMhz: 14.074,
      detail: 'Found the port: COM5 @ 38400 baud',
      modelSeeded: true,
    } as never)
    renderWizard()
    clickNext()
    fireEvent.click(await screen.findByRole('button', { name: /Auto-test my ports/ }))
    await screen.findByText(/the exact model is a guess/)
    expect(screen.getByText(/Selected: .*COM5 @ 38400 baud/)).toBeTruthy()
  })

  it('confirming a fixed-rate model sets its one true baud (the Settings rule)', async () => {
    vi.mocked(api.getRigModels).mockResolvedValue([[3088, 'Xiegu G90']] as never)
    const { onApply } = renderWizard()
    clickNext() // → rig step
    // Probe finds the port with a guessed model; the operator then confirms a G90.
    vi.mocked(api.probeCatPorts).mockResolvedValue({
      found: true,
      portName: 'COM9',
      baud: 38400,
      model: 1042,
      modelName: 'Yaesu FTDX10',
      freqMhz: 14.074,
      detail: 'found',
      modelSeeded: true,
    } as never)
    fireEvent.click(await screen.findByRole('button', { name: /Auto-test my ports/ }))
    const select = (await screen.findByLabelText(/Which radio is this/)) as HTMLSelectElement
    fireEvent.change(select, { target: { value: '3088' } })
    expect(screen.getByText(/Selected: .*COM9 @ 19200 baud/)).toBeTruthy()
    // The draft carries the confirmed model, its fixed baud, and CAT keying — the
    // silent-VOX default is dead on every wizard-configured rig.
    clickNext() // → log
    clickNext() // → goals
    fireEvent.click(screen.getByRole('button', { name: /Finish — everything on/ }))
    const draft = onApply.mock.calls[0][4]
    expect(draft.rigModel).toBe(3088)
    expect(draft.baud).toBe(19200)
    expect(draft.pttMethod).toBe('cat')
    expect(draft.serialPort).toBe('COM9')
  })
  it('reaches an extended-tier rig — the curated list is not the whole catalog', async () => {
    // The IC-7600 was reported missing and was in the catalog the whole time, in the
    // extended tier. Settings has always had a box to reveal that tier; the wizard had
    // none, so here those models were not hidden — they were UNREACHABLE.
    vi.mocked(api.getRigModels).mockResolvedValue([[1042, 'Yaesu FTDX10']] as never)
    vi.mocked(api.getAllRigModels).mockResolvedValue([
      [1042, 'Yaesu FTDX10'],
      [3063, 'Icom IC-7600'],
    ] as never)
    renderWizard()
    clickNext()
    vi.mocked(api.probeCatPorts).mockResolvedValue({
      found: true,
      portName: 'COM5',
      baud: 38400,
      model: 1042,
      modelName: 'Yaesu FTDX10',
      freqMhz: 14.074,
      detail: 'found',
      modelSeeded: true,
    } as never)
    fireEvent.click(await screen.findByRole('button', { name: /Auto-test my ports/ }))
    const select = (await screen.findByLabelText(/Which radio is this/)) as HTMLSelectElement

    // Unticked, the curated tier is still the default — the box does not silently widen
    // the list. The FTDX10 assertion is this test's own positive control: it proves the
    // select is POPULATED, so "IC-7600 absent" cannot pass on an empty dropdown.
    expect(within(select).getByText('Yaesu FTDX10')).toBeTruthy()
    expect(within(select).queryByText('Icom IC-7600')).toBeNull()

    fireEvent.click(screen.getByLabelText(/Show all Hamlib rig models/))
    await waitFor(() => expect(within(select).getByText('Icom IC-7600')).toBeTruthy())
    // ...and the curated entry survives the widening rather than being replaced.
    expect(within(select).getByText('Yaesu FTDX10')).toBeTruthy()
  })

})

describe('SetupWizard second-radio card', () => {
  beforeEach(() => {
    cleanup()
    vi.mocked(api.detectRigs).mockReset()
    vi.mocked(api.probeCatPorts).mockReset()
    vi.mocked(api.addRadio).mockClear()
    vi.mocked(api.updateRadioProfile).mockClear()
  })

  it('adds, probes with radio 1 excluded, and writes radio 2 BY ID with its OWN codec', async () => {
    // The scan's 1:1 codec pass gave each port its own device; the card must write the
    // NEW port's grant to the NEW profile — never radio 1's — through the by-id verb
    // (updateRadioProfile), which is what dodges the Edit-vs-Active trap.
    vi.mocked(api.detectRigs).mockResolvedValue([
      {
        portName: 'COM5',
        suggestedModel: 1042,
        suggestedModelName: 'Yaesu FTDX10',
        suggestedAudio: 'CODEC A',
        suggestedAudioOut: 'CODEC A OUT',
        chip: 'CP210x',
        civSide: null,
        interfaceName: null,
        interfacePttMethod: null,
      },
      {
        portName: 'COM7',
        suggestedModel: 1035,
        suggestedModelName: 'Yaesu FT-991',
        suggestedAudio: 'CODEC B',
        suggestedAudioOut: 'CODEC B OUT',
        chip: 'CP210x',
        civSide: null,
        interfaceName: null,
        interfacePttMethod: null,
      },
    ] as never)
    vi.mocked(api.getSettings).mockResolvedValue({
      radios: [
        { id: 0, serialPort: 'COM5', baud: 38400 },
        { id: 1, serialPort: '', baud: 38400 },
      ],
    } as never)
    vi.mocked(api.probeCatPorts).mockResolvedValue({
      found: true,
      portName: 'COM7',
      baud: 38400,
      model: 1035,
      modelName: 'Yaesu FT-991',
      freqMhz: 7.074,
      detail: 'FT-991 on COM7',
      modelSeeded: false,
    } as never)
    renderWizard()
    clickNext() // → rig step; scan-on-entry populates the roster rows
    // Radio 1: pick the FTDX10 row so the card has a first radio to exclude.
    fireEvent.click(await screen.findByRole('button', { name: /FTDX10/ }))
    fireEvent.click(await screen.findByRole('button', { name: /second radio/ }))
    await waitFor(() => expect(api.updateRadioProfile).toHaveBeenCalledTimes(1))
    expect(api.addRadio).toHaveBeenCalledTimes(1)
    const [id, patch] = vi.mocked(api.updateRadioProfile).mock.calls[0]
    expect(id).toBe(1)
    expect(patch.serialPort).toBe('COM7')
    expect(patch.pttMethod).toBe('cat')
    expect(patch.audioIn).toBe('CODEC B')
    expect(patch.audioOut).toBe('CODEC B OUT')
    // Radio 1's codec must NOT have been given to radio 2.
    expect(patch.audioIn).not.toBe('CODEC A')
    await screen.findByText(/Second radio: .*FT-991 on COM7/)
    // The run-both-at-once fact, surfaced at last (it lived only in a picker subtitle).
    expect(screen.getByText(/open Nexus twice/)).toBeTruthy()
  })
})

describe('SetupWizard keying on a radio Hamlib cannot key over CAT', () => {
  // Thirteen radios in the catalog answer CAT keying without keying: the bundled Hamlib says
  // `Can set PTT: N` for each (NO_CAT_PTT_MODELS), and an IC-706MKIIG answers `T 1` with RPRT 0
  // while it stays on receive. The wizard switched PTT to CAT for any rig it could name, so Nexus
  // showed TX and nothing went out. The draft holds only what changed: a draft with no
  // `pttMethod` leaves PTT as saved (VOX when nothing is).
  //
  // The twelve transceivers among them also ask how the interface keys the radio, and the wizard
  // cannot finish until that is answered. So the tests below that stop at a pick read the draft
  // the way Test CAT sends it (the same `draft()` Finish sends): nothing has been picked for the
  // keying yet, so a `pttMethod` there could only have come from the model pick itself.
  const BRIDGE = {
    portName: 'COM4',
    suggestedModel: null,
    suggestedModelName: null,
    suggestedAudio: null,
    suggestedAudioOut: null,
    chip: 'FT232R',
    civSide: null,
    interfaceName: null,
    interfacePttMethod: null,
  }
  const DIGIRIG = {
    ...BRIDGE,
    portName: 'COM6',
    chip: 'CP2102',
    interfaceName: 'Digirig Mobile',
    interfacePttMethod: 'rts',
  }
  /** A trusted Auto-test hit (the model a radio's saved settings name). */
  const hit = (portName: string, model: number, modelName: string) => ({
    found: true,
    portName,
    baud: 19200,
    model,
    modelName,
    freqMhz: 7.074,
    detail: 'the probe answered',
    modelSeeded: false,
  })

  beforeEach(() => {
    cleanup()
    vi.mocked(api.getRigModels).mockResolvedValue([
      [3011, 'Icom IC-706MKIIG'],
      [3073, 'Icom IC-7300'],
      [2049, 'Malachite DSP SDR'],
    ] as never)
    vi.mocked(api.detectRigs).mockResolvedValue([BRIDGE] as never)
    vi.mocked(api.probeCatPorts).mockReset()
    vi.mocked(api.probeCatPorts).mockResolvedValue({
      found: false,
      detail: 'no rig answered',
    } as never)
    vi.mocked(api.updateRadioProfile).mockClear()
  })

  /** Enter the rig step and click the scanned row on `port`. A bare bridge chip chains into
   *  Auto-test, which finds nothing here unless a test says otherwise. */
  async function chooseRow(port: RegExp) {
    clickNext() // → rig step; the scan on entry lists the rows
    fireEvent.click(await screen.findByRole('button', { name: port }))
  }
  async function pick(model: number) {
    const select = (await screen.findByLabelText(/Which radio is this/)) as HTMLSelectElement
    // The catalog loads after mount; a change to a value with no option would pick model 0.
    await waitFor(() => expect(within(select).getByText('Icom IC-706MKIIG')).toBeTruthy())
    fireEvent.change(select, { target: { value: String(model) } })
  }
  function finishDraft(onApply: ReturnType<typeof vi.fn>) {
    clickNext() // → log
    clickNext() // → goals
    fireEvent.click(screen.getByRole('button', { name: /Finish — everything on/ }))
    return onApply.mock.calls[0][4]
  }
  /** The draft as Test CAT sends it, for a radio the wizard will not finish on until the keying
   *  is picked. */
  async function testCatDraft(onTestCat: ReturnType<typeof vi.fn>) {
    fireEvent.click(screen.getByRole('button', { name: /Test CAT/ }))
    await waitFor(() => expect(onTestCat).toHaveBeenCalledTimes(1))
    return onTestCat.mock.calls[0][0]
  }

  it('a pick of the IC-706MKIIG leaves PTT on VOX', async () => {
    const { onTestCat } = renderWizard()
    await chooseRow(/COM4/)
    await pick(3011)
    const draft = await testCatDraft(onTestCat)
    expect(draft.rigModel).toBe(3011)
    expect(draft.pttMethod).toBeUndefined()
  })

  it('positive control: a pick of the IC-7300 still sets CAT', async () => {
    const { onApply } = renderWizard()
    await chooseRow(/COM4/)
    await pick(3073)
    const draft = finishDraft(onApply)
    expect(draft.rigModel).toBe(3073)
    expect(draft.pttMethod).toBe('cat')
  })

  it('correcting a pick to the IC-706MKIIG undoes the CAT the first pick chose', async () => {
    const { onTestCat } = renderWizard()
    await chooseRow(/COM4/)
    await pick(3073)
    await pick(3011)
    const draft = await testCatDraft(onTestCat)
    expect(draft.rigModel).toBe(3011)
    expect(draft.pttMethod).toBeUndefined()
  })

  it('a saved CAT stays: an interface that keys on audio (a SignaLink) transmits on CAT', async () => {
    const { onTestCat } = renderWizard({ settings: { pttMethod: 'cat' } })
    await chooseRow(/COM4/)
    await pick(3011)
    const draft = await testCatDraft(onTestCat)
    expect(draft.rigModel).toBe(3011)
    // Unchanged from the saved CAT. Rewritten to VOX, the draft would carry `pttMethod: 'vox'`.
    expect(draft.pttMethod).toBeUndefined()
  })

  it("a recognised interface's RTS still wins when Auto-test finds the IC-706MKIIG", async () => {
    vi.mocked(api.detectRigs).mockResolvedValue([DIGIRIG] as never)
    const { onApply } = renderWizard()
    await chooseRow(/COM6/)
    vi.mocked(api.probeCatPorts).mockResolvedValue(hit('COM6', 3011, 'Icom IC-706MKIIG') as never)
    fireEvent.click(screen.getByRole('button', { name: /Auto-test my ports/ }))
    await screen.findByText('the probe answered')
    const draft = finishDraft(onApply)
    expect(draft.rigModel).toBe(3011)
    expect(draft.pttMethod).toBe('rts')
  })

  it('a scanned row that names the IC-706MKIIG leaves PTT on VOX', async () => {
    // None of these radios names itself over USB today; the rule holds if one ever does.
    vi.mocked(api.detectRigs).mockResolvedValue([
      { ...BRIDGE, suggestedModel: 3011, suggestedModelName: 'Icom IC-706MKIIG' },
    ] as never)
    const { onTestCat } = renderWizard()
    await chooseRow(/COM4/)
    const draft = await testCatDraft(onTestCat)
    expect(draft.rigModel).toBe(3011)
    expect(draft.pttMethod).toBeUndefined()
  })

  it("a second radio Auto-test finds as the IC-706MKIIG keeps its profile's VOX", async () => {
    vi.mocked(api.detectRigs).mockResolvedValue([
      { ...BRIDGE, portName: 'COM5', suggestedModel: 1042, suggestedModelName: 'Yaesu FTDX10' },
      BRIDGE,
    ] as never)
    vi.mocked(api.getSettings).mockResolvedValue({
      radios: [
        { id: 0, serialPort: 'COM5', baud: 38400, pttMethod: 'cat' },
        { id: 1, serialPort: '', baud: 38400, pttMethod: 'vox' },
      ],
    } as never)
    vi.mocked(api.probeCatPorts).mockResolvedValue(hit('COM4', 3011, 'Icom IC-706MKIIG') as never)
    renderWizard()
    await chooseRow(/FTDX10/)
    fireEvent.click(await screen.findByRole('button', { name: /second radio/ }))
    await waitFor(() => expect(api.updateRadioProfile).toHaveBeenCalledTimes(1))
    const [id, patch] = vi.mocked(api.updateRadioProfile).mock.calls[0]
    expect(id).toBe(1)
    expect(patch.rigModel).toBe(3011)
    expect(patch.pttMethod).toBe('vox')
  })

  // ── How the interface keys the radio: the twelve transceivers among the thirteen ──────────
  // For these the wizard asks VOX, Serial RTS or Serial DTR, says in a line what each means, and
  // puts Prove TX beside the answer. The Malachite DSP can set no PTT either, but it is a receiver.
  const NOTE = /^Hamlib has no way to key this radio over CAT\./
  const keying = () => screen.queryByRole('group', { name: 'How does your interface key the radio?' })
  const option = (name: RegExp) => within(keying()!).getByRole('button', { name })
  /** The options shown as picked, by their text. */
  const pressed = () =>
    within(keying()!)
      .getAllByRole('button')
      .filter((b) => b.getAttribute('aria-pressed') === 'true')
      .map((b) => b.textContent)

  it('asks for the IC-706MKIIG, not for the IC-7300 (Hamlib keys it) or the Malachite DSP (a receiver)', async () => {
    renderWizard()
    await chooseRow(/COM4/)
    await pick(3011)
    expect(keying()).not.toBeNull()
    await pick(3073)
    expect(keying()).toBeNull()
    await pick(2049)
    expect(keying()).toBeNull()
    // Control: the same select brings it back, so each absence above is the model's doing.
    await pick(3011)
    expect(keying()).not.toBeNull()
  })

  it.each([
    [/Serial RTS/, 'rts'],
    [/Serial DTR/, 'dtr'],
    [/VOX/, 'vox'],
  ])('picking %s drafts %s', async (name, value) => {
    // Saved on CAT, so every pick disagrees with what is saved, and RTS and DTR with the default.
    const { onApply } = renderWizard({ settings: { pttMethod: 'cat' } })
    await chooseRow(/COM4/)
    await pick(3011)
    expect(pressed()).toEqual([])
    fireEvent.click(option(name))
    expect(option(name).getAttribute('aria-pressed')).toBe('true')
    expect(finishDraft(onApply).pttMethod).toBe(value)
  })

  it('a recognised Digirig pre-picks RTS on the IC-706MKIIG, and the wizard can finish', async () => {
    vi.mocked(api.detectRigs).mockResolvedValue([DIGIRIG] as never)
    // Saved on DTR: the RTS below can only have come from the interface.
    const { onApply } = renderWizard({ settings: { pttMethod: 'dtr' } })
    await chooseRow(/COM6/)
    await pick(3011)
    expect(pressed()).toEqual([expect.stringMatching(/^Serial RTS/)])
    expect(screen.getByText(NOTE).getAttribute('role')).toBeNull()
    expect(finishDraft(onApply).pttMethod).toBe('rts')
  })

  it('cannot finish on the IC-706MKIIG until the keying is picked', async () => {
    // A bare bridge chip: no recognised interface answers for the owner, and VOX is only the
    // default, which is no answer.
    const { onApply } = renderWizard()
    await chooseRow(/COM4/)
    await pick(3011)
    expect(pressed()).toEqual([])
    // Required reading, as a guessed model is: the note is an alert and nothing moves on.
    expect(screen.getByText(NOTE).getAttribute('role')).toBe('alert')
    const next = () => screen.getByRole('button', { name: /Next/ }) as HTMLButtonElement
    const dots = () => [...document.querySelectorAll<HTMLButtonElement>('.wizard-dot')]
    expect(next().disabled).toBe(true)
    expect(dots().map((d) => d.disabled)).toEqual([false, false, true, true])
    // Nor from the step before: the dots still stop at the radio step.
    fireEvent.click(screen.getByRole('button', { name: /Back/ }))
    expect(dots().map((d) => d.disabled)).toEqual([false, false, true, true])
    fireEvent.click(dots()[3])
    expect(screen.queryByRole('button', { name: /Finish — everything on/ })).toBeNull()
    expect(onApply).not.toHaveBeenCalled()
    // Control: one pick, and the same wizard finishes. VOX was already the setting, so nothing is
    // rewritten.
    fireEvent.click(dots()[1])
    fireEvent.click(option(/VOX/))
    expect(next().disabled).toBe(false)
    const draft = finishDraft(onApply)
    expect(draft.rigModel).toBe(3011)
    expect(draft.pttMethod).toBeUndefined()
  })

  it('a saved RTS shows as picked, with its keying port, and stays as saved', async () => {
    const { onApply } = renderWizard({ settings: { pttMethod: 'rts', pttSerialPort: 'COM9' } })
    await chooseRow(/COM4/)
    await pick(3011)
    expect(pressed()).toEqual([expect.stringMatching(/^Serial RTS/)])
    expect((screen.getByLabelText(/PTT Serial Port/) as HTMLInputElement).value).toBe('COM9')
    const draft = finishDraft(onApply)
    expect(draft.rigModel).toBe(3011)
    // Neither is rewritten: a draft carries only what changed.
    expect(draft.pttMethod).toBeUndefined()
    expect(draft.pttSerialPort).toBeUndefined()
  })

  it('Serial RTS or DTR keys on the CAT port unless another port is picked', async () => {
    const { onApply } = renderWizard({ settings: { pttMethod: 'vox', pttSerialPort: '' } })
    await chooseRow(/COM4/)
    await pick(3011)
    expect(screen.queryByLabelText(/PTT Serial Port/)).toBeNull() // VOX keys no line
    fireEvent.click(option(/Serial DTR/))
    const port = screen.getByLabelText(/PTT Serial Port/) as HTMLInputElement
    expect(port.value).toBe('')
    // The words only: no sample port (it showed "e.g. COM16 — …", a value that reads as filled in).
    expect(port.placeholder).toBe('blank = use the CAT port')
    fireEvent.change(port, { target: { value: 'COM9' } })
    const draft = finishDraft(onApply)
    expect(draft.pttMethod).toBe('dtr')
    expect(draft.pttSerialPort).toBe('COM9')
  })

  /** A stand-in for App: `onTestCat` saves the draft it is given, as App's does, and `onProveTx`
   *  notes the method saved at the moment it is called, which is the moment App keys the tune
   *  carrier: the radio loop keys with what is saved. */
  function station(saved: Record<string, unknown>) {
    const keyedWith: unknown[] = []
    const onTestCat = vi.fn(async (d: Record<string, unknown>) => {
      Object.assign(saved, d)
      return { ok: true, detail: 'the radio answered' }
    })
    const onProveTx = vi.fn(() => {
      keyedWith.push(saved.pttMethod)
    })
    return { keyedWith, onTestCat, onProveTx }
  }
  async function proveTx(from: HTMLElement) {
    fireEvent.click(within(from).getByRole('button', { name: 'Prove TX' }))
    const ask = await screen.findByRole('dialog', { name: 'Prove the transmit path?' })
    return () => fireEvent.click(within(ask).getByRole('button', { name: 'Prove TX' }))
  }

  it('Prove TX beside the choice keys with the method just picked', async () => {
    const st = station({ pttMethod: 'vox' })
    renderWizard({ settings: { pttMethod: 'vox' }, onTestCat: st.onTestCat, onProveTx: st.onProveTx })
    await chooseRow(/COM4/)
    await pick(3011)
    fireEvent.click(option(/Serial RTS/))
    const agree = await proveTx(keying()!)
    // Nothing is saved or keyed on the ask.
    expect(st.onTestCat).not.toHaveBeenCalled()
    expect(st.onProveTx).not.toHaveBeenCalled()
    agree()
    await waitFor(() => expect(st.onProveTx).toHaveBeenCalledTimes(1))
    expect(st.keyedWith).toEqual(['rts'])
  })

  it("the health strip's Prove TX keys with the wizard's draft too", async () => {
    // An IC-7300 picked: the wizard drafts CAT over the saved VOX, and the strip has to key that.
    const st = station({ pttMethod: 'vox' })
    renderWizard({ settings: { pttMethod: 'vox' }, onTestCat: st.onTestCat, onProveTx: st.onProveTx })
    await chooseRow(/COM4/)
    await pick(3073)
    const agree = await proveTx(screen.getByRole('status', { name: 'Setup health' }))
    agree()
    await waitFor(() => expect(st.onProveTx).toHaveBeenCalledTimes(1))
    expect(st.keyedWith).toEqual(['cat'])
  })

  it('Prove TX keys nothing when saving the pick fails', async () => {
    const onTestCat = vi.fn(() => Promise.reject(new Error('settings write failed')))
    const onProveTx = vi.fn()
    renderWizard({ onTestCat, onProveTx })
    await chooseRow(/COM4/)
    await pick(3011)
    fireEvent.click(option(/Serial DTR/))
    const agree = await proveTx(keying()!)
    agree()
    await screen.findByText(/settings write failed/)
    expect(onTestCat).toHaveBeenCalledTimes(1)
    expect(onProveTx).not.toHaveBeenCalled()
  })
})

describe('SetupWizard starter-pack offer', () => {
  const gotoGoals = () => {
    clickNext() // 0 → 1
    clickNext() // 1 → 2
    clickNext() // 2 → 3 goals
  }

  // Unmount any prior render (this file relies on auto-cleanup that isn't registered) so
  // the step-3 headings — which recur in every render — resolve to a single element.
  beforeEach(() => {
    cleanup()
    memoriesStore.set(emptyBank()) // first-run: a blank bank
  })

  it('offers packs on first run and seeds the pre-checked ones on completion', () => {
    const { onApply } = renderWizard()
    gotoGoals()
    expect(screen.getByText(/Start with some channels/)).toBeTruthy()
    // "Turn everything on (expert)" completes setup (no goal selection needed) — it must
    // seed the packs checked by default (VHF/UHF Calling + HF Digital).
    fireEvent.click(screen.getByRole('button', { name: /Finish — everything on/ }))
    expect(onApply).toHaveBeenCalledTimes(1)
    const mems = memoriesStore.get().memories
    expect(mems.some((m) => m.rxMhz === 146.52)).toBe(true) // na-calling: 2m FM Calling
    expect(mems.some((m) => m.rxMhz === 14.074 && m.mode === 'FT8')).toBe(true) // na-digital
    // POTA wasn't pre-checked, so its SSB-only channels shouldn't appear.
    expect(mems.some((m) => m.rxMhz === 14.285)).toBe(false)
  })

  it('seeds nothing when the operator skips setup', () => {
    const { onSkip } = renderWizard()
    gotoGoals()
    fireEvent.click(screen.getByRole('button', { name: /set it up myself/ }))
    expect(onSkip).toHaveBeenCalledTimes(1)
    expect(memoriesStore.get().memories).toHaveLength(0)
  })

  it('hides the offer once the operator already has memories (re-open never re-adds)', () => {
    memoriesStore.set(addMemory(emptyBank(), { rxMhz: 146.52, mode: 'FM' }))
    renderWizard()
    clickNext()
    clickNext()
    clickNext()
    expect(screen.getByText(/You get everything/)).toBeTruthy() // on the goals step
    expect(screen.queryByText(/Start with some channels/)).toBeNull()
  })

  it('finishes with the everything profile — no goal or mode questions asked', () => {
    // The goal/mode pickers are gone (operator, 2026-08-09: "start with it all"): the
    // finish applies the everything profile, so every mode is on without being asked for.
    const { onApply } = renderWizard()
    gotoGoals()
    expect(screen.queryByText(/What do you mostly want to do/)).toBeNull()
    expect(screen.queryByText(/Which modes do you operate/)).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: /Finish — everything on/ }))
    expect(onApply).toHaveBeenCalledTimes(1)
    expect(onApply.mock.calls[0][0]).toEqual(['everything'])
  })
})

// The walkthrough is offered "instead of dismissing": the wizard's last step can
// hand the operator the Getting started guide for what they just set up. Both
// completion paths go through one `finish`, so both must honour the offer — and
// neither may open the guide unasked, which is the whole point of it being an
// offer. Skipping setup is not a completion and must open nothing.
describe('SetupWizard walkthrough offer', () => {
  const gotoGoals = () => {
    clickNext() // 0 → 1
    clickNext() // 1 → 2
    clickNext() // 2 → 3 goals
  }

  beforeEach(() => {
    cleanup()
    memoriesStore.set(emptyBank())
  })

  it('is not offered at all when the host cannot open the guide', () => {
    renderWizard()
    gotoGoals()
    expect(screen.queryByText(/Want a walkthrough/)).toBeNull()
  })

  it('stays quiet unless the operator asks for it', () => {
    const { onApply, onOpenGuide } = renderWizard({ guide: true })
    gotoGoals()
    expect(screen.getByText(/Want a walkthrough/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /^Show me Getting started/ })) // on
    fireEvent.click(screen.getByRole('button', { name: /^Show me Getting started/ })) // off again
    fireEvent.click(screen.getByRole('button', { name: /Finish — everything on/ }))
    expect(onApply).toHaveBeenCalledTimes(1)
    expect(onOpenGuide).not.toHaveBeenCalled()
  })

  it('opens the guide after applying, on the goal path', () => {
    const { onApply, onOpenGuide } = renderWizard({ guide: true })
    gotoGoals()
    fireEvent.click(screen.getByRole('button', { name: /^Show me Getting started/ }))
    fireEvent.click(screen.getByRole('button', { name: /Finish — everything on/ }))
    expect(onApply).toHaveBeenCalledTimes(1)
    expect(onOpenGuide).toHaveBeenCalledTimes(1)
  })

  it('opens the guide on the "turn everything on" path too', () => {
    const { onApply, onOpenGuide } = renderWizard({ guide: true })
    gotoGoals()
    fireEvent.click(screen.getByRole('button', { name: /^Show me Getting started/ }))
    fireEvent.click(screen.getByRole('button', { name: /Finish — everything on/ }))
    expect(onApply).toHaveBeenCalledTimes(1)
    expect(onOpenGuide).toHaveBeenCalledTimes(1)
  })

  it('opens nothing when the operator skips setup instead of finishing it', () => {
    const { onSkip, onApply, onOpenGuide } = renderWizard({ guide: true })
    gotoGoals()
    fireEvent.click(screen.getByRole('button', { name: /^Show me Getting started/ }))
    fireEvent.click(screen.getByRole('button', { name: /set it up myself/ }))
    expect(onSkip).toHaveBeenCalledTimes(1)
    expect(onApply).not.toHaveBeenCalled()
    expect(onOpenGuide).not.toHaveBeenCalled()
  })
})

// THE WIZARD USED TO DROP THE ADDRESS IT HAD JUST DISCOVERED (2026-08-17 Flex audit, wave-1
// #29/#52). Picking a discovered Flex set conn/model/name and threw `f.ip` away — the exact
// regression the Settings twin records as fixed, still live in this sibling. Downstream, both
// native toggles are OFFERED in Settings for a 2036 network rig, and with no address neither can
// ever start a worker: the operator switches one on, saves, and nothing happens or is said.
describe('SetupWizard FlexRadio discovery', () => {
  const FLEX = { model: 'FLEX-6400', nickname: 'Shack', ip: '192.0.2.77' }
  const discoverFlex = api.discoverFlex as ReturnType<typeof vi.fn>

  beforeEach(() => {
    cleanup()
    discoverFlex.mockResolvedValue([FLEX])
  })

  it('keeps the discovered radio address in the draft it applies', async () => {
    const { onApply } = renderWizard()
    clickNext() // 0 station → 1 rig, which runs detection
    const row = await screen.findByRole('button', { name: /FLEX-6400/ })
    fireEvent.click(row)

    clickNext() // 1 rig → 2 log
    clickNext() // 2 log → 3 goals
    fireEvent.click(screen.getByRole('button', { name: /Finish — everything on/ }))

    expect(onApply).toHaveBeenCalledTimes(1)
    const draft = onApply.mock.calls[0][4] as Record<string, unknown>
    expect(draft.flexRadioIp).toBe('192.0.2.77')
    // …and the rest of the one-click apply still lands, so this is an addition, not a swap.
    expect(draft.rigModel).toBe(2036)
    expect(draft.rigConn).toBe('network')
  })
})

// CLOSED, THE KEYBOARD GOES BACK TO WHERE THE WIZARD WAS OPENED FROM (focusReturn.ts): Settings'
// "Re-run the setup wizard". It was left on the page itself.
describe('the wizard gives the keyboard back when it closes', () => {
  it('to the control that opened it', async () => {
    function Host() {
      const [open, setOpen] = useState(false)
      return (
        <>
          <button type="button" onClick={() => setOpen(true)}>
            Re-run the setup wizard
          </button>
          {open && (
            <SetupWizard settings={null} onApply={vi.fn()} onTestCat={vi.fn(() => Promise.resolve({} as never))} onSkip={() => setOpen(false)} />
          )}
        </>
      )
    }
    cleanup() // this file unmounts by hand (see above)
    render(<Host />)
    const opener = screen.getByRole('button', { name: 'Re-run the setup wizard' })
    act(() => opener.focus())
    fireEvent.click(opener)
    await screen.findByRole('dialog')
    fireEvent.keyDown(document.activeElement!, { key: 'Escape' })
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    await waitFor(() => expect(document.activeElement).toBe(opener))
    cleanup()
  })
})

// AN EMPTY BOX SHOWS NO MADE-UP SAMPLE. The station step showed N0CALL and FN31 in grey, and the
// network rig's address box 127.0.0.1:5002 — a sample reads as a value already filled in.
describe('the wizard shows no sample value in an empty box', () => {
  const placeholderOf = (label: RegExp) => (screen.getByLabelText(label) as HTMLInputElement).getAttribute('placeholder')

  it('the station step: callsign and grid', () => {
    renderWizard({ settings: { mycall: '', mygrid: '' } })
    expect(placeholderOf(/^Callsign/)).toBeNull()
    expect(placeholderOf(/^Grid square/)).toBeNull()
  })

  it("the radio step: a network rig's address", () => {
    renderWizard({ settings: { rigConn: 'network', rigAddr: '' } })
    clickNext() // 0 station → 1 rig
    expect(placeholderOf(/^Address/)).toBeNull()
  })
})

describe('SetupWizard licence class', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  // The grandfathered US classes are offered beside the others, and the pick reaches onApply as
  // the token the engine parses; an unknown token there would fall to Open and lift the lockout.
  it.each([
    [/^Novice/, 'novice'],
    [/^Advanced/, 'advanced'],
    [/^Technician/, 'technician'],
  ])('a pick of %s applies the %s class', (name, token) => {
    const { onApply } = renderWizard()
    gotoLogStep()
    clickNext() // → goals
    fireEvent.click(screen.getByRole('button', { name }))
    fireEvent.click(screen.getByRole('button', { name: /Finish — everything on/ }))
    expect(onApply).toHaveBeenCalledTimes(1)
    expect(onApply.mock.calls[0][3]).toBe(token)
  })
})
