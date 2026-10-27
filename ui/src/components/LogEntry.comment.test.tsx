// @vitest-environment jsdom
//
// THE SHARED COMMENT HAS A PLACE OF ITS OWN, AND IT IS ONE LINE OF DATA.
//
// An operator: the "Comment (sharable)" box was compressed (it was the fourth field of the QTH
// row, sharing a wrapped line with QTH, State and Country), and asked for it to sit on its own,
// fixed, directly above the private Notes, as a multi-line box. It is a full-width textarea now,
// placed in the markup right before Notes, so no other field can reflow into its line.
//
// What it holds stays one line: ADIF's COMMENT is a String (no line breaks), NOTES the multi-line
// field. A pasted line break becomes a space as it lands, and Enter logs the contact as it always
// did from this box rather than starting a new line.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import { logQso } from '../api'
import type { AppSnapshot } from '../types'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => null) : actual[k]
  }
  return {
    ...auto,
    qrzLookup: vi.fn(async () => null),
    resolveEntity: vi.fn(async () => null),
    lookupPark: vi.fn(async () => null),
    lookupParkLive: vi.fn(async () => null),
    searchParks: vi.fn(async () => []),
    setLogFormGrid: vi.fn(async () => {}),
    logQso: vi.fn(async (rec: unknown) => rec),
    askLog: vi.fn(() => new Promise(() => {})),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action().catch(() => null)),
}))

const mockedLog = vi.mocked(logQso)
const snap = { mycall: 'KD9TAW', mygrid: 'EN52', radio: { band: '20m', dialMhz: 14.25 }, hunt: null } as unknown as AppSnapshot

const strip = (exchange: 'terrestrial' | 'satellite' = 'terrestrial') => (
  <LogEntry snap={snap} mode="SSB" defaultRst="59" exchange={exchange} titled={false} />
)
const comment = () => document.querySelector<HTMLTextAreaElement>('.le-comment')!
const notes = () => document.querySelector<HTMLTextAreaElement>('textarea.le-notes')!

beforeEach(() => {
  mockedLog.mockReset().mockImplementation(async (rec: unknown) => rec as never)
})

describe('the shared comment', () => {
  for (const ex of ['terrestrial', 'satellite'] as const) {
    it(`is a multi-line box of its own, directly above Notes (${ex})`, () => {
      render(strip(ex))
      expect(comment().tagName, 'the comment is still a one-line input').toBe('TEXTAREA')
      expect(comment().closest('.le-row'), 'the comment still shares a field row').toBeNull()
      expect(comment().nextElementSibling, 'the comment is not directly above Notes').toBe(notes())
    })
  }

  it('turns a pasted line break into a space, and logs it on one line', async () => {
    render(strip())
    fireEvent.change(document.querySelector<HTMLInputElement>('input.le-call')!, { target: { value: 'EA8XX' } })
    fireEvent.change(comment(), { target: { value: 'tnx for\nthe QSO\r\n\r\n73' } })
    expect(comment().value).toBe('tnx for the QSO 73')
    fireEvent.click(screen.getByRole('button', { name: /^log$/i }))
    await waitFor(() => expect(mockedLog).toHaveBeenCalledTimes(1))
    expect(mockedLog.mock.calls[0][0]).toMatchObject({ call: 'EA8XX', comment: 'tnx for the QSO 73' })
  })

  it('Enter logs the contact and types no line break', async () => {
    render(strip())
    fireEvent.change(document.querySelector<HTMLInputElement>('input.le-call')!, { target: { value: 'EA8XX' } })
    fireEvent.change(comment(), { target: { value: 'first QSO' } })
    const typed = fireEvent.keyDown(comment(), { key: 'Enter' })
    expect(typed, 'Enter would type a line break into the comment').toBe(false)
    await waitFor(() => expect(mockedLog).toHaveBeenCalledTimes(1))
    expect(mockedLog.mock.calls[0][0]).toMatchObject({ call: 'EA8XX', comment: 'first QSO' })
  })
})
