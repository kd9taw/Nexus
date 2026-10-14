// @vitest-environment jsdom
//
// ⭐ THE CONTEST SCREEN'S EDIT — any row's Edit opens its contact in place (a county line as one),
// Enter or Save sends the edit with the identity shown, Esc leaves it, and the line says what
// came of it. At a club the screen says how a contact is fixed there instead; never from afar.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, fireEvent, cleanup, act } from '@testing-library/react'
import { ContestView } from './ContestView'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FdClubStatus, FieldDayQso, FieldDayStatus } from '../types'
import { StreamInputDispatcher } from '../remote-native/stream-input'

vi.mock('../api', () => ({
  getSettings: vi.fn(async () => ({ ...defaultSettings, fdOperator: '' })),
  setSettings: vi.fn(async () => ({})),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  openPanelWindow: vi.fn(async () => {}),
  contestRemoved: vi.fn(async () => []),
  contestUnmerged: vi.fn(async () => []),
  contestRemoveLast: vi.fn(),
  contestRestore: vi.fn(),
  contestEdit: vi.fn(),
  // The country of the call in an edit, which the strip's own rule reads. None unless a test says so.
  resolveEntity: vi.fn(async () => null),
  // The club block's own commands, there while the screen shows a club.
  fdClubGivePosition: vi.fn(),
  fdClubRemovePosition: vi.fn(async () => ({ outcome: 'done' })),
  fdClubAllowBack: vi.fn(async () => ({ outcome: 'done' })),
  fdClubImport: vi.fn(async () => ({})),
  fdClubExport: vi.fn(),
}))
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>

const T0 = 1_792_342_800 // 2026-10-18 17:00Z

const row = (call: string, qth: string, whenUnix: number): FieldDayQso =>
  ({ call, class: '', section: '', band: '20m', mode: 'CW', submode: '', whenUnix, rcvd: ['599', qth] }) as FieldDayQso

const party = (log: FieldDayQso[], club?: FdClubStatus): FieldDayStatus =>
  ({
    running: true,
    state: 'sp',
    qsoCount: log.length,
    sections: 0,
    points: 0,
    log,
    event: 'ilqp',
    rulesYear: 2026,
    role: 'in_state',
    bands: ['80m', '40m', '20m'],
    receives: [
      { key: 'RST', kind: 'rst', required: true },
      { key: 'QTH', kind: 'enum', required: true, domain: 'il_counties', domains: ['il_counties'] },
    ],
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'QTH', raw: 'KANE', domain: 'il_counties' },
    ],
    club,
  }) as unknown as FieldDayStatus

const LOG = [row('K9AAA', 'COOK', T0), row('W9BBB', 'LAKE', T0 + 60)]

/** A position joined to a club, synced. */
const CLUB = {
  syncState: 'synced',
  queued: 0,
  offlineSinceUnix: 0,
  hosting: false,
  event: 'ILQP club',
  hostCall: 'W9ABC',
  score: 0,
  qsos: 2,
  sections: 0,
  skewSecs: 0,
  dupes: [],
  board: [],
} as unknown as FdClubStatus

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

async function show(fieldDay: FieldDayStatus, observed = false) {
  render(
    <ContestView
      fieldDay={fieldDay}
      onSetMode={() => {}}
      fdActive
      tier="FT8"
      observation={observed ? { fdOperator: '', fdPowerMult: 2, fdBonuses: [], fdBonusesPlanned: [] } : undefined}
    />,
  )
  await settle()
}

const editButtons = () => [...document.querySelectorAll<HTMLButtonElement>('.fd-log .fd-edit')]
const editor = () => document.querySelector<HTMLElement>('.fd-log .contest-edit')
const boxes = () => [...(editor()?.querySelectorAll<HTMLInputElement>('input') ?? [])]
const note = () => document.querySelector('.fd-removed-note')?.textContent ?? null
const type = (el: HTMLInputElement, value: string) => fireEvent.change(el, { target: { value } })

afterEach(() => {
  cleanup()
  for (const f of Object.values(api)) f.mockClear()
})

describe('Edit opens a contact in place', () => {
  it('is on every row, and opens that row’s contact where it stands, the caret in its call', async () => {
    await show(party(LOG))
    expect(editButtons()).toHaveLength(2)
    fireEvent.click(editButtons()[0])
    expect(editor()).not.toBeNull()
    expect(boxes().map((b) => b.value)).toEqual(['K9AAA', '599', 'COOK'])
    expect(document.activeElement).toBe(boxes()[0])
    const shown = [...document.querySelectorAll('.fd-log-row')].map((r) => r.querySelector('.call')?.textContent)
    expect(shown, 'the row it edits is the edit now').toEqual(['W9BBB'])
    expect(api.contestEdit).not.toHaveBeenCalled()
  })

  it('sends, on Enter, the edit with the identity shown — and says what came of it', async () => {
    api.contestEdit.mockResolvedValueOnce({
      outcome: 'edited',
      before: [LOG[0]],
      after: [row('K9AAB', 'DUPG', T0)],
      sentTo: { wsjtx: false, logbook: false, uploaded: [] },
    })
    await show(party(LOG))
    fireEvent.click(editButtons()[0])
    type(boxes()[0], 'k9aab')
    type(boxes()[2], 'DuPage')
    await act(async () => {
      fireEvent.keyDown(boxes()[2], { key: 'Enter' })
    })
    await settle()
    expect(api.contestEdit).toHaveBeenCalledWith('K9AAA', T0, {
      call: 'K9AAB',
      rows: [[['RST', '599'], ['QTH', 'DUPG']]],
      band: '20m',
      mode: '',
    })
    expect(editor()).toBeNull()
    expect(note()).toBe('Edited K9AAA: now K9AAB · 20m CW · 17:00 · 599 DUPG.')
  })

  it('edits a county line as one contact: one edit in place of its rows, one vector per county', async () => {
    api.contestEdit.mockResolvedValueOnce({ outcome: 'refused', refusal: 'changed' })
    const line = [row('K9AAA', 'COOK', T0), row('K9AAA', 'LAKE', T0), row('W9BBB', 'LAKE', T0 + 60)]
    await show(party(line))
    fireEvent.click(editButtons()[1])
    expect(boxes().map((b) => b.value)).toEqual(['K9AAA', '599', 'COOK/LAKE'])
    expect(document.querySelectorAll('.fd-log-row')).toHaveLength(1)
    type(boxes()[2], 'COOK/DUPG')
    await act(async () => {
      fireEvent.click(editor()!.querySelector('.contest-edit-save')!)
    })
    await settle()
    expect(api.contestEdit).toHaveBeenCalledWith('K9AAA', T0, {
      call: 'K9AAA',
      rows: [
        [['RST', '599'], ['QTH', 'COOK']],
        [['RST', '599'], ['QTH', 'DUPG']],
      ],
      band: '20m',
      mode: '',
    })
    expect(note(), 'a refusal is said').toBe(
      'That contact changed in the contest log since it was shown, so nothing was edited.',
    )
  })

  it('sends a new band and mode, and nothing for a box the strip would refuse', async () => {
    api.contestEdit.mockResolvedValueOnce({ outcome: 'refused', refusal: 'changed' })
    await show(party(LOG))
    fireEvent.click(editButtons()[1])
    type(boxes()[2], 'XXXX')
    fireEvent.click(editor()!.querySelector('.contest-edit-save')!)
    await settle()
    expect(api.contestEdit).not.toHaveBeenCalled()
    expect(editor()!.querySelector('.contest-edit-why')?.textContent).toBe(
      'County "XXXX" isn\'t a known value — required to log.',
    )
    type(boxes()[2], 'LAKE')
    const [band, mode] = [...editor()!.querySelectorAll<HTMLSelectElement>('select')]
    fireEvent.change(band, { target: { value: '40m' } })
    fireEvent.change(mode, { target: { value: 'USB' } })
    await act(async () => {
      fireEvent.keyDown(band, { key: 'Enter' })
    })
    await settle()
    expect(api.contestEdit).toHaveBeenCalledWith('W9BBB', T0 + 60, {
      call: 'W9BBB',
      rows: [[['RST', '599'], ['QTH', 'LAKE']]],
      band: '40m',
      mode: 'USB',
    })
  })

  it('closes when its contact leaves the log meanwhile', async () => {
    const view = (log: FieldDayQso[]) => (
      <ContestView fieldDay={party(log)} onSetMode={() => {}} fdActive tier="FT8" />
    )
    const { rerender } = render(view(LOG))
    await settle()
    fireEvent.click(editButtons()[0])
    expect(editor()).not.toBeNull()
    rerender(view([LOG[1]]))
    await settle()
    expect(editor()).toBeNull()
    expect(editButtons()).toHaveLength(1)
    rerender(view(LOG))
    await settle()
    expect(editor(), 'restored meanwhile, it does not open again by itself').toBeNull()
  })

  it('Esc and Cancel leave it as it was, and send nothing', async () => {
    await show(party(LOG))
    fireEvent.click(editButtons()[0])
    type(boxes()[0], 'N0NE')
    fireEvent.keyDown(boxes()[0], { key: 'Escape' })
    expect(editor()).toBeNull()
    fireEvent.click(editButtons()[0])
    expect(boxes()[0].value, 'it opens again as the log has it').toBe('K9AAA')
    fireEvent.click([...editor()!.querySelectorAll('button')].find((b) => b.textContent === 'Cancel')!)
    expect(editor()).toBeNull()
    expect(api.contestEdit).not.toHaveBeenCalled()
  })

  it('refuses a QTH on no list by the country of the call in the edit, as the log line does', async () => {
    const country: Record<string, string> = { K9AAA: 'United States', DL1ABC: 'Fed. Rep. of Germany' }
    api.resolveEntity.mockImplementation(async (call: string) => country[call] ?? null)
    try {
      // The Illinois QSO Party's own box: its counties, its states and provinces, and the free-text
      // arm a DX station's country goes in.
      await show({
        ...party(LOG),
        receives: [
          { key: 'RST', kind: 'rst', required: true },
          { key: 'QTH', kind: 'oneOf', required: true, domains: ['il_counties', 'il_mults'] },
        ],
      } as unknown as FieldDayStatus)
      fireEvent.click(editButtons()[0])
      await settle()
      type(boxes()[2], 'GERMANY')
      fireEvent.click(editor()!.querySelector('.contest-edit-save')!)
      await settle()
      expect(api.contestEdit).not.toHaveBeenCalled()
      expect(editor()!.querySelector('.contest-edit-why')?.textContent).toBe(
        "GERMANY is not a county, state or province on this contest's list.",
      )
      // CONTROL: the same value, once the call in the edit is a DX station's.
      api.contestEdit.mockResolvedValueOnce({ outcome: 'refused', refusal: 'changed' })
      type(boxes()[0], 'DL1ABC')
      await settle()
      await act(async () => {
        fireEvent.click(editor()!.querySelector('.contest-edit-save')!)
      })
      await settle()
      expect(api.contestEdit).toHaveBeenCalledWith('K9AAA', T0, {
        call: 'DL1ABC',
        rows: [[['RST', '599'], ['QTH', 'GERMANY']]],
        band: '20m',
        mode: '',
      })
    } finally {
      api.resolveEntity.mockImplementation(async () => null)
    }
  })
})

describe('at a club, and from afar', () => {
  it('at a club says how a contact is fixed there, and opens no edit; CONTROL: alone it opens', async () => {
    await show(party(LOG, CLUB))
    fireEvent.click(editButtons()[1])
    expect(editor()).toBeNull()
    expect(note()).toMatch(/^At a club an edit cannot reach the other laptops yet, so nothing was edited\./)
    cleanup()
    await show(party(LOG))
    fireEvent.click(editButtons()[1])
    expect(editor()).not.toBeNull()
  })

  it('offers no Edit when observing', async () => {
    await show(party(LOG), true)
    expect(editButtons()).toHaveLength(0)
  })

  describe('a press through the Remote stream edits nothing', () => {
    let under: Element | null = null
    let stream: StreamInputDispatcher | null = null
    beforeEach(() => {
      under = null
      Object.defineProperty(document, 'elementFromPoint', { configurable: true, value: () => under })
      stream = new StreamInputDispatcher(window)
    })
    afterEach(() => {
      stream?.dispose()
      stream = null
      delete (document as { elementFromPoint?: unknown }).elementFromPoint
    })
    const pointer = (action: 'down' | 'up') =>
      ({ type: 'pointer', action, x: 0.5, y: 0.5, button: 0, buttons: action === 'down' ? 1 : 0,
        modifiers: 0, pointerType: 'mouse', clicks: 1 })

    it('refuses Edit clicked through the stream, and says why; CONTROL: at the station it opens', async () => {
      await show(party(LOG))
      under = editButtons()[1]
      await act(async () => { stream!.handle(pointer('down')); stream!.handle(pointer('up')) })
      expect(editor()).toBeNull()
      expect(note()).toBe('Only at the station: a press through Remote edits no contact.')
      fireEvent.click(editButtons()[1])
      expect(editor()).not.toBeNull()
    })

    it('refuses Save pressed through the stream on an edit opened at the station', async () => {
      await show(party(LOG))
      fireEvent.click(editButtons()[1])
      type(boxes()[0], 'W9BBC')
      under = editor()!.querySelector('.contest-edit-save')
      await act(async () => { stream!.handle(pointer('down')); stream!.handle(pointer('up')) })
      await settle()
      expect(api.contestEdit).not.toHaveBeenCalled()
      expect(note()).toBe('Only at the station: a press through Remote edits no contact.')
    })
  })
})
