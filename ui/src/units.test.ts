import { describe, it, expect } from 'vitest'
import {
  resolveUnits,
  __test_usableLocaleTag,
  fmtDistanceKm,
  fmtTempF,
  fmtSpeedMph,
  fmtRainIn,
  fmtKmTokens,
  fmtSpeedKmS,
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

// ---------------------------------------------------------------------------
// Distances inside sentences the backend writes (2026-10-06)
// ---------------------------------------------------------------------------
//
// The Chase line ("heard by K9LC (EN52, 26 km)"), Journey's distance firsts and feats and the
// sporadic-E watch were composed in Rust with the unit already in them, so they read in km or
// miles whatever Units said. The backend now writes each distance as a km token (`km_token`,
// crates/propagation/src/geo.rs) and the screen writes the figure. The token texts below are the
// backend's own shapes, full precision and all.
describe('distances inside backend sentences', () => {
  it('writes each km token in the units it is given', () => {
    const line = 'heard by K9LC (EN62, {km:111.19492664455873}) + N9CO (EN52, {km:166.9})'
    expect(fmtKmTokens(line, 'imperial')).toBe('heard by K9LC (EN62, 69 mi) + N9CO (EN52, 104 mi)')
    expect(fmtKmTokens(line, 'metric')).toBe('heard by K9LC (EN62, 111 km) + N9CO (EN52, 167 km)')
  })

  it('writes a span with one unit', () => {
    const watch = 'Es is minutes-long, {km:500-2500}'
    expect(fmtKmTokens(watch, 'metric')).toBe('Es is minutes-long, 500–2500 km')
    expect(fmtKmTokens(watch, 'imperial')).toBe('Es is minutes-long, 311–1553 mi')
  })

  it('rounds once, from the full kilometres', () => {
    // 41.4 km is 25.7 mi. Rounded to 41 km first, it would read 25 mi.
    expect(fmtKmTokens('{km:41.4}', 'imperial')).toBe('26 mi')
  })

  it('leaves a sentence without a token as it came', () => {
    // Every other evidence line, and anything an older backend wrote with its own "26 km".
    expect(fmtKmTokens('spotted by K9IMM via RBN', 'imperial')).toBe('spotted by K9IMM via RBN')
    expect(fmtKmTokens('heard by K9LC (EN52, 26 km)', 'imperial')).toBe('heard by K9LC (EN52, 26 km)')
  })
})

describe('a satellite range-rate', () => {
  it('reads in mi/s on Imperial and km/s on Metric, sign and all', () => {
    expect(fmtSpeedKmS(1.234, 'metric')).toBe('1.23 km/s')
    expect(fmtSpeedKmS(1.234, 'imperial')).toBe('0.77 mi/s')
    expect(fmtSpeedKmS(-6.5, 'metric')).toBe('-6.50 km/s')
    expect(fmtSpeedKmS(-6.5, 'imperial')).toBe('-4.04 mi/s')
  })
})
