import { describe, it, expect } from 'vitest'
import {
  resolveUnits,
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
