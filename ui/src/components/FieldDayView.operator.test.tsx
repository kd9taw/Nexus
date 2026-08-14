// @vitest-environment jsdom
//
// The seat swap is a MID-QSO act — that is what the operator field is for, handing the key
// over while the station keeps running. Writing it through the whole-settings save ran the
// engine's heavyweight apply (#54): mode back to Chat, TX queue cleared, TX cycle re-derived
// from whatever settings snapshot this component was holding. The write has to stay narrow.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react'
import { FieldDayView } from './FieldDayView'
import { setFdOperator, setSettings } from '../api'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  getSettings: vi.fn(async () => ({ ...defaultSettings, fdOperator: '' })),
  setSettings: vi.fn(async () => ({})),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  openPanelWindow: vi.fn(async () => {}),
}))

const FD: FieldDayStatus = {
  myClass: '1A',
  mySection: 'IL',
  running: true,
  state: 'sp',
  qsoCount: 0,
  sections: 0,
  points: 0,
  log: [],
}

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

beforeEach(() => {
  vi.mocked(setFdOperator).mockClear()
  vi.mocked(setSettings).mockClear()
})
afterEach(() => cleanup())

describe('Field Day operator swap', () => {
  it('persists the new operator through the narrow write, not the whole-settings save', async () => {
    render(<FieldDayView fieldDay={FD} onSetMode={() => {}} />)
    await settle()

    const input = screen.getByLabelText('Field Day operator (call or initials)')
    fireEvent.change(input, { target: { value: 'w1abc' } })
    fireEvent.blur(input)
    await settle()

    expect(vi.mocked(setFdOperator).mock.calls).toEqual([['W1ABC']])
    expect(vi.mocked(setSettings)).not.toHaveBeenCalled()
  })
})
