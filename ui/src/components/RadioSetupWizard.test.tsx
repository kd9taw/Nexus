// @vitest-environment jsdom
//
// ⭐ THE RADIO SETUP: Cancel at any step writes nothing; Finish writes exactly the rules the Check
// step shows, through the routing verbs; an untouched Finish writes nothing; "Where would this go?"
// asks the backend about the draft on screen (never a copy of the rules here); and Test CAT on a
// radio that is not in use is the read-only probe, never a radio switch.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor, within } from '@testing-library/react'
import { RadioSetupWizard } from './RadioSetupWizard'
import type { RadioProfile, RoutingRule, Settings } from '../types'
import type { RoutingDraft } from '../api'
import { ROUTE_MODE_LABEL } from '../settings/describeRouting'

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

/** Every call that writes a setting or moves a radio. None may run before Finish. */
const WRITERS = [
  'renameRadio',
  'setRadioEnabled',
  'setRadioBands',
  'setRoutingRules',
  'setDefaultRadio',
  'setRadioSwitchPolicy',
  'updateRadioProfile',
  'setSettings',
  'patchSettings',
  'selectRadio',
  'setActiveRadio',
  'addRadio',
  'removeRadio',
  'setRadioPegged',
]
const BANDS = ['40m', '20m', '6m', '2m', '70cm']
const radio = (id: number, name: string, bands: string[]) =>
  ({ id, name, bands, enabled: true, rigModel: 1, rigModelName: name, serialPort: `COM${id + 3}`, rigConn: 'serial', rigAddr: '', audioIn: '', audioOut: '', pttMethod: 'cat' }) as unknown as RadioProfile
const FM: RoutingRule = { bands: ['2m'], mode: 'fm', context: null, radio: 2 }
function station(): Settings {
  return {
    radios: [radio(0, 'FTdx10', []), radio(1, 'IC-9700', ['2m', '70cm']), radio(2, 'FT-991A', ['2m', '70cm'])],
    activeRadio: 0,
    routingRules: [FM],
    defaultRadio: 0,
    radioSwitchPolicy: 'act',
  } as Settings
}

const writes = () => WRITERS.filter((n) => api.spies[n]?.mock.calls.length)
const next = () => fireEvent.click(screen.getByRole('button', { name: 'Next' }))

beforeEach(() => {
  for (const s of Object.values(api.spies)) s.mockReset()
  api.get('getSnapshot').mockResolvedValue({ radios: [] })
  api.get('routePreviewDraft').mockResolvedValue({ radio: 1, name: 'IC-9700' })
  api.get('testRadioCat').mockResolvedValue({ id: 1, state: 'answered', ok: true, dialMhz: 144.174 })
  api.get('testCat').mockResolvedValue({ ok: true, detail: 'Connected — 14.074 MHz' })
  for (const n of WRITERS) api.get(n).mockResolvedValue({})
})
afterEach(cleanup)

describe('the radio setup', () => {
  it('Cancel at any step writes nothing', async () => {
    for (let at = 0; at < 7; at++) {
      const onClose = vi.fn()
      render(<RadioSetupWizard settings={station()} bands={BANDS} onClose={onClose} />)
      // Answer things on the way, so Cancel has something it could have saved.
      fireEvent.change(screen.getAllByRole('textbox')[1], { target: { value: 'IC-9700 shack' } })
      for (let i = 0; i < at; i++) {
        if (screen.queryByRole('checkbox', { name: 'FTdx10 on 40m' })) fireEvent.click(screen.getByRole('checkbox', { name: 'FTdx10 on 40m' }))
        if (screen.queryByRole('combobox', { name: '2m Weak-signal digital' }))
          fireEvent.change(screen.getByRole('combobox', { name: '2m Weak-signal digital' }), { target: { value: '1' } })
        if (screen.queryByRole('radio', { name: 'Tell me, with Undo' })) fireEvent.click(screen.getByRole('radio', { name: 'Tell me, with Undo' }))
        if (!screen.queryByRole('button', { name: 'Next' })) break
        next()
      }
      fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
      expect(onClose).toHaveBeenCalled()
      await Promise.resolve()
      expect(writes(), `Cancel after ${at} Next`).toEqual([])
      cleanup()
    }
  })

  it('an untouched Finish writes nothing', async () => {
    const onClose = vi.fn()
    render(<RadioSetupWizard settings={station()} bands={BANDS} onClose={onClose} />)
    while (screen.queryByRole('button', { name: 'Next' })) next()
    fireEvent.click(screen.getByRole('button', { name: 'Finish' }))
    await waitFor(() => expect(onClose).toHaveBeenCalled())
    expect(writes()).toEqual([])
  })

  it('Finish writes exactly the rules the Check step shows, and the preview asked about them', async () => {
    const onClose = vi.fn()
    const onFinished = vi.fn()
    render(<RadioSetupWizard settings={station()} bands={BANDS} onClose={onClose} onFinished={onFinished} />)
    next() // bands
    next() // shared bands
    fireEvent.change(screen.getByRole('combobox', { name: '2m Weak-signal digital' }), { target: { value: '1' } })
    fireEvent.change(screen.getByRole('combobox', { name: '70cm CW' }), { target: { value: '2' } })
    while (screen.queryByRole('button', { name: 'Next' })) next()

    const shown = within(document.querySelector('.radio-setup-rules') as HTMLElement)
      .getAllByRole('listitem')
      .map((li) => li.textContent)
    await waitFor(() => expect(api.spies.routePreviewDraft).toHaveBeenCalled())
    const calls = api.spies.routePreviewDraft.mock.calls
    const previewed = (calls[calls.length - 1][0] as RoutingDraft).rules

    fireEvent.click(screen.getByRole('button', { name: 'Finish' }))
    await waitFor(() => expect(onClose).toHaveBeenCalled())
    expect(api.spies.setRoutingRules).toHaveBeenCalledTimes(1)
    const written = api.spies.setRoutingRules.mock.calls[0][0] as RoutingRule[]
    const name = (id: number) => station().radios!.find((r) => r.id === id)!.name
    expect(written.map((r) => `${r.bands.join(', ')} · ${r.mode ? ROUTE_MODE_LABEL[r.mode] : ''} → ${name(r.radio)}`)).toEqual(shown)
    expect(written).toEqual(previewed)
    expect(written).toEqual([
      { bands: ['2m'], mode: 'digital', context: null, radio: 1 },
      { bands: ['2m'], mode: 'fm', context: null, radio: 2 },
      { bands: ['70cm'], mode: 'cw', context: null, radio: 2 },
    ])
    // Nothing else changed, so nothing else was written.
    expect(writes()).toEqual(['setRoutingRules'])
    expect(onFinished).toHaveBeenCalledWith(expect.objectContaining({ rules: written }))
  })

  it('Test CAT on a radio not in use is the read-only probe; on the radio in use it is Test CAT', async () => {
    render(<RadioSetupWizard settings={station()} bands={BANDS} onClose={() => {}} />)
    while (!screen.queryByRole('heading', { name: 'Audio and keying' })) next()
    const tests = screen.getAllByRole('button', { name: 'Test CAT' })
    fireEvent.click(tests[1])
    await screen.findByText('Answering: 144.174 MHz')
    expect(api.spies.testRadioCat).toHaveBeenCalledWith(1)
    expect(api.spies.testCat).not.toHaveBeenCalled()
    fireEvent.click(tests[0])
    await screen.findByText('Connected — 14.074 MHz')
    expect(api.spies.testCat).toHaveBeenCalledTimes(1)
    expect(api.spies.testRadioCat).toHaveBeenCalledTimes(1)
    expect(writes()).toEqual([])
  })
})
