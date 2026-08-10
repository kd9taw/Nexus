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
    expect(screen.getByText(/What do you mostly want to do/)).toBeTruthy()
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
    fireEvent.click(screen.getByRole('button', { name: /Turn everything on/ }))
    const draft = onApply.mock.calls[0][4]
    expect(draft.rigModel).toBe(3088)
    expect(draft.baud).toBe(19200)
    expect(draft.pttMethod).toBe('cat')
    expect(draft.serialPort).toBe('COM9')
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
    fireEvent.click(screen.getByRole('button', { name: /Turn everything on/ }))
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
    expect(screen.getByText(/What do you mostly want to do/)).toBeTruthy() // on the goals step
    expect(screen.queryByText(/Start with some channels/)).toBeNull()
  })

  it('force-enables RTTY and SSTV when checked on the goals step', () => {
    const { onApply } = renderWizard()
    gotoGoals()
    // Pick a goal so the completion button enables; modes ride on top of the profile.
    fireEvent.click(document.querySelector<HTMLButtonElement>('.wizard-goal')!)
    // Anchor to the start of the accessible name so these match the mode toggles, not a
    // starter-pack offer whose name happens to list "RTTY"/"SSTV" (e.g. the digital pack).
    fireEvent.click(screen.getByRole('button', { name: /^RTTY/ }))
    fireEvent.click(screen.getByRole('button', { name: /^SSTV/ }))
    fireEvent.click(document.querySelector<HTMLButtonElement>('.wizard-go')!)
    expect(onApply).toHaveBeenCalledTimes(1)
    const modes = onApply.mock.calls[0][2] as string[]
    expect(modes).toContain('rtty')
    expect(modes).toContain('sstv')
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
    fireEvent.click(document.querySelector<HTMLButtonElement>('.wizard-goal')!)
    fireEvent.click(screen.getByRole('button', { name: /^Show me Getting started/ })) // on
    fireEvent.click(screen.getByRole('button', { name: /^Show me Getting started/ })) // off again
    fireEvent.click(document.querySelector<HTMLButtonElement>('.wizard-go')!)
    expect(onApply).toHaveBeenCalledTimes(1)
    expect(onOpenGuide).not.toHaveBeenCalled()
  })

  it('opens the guide after applying, on the goal path', () => {
    const { onApply, onOpenGuide } = renderWizard({ guide: true })
    gotoGoals()
    fireEvent.click(document.querySelector<HTMLButtonElement>('.wizard-goal')!)
    fireEvent.click(screen.getByRole('button', { name: /^Show me Getting started/ }))
    fireEvent.click(document.querySelector<HTMLButtonElement>('.wizard-go')!)
    expect(onApply).toHaveBeenCalledTimes(1)
    expect(onOpenGuide).toHaveBeenCalledTimes(1)
  })

  it('opens the guide on the "turn everything on" path too', () => {
    const { onApply, onOpenGuide } = renderWizard({ guide: true })
    gotoGoals()
    fireEvent.click(screen.getByRole('button', { name: /^Show me Getting started/ }))
    fireEvent.click(screen.getByRole('button', { name: /Turn everything on/ }))
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
