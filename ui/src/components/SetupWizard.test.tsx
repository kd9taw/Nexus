// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/react'
import { SetupWizard } from './SetupWizard'
import * as api from '../api'
import { memoriesStore, emptyBank, addMemory } from '../features/memories'

vi.mock('../api', () => ({
  importAdif: vi.fn(),
  detectRigs: vi.fn(() => Promise.resolve([])),
  discoverFlex: vi.fn(() => Promise.resolve([])),
  getAudioDevices: vi.fn(() => Promise.resolve({ input: [], output: [] })),
  getRigModels: vi.fn(() => Promise.resolve([])),
  probeCatPorts: vi.fn(() => Promise.resolve({ found: false, detail: 'no rig answered' })),
  addRadio: vi.fn(() => Promise.resolve(null)),
  getSettings: vi.fn(() => Promise.resolve({ radios: [] })),
  updateRadioProfile: vi.fn(() => Promise.resolve(null)),
}))

const importAdif = api.importAdif as ReturnType<typeof vi.fn>

/** `guide: true` supplies onOpenGuide, which is what makes the last step offer
 *  the Getting started walkthrough. Omitted, the offer must not appear at all. */
function renderWizard(opts: { guide?: boolean } = {}) {
  const onApply = vi.fn()
  const onSkip = vi.fn()
  const onOpenGuide = vi.fn()
  render(
    <SetupWizard
      settings={null}
      onApply={onApply}
      onTestCat={vi.fn(() => Promise.resolve({} as never))}
      onSkip={onSkip}
      onOpenGuide={opts.guide ? onOpenGuide : undefined}
    />,
  )
  return { onApply, onSkip, onOpenGuide }
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
  beforeEach(() => importAdif.mockReset())

  it('renders the optional log step and imports an ADIF file, reporting the count', async () => {
    importAdif.mockResolvedValue({ added: 5, skipped: 1, total: 6 })
    renderWizard()
    gotoLogStep()
    expect(screen.getByText(/Bring in your existing log/)).toBeTruthy()
    fireImport('<call:5>K1ABC<eor>')
    await waitFor(() => expect(importAdif).toHaveBeenCalledTimes(1))
    const result = await screen.findByText(/Imported/)
    expect(result.textContent).toContain('5')
    expect(result.textContent).toMatch(/seeded/)
  })

  it('treats a 0-QSO import as a warning, not a false "seeded" success', async () => {
    importAdif.mockResolvedValue({ added: 0, skipped: 0, total: 0 })
    renderWizard()
    gotoLogStep()
    fireImport('this is not an ADIF file')
    expect(await screen.findByText(/No QSOs found/)).toBeTruthy()
    expect(screen.queryByText(/now seeded/)).toBeNull()
  })

  it('is skippable — Next advances to goals without importing', () => {
    renderWizard()
    gotoLogStep()
    clickNext() // 2 log → 3 goals, no file chosen
    expect(screen.getByText(/You get everything/)).toBeTruthy()
    expect(importAdif).not.toHaveBeenCalled()
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
