// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { AmpStrip } from './AmpStrip'
import type { AmpStatus } from '../types'

// Mock derived from the real module, never a hand-kept literal. A partial mock leaves every
// other export undefined, and a component that calls one of them throws on mount — which
// surfaces as a red suite in an unrelated file. Five test files in this directory carry that
// trap today; this one does not join them.
vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  ampCommand: vi.fn(async () => true),
}))
import { ampCommand } from '../api'

const linked = (over: Partial<AmpStatus> = {}): AmpStatus =>
  ({
    family: 'spe',
    model: '15K',
    linked: true,
    reason: '',
    operate: false,
    transmitting: false,
    outputWatts: 0,
    bandLabel: '80m',
    alarm: 'none',
    alarmRaised: false,
    warning: 'none',
    warningRaised: false,
    ...over,
  }) as AmpStatus

beforeEach(() => {
  cleanup()
  vi.mocked(ampCommand).mockClear()
})

describe('AmpStrip', () => {
  it('renders NOTHING when no amplifier is configured — the state of almost every station', () => {
    const { container } = render(<AmpStrip amp={null} />)
    expect(container.innerHTML).toBe('')
    // The control: the same component DOES render when there is one, so the assertion above is
    // about the null case and not about a component that never renders at all.
    render(<AmpStrip amp={linked()} />)
    expect(screen.getByRole('group', { name: /amplifier/i })).toBeTruthy()
  })

  it('shows the band the AMPLIFIER reports, and its power out', () => {
    render(<AmpStrip amp={linked({ bandLabel: '20m', outputWatts: 1200 })} />)
    expect(screen.getByText('20m')).toBeTruthy()
    expect(screen.getByText('1200 W')).toBeTruthy()
  })

  it('shows an em dash rather than guessing when the amplifier reports a band we cannot name', () => {
    render(<AmpStrip amp={linked({ bandLabel: null })} />)
    // A wrong band name in front of a kilowatt is worse than an honest blank.
    expect(screen.getByText('—')).toBeTruthy()
  })

  it('drives Operate from the amplifier, not from the click', () => {
    render(<AmpStrip amp={linked({ operate: false })} />)
    fireEvent.click(screen.getByRole('button', { name: /standby/i }))
    expect(vi.mocked(ampCommand)).toHaveBeenCalledWith('operate')
    // OPERATE is a toggle with no idempotent set, so a lost or duplicated frame would invert an
    // optimistic control. The label must still read Standby: only the next poll moves it.
    expect(screen.getByRole('button', { name: /standby/i })).toBeTruthy()
  })

  it('⛔ refuses every command while the amplifier is transmitting', async () => {
    render(<AmpStrip amp={linked({ transmitting: true, operate: true })} />)
    for (const name of [/operate/i, /band down/i, /band up/i]) {
      const btn = screen.getByRole('button', { name })
      expect((btn as HTMLButtonElement).disabled).toBe(true)
    }
    // The control: the same three are enabled when it is NOT transmitting, so this is testing
    // the interlock and not three buttons that are always dead.
    render(<AmpStrip amp={linked({ transmitting: false })} />)
    expect(
      screen.getAllByRole('button', { name: /band up/i }).some((b) => !(b as HTMLButtonElement).disabled),
    ).toBe(true)
  })

  it('disables the controls when the link is down but keeps the strip on screen', () => {
    render(<AmpStrip amp={linked({ linked: false, reason: 'noAnswer', outputWatts: null })} />)
    // The strip must not vanish: gone looks identical to never-placed.
    expect(screen.getByRole('group', { name: /amplifier/i })).toBeTruthy()
    expect((screen.getByRole('button', { name: /band up/i }) as HTMLButtonElement).disabled).toBe(true)
  })

  it('says so when a keystroke is refused rather than swallowing it', async () => {
    vi.mocked(ampCommand).mockResolvedValueOnce(false)
    render(<AmpStrip amp={linked()} />)
    fireEvent.click(screen.getByRole('button', { name: /band up/i }))
    expect(await screen.findByText(/not sent/i)).toBeTruthy()
  })

  it('carries no control whose accessible name a stop-line sweep would match', () => {
    render(<AmpStrip amp={linked()} />)
    // Standby is NOT a stop: the exciter keeps keying and the drive passes straight through.
    // If a name here ever matched one of the sweep's patterns, the sweep would start demanding
    // this strip be present in cockpits that have no amplifier.
    const stopish = [/^stop tx$/i, /^tune$/i, /^tuning…$/i, /^▼ tx on$/i, /^■ tx off$/i]
    for (const b of screen.getAllByRole('button')) {
      const name = (b.getAttribute('aria-label') ?? b.textContent ?? '').trim()
      for (const p of stopish) expect(p.test(name)).toBe(false)
    }
  })
})
