// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import { fdLogManual, logQso } from '../api'
import type { AppSnapshot, FieldDayStatus } from '../types'

// The FD log + standard-log seams matter here; the other api functions are imported by the
// component but never reached on these render paths, so stub them harmlessly.
vi.mock('../api', () => ({
  fdLogManual: vi.fn(() => Promise.resolve({})),
  logQso: vi.fn(() => Promise.resolve({})),
  getLog: vi.fn(() => Promise.resolve([])),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
}))

const mockedFdLog = vi.mocked(fdLogManual)
const mockedLogQso = vi.mocked(logQso)

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

beforeEach(() => {
  mockedFdLog.mockClear()
  mockedLogQso.mockClear()
})
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

describe('LogEntry standard variant — other-radio override (band/freq/mode/UTC time)', () => {
  // snap.radio is the LIVE (HF) rig: 20m / 14.2 MHz. mode="SSB" is the cockpit's live mode.
  function renderStd() {
    render(<LogEntry snap={snap} mode="SSB" defaultRst="59" fieldDay={null} fdMode={undefined} />)
  }
  const overrideToggle = () => screen.getByRole('button', { name: /another radio/i })
  const logBtn = () => screen.getByRole('button', { name: 'Log' })

  it('logs the hand-entered band / freq / mode / UTC time when the override is open', () => {
    renderStd()

    // Opt in, then set a contact made on the 2 m rig that Nexus can't see.
    fireEvent.click(overrideToggle())
    fireEvent.change(screen.getByLabelText('Band'), { target: { value: '2m' } })
    fireEvent.change(screen.getByLabelText('Freq (MHz)'), { target: { value: '146.520' } })
    fireEvent.change(screen.getByLabelText('Mode'), { target: { value: 'FM' } })
    fireEvent.change(screen.getByLabelText('Date (UTC)'), { target: { value: '2026-03-15' } })
    fireEvent.change(screen.getByLabelText('Time (UTC)'), { target: { value: '14:30' } })
    fireEvent.change(screen.getByPlaceholderText('Call'), { target: { value: 'k9xyz' } })

    fireEvent.click(logBtn())

    // The exact UTC instant — NOT a local-zone reading of the inputs, NOT "now".
    const expectedWhen = Math.floor(Date.UTC(2026, 2, 15, 14, 30, 0) / 1000)
    expect(mockedLogQso).toHaveBeenCalledTimes(1)
    expect(mockedLogQso).toHaveBeenCalledWith(
      expect.objectContaining({
        call: 'K9XYZ',
        band: '2m',
        freqMhz: 146.52,
        mode: 'FM',
        whenUnix: expectedWhen,
      }),
    )
    // Proves the live-rig defaults were genuinely overridden, not merely added alongside.
    const rec = mockedLogQso.mock.calls[0][0]
    expect(rec.band).not.toBe('20m')
    expect(rec.freqMhz).not.toBe(14.2)
  })

  it('picking a band fills a consistent in-band frequency (never "2m band / 14.2 MHz")', () => {
    renderStd()
    fireEvent.click(overrideToggle())
    // Open seeds from the live 20 m rig; switching the band must move the frequency with it.
    fireEvent.change(screen.getByLabelText('Band'), { target: { value: '2m' } })
    expect((screen.getByLabelText('Freq (MHz)') as HTMLInputElement).value).toBe('146.52')
    // And typing a frequency snaps the band to the plan it lands in.
    fireEvent.change(screen.getByLabelText('Freq (MHz)'), { target: { value: '446.000' } })
    expect((screen.getByLabelText('Band') as HTMLSelectElement).value).toBe('70cm')
  })

  it('closed override (the common flow) still logs the live rig + now, unchanged', () => {
    renderStd()
    const before = Math.floor(Date.now() / 1000)
    fireEvent.change(screen.getByPlaceholderText('Call'), { target: { value: 'w1aw' } })
    fireEvent.click(logBtn())
    const after = Math.floor(Date.now() / 1000)

    expect(mockedLogQso).toHaveBeenCalledTimes(1)
    const rec = mockedLogQso.mock.calls[0][0]
    expect(rec.band).toBe('20m')
    expect(rec.freqMhz).toBe(14.2)
    expect(rec.mode).toBe('SSB')
    expect(rec.whenUnix).toBeGreaterThanOrEqual(before)
    expect(rec.whenUnix).toBeLessThanOrEqual(after)
  })

  it('BLOCKS logging a mismatched record when the override is open but the freq is invalid', () => {
    // Review finding: with the override open and the freq cleared, band+freq once fell back to
    // the live rig while mode+time stayed the override — logging e.g. 20m/14.2 tagged FM at a
    // past time. It must refuse to log at all until the freq is valid or the override is closed.
    renderStd()
    fireEvent.click(overrideToggle())
    fireEvent.change(screen.getByLabelText('Mode'), { target: { value: 'FM' } })
    fireEvent.change(screen.getByLabelText('Freq (MHz)'), { target: { value: '' } }) // fumbled
    fireEvent.change(screen.getByPlaceholderText('Call'), { target: { value: 'k9xyz' } })

    fireEvent.click(logBtn())
    expect(mockedLogQso).not.toHaveBeenCalled() // no mismatched record written

    // Fix the frequency → it logs, fully consistent (2 m, in-band freq, FM).
    fireEvent.change(screen.getByLabelText('Freq (MHz)'), { target: { value: '146.520' } })
    fireEvent.click(logBtn())
    expect(mockedLogQso).toHaveBeenCalledTimes(1)
    const rec = mockedLogQso.mock.calls[0][0]
    expect(rec.band).toBe('2m')
    expect(rec.freqMhz).toBe(146.52)
    expect(rec.mode).toBe('FM')
  })

  it('offers no USB/LSB in the mode picker — those are ADIF submodes TQSL rejects as a MODE', () => {
    renderStd()
    fireEvent.click(overrideToggle())
    const modes = Array.from(
      (screen.getByLabelText('Mode') as HTMLSelectElement).options,
      (o) => o.value,
    )
    expect(modes).toContain('SSB')
    expect(modes).not.toContain('USB')
    expect(modes).not.toContain('LSB')
  })
})
