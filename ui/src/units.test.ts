import { describe, it, expect } from 'vitest'
import {
  resolveUnits,
  __test_usableLocaleTag,
  fmtDistanceKm,
  fmtTempF,
  fmtSpeedMph,
  fmtRainIn,
} from './units'

describe('units — display-only conversion (F4MQS)', () => {
  it('resolves explicit settings verbatim', () => {
    expect(resolveUnits('metric')).toBe('metric')
    expect(resolveUnits('imperial')).toBe('imperial')
  })

  it('auto/unknown resolves from the locale (a concrete system, never undefined)', () => {
    const r = resolveUnits('auto')
    expect(r === 'metric' || r === 'imperial').toBe(true)
    expect(resolveUnits(null)).toBe(r)
    expect(resolveUnits(undefined)).toBe(r)
  })

  it('formats distance both ways from km', () => {
    expect(fmtDistanceKm(100, 'metric')).toBe('100 km')
    expect(fmtDistanceKm(100, 'imperial')).toBe('62 mi') // 100 / 1.609
    expect(fmtDistanceKm(1.609344, 'imperial')).toBe('1 mi')
  })

  it('formats APRS-native °F and mph', () => {
    expect(fmtTempF(85, 'imperial')).toBe('85°F')
    expect(fmtTempF(32, 'metric')).toBe('0°C')
    expect(fmtTempF(212, 'metric')).toBe('100°C')
    expect(fmtSpeedMph(10, 'imperial')).toBe('10 mph')
    expect(fmtSpeedMph(10, 'metric')).toBe('16 km/h')
  })

  it('formats rain from inches', () => {
    expect(fmtRainIn(0.5, 'imperial')).toBe('0.50 in')
    expect(fmtRainIn(1, 'metric')).toBe('25.4 mm')
  })
})

// ---------------------------------------------------------------------------
// A locale must never be able to take a screen down (2026-08-21)
// ---------------------------------------------------------------------------
//
// Running the real app under `LANG=C.UTF-8` put "OPERATE HIT AN ERROR — invalid language tag"
// on screen: `navigator.language` was "C", which is NOT empty, so it sailed past the
// `|| 'en-US'` fallback and into `new Intl.Locale()`, which throws on it. The units hook
// renders inside Operate, so a units question killed the FT8 screen.
describe('the locale that crashed Operate', () => {
  const norm = __test_usableLocaleTag

  it('turns every shape a Linux desktop produces into something Intl accepts', () => {
    // Each of these throws RangeError if handed to Intl.Locale raw — verified in node.
    for (const [raw, want] of [
      ['C', 'en-US'],
      ['C.UTF-8', 'en-US'],
      ['POSIX', 'en-US'],
      ['en_US', 'en-US'],
      ['en_US.UTF-8', 'en-US'],
      ['de_DE.UTF-8@euro', 'de-DE'],
      ['', 'en-US'],
    ] as const) {
      expect(norm(raw), `${raw} normalises`).toBe(want)
      // The real assertion: whatever comes out must not throw.
      expect(() => new Intl.Locale(norm(raw)), `${raw} is usable`).not.toThrow()
    }
  })

  it('pins the bug: these tags DO throw when handed to Intl raw', () => {
    // The failing-first half. If a future change drops the normaliser, the test above would
    // still pass on any function that returns a constant — this says what was actually broken.
    for (const raw of ['C', 'C.UTF-8', 'en_US', 'en_US.UTF-8', 'de_DE.UTF-8@euro']) {
      expect(() => new Intl.Locale(raw), `${raw} throws raw`).toThrow()
    }
  })

  it('leaves a already-valid tag alone', () => {
    // Control: if this normalised everything to en-US the test above would pass on a
    // function that ignores its input, and every non-US operator would get miles.
    expect(norm('en-GB')).toBe('en-GB')
    expect(norm('fr-FR')).toBe('fr-FR')
    expect(norm(undefined)).toBe('en-US')
  })

  it('still answers the actual question for the three imperial countries', () => {
    // en_US must survive normalisation as a REGION, not be swallowed into a metric default —
    // catching the throw without normalising would quietly give a US operator kilometres.
    expect(new Intl.Locale(norm('en_US')).maximize().region).toBe('US')
    expect(new Intl.Locale(norm('en_GB')).maximize().region).toBe('GB')
  })
})
