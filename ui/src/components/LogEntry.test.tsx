// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import { fdLogManual } from '../api'
import type { AppSnapshot, FieldDayStatus } from '../types'

// Only the FD log seam matters here; the other api functions are imported by the
// component but never reached on the Field Day render path, so stub them harmlessly.
vi.mock('../api', () => ({
  fdLogManual: vi.fn(() => Promise.resolve({})),
  getLog: vi.fn(() => Promise.resolve([])),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
}))

const mockedFdLog = vi.mocked(fdLogManual)

const snap = {
  radio: { band: '20m', dialMhz: 14.2 },
  hunt: null,
} as unknown as AppSnapshot

const fieldDay = {
  myClass: '',
  mySection: '',
  running: true,
  state: '',
  qsoCount: 0,
  sections: 0,
  points: 0,
  log: [],
} as unknown as FieldDayStatus

function renderFd() {
  render(<LogEntry snap={snap} mode="PH" defaultRst="59" fieldDay={fieldDay} fdMode="PH" />)
}

const call = () => screen.getByPlaceholderText('W1AW')
const klass = () => screen.getByPlaceholderText('1D')
const section = () => screen.getByPlaceholderText('WI')
const logBtn = () => screen.getByRole('button', { name: /log fd/i }) as HTMLButtonElement

beforeEach(() => mockedFdLog.mockClear())
afterEach(() => cleanup())

describe('LogEntry Field Day exchange gate', () => {
  it('blocks logging (button disabled, no fdLogManual) when the section is blank', () => {
    renderFd()
    fireEvent.change(call(), { target: { value: 'w1aw' } })
    fireEvent.change(klass(), { target: { value: '2a' } })
    // section left blank — the old code would have logged it as the literal '?'
    expect(logBtn().disabled).toBe(true)
    fireEvent.click(logBtn())
    expect(mockedFdLog).not.toHaveBeenCalled()
  })

  it('blocks logging when the section is not a real ARRL/RAC code', () => {
    renderFd()
    fireEvent.change(call(), { target: { value: 'w1aw' } })
    fireEvent.change(klass(), { target: { value: '2A' } })
    fireEvent.change(section(), { target: { value: 'ZZ' } })
    expect(logBtn().disabled).toBe(true)
    fireEvent.click(logBtn())
    expect(mockedFdLog).not.toHaveBeenCalled()
  })

  it('logs the real class + section once both are valid (never a "?" substitution)', () => {
    renderFd()
    fireEvent.change(call(), { target: { value: 'w1aw' } })
    fireEvent.change(klass(), { target: { value: '2a' } })
    fireEvent.change(section(), { target: { value: 'wi' } })
    expect(logBtn().disabled).toBe(false)
    fireEvent.click(logBtn())
    expect(mockedFdLog).toHaveBeenCalledWith('W1AW', '2A', 'WI', 'PH')
  })
})

describe('LogEntry standard variant — State + Country', () => {
  function renderStd() {
    render(<LogEntry snap={snap} mode="PH" defaultRst="59" fieldDay={null} fdMode={undefined} />)
  }

  it('shows editable State and Country fields in the main area', () => {
    renderStd()
    // They were previously write-only: auto-filled from QRZ and visible only in the summary
    // line, so an operator who heard the state on air had to open the logbook to fix it.
    expect(screen.getByPlaceholderText('State')).toBeTruthy()
    expect(screen.getByPlaceholderText('Country')).toBeTruthy()
  })

  it('accepts operator edits to State and Country', () => {
    renderStd()
    const st = screen.getByPlaceholderText('State') as HTMLInputElement
    const co = screen.getByPlaceholderText('Country') as HTMLInputElement
    fireEvent.change(st, { target: { value: 'WI' } })
    fireEvent.change(co, { target: { value: 'United States' } })
    expect(st.value).toBe('WI')
    expect(co.value).toBe('United States')
  })
})
