// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/react'
import { RttyCockpit, confidenceRuns, RTTY_MAX_RUNS } from './RttyCockpit'
import * as api from '../api'
import * as toast from '../toast'
import type { AppSnapshot, RttyState } from '../types'

vi.mock('../api', () => ({
  getRttyState: vi.fn(),
  rttyArm: vi.fn(),
  getLicensedBandPlan: vi.fn(),
  rttySend: vi.fn(),
  rttySetLatched: vi.fn(),
  rttyType: vi.fn(),
  rttyStop: vi.fn(),
  rttyClear: vi.fn(),
  rttyAfcReset: vi.fn(),
  haltTx: vi.fn(),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  // Pass-through like the real one: run the action, null on failure.
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => {
    try {
      return await action()
    } catch {
      return null
    }
  }),
}))

const getRttyState = api.getRttyState as ReturnType<typeof vi.fn>
const rttyArm = api.rttyArm as ReturnType<typeof vi.fn>
const getLicensedBandPlan = api.getLicensedBandPlan as ReturnType<typeof vi.fn>
const rttySend = api.rttySend as ReturnType<typeof vi.fn>
const rttyStop = api.rttyStop as ReturnType<typeof vi.fn>
const rttySetLatched = api.rttySetLatched as ReturnType<typeof vi.fn>
const rttyType = api.rttyType as ReturnType<typeof vi.fn>
const haltTx = api.haltTx as ReturnType<typeof vi.fn>
const pushToast = toast.pushToast as ReturnType<typeof vi.fn>

const snap = {
  mycall: 'KD9TAW',
  radio: {
    dialMhz: 14.08,
    band: '20m',
    catOk: true,
    sideband: 'USB',
    transmitting: false,
    txEnabled: true,
    tuning: false,
    txAllowed: true,
  },
} as unknown as AppSnapshot

const IDLE: RttyState = {
  armed: false,
  afcHz: 0,
  afcLocked: false,
  text: '',
  charConf: [],
  baud: 45.45,
  shiftHz: 170,
  backend: 'afsk',
  sending: false,
  latched: false,
  keyerError: null,
  markHz: 2125,
  spaceHz: 2295,
  auto: false,
  seqState: 'idle',
  peer: null,
  peerExchange: [],
  heardCq: null,
}

beforeEach(() => {
  getRttyState.mockReset().mockResolvedValue(IDLE)
  rttyArm.mockReset().mockResolvedValue({ ...IDLE, armed: true })
  getLicensedBandPlan.mockReset().mockResolvedValue([])
  rttySend.mockReset().mockResolvedValue({ ...IDLE, sending: true })
  rttyStop.mockReset().mockResolvedValue(IDLE)
  rttySetLatched.mockReset().mockResolvedValue({ ...IDLE, latched: true, sending: true })
  rttyType.mockReset().mockResolvedValue({ ...IDLE, latched: true, sending: true })
  haltTx.mockReset().mockResolvedValue(snap)
  pushToast.mockReset()
})
afterEach(cleanup)

describe('RttyCockpit RX wiring', () => {
  it('renders without a snapshot (stream + macros + compose, no header)', async () => {
    render(<RttyCockpit snap={null} />)
    expect(screen.getByText('Arm RX to decode RTTY from the receive audio')).toBeTruthy()
    // No snapshot → no CockpitHeader (it needs live radio state).
    expect(document.querySelector('.cockpit-header')).toBeNull()
    expect(screen.getByLabelText('RTTY compose')).toBeTruthy()
    await waitFor(() => expect(getRttyState).toHaveBeenCalled())
  })

  it('renders the mode badge + keying-backend pill with a snapshot', async () => {
    render(<RttyCockpit snap={snap} />)
    await screen.findByText('RTTY 45.45 · 170 Hz')
    expect(screen.getByText('AFSK')).toBeTruthy()
    // No onSetFrequency handler → the display-only band pill.
    expect(screen.getByText('20m')).toBeTruthy()
  })

  it('offers the licensed RTTY band plan and QSYs through onSetFrequency', async () => {
    getLicensedBandPlan.mockResolvedValue([
      {
        band: '20m',
        group: 'HF',
        dialMhz: 14.083,
        mode: 'LSB',
        label: '20 m · RTTY',
        note: 'the 14.080–14.090 RTTY window',
      },
    ])
    const onSetFrequency = vi.fn()
    render(<RttyCockpit snap={snap} onSetFrequency={onSetFrequency} />)
    expect(getLicensedBandPlan).toHaveBeenCalledWith('rtty')
    const select = (await screen.findByLabelText('Band channel preset')) as HTMLSelectElement
    await waitFor(() => expect(select.querySelectorAll('option').length).toBeGreaterThan(1))
    fireEvent.change(select, { target: { value: '20m' } })
    // Lands on the watering hole with the channel's own sideband (RTTY = LSB).
    expect(onSetFrequency).toHaveBeenCalledWith(14.083, '20m', 'LSB')
  })

  it('polls the decoder and renders confidence-faded text + the locked AFC pill', async () => {
    getRttyState.mockResolvedValue({
      ...IDLE,
      armed: true,
      afcHz: 12.4,
      afcLocked: true,
      text: 'CQ TEST',
      // "CQ TE" solid, "ST" low-confidence → faint tail run.
      charConf: [95, 95, 95, 90, 90, 20, 20],
    })
    render(<RttyCockpit snap={snap} />)
    await screen.findByText('RX armed')
    expect(screen.getByText('+12 Hz 🔒')).toBeTruthy()
    const faint = screen.getByText('ST')
    expect(faint.style.opacity).toBe('0.3')
    expect(screen.getByText('CQ TE').style.opacity).toBe('')
  })

  it('shows the unlocked AFC offset plain (no lock glyph)', async () => {
    getRttyState.mockResolvedValue({ ...IDLE, armed: true, afcHz: -8.2 })
    render(<RttyCockpit snap={snap} />)
    await screen.findByText('-8 Hz')
    expect(screen.queryByText(/🔒/)).toBeNull()
  })

  it('arms the RX decoder through rtty_arm and reflects the returned state', async () => {
    render(<RttyCockpit snap={snap} />)
    const arm = await screen.findByText('Arm RX')
    fireEvent.click(arm)
    expect(rttyArm).toHaveBeenCalledWith(true)
    await screen.findByText('RX armed')
  })

  it('does not poll while inactive (hidden keep-alive host)', () => {
    render(<RttyCockpit snap={snap} active={false} />)
    expect(getRttyState).not.toHaveBeenCalled()
  })
})

describe('RttyCockpit TX wiring', () => {
  it('sends the CQ macro with {MYCALL} expanded — an explicit operator action', async () => {
    render(<RttyCockpit snap={snap} />)
    fireEvent.click(screen.getByText('CQ'))
    await waitFor(() =>
      expect(rttySend).toHaveBeenCalledWith('CQ CQ CQ DE KD9TAW KD9TAW K'),
    )
  })

  it('refuses a {CALL} macro until their call is entered — then expands it', async () => {
    render(<RttyCockpit snap={snap} />)
    fireEvent.click(screen.getByText('Answer'))
    expect(rttySend).not.toHaveBeenCalled()
    expect(pushToast).toHaveBeenCalled()
    fireEvent.change(screen.getByLabelText('Worked station callsign (the {CALL} macro token)'), {
      target: { value: 'w1abc' },
    })
    fireEvent.click(screen.getByText('Answer'))
    await waitFor(() =>
      expect(rttySend).toHaveBeenCalledWith('W1ABC DE KD9TAW KD9TAW K'),
    )
  })

  it('sends typed compose text on Send and clears the input', async () => {
    render(<RttyCockpit snap={snap} />)
    const input = screen.getByLabelText('RTTY compose') as HTMLInputElement
    fireEvent.change(input, { target: { value: 'tu 73' } })
    fireEvent.click(screen.getByText('Send'))
    await waitFor(() => expect(rttySend).toHaveBeenCalledWith('tu 73'))
    expect(input.value).toBe('')
  })

  it('refuses to key outside license privileges (surfaced up front)', () => {
    const locked = {
      ...snap,
      radio: { ...snap.radio, txAllowed: false },
    } as unknown as AppSnapshot
    render(<RttyCockpit snap={locked} />)
    fireEvent.click(screen.getByText('CQ'))
    expect(rttySend).not.toHaveBeenCalled()
    expect(pushToast).toHaveBeenCalledWith(
      'TX locked — this frequency is outside your license privileges',
      'info',
      3500,
    )
  })

  it('shows the sending pill while an over is on the air and Stop aborts + halts', async () => {
    getRttyState.mockResolvedValue({ ...IDLE, sending: true })
    render(<RttyCockpit snap={snap} />)
    await screen.findByText('TX ▲')
    const stop = screen.getByText('Stop').closest('button') as HTMLButtonElement
    expect(stop.disabled).toBe(false)
    fireEvent.click(stop)
    expect(rttyStop).toHaveBeenCalled()
    expect(haltTx).toHaveBeenCalled()
  })

  it('latches continuous TX through rtty_set_latched and reflects it on the button', async () => {
    render(<RttyCockpit snap={snap} />)
    const tx = (await screen.findByText('Continuous')).closest('button') as HTMLButtonElement
    expect(tx.getAttribute('aria-pressed')).toBe('false')
    fireEvent.click(tx)
    expect(rttySetLatched).toHaveBeenCalledWith(true)
    await screen.findByText('On air')
    fireEvent.click(screen.getByText('On air').closest('button') as HTMLButtonElement)
    expect(rttySetLatched).toHaveBeenLastCalledWith(false)
  })

  it('keeps Stop live the instant the latch is up, before anything is on the air', async () => {
    // ⭐ THE AUDIT'S FINDING. Stop is on RTTY's stop-line census, and `sending` is
    // stamped by the radio loop from the audio ACTUALLY in flight — so it is false
    // for the tick between the latch going up and the first chunk being keyed, and
    // false for good if the FSK keyline never opens. Gating Stop on `sending`
    // alone leaves a keyed transmitter whose Stop button is mounted and dead,
    // which the stop line counts as the same loss as one that is gone.
    getRttyState.mockResolvedValue({ ...IDLE, latched: true, sending: false })
    render(<RttyCockpit snap={snap} />)
    await screen.findByText('On air')
    const stop = screen.getByText('Stop').closest('button') as HTMLButtonElement
    expect(stop.disabled).toBe(false)
    fireEvent.click(stop)
    expect(rttyStop).toHaveBeenCalled()
    expect(haltTx).toHaveBeenCalled()
  })

  it('streams typed characters one insertion at a time and refuses every edit', async () => {
    // RTTY HAS NO UN-SEND. A character that reaches rttyType is already on the
    // air, so while latched the field is append-only: an insertion is streamed,
    // and a backspace, a paste or a drop cannot touch what was sent.
    getRttyState.mockResolvedValue({ ...IDLE, latched: true, sending: true })
    render(<RttyCockpit snap={snap} />)
    await screen.findByText('On air')
    const input = screen.getByLabelText('RTTY compose') as HTMLInputElement

    const insert = (data: string) =>
      input.dispatchEvent(
        new (window as any).InputEvent('beforeinput', {
          inputType: 'insertText',
          data,
          bubbles: true,
          cancelable: true,
        }),
      )
    insert('C')
    insert('Q')
    expect(rttyType.mock.calls.map((c: unknown[]) => c[0])).toEqual(['C', 'Q'])
    await waitFor(() => expect(input.value).toBe('CQ'))

    // Anything that is not a single-character insertion is cancelled outright and
    // never reaches the transmitter.
    rttyType.mockClear()
    for (const inputType of [
      'deleteContentBackward',
      'insertFromPaste',
      'insertFromDrop',
      'deleteWordBackward',
      'insertLineBreak',
    ]) {
      const ev = new (window as any).InputEvent('beforeinput', {
        inputType,
        data: 'XXX',
        bubbles: true,
        cancelable: true,
      })
      input.dispatchEvent(ev)
      expect(ev.defaultPrevented).toBe(true)
    }
    expect(rttyType).not.toHaveBeenCalled()
    expect(input.value).toBe('CQ')

    // Enter is a NEW LINE on the air while latched, never a send: the transmitter
    // is already up, so there is nothing to send.
    fireEvent.keyDown(input, { key: 'Enter' })
    expect(rttyType).toHaveBeenCalledWith('\r\n')
    expect(rttySend).not.toHaveBeenCalled()
  })

  it('leaves the unlatched compose bar exactly as it was — Enter still sends a line', async () => {
    // The regression guard for every operator who does NOT use continuous TX: the
    // send-and-done path is untouched, and the streaming interception is bound
    // only while latched.
    render(<RttyCockpit snap={snap} />)
    const input = (await screen.findByLabelText('RTTY compose')) as HTMLInputElement
    fireEvent.change(input, { target: { value: 'CQ TEST' } })
    expect(input.value).toBe('CQ TEST')
    fireEvent.keyDown(input, { key: 'Enter' })
    expect(rttySend).toHaveBeenCalledWith('CQ TEST')
    expect(rttyType).not.toHaveBeenCalled()
  })

  it('binds Esc to Stop, and only while this is the visible view', async () => {
    // RTTY had no keyboard handler at all — the "Esc" glyph on the Stop macro was
    // decoration. A latched transmitter is what makes that gap matter. The cockpit
    // stays MOUNTED in the keep-alive host, so an unconditional listener would
    // fire Stop TX from inside another section.
    const { rerender } = render(<RttyCockpit snap={snap} active={false} />)
    fireEvent.keyDown(window, { key: 'Escape' })
    expect(rttyStop).not.toHaveBeenCalled()
    rerender(<RttyCockpit snap={snap} active />)
    fireEvent.keyDown(window, { key: 'Escape' })
    expect(rttyStop).toHaveBeenCalled()
    expect(haltTx).toHaveBeenCalled()
  })

  it('surfaces a keyer failure from the poll', async () => {
    getRttyState.mockResolvedValue({
      ...IDLE,
      keyerError: 'FSK keyline: couldn’t open COM7.',
    })
    render(<RttyCockpit snap={snap} />)
    await screen.findByRole('alert')
    expect(screen.getByRole('alert').textContent).toContain('FSK keyline')
  })
})

describe('confidenceRuns', () => {
  it('groups equal-confidence chars into runs and fades the low ones', () => {
    expect(confidenceRuns('ABCD', [90, 90, 20, 20])).toEqual([
      { text: 'AB', opacity: 1 },
      { text: 'CD', opacity: 0.3 },
    ])
  })

  it('treats missing confidence as solid — decoded text is never hidden', () => {
    expect(confidenceRuns('AB', [])).toEqual([{ text: 'AB', opacity: 1 }])
  })
})

// The RTTY field-hang class: the transcript ring holds RTTY_TEXT_CAP (4000)
// characters, and confidence is a CONTINUOUS per-character slicer margin, so
// degraded copy crosses the fade thresholds constantly. Grouping strictly by
// equal level therefore produced ~1000–1900 spans on marginal copy (one span
// per character in the worst case) — re-rendered whole twice a second, because
// the ring drains from the FRONT and shifts every span's text. Clean copy
// collapsed to one span, which is why this never showed on the bench: the cost
// is gated on band conditions, exactly matching an intermittent field report.
describe('confidenceRuns bounds the transcript (the RTTY hang class)', () => {
  /** A full ring, worst case: every character flips the confidence bucket. */
  const adversarial = (n: number) => ({
    text: 'A'.repeat(n),
    conf: Array.from({ length: n }, (_, i) => (i % 2 === 0 ? 95 : 5)),
  })

  it('caps the run count on a full adversarial ring', () => {
    const { text, conf } = adversarial(4000)
    expect(confidenceRuns(text, conf).length).toBeLessThanOrEqual(RTTY_MAX_RUNS)
  })

  it('caps the rendered spans, not just the runs', async () => {
    const { text, conf } = adversarial(4000)
    getRttyState.mockResolvedValue({ ...IDLE, armed: true, text, charConf: conf })
    const { container } = render(<RttyCockpit snap={snap} />)
    await waitFor(() =>
      expect(container.querySelectorAll('.cw-decode-text span').length).toBeGreaterThan(0),
    )
    expect(container.querySelectorAll('.cw-decode-text span').length).toBeLessThanOrEqual(
      RTTY_MAX_RUNS,
    )
  })

  it('keeps every decoded character — the cap coarsens the fade, never the text', () => {
    const { text, conf } = adversarial(4000)
    expect(
      confidenceRuns(text, conf)
        .map((r) => r.text)
        .join(''),
    ).toBe(text)
  })

  it('still fades a genuinely weak stretch inside a full ring', () => {
    // 4000 chars: the last quarter copied badly. The cap must not flatten a
    // real quality change into one solid block.
    const n = 4000
    const conf = Array.from({ length: n }, (_, i) => (i < n * 0.75 ? 95 : 10))
    const runs = confidenceRuns('A'.repeat(n), conf)
    expect(runs.length).toBeGreaterThan(1)
    expect(runs[0].opacity).toBe(1)
    expect(runs[runs.length - 1].opacity).toBeLessThan(1)
  })

  // THE SHORT-BURST CASE, which is the normal RTTY error shape: a few characters
  // of a callsign or an exchange element get mangled, not a whole quarter of the
  // transcript. Scoring the fade over blocks UNCONDITIONALLY averaged a burst
  // this short back above the threshold and rendered it fully solid — the
  // operator was shown certainty the decoder never reported. Verified failing
  // first: against the always-block version EVERY burst below rendered the whole
  // 4000-char ring as one fully-solid span (block = 20, so 10 zero-confidence
  // characters split 5/5 across a boundary still averaged to exactly 75 — the
  // solid threshold). Ten characters is over a second of copy at 45.45 baud.
  it('fades a SHORT bad burst inside an otherwise-clean full ring', () => {
    const n = 4000
    for (const burst of [1, 4, 7, 10]) {
      const conf = Array.from({ length: n }, () => 100)
      // Straddling a 20-char block boundary is the alignment that hid it best.
      const start = 1015
      for (let i = start; i < start + burst; i++) conf[i] = 0
      const runs = confidenceRuns('A'.repeat(n), conf)
      expect(runs.length).toBeLessThanOrEqual(RTTY_MAX_RUNS)
      const faded = runs.filter((r) => r.opacity < 1)
      expect(faded.map((r) => r.text.length).reduce((a, b) => a + b, 0)).toBe(burst)
      expect(Math.min(...runs.map((r) => r.opacity))).toBe(0.3)
    }
  })
})
