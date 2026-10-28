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
  // What the station holds when Finish reads it: as the setup opened, unless a test changes it.
  api.get('getSettings').mockImplementation(() => Promise.resolve(station()))
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
        if (screen.queryAllByRole('combobox', { name: 'PTT Method' }).length)
          fireEvent.change(screen.getAllByRole('combobox', { name: 'PTT Method' })[1], { target: { value: 'vox' } })
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
      // The two cells changed, then the stored rule, as it was.
      { bands: ['2m'], mode: 'digital', context: null, radio: 1 },
      { bands: ['70cm'], mode: 'cw', context: null, radio: 2 },
      FM,
    ])
    // Nothing else changed, so nothing else was written.
    expect(writes()).toEqual(['setRoutingRules'])
    expect(onFinished).toHaveBeenCalledWith(expect.objectContaining({ rules: written }), true)
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

  it('Audio and keying: Finish writes only the fields changed, to that radio, over the radio as stored then', async () => {
    // The station as it is when Finish writes: radio 1's receive sound card was set elsewhere (Settings'
    // Edit of it) after the setup opened. The setup changed only its transmit sound card.
    const later = station()
    later.radios = later.radios!.map((r) => (r.id === 1 ? { ...r, audioIn: 'in-elsewhere' } : r))
    api.get('getSettings').mockResolvedValue(later)
    api.get('getAudioDevices').mockResolvedValue({
      input: [{ name: 'in-a', label: 'USB Audio CODEC (A)' }],
      output: [{ name: 'out-b', label: 'USB Audio CODEC (B)' }],
    })
    const onClose = vi.fn()
    render(<RadioSetupWizard settings={station()} bands={BANDS} onClose={onClose} />)
    while (!screen.queryByRole('heading', { name: 'Audio and keying' })) next()
    const tx = screen.getAllByRole('combobox', { name: 'Output Device (TX)' })
    await within(tx[1]).findByRole('option', { name: 'USB Audio CODEC (B)' })
    fireEvent.change(tx[1], { target: { value: 'out-b' } })
    fireEvent.change(screen.getAllByRole('combobox', { name: 'PTT Method' })[2], { target: { value: 'vox' } })
    // A change made and taken back is no change.
    fireEvent.change(tx[0], { target: { value: 'out-b' } })
    fireEvent.change(tx[0], { target: { value: '' } })
    while (screen.queryByRole('button', { name: 'Next' })) next()
    fireEvent.click(screen.getByRole('button', { name: 'Finish' }))
    await waitFor(() => expect(onClose).toHaveBeenCalled())
    expect(writes()).toEqual(['updateRadioProfile'])
    const calls = api.spies.updateRadioProfile.mock.calls as [number, Record<string, unknown>][]
    expect(calls.map(([id]) => id)).toEqual([1, 2])
    expect([calls[0][1].audioOut, calls[0][1].audioIn, calls[0][1].pttMethod]).toEqual(['out-b', 'in-elsewhere', 'cat'])
    expect([calls[1][1].audioOut, calls[1][1].audioIn, calls[1][1].pttMethod]).toEqual(['', '', 'vox'])
  })

  describe("the keying choice gets Settings' save-time checks", () => {
    /** Radio 2 on VOX, with `model` as its rig model; the other two as `station()` has them. */
    const withRadio2 = (model: number, pttMethod = 'vox') => {
      const s = station()
      s.radios = s.radios!.map((r) => (r.id === 2 ? { ...r, rigModel: model, pttMethod } : r))
      api.get('getSettings').mockImplementation(() => Promise.resolve(structuredClone(s)))
      return s
    }
    const openAudio = (s: Settings) => {
      const onClose = vi.fn()
      render(<RadioSetupWizard settings={s} bands={BANDS} onClose={onClose} />)
      while (!screen.queryByRole('heading', { name: 'Audio and keying' })) next()
      return onClose
    }
    const keying = (i: number) => screen.getAllByRole('combobox', { name: 'PTT Method' })[i] as HTMLSelectElement
    const finish = () => {
      while (screen.queryByRole('button', { name: 'Next' })) next()
      fireEvent.click(screen.getByRole('button', { name: 'Finish' }))
    }
    const CAT_NO_MODEL = /PTT is set to CAT but the rig model is None\/VOX/

    it('CAT keying on a radio with no rig model is refused at Finish, saving nothing', async () => {
      const onClose = openAudio(withRadio2(0))
      fireEvent.change(keying(2), { target: { value: 'cat' } })
      expect(screen.getByText(CAT_NO_MODEL)).toBeTruthy()
      finish()
      expect((await screen.findByRole('alert')).textContent).toMatch(CAT_NO_MODEL)
      await Promise.resolve()
      expect(writes()).toEqual([])
      expect(onClose).not.toHaveBeenCalled()
    })

    it('positive control: CAT keying on a radio with a rig model is written', async () => {
      const onClose = openAudio(withRadio2(1))
      fireEvent.change(keying(2), { target: { value: 'cat' } })
      expect(screen.queryByText(CAT_NO_MODEL)).toBeNull()
      finish()
      await waitFor(() => expect(onClose).toHaveBeenCalled())
      expect(api.spies.updateRadioProfile.mock.calls.map(([id, p]) => [id, (p as Record<string, unknown>).pttMethod])).toEqual([[2, 'cat']])
    })

    it('a problem the setup did not make is Settings\' to report: a sound card change on such a radio is written', async () => {
      api.get('getAudioDevices').mockResolvedValue({ input: [], output: [{ name: 'out-b', label: 'USB Audio CODEC (B)' }] })
      const onClose = openAudio(withRadio2(0, 'cat'))
      const tx = screen.getAllByRole('combobox', { name: 'Output Device (TX)' })[2]
      await within(tx).findByRole('option', { name: 'USB Audio CODEC (B)' })
      fireEvent.change(tx, { target: { value: 'out-b' } })
      finish()
      await waitFor(() => expect(onClose).toHaveBeenCalled())
      expect(api.spies.updateRadioProfile.mock.calls.map(([id]) => id)).toEqual([2])
    })

    it('a radio Hamlib cannot key over CAT says so under the picker, and CAT on it warns but saves', async () => {
      const onClose = openAudio(withRadio2(3011))
      const card = keying(2).closest('.radio-setup-card') as HTMLElement
      expect(within(card).getByText(/Hamlib has no way to key this radio over CAT/)).toBeTruthy()
      expect(within(keying(0).closest('.radio-setup-card') as HTMLElement).queryByText(/Hamlib has no way to key this radio/)).toBeNull()
      fireEvent.change(keying(2), { target: { value: 'cat' } })
      expect(within(card).getByText(/Hamlib can't key this radio over CAT/)).toBeTruthy()
      finish()
      await waitFor(() => expect(onClose).toHaveBeenCalled())
      expect(api.spies.updateRadioProfile).toHaveBeenCalledTimes(1)
    })

    it('Serial RTS or DTR offers the keying port, and Finish writes it', async () => {
      const onClose = openAudio(withRadio2(1))
      expect(screen.queryByRole('textbox', { name: 'PTT Serial Port' })).toBeNull()
      fireEvent.change(keying(2), { target: { value: 'rts' } })
      fireEvent.change(screen.getByRole('textbox', { name: 'PTT Serial Port' }), { target: { value: 'COM9' } })
      finish()
      await waitFor(() => expect(onClose).toHaveBeenCalled())
      const [id, patch] = api.spies.updateRadioProfile.mock.calls[0] as [number, Record<string, unknown>]
      expect([id, patch.pttMethod, patch.pttSerialPort]).toEqual([2, 'rts', 'COM9'])
    })
  })

  it('from a computer driving the station, the keying method is shown, not offered', () => {
    render(<RadioSetupWizard settings={{ ...station(), stationOnlySettings: ['pttMethod'] }} bands={BANDS} onClose={() => {}} />)
    while (!screen.queryByRole('heading', { name: 'Audio and keying' })) next()
    for (const box of screen.getAllByRole('combobox', { name: /PTT Method/ })) expect((box as HTMLSelectElement).disabled).toBe(true)
  })

  it('Finish refuses, saving nothing, when the radios or rules changed since the setup opened', async () => {
    const later = station()
    later.routingRules = [{ bands: ['2m'], mode: 'fm', context: null, radio: 1 }]
    api.get('getSettings').mockResolvedValue(later)
    const onClose = vi.fn()
    const onFinished = vi.fn()
    render(<RadioSetupWizard settings={station()} bands={BANDS} onClose={onClose} onFinished={onFinished} />)
    fireEvent.change(screen.getAllByRole('textbox')[1], { target: { value: 'IC-9700 shack' } })
    while (screen.queryByRole('button', { name: 'Next' })) next()
    fireEvent.click(screen.getByRole('button', { name: 'Finish' }))
    await screen.findByText(/changed while this was open/)
    expect(writes()).toEqual([])
    expect(onClose).not.toHaveBeenCalled()
    expect(onFinished).not.toHaveBeenCalled()
  })

  it('a Finish refused partway says so, stays open, and reports what landed so Settings re-reads it', async () => {
    api.get('setRoutingRules').mockRejectedValue('the station refused it')
    const onClose = vi.fn()
    const onFinished = vi.fn()
    render(<RadioSetupWizard settings={station()} bands={BANDS} onClose={onClose} onFinished={onFinished} />)
    fireEvent.change(screen.getAllByRole('textbox')[1], { target: { value: 'IC-9700 shack' } })
    next() // bands
    next() // shared bands
    fireEvent.change(screen.getByRole('combobox', { name: '2m Weak-signal digital' }), { target: { value: '1' } })
    while (screen.queryByRole('button', { name: 'Next' })) next()
    fireEvent.click(screen.getByRole('button', { name: 'Finish' }))
    await screen.findByText(/the station refused it/)
    expect(api.spies.renameRadio).toHaveBeenCalledWith(1, 'IC-9700 shack')
    expect(onFinished).toHaveBeenCalledWith(expect.objectContaining({ renames: [{ id: 1, name: 'IC-9700 shack' }] }), false)
    expect(onClose).not.toHaveBeenCalled()
  })

  it('Test CAT on a radio that has become the one in use since the setup opened is Test CAT', async () => {
    api.get('testRadioCat').mockResolvedValue({ id: 1, state: 'inUse', ok: null, dialMhz: null })
    render(<RadioSetupWizard settings={station()} bands={BANDS} onClose={() => {}} />)
    while (!screen.queryByRole('heading', { name: 'Audio and keying' })) next()
    fireEvent.click(screen.getAllByRole('button', { name: 'Test CAT' })[1])
    await screen.findByText('Connected — 14.074 MHz')
    expect(api.spies.testCat).toHaveBeenCalledTimes(1)
  })

  it('a band stored in other capitals shows ticked, and unticking it takes it off', async () => {
    // The engine reads band lists without case: a stored "2M" is 2 m.
    const s = station()
    s.radios = s.radios!.map((r) => (r.id === 2 ? { ...r, bands: ['2M', '70cm'] } : r))
    api.get('getSettings').mockResolvedValue(s)
    const onClose = vi.fn()
    render(<RadioSetupWizard settings={s} bands={BANDS} onClose={onClose} />)
    next()
    const box = screen.getByRole('checkbox', { name: 'FT-991A on 2m' }) as HTMLInputElement
    expect(box.checked).toBe(true)
    fireEvent.click(box)
    while (screen.queryByRole('button', { name: 'Next' })) next()
    fireEvent.click(screen.getByRole('button', { name: 'Finish' }))
    await waitFor(() => expect(onClose).toHaveBeenCalled())
    expect(api.spies.setRadioBands).toHaveBeenCalledWith(2, ['70cm'])
  })

  it('ticking a band keeps a band the table does not show', async () => {
    const s = station()
    s.radios = s.radios!.map((r) => (r.id === 2 ? { ...r, bands: ['2m', '1.25m'] } : r))
    api.get('getSettings').mockResolvedValue(s)
    const onClose = vi.fn()
    render(<RadioSetupWizard settings={s} bands={BANDS} onClose={onClose} />)
    next()
    fireEvent.click(screen.getByRole('checkbox', { name: 'FT-991A on 70cm' }))
    while (screen.queryByRole('button', { name: 'Next' })) next()
    fireEvent.click(screen.getByRole('button', { name: 'Finish' }))
    await waitFor(() => expect(onClose).toHaveBeenCalled())
    expect(api.spies.setRadioBands).toHaveBeenCalledWith(2, ['2m', '1.25m', '70cm'])
  })
})

