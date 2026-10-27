// @vitest-environment jsdom
//
// The first-run question, "How much have you done?" (operator decision, 2026-10-27): asked once,
// first, on a fresh install's first run; the answer picks the starting features, the starter
// packs and whether the Getting started guide and the first-contact walk open after Finish.
// Every step stays skippable, and skipping writes nothing. Re-opened from Settings, the wizard
// does not ask again and starts the features the station's stored answer picks.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { SetupWizard } from './SetupWizard'
import { memoriesStore, emptyBank } from '../features/memories'
import { EN } from '../i18n'

vi.mock('../api', () => ({
  importAdif: vi.fn(),
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

function renderWizard(opts: { ask?: boolean; rerun?: boolean; settings?: Record<string, unknown> } = {}) {
  const onApply = vi.fn()
  const onSkip = vi.fn()
  const onOpenGuide = vi.fn()
  const onOpenWalk = vi.fn()
  render(
    <SetupWizard
      settings={(opts.settings ?? null) as never}
      onApply={onApply}
      onTestCat={vi.fn(() => Promise.resolve({} as never)) as never}
      onSkip={onSkip}
      onOpenGuide={onOpenGuide}
      onOpenWalk={onOpenWalk}
      askExperience={opts.ask ?? true}
      rerun={opts.rerun ?? false}
    />,
  )
  return { onApply, onSkip, onOpenGuide, onOpenWalk }
}

const next = () => fireEvent.click(screen.getByRole('button', { name: /Next/ }))
const answer = (key: 'setup.about.newToHobby' | 'setup.about.newToDigital' | 'setup.about.experienced') =>
  fireEvent.click(screen.getByRole('button', { name: new RegExp(`^${EN[key]}`) }))
/** About you → station → rig → log → finish. */
const toFinish = () => {
  for (let i = 0; i < 4; i++) next()
}
const finishButton = () => screen.getByRole('button', { name: /^Finish/ })
const pressed = (label: string) =>
  screen.getByRole('button', { name: new RegExp(`^${label}`) }).getAttribute('aria-pressed')

beforeEach(() => memoriesStore.set(emptyBank()))
afterEach(() => cleanup())

describe('the About-you step', () => {
  it('comes first on a first run, offering the three answers', () => {
    renderWizard()
    expect(screen.getByRole('heading', { name: EN['setup.about.title'] })).toBeTruthy()
    for (const k of ['setup.about.newToHobby', 'setup.about.newToDigital', 'setup.about.experienced'] as const) {
      expect(screen.getByRole('button', { name: new RegExp(`^${EN[k]}`) })).toBeTruthy()
    }
    // The station step is not up yet.
    expect(screen.queryByText(EN['setup.station.title'])).toBeNull()
  })

  it('new to the hobby: the starter set, packs, the guide and the walk', () => {
    const { onApply, onOpenGuide, onOpenWalk } = renderWizard()
    answer('setup.about.newToHobby')
    toFinish()
    expect(pressed(EN['setup.finish.guide.label'])).toBe('true')
    expect(pressed(EN['setup.finish.walk.label'])).toBe('true')
    // Not "everything on": the starter set is what it starts with.
    expect(finishButton().textContent).toBe(EN['setup.nav.finishPlain'])
    fireEvent.click(finishButton())
    expect(onApply).toHaveBeenCalledTimes(1)
    const [ids, landing, modes, , , level] = onApply.mock.calls[0]
    expect(ids).toEqual(['starter'])
    expect(landing).toBe('operate')
    expect(modes).toEqual([])
    expect(level).toBe('newToHobby')
    expect(onOpenGuide).toHaveBeenCalledTimes(1)
    expect(onOpenWalk).toHaveBeenCalledTimes(1)
    expect(memoriesStore.get().memories.some((m) => m.rxMhz === 14.074)).toBe(true)
  })

  it('new to digital: the starter set with Phone and CW, the walk offered but not opened', () => {
    const { onApply, onOpenGuide, onOpenWalk } = renderWizard()
    answer('setup.about.newToDigital')
    toFinish()
    expect(pressed(EN['setup.finish.walk.label'])).toBe('false')
    fireEvent.click(finishButton())
    const [ids, , modes, , , level] = onApply.mock.calls[0]
    expect(ids).toEqual(['starter'])
    expect(modes).toEqual(['phone', 'cw'])
    expect(level).toBe('newToDigital')
    expect(onOpenGuide).not.toHaveBeenCalled()
    expect(onOpenWalk).not.toHaveBeenCalled()
    expect(memoriesStore.get().memories.length).toBeGreaterThan(0)
  })

  it('experienced: everything on, no packs pre-checked, nothing opened', () => {
    const { onApply, onOpenGuide, onOpenWalk } = renderWizard()
    answer('setup.about.experienced')
    toFinish()
    expect(finishButton().textContent).toBe(EN['setup.nav.finish'])
    fireEvent.click(finishButton())
    const [ids, , modes, , , level] = onApply.mock.calls[0]
    expect(ids).toEqual(['everything'])
    expect(modes).toEqual([])
    expect(level).toBe('experienced')
    expect(onOpenGuide).not.toHaveBeenCalled()
    expect(onOpenWalk).not.toHaveBeenCalled()
    expect(memoriesStore.get().memories).toHaveLength(0)
  })

  it('may be passed without an answer: everything on, as before, and no answer written', () => {
    const { onApply } = renderWizard()
    toFinish()
    fireEvent.click(finishButton())
    const [ids, , , , , level] = onApply.mock.calls[0]
    expect(ids).toEqual(['everything'])
    expect(level).toBeNull()
  })

  it('Back from the station step returns to it, with the answer kept', () => {
    renderWizard()
    answer('setup.about.newToHobby')
    next()
    expect(screen.getByText(EN['setup.station.title'])).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /Back/ }))
    expect(pressed(EN['setup.about.newToHobby'])).toBe('true')
  })

  it('skipping on the question writes nothing', () => {
    const { onApply, onSkip } = renderWizard()
    answer('setup.about.newToHobby')
    fireEvent.click(screen.getByRole('button', { name: /set it up myself/ }))
    expect(onSkip).toHaveBeenCalledTimes(1)
    expect(onApply).not.toHaveBeenCalled()
    expect(memoriesStore.get().memories).toHaveLength(0)
  })
})

describe('the wizard re-opened from Settings', () => {
  it('does not ask again, and its Finish promises no change of screens', () => {
    const { onApply } = renderWizard({ ask: false, rerun: true, settings: { experienceLevel: 'experienced' } })
    expect(screen.queryByText(EN['setup.about.title'])).toBeNull()
    expect(screen.getByText(EN['setup.station.title'])).toBeTruthy()
    for (let i = 0; i < 3; i++) next()
    // App keeps the screens on a re-run (App.experience.test.tsx), so the button says plain Finish,
    // never "everything on".
    expect(finishButton().textContent).toBe(EN['setup.nav.finishPlain'])
    fireEvent.click(finishButton())
    const [, , , , , level] = onApply.mock.calls[0]
    // Nothing answered in this run: the station's answer is not rewritten.
    expect(level).toBeNull()
  })
})
