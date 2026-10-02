// @vitest-environment jsdom
//
// THE AMPLIFIER STRIP LOCKS WHENEVER THE TRANSMITTER IS ON THE AIR, WHOEVER KEYED IT.
//
// Stepping an amplifier's band under drive pits relays and can take a PA out, so the strip's
// Operate and band buttons go dead while keyed (AmpStrip.tsx, property 3). The amplifier's own
// transmit flag is used when it has one; an Elecraft KPA reports none, and an SPE's idle flag
// must never override a keyed exciter, so the strip also takes the radio's keyed state from its
// one host, this header. That was `transmitting || rigKeyed`: the FT slot flag and a key at the
// radio. A voice over, CW, RTTY, PSK, SSTV, the tune carrier and Nexus's own PTT were missing,
// so during them the buttons stayed live. The header now hands it `isOnAir()`, the answer its ON
// AIR sign shows (operator, 2026-09-27: "Tighten it"). It only refuses more: nothing keys
// differently, and the poll thread's own refusal is still what guards the hardware.
//
// AmpStrip.test.tsx proves the strip obeys `radioTransmitting`; this proves the header passes
// the right answer, which the strip's own tests cannot see.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup, screen, within } from '@testing-library/react'
import { CockpitHeader } from './CockpitHeader'
import { CockpitTxStrip } from './CockpitTxStrip'
import type { AmpStatus, AppSnapshot, RadioStatus } from '../types'

// Derived from the real module (AmpStrip.test.tsx's rule): a partial mock leaves every other
// export undefined for whatever the header mounts.
vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  ampCommand: vi.fn(async () => true),
}))

afterEach(cleanup)

/** A linked amplifier. `transmitting: null` is an Elecraft KPA (no transmit flag, no band);
 *  `false` is an SPE reporting idle. */
const amp = (transmitting: boolean | null): AmpStatus =>
  ({
    family: transmitting === null ? 'kpa' : 'spe',
    model: transmitting === null ? '' : '15K',
    linked: true,
    reason: '',
    operate: true,
    transmitting,
    outputWatts: 0,
    bandLabel: transmitting === null ? null : '20m',
    alarm: 'none',
    alarmRaised: false,
    warning: 'none',
    warningRaised: false,
  }) as AmpStatus

function ampButtons(over: Partial<RadioStatus>, ampTx: boolean | null): HTMLButtonElement[] {
  const snap = {
    activeRadioId: 0,
    radio: {
      dialMhz: 14.2, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: true,
      tuning: false, txAllowed: true, txBusyReason: null, rigKeyed: false, amp: amp(ampTx), ...over,
    },
  } as unknown as AppSnapshot
  render(<CockpitHeader snap={snap} modeIndicator={<span>SSB</span>} bandControl={<span>—</span>} />)
  const strip = screen.getByRole('group', { name: /amplifier/i })
  return within(strip).getAllByRole('button') as HTMLButtonElement[]
}

/** Every over Nexus keys itself, as the snapshot reports it: `transmitting` false, the rig's
 *  read-back false, only the arbiter saying so. The sentences are the engine's own. */
const NEXUS_KEYED: [string, Partial<RadioStatus>][] = [
  ['a Phone voice over', { txBusyReason: 'A voice message is transmitting — stop it first' }],
  ['the PTT button held in Nexus', { txBusyReason: 'Mic PTT is held — release it first' }],
  ['a CW over', { txBusyReason: 'CW is sending — stop it first' }],
  ['an RTTY over', { txBusyReason: 'RTTY is transmitting — stop it first' }],
  ['a PSK over', { txBusyReason: 'PSK is transmitting — stop it first' }],
  ['an SSTV image', { txBusyReason: 'An SSTV image is transmitting — stop it first' }],
  ['the tune carrier', { tuning: true, txBusyReason: 'Tune carrier is up — stop tuning first' }],
]
const AMPS: [string, boolean | null][] = [
  ['no transmit flag (Elecraft)', null],
  ['its own flag saying idle (SPE)', false],
]
const cases = NEXUS_KEYED.flatMap(([what, over]) => AMPS.map(([kind, tx]) => [what, kind, over, tx] as const))

describe("the header's amplifier strip locks during every over", () => {
  it.each(cases)('%s, with an amplifier reporting %s: every button disabled', (_what, _kind, over, tx) => {
    const buttons = ampButtons(over, tx)
    expect(buttons.length, 'Operate, band down, band up').toBe(3)
    for (const b of buttons) expect(b.disabled, `${b.getAttribute('aria-label') ?? b.textContent} is live while keyed`).toBe(true)
  })

  it.each(AMPS)('nothing keyed, amplifier reporting %s: the buttons are live (the lock can say no)', (_kind, tx) => {
    for (const b of ampButtons({}, tx)) expect(b.disabled, b.getAttribute('aria-label') ?? b.textContent ?? '').toBe(false)
  })

  it.each([
    ['an FT over', { transmitting: true }],
    ['a key held at the radio', { rigKeyed: true }],
  ] as [string, Partial<RadioStatus>][])('%s still locks it, as before', (_what, over) => {
    for (const b of ampButtons(over, null)) expect(b.disabled).toBe(true)
  })

  // The ON AIR sign waits out an over armed through the stream (the operator's pick "Header ON AIR
  // waits too", 2026-09-28). That is the SIGN only: the armed over owns the transmitter, and the
  // strip keeps reading the arbiter, so it stays locked. The sign left the header for the TX strip
  // under the scope (2026-10-01, `CockpitTxStrip`), so the premise reads it there.
  it.each(AMPS)('an over armed through the stream, which the sign waits out, still locks it (amplifier reporting %s)', (_kind, tx) => {
    const over = { streamMic: 'armed', txBusyReason: 'The Remote microphone is transmitting — stop it first' } as const
    const buttons = ampButtons(over, tx)
    render(<CockpitTxStrip radio={{ transmitting: false, txEnabled: true, tuning: false, rigKeyed: false, ...over } as unknown as RadioStatus} />)
    expect(document.querySelector('.cockpit-txstrip .cq-statecap')?.classList.contains('tx'), 'premise: the sign waits').toBe(false)
    for (const b of buttons) expect(b.disabled, `${b.getAttribute('aria-label') ?? b.textContent} is live under an armed over`).toBe(true)
  })
})
