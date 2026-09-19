// The UTC round-trip behind manual QSO entry.
//
// A log is UTC. Parse a zoneless date + time with `new Date(thatString)` and it reads as LOCAL
// time, so an operator reading UTC off the shack clock and typing it in would have it silently
// shifted by their offset — six hours wrong in EN52, and wrong in every ADIF upload downstream.
// And draw the time through the OS locale and a 12-hour PC shows 00:58 as "12:58 AM" (#280).
// These pin the conversion, on the module both forms use (this file used to test copies).
import { describe, it, expect } from 'vitest'
import { parseUtcTime, utcDate, utcDateTimeToUnix, utcTime } from './utcLog'

const at = (y: number, mo: number, d: number, h: number, m: number, s = 0) =>
  Math.floor(Date.UTC(y, mo - 1, d, h, m, s) / 1000)

describe('manual-log UTC handling', () => {
  it('reads the typed time as UTC, not as browser-local', () => {
    // 2026-07-25 18:30 UTC. If this were parsed as local time the answer would be off by the
    // test machine's offset — which is exactly the bug.
    expect(utcDateTimeToUnix('2026-07-25', '18:30')).toBe(at(2026, 7, 25, 18, 30))
  })

  it('is not affected by the machine timezone', () => {
    // Whatever TZ this runs in, the same strings must yield the same instant.
    const a = utcDateTimeToUnix('2026-01-01', '00:00')
    expect(a).toBe(at(2026, 1, 1, 0, 0))
    const b = utcDateTimeToUnix('2026-07-01', '00:00')
    expect(b).toBe(at(2026, 7, 1, 0, 0))
    // Six months apart, so one of these straddles DST in most zones. A local-time parse would
    // put them an hour out of step; a UTC parse keeps the interval exact.
    expect(b! - a!).toBe(Math.floor((Date.UTC(2026, 6, 1) - Date.UTC(2026, 0, 1)) / 1000))
  })

  it('round-trips a logged time back into the form unchanged — to the second', () => {
    for (const when of [at(2026, 7, 25, 3, 7), at(2026, 9, 14, 0, 58, 37), at(2026, 12, 31, 23, 59, 59)]) {
      expect(utcDateTimeToUnix(utcDate(when), utcTime(when))).toBe(when)
    }
  })

  it('pads single digits, and shows seconds only when there are some', () => {
    expect(utcDate(at(2026, 7, 5, 3, 7))).toBe('2026-07-05')
    expect(utcTime(at(2026, 7, 5, 3, 7))).toBe('03:07')
    expect(utcTime(at(2026, 9, 14, 0, 58))).toBe('00:58')
    expect(utcTime(at(2026, 9, 14, 0, 58, 5))).toBe('00:58:05')
  })

  it('returns null for empty or malformed input so the caller can refuse it', () => {
    expect(utcDateTimeToUnix('', '')).toBeNull()
    expect(utcDateTimeToUnix('   ', '   ')).toBeNull()
    expect(utcDateTimeToUnix('not a date', '12:00')).toBeNull()
    expect(utcDateTimeToUnix('2026-07-25', '')).toBeNull() // date with no time
    expect(utcDateTimeToUnix('', '18:30')).toBeNull() // time with no date
    // Date.UTC would roll these forward to a day nobody typed.
    expect(utcDateTimeToUnix('2026-02-30', '12:00')).toBeNull()
    expect(utcDateTimeToUnix('2026-13-01', '12:00')).toBeNull()
  })
})

describe('the 24-hour UTC time box', () => {
  it('takes HH:MM, H:MM and HH:MM:SS', () => {
    expect(parseUtcTime('00:58')).toEqual({ h: 0, m: 58, s: null })
    expect(parseUtcTime('7:05')).toEqual({ h: 7, m: 5, s: null })
    expect(parseUtcTime(' 23:59:59 ')).toEqual({ h: 23, m: 59, s: 59 })
    expect(utcDateTimeToUnix('2026-09-14', '00:58:37')).toBe(at(2026, 9, 14, 0, 58, 37))
  })

  it('refuses what is not a 24-hour time: 25:00, 24:00, 12:60, 12:58:60, 1:5, and 12-hour forms', () => {
    for (const bad of ['25:00', '24:00', '12:60', '12:58:60', '1:5', '12:58 AM', '12:58pm', '0058', '12', '']) {
      expect(parseUtcTime(bad), bad).toBeNull()
      expect(utcDateTimeToUnix('2026-09-14', bad), bad).toBeNull()
    }
  })
})
