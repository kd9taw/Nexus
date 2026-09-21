// @vitest-environment jsdom
//
// THE THREE ANALOG LEVELS — AF gain, RF gain, squelch.
//
// Nexus had the DSP half of a radio and not the analog half: filter width, AGC, NR, notch,
// VOX, speech processor, mic gain and RF power were all present and polled with read-back,
// while the three controls a voice operator rides continuously were absent from the CAT layer
// entirely. That is what a digital-first history predicts — running FT8 the computer does the
// listening, so nobody ever reached for AF or RF gain.
//
// WHAT THIS FILE PINS, in the shape `PhoneCockpit.notch.test.tsx` established for #95: each
// control renders ONLY when the rig reports its own field, so no radio grows a slider with
// nothing behind it; and the decode-audio warning appears exactly in the zone where it is
// true and not otherwise.
//
// ⚠️ WHAT IT DOES NOT COVER, written down rather than implied. Every case here mounts with
// station CONTROL held, so each slider reads its mirrored `useState` and the
// `levels.draft(…) ?? snap.radio.…` branch the remote OBSERVER takes is never evaluated —
// verified by mutation: swapping the field in that branch leaves all 18 green, while
// swapping it in the mirroring effect turns three of them red. The observer path for the
// pre-existing levels is covered by `remote-web/RadioLevels.test.tsx`, and the three added
// here ride the identical `fields` map entry.
import { describe, it, expect, afterEach, beforeAll, vi } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import type { AppSnapshot } from '../types'

// Auto-mock every api export rather than listing the ones this file uses. THE AF SLIDER IS A
// CHILD OF `CockpitHeader`, so unlike `PhoneCockpit.notch.test.tsx` the header cannot be
// stubbed away here — and mounting it for real pulls in RotorStrip and the band plan, whose
// api calls a hand-written mock silently omits. Listing them was the first version of this
// file and it failed on `getSettings`. (`RadioLevels.test.tsx` uses the same shape.)
vi.mock('../api', async original => {
  const actual = await original<Record<string, unknown>>()
  const reads: Record<string, unknown> = { getLicensedBandPlan: [], getBandPlan: [], getCatCwUnprovenRigModels: [] }
  return Object.fromEntries(Object.entries(actual).map(([name, value]) => [name,
    typeof value === 'function' ? vi.fn(async () => structuredClone(reads[name] ?? {})) : value]))
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
// The scope, band strip, keyer and log form are not what this file is about. CockpitHeader is
// deliberately NOT stubbed — the AF slider renders inside it.
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="vk-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

beforeAll(() => {
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(cleanup)

/**
 * A Phone-cockpit snapshot reporting exactly the analog surface given.
 *
 * ⚠️ NO DEFAULTED LEVELS. Every field under test is supplied by the caller and nothing here
 * fills one in — a default parameter would fire on an explicit `undefined` and quietly turn a
 * negative case into a second copy of the positive one.
 */
function snapWith(radio: Record<string, unknown>): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    radio: {
      dialMhz: 14.2,
      band: '20m',
      sideband: 'USB',
      transmitting: false,
      txEnabled: false,
      txAllowed: true,
      slot: 0,
      nextSlotMs: 0,
      ...radio,
    },
    link: {},
    qso: {},
    stations: [],
    conversations: [],
  } as unknown as AppSnapshot
}

/** `phoneMode` is a PROP, not a snapshot field — it is what resolves the cockpit's
 *  commanded mode to FM, and passing it is the only way to reach the FM branch. */
const mount = (radio: Record<string, unknown>, phoneMode?: string) =>
  render(<PhoneCockpit snap={snapWith(radio)} theme="dark" phoneMode={phoneMode} />)

describe('the analog levels a voice operator rides', () => {
  it('offers an AF gain slider in the header when the rig reports one', () => {
    mount({ afGain: 0.62 })
    const slider = screen.getByLabelText('AF gain') as HTMLInputElement
    expect(slider.value).toBe('62')
    expect(slider.min).toBe('0')
    expect(slider.max).toBe('100')
  })

  it('offers an RF gain slider when the rig reports one', () => {
    mount({ rfGain: 0.85 })
    expect((screen.getByLabelText('RF gain') as HTMLInputElement).value).toBe('85')
  })

  it('offers a squelch slider when the rig reports one', () => {
    mount({ squelch: 0.4 })
    expect((screen.getByLabelText('Squelch') as HTMLInputElement).value).toBe('40')
  })

  it('shows the RIG’S value, not a default the component was seeded with', () => {
    // The three useState seeds are 50 / 100 / 0. Reading any of those back here would mean
    // the slider is showing the component's own idea rather than the radio's, which is the
    // failure the whole desired-vs-observed split exists to prevent — so every value below
    // is deliberately NOT its control's seed.
    mount({ afGain: 0.11, rfGain: 0.22, squelch: 0.33 })
    expect((screen.getByLabelText('AF gain') as HTMLInputElement).value).toBe('11')
    expect((screen.getByLabelText('RF gain') as HTMLInputElement).value).toBe('22')
    expect((screen.getByLabelText('Squelch') as HTMLInputElement).value).toBe('33')
  })

  it('keeps RF gain and transmit power apart', () => {
    // `RF` and `RFPOWER` are different Hamlib levels and different knobs on the radio.
    // Reporting only RF gain must not put a value into the transmit-power slider.
    //
    // ⚠️ REWRITTEN WITH THE 2026-09-20 RULING, because its old second line used the ABSENCE
    // of the AF slider as its proxy — and under the ruling nothing is absent any more, so
    // the proxy would have gone permanently red while saying nothing about RF-versus-power.
    // What it was always reaching for is asserted directly instead: this rig's 0.3 RF gain
    // reads on the RF control and NOWHERE ELSE. 30% appearing on the power slider would mean
    // the receive gain had been plumbed into the transmitter — which is the whole case.
    mount({ rfGain: 0.3 })
    expect((screen.getByLabelText('RF gain') as HTMLInputElement).value).toBe('30')
    const power = screen.getByLabelText('Power') as HTMLInputElement
    expect(power.value, 'the receive RF gain reached the transmit-power slider').not.toBe('30')
    // …and they are not even in the same place any more: RF gain is a receive control and
    // lives in the receive chain, while power stays in the header with the transmit chrome.
    expect(screen.getByLabelText('RF gain').closest('[data-pane]')!.getAttribute('data-pane')).toBe('receiver')
    expect(power.closest('[data-pane]'), 'the power slider drifted into a pane').toBeNull()
  })
})

// ⭐ THESE FOUR USED TO PIN THE OPPOSITE, AND THE REASON THEY GAVE WAS OVERRULED, NOT FORGOTTEN.
//
// The reason, from this file's twin and quoted in it: *"a control that does nothing is worse
// than none. An auto-notch-only radio must not grow a frequency slider with nothing behind
// it."* It applied here unchanged — a live AF slider over a radio with no CAT AF gain is a lie
// about the radio.
//
// OPERATOR RULING, 2026-09-20: overruled. A control that VANISHES is indistinguishable from
// one that was never built, and of the three things an absent AF slider could mean — the radio
// has no CAT AF gain, the backend does not expose it, Nexus never wrote it — the operator acts
// on the third, because it is the only one he can do anything about. The argument survives as
// a CONSTRAINT: the control must be visibly dead and must say why. That is what each assertion
// below now demands, and it is three things — present AND disabled AND the reason — because
// any two of them pass on a half-built control.
describe('and the dead controls it must not HIDE', () => {
  /** Present, disabled, and carrying a reason. */
  function deadAndSaid(el: HTMLElement | null, what: string) {
    expect(el, `${what}: not on screen — a vanished control reads as one Nexus never built`).toBeTruthy()
    expect((el as HTMLInputElement).disabled, `${what}: live over a radio that does not report it`).toBe(true)
    const mark = el!.closest('[data-chain]')?.querySelector('.ph-unavail')
    expect(mark, `${what}: dead and silent — the operator cannot tell "your radio can't" from "Nexus didn't"`).not.toBeNull()
    expect(mark!.textContent, `${what}: the reason is not text`).toMatch(/\S/)
  }

  it('shows the AF slider DISABLED, with the reason, on a rig that does not report one', () => {
    mount({ rfGain: 0.5, squelch: 0.5 })
    deadAndSaid(screen.queryByLabelText('AF gain'), 'AF gain')
    // …and its two neighbours are LIVE in the same render, which is what makes this able to
    // differ: a build that marked the whole pane dead would pass the line above alone.
    expect((screen.getByLabelText('RF gain') as HTMLInputElement).disabled).toBe(false)
    expect((screen.getByLabelText('Squelch') as HTMLInputElement).disabled).toBe(false)
  })

  it('shows the RF gain slider DISABLED, with the reason, on a rig that does not report one', () => {
    mount({ afGain: 0.5, squelch: 0.5 })
    deadAndSaid(screen.queryByLabelText('RF gain'), 'RF gain')
    expect((screen.getByLabelText('AF gain') as HTMLInputElement).disabled).toBe(false)
  })

  it('shows the squelch slider DISABLED, with the reason, on a rig that does not report one', () => {
    mount({ afGain: 0.5, rfGain: 0.5 })
    deadAndSaid(screen.queryByLabelText('Squelch'), 'squelch')
    expect((screen.getByLabelText('AF gain') as HTMLInputElement).disabled).toBe(false)
  })

  it('control: a bare rig grows all three, every one dead and every one saying why', () => {
    // The direction this control guards has INVERTED with the ruling. It used to stop the
    // three "queryBy-toBeNull" assertions passing on a component that rendered nothing at
    // all; the risk now is the mirror image — three LIVE sliders over a radio that reports
    // no level at all, which is the lie the overruled argument was written against.
    mount({})
    deadAndSaid(screen.queryByLabelText('AF gain'), 'AF gain')
    deadAndSaid(screen.queryByLabelText('RF gain'), 'RF gain')
    deadAndSaid(screen.queryByLabelText('Squelch'), 'squelch')
  })

  it('control: a rig whose ONLY level is one of these still gets it, live', () => {
    // The #95 lesson from the other side. It used to be about the PANE's existence being a
    // separate boolean from the control's — a rig reporting squelch and no NR and no AGC had
    // the control built and then never rendered, because `canDspLevels` said the pane could
    // not exist. That whole boolean is gone with the rebuild (the receive pane always
    // renders), so what is left to check is the control itself.
    mount({ squelch: 0.2, nrLevel: undefined, agc: undefined })
    expect((screen.getByLabelText('Squelch') as HTMLInputElement).disabled).toBe(false)
  })
})

describe('the decode-audio hazard', () => {
  // On a station whose soundcard is fed from the rig's speaker or headphone jack, AF gain is
  // also the decoder's audio level; and on most rigs a closed squelch mutes that path too,
  // including the USB feed. Nexus cannot tell the wiring from a capture-device name, so the
  // warning lives at the control.
  const warn = () => screen.queryAllByText('DECODE?')

  it('warns when AF is wound almost all the way down', () => {
    mount({ afGain: 0.02 })
    expect(warn()).toHaveLength(1)
  })

  it('does NOT warn at a normal AF setting', () => {
    mount({ afGain: 0.5 })
    expect(warn()).toHaveLength(0)
  })

  it('warns when the squelch is up on SSB', () => {
    mount({ squelch: 0.35, rigMode: 'USB' })
    expect(warn()).toHaveLength(1)
  })

  it('does NOT warn about a raised squelch on FM, where it is correct operating', () => {
    // THE DISCONFIRMING CASE, and the reason the predicate excludes FM rather than merely
    // ranking it lower: on FM the squelch is supposed to be up, and a warning there would be
    // standing noise on every FM contact.
    mount({ squelch: 0.35 }, 'fm')
    expect(warn()).toHaveLength(0)
  })

  it('control: that SAME squelch on SSB does warn', () => {
    // Pairs with the FM case above. Without it, the FM assertion passes on a predicate that
    // never fires at all — "no warning on FM" would be proving nothing about FM.
    mount({ squelch: 0.35 })
    expect(warn()).toHaveLength(1)
  })

  it('does NOT warn at an open squelch', () => {
    mount({ squelch: 0, rigMode: 'USB' })
    expect(warn()).toHaveLength(0)
  })

  it('warns once per control, so both can be wrong at once', () => {
    mount({ afGain: 0, squelch: 0.6, rigMode: 'USB' })
    expect(warn()).toHaveLength(2)
  })

  it('notifies and never acts — the level the operator set is the level shown', () => {
    // An alert that silently restored or clamped the control would be the worse bug: a zero
    // AF is a legitimate thing to want, and the operator can see their own cabling.
    mount({ afGain: 0, squelch: 0.6, rigMode: 'USB' })
    expect((screen.getByLabelText('AF gain') as HTMLInputElement).value).toBe('0')
    expect((screen.getByLabelText('Squelch') as HTMLInputElement).value).toBe('60')
  })
})
