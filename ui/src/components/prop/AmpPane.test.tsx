// @vitest-environment jsdom
//
// The Amplifier pane's honesty rules, computed by rendering it.
//
// Every assertion here is a way this pane could lie to an operator about a kilowatt, and each
// one names the rule it holds. The last two are safety rather than honesty: this pane renders
// no control at all, so it can never become a thing an operator reaches for to stop an over.
import { describe, it, expect, afterEach } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import { AmpPane } from './AmpPane'
import type { AmpStatus } from '../../types'

afterEach(cleanup)

const live = (over: Partial<AmpStatus> = {}): AmpStatus => ({
  family: 'spe',
  model: '15K',
  linked: true,
  reason: '',
  operate: true,
  transmitting: true,
  outputWatts: 1200,
  swr: 1.4,
  swrAtu: 1.1,
  volts: 48.0,
  amps: 32.5,
  temp: 41,
  tempCelsius: false,
  alarm: 'none',
  alarmRaised: false,
  warning: 'none',
  warningRaised: false,
  kpaFault: null,
  ...over,
})

describe('the amplifier pane tells the truth about a kilowatt', () => {
  it('renders NOTHING when no amplifier is configured — an empty frame would be an ornament', () => {
    const { container } = render(<AmpPane amp={null} />)
    expect(container.innerHTML).toBe('')
    // The other absence spelling the wire can produce.
    cleanup()
    const u = render(<AmpPane amp={undefined} />)
    expect(u.container.innerHTML).toBe('')
  })

  it('KEEPS the pane when the amplifier is configured and silent, and says why', () => {
    // The rotator's shipped defect, not repeated: `if (x == null) return null` deleted a whole
    // control surface the moment a readback failed.
    render(<AmpPane amp={live({ linked: false, reason: 'portBusy' })} />)
    expect(screen.getByText(/port in use/i)).toBeTruthy()
  })

  it('shows NO stale reading from a dead link — every value is an em dash', () => {
    // The backend already clears each reading on the first failed poll. This is the second
    // half of the same rule: even a value that somehow survived must not reach the screen.
    const { container } = render(
      // Deliberately a status carrying full readings AND linked:false — the shape a bug
      // upstream would produce. Nothing may render from it.
      <AmpPane amp={live({ linked: false, reason: 'noAnswer' })} />,
    )
    expect(container.textContent).not.toMatch(/1200|1\.4:1|48\.0|32\.5/)
    expect(container.querySelectorAll('.amp-v').length).toBeGreaterThan(3)
    for (const v of container.querySelectorAll('.amp-v')) expect(v.textContent).toBe('—')

    // POSITIVE CONTROL: the identical readings DO render when the link is up, so the
    // assertion above is reading the gate and not an empty pane.
    cleanup()
    const up = render(<AmpPane amp={live()} />)
    expect(up.container.textContent).toMatch(/1200 W/)
    expect(up.container.textContent).toMatch(/1\.4:1/)
  })

  it('prints NO scale letter on an SPE temperature, and does print one for the KPA', () => {
    // §5 says "Temp in °C or F" — the amplifier reports whatever its own front panel is set
    // to and the wire does not say which. A guessed °C is a false statement half the time.
    const spe = render(<AmpPane amp={live({ temp: 41, tempCelsius: false })} />)
    expect(spe.container.textContent).toMatch(/41°/)
    expect(spe.container.textContent).not.toMatch(/41 ?°C|41 ?°F/)
    cleanup()
    // Elecraft's ^TM IS documented Celsius, so this one is labelled.
    const kpa = render(<AmpPane amp={live({ family: 'kpa', temp: 52, tempCelsius: true })} />)
    expect(kpa.container.textContent).toMatch(/52 °C/)
  })

  it('rounds every number instead of interpolating a raw float', () => {
    // `invariantNumber` is `String(n)`, so an unrounded f32 SWR would print in full.
    const { container } = render(
      <AmpPane amp={live({ swr: 1.2000000476837158, outputWatts: 1199.7 as unknown as number })} />,
    )
    expect(container.textContent).not.toMatch(/1\.2000000/)
    expect(container.textContent).toMatch(/1\.2:1/)
    expect(container.textContent).toMatch(/1200 W/)
  })

  it('⭐ renders an alarm the amplifier did not name as a FAULT, never as silence', () => {
    // The failure direction of a status decoder in front of a kilowatt has to be toward
    // reporting a fault. A later firmware's new alarm letter arrives as the tag `unknown`.
    const { container } = render(
      <AmpPane amp={live({ alarm: 'unknown', alarmRaised: true })} />,
    )
    expect(container.querySelector('.amp-alarm')).not.toBeNull()
    expect(container.textContent).toMatch(/did not name/i)

    // And a tag this build has never seen at all still raises, because the render keys off
    // `alarmRaised` — the amplifier's own judgement — and not off a tag comparison.
    cleanup()
    const future = render(
      <AmpPane amp={live({ alarm: 'somethingShippedIn2030', alarmRaised: true })} />,
    )
    expect(future.container.querySelector('.amp-alarm')).not.toBeNull()

    // POSITIVE CONTROL: a quiet amplifier shows no fault line, so the assertion is not
    // simply always true.
    cleanup()
    const quiet = render(<AmpPane amp={live()} />)
    expect(quiet.container.querySelector('.amp-fault')).toBeNull()
  })

  it('omits the pre-ATU SWR cell entirely for a family that has none', () => {
    // A permanent '—' beside a meter the amplifier does not have reads as a broken reading.
    const kpa = render(<AmpPane amp={live({ family: 'kpa', swrAtu: null })} />)
    expect(kpa.container.textContent).not.toMatch(/ATU/)
    cleanup()
    const spe = render(<AmpPane amp={live()} />)
    expect(spe.container.textContent).toMatch(/ATU/)
  })

  it('⛔ renders NO control of any kind — it can never become a stop an operator relies on', () => {
    // Putting an amplifier in standby is not a way to stop a transmission: the exciter keeps
    // keying and the drive passes straight through. So this pane has no button, and nothing
    // in it may ever appear on a cockpit's stop-line census.
    for (const amp of [live(), live({ linked: false, reason: 'noAnswer' })]) {
      const { container } = render(<AmpPane amp={amp} />)
      expect(container.querySelectorAll('button, input, select, textarea, a[href]').length).toBe(0)
      cleanup()
    }
  })

  it('has no accessible name a stop-line sweep could mistake for a stop control', () => {
    // The sweeps match /^stop tx$/i, /^tune$|^tuning…$/i, /^esc\s*stop$/i and /^stop$/i by
    // accessible name. Nothing here may read as one of those — which is why an ATU state is
    // never shortened to "Tune" and no readout is ever labelled "Stop".
    const { container } = render(<AmpPane amp={live({ alarmRaised: true, alarm: 'combinerFault' })} />)
    const words = (container.textContent ?? '').split(/\s+/).filter(Boolean)
    for (const w of words) {
      expect(/^(stop|tune|tuning…)$/i.test(w), `"${w}" collides with a stop-sweep matcher`).toBe(
        false,
      )
    }
    // CONTROL: the scan really has words to look at.
    expect(words.length).toBeGreaterThan(5)
  })
})
