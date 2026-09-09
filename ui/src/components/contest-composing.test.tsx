// @vitest-environment jsdom
//
// THE SESSION'S COMPOSING EXCHANGE STILL REACHES THE SCREEN.
//
// `FieldDayStatus.myClass`/`mySection` were deleted (spec §3.3 mechanism 2): two interop
// emitters read that session-level pair inside their per-QSO loops and stamped one
// exchange onto every row. Three surfaces legitimately showed it — they describe what is
// ABOUT to go on the air, not a contact already logged — and each was moved onto the
// `composing` vector instead.
//
// A type-check cannot see the failure that migration invites: `composingSlot` returning
// '' for a slot renamed or missing renders an empty header and an absent chip, and every
// existing test still passes because none of them assert what the operator sees. So these
// do, with a positive control that the assertions can fail.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, act, cleanup } from '@testing-library/react'
import { ContestView } from './ContestView'
import { Composer } from './Composer'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FieldDayStatus, Settings } from '../types'
import { composingSlot, composingText } from '../features/contestExchange'

vi.mock('../api', () => ({
  getSettings: vi.fn(async () => ({ ...defaultSettings, fdOperator: '' })),
  setSettings: vi.fn(async () => ({})),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  fdSetUpload: vi.fn(async () => ({})),
  fdMergeToGeneral: vi.fn(async () => ({ added: 0, already: 0, refused: 0, queued: false })),
  fdClubExport: vi.fn(async () => ''),
  saveTextToDownloads: vi.fn(async () => {}),
  openPanelWindow: vi.fn(async () => {}),
}))

const FD: FieldDayStatus = {
  composing: [
    { key: 'CLASS', raw: '2A' },
    { key: 'SECTION', raw: 'WI', domain: 'fd_sections' },
  ],
  running: true,
  state: 'sp',
  qsoCount: 0,
  sections: 0,
  points: 0,
  log: [],
  event: 'wfd',
}

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

afterEach(cleanup)

describe("the session's composing exchange, on the surfaces that show it", () => {
  it("puts the session's class and section in the ContestView header", async () => {
    render(
      <ContestView fieldDay={FD} tier="FT8" onSetMode={() => {}} />,
    )
    await settle()
    const ident = document.querySelector('.fd-class')
    expect(ident).not.toBeNull()
    expect(ident!.textContent).toContain('2A')
    expect(ident!.textContent).toContain('WI')
    // The positive control: an em dash is what an EMPTY composing vector renders, so a
    // header showing one has lost the exchange rather than found it.
    expect(ident!.textContent).not.toContain('—')
  })

  it('offers the exchange as the WFD one-tap chip', () => {
    render(
      <Composer
        peer="W1AW"
        mode="fieldDay"
        fieldDay={FD}
        macros={defaultSettings.macros as Settings['macros']}
        onSend={() => {}}
      />,
    )
    expect(screen.getByRole('button', { name: '2A WI' })).toBeTruthy()
  })

  it('reads a slot by id and joins in send order, skipping empties', () => {
    expect(composingSlot(FD.composing, 'CLASS')).toBe('2A')
    expect(composingSlot(FD.composing, 'SECTION')).toBe('WI')
    // A slot the session does not compose is '', never a neighbouring slot's value.
    expect(composingSlot(FD.composing, 'NR')).toBe('')
    expect(composingSlot(undefined, 'CLASS')).toBe('')
    expect(composingText(FD.composing)).toBe('2A WI')
    expect(composingText([{ key: 'CLASS', raw: '2A' }, { key: 'SECTION', raw: '' }])).toBe('2A')
    expect(composingText(undefined)).toBe('')
  })
})
