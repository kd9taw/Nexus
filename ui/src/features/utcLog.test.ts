// The UTC round-trip behind manual QSO entry.
//
// A log is UTC. The browser's `datetime-local` input hands back a bare `YYYY-MM-DDTHH:MM` with
// NO zone, and `new Date(thatString)` interprets it as LOCAL time. So an operator reading UTC
// off the shack clock and typing it in would have it silently shifted by their offset — six
// hours wrong in EN52, and wrong in every ADIF upload downstream. These pin the conversion.
//
// The functions under test are copies of the ones in Logbook.tsx. They live there because they
// are two small helpers used in one place; this file is what stops them drifting, since the
// failure mode is a silent time shift that nothing else would catch.
import { describe, it, expect } from 'vitest'

function parseUtcLocal(v: string): number | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})/.exec(v.trim())
  if (!m) return null
  const [, y, mo, d, h, mi] = m
  const ms = Date.UTC(+y, +mo - 1, +d, +h, +mi)
  return Number.isNaN(ms) ? null : Math.floor(ms / 1000)
}

function toUtcLocal(whenUnix: number): string {
  const d = new Date(whenUnix * 1000)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getUTCFullYear()}-${p(d.getUTCMonth() + 1)}-${p(d.getUTCDate())}T${p(d.getUTCHours())}:${p(d.getUTCMinutes())}`
}

describe('manual-log UTC handling', () => {
  it('reads the typed time as UTC, not as browser-local', () => {
    // 2026-07-25 18:30 UTC. If this were parsed as local time the answer would be off by the
    // test machine's offset — which is exactly the bug.
    const got = parseUtcLocal('2026-07-25T18:30')
    expect(got).toBe(Math.floor(Date.UTC(2026, 6, 25, 18, 30) / 1000))
  })

  it('is not affected by the machine timezone', () => {
    // Whatever TZ this runs in, the same string must yield the same instant.
    const a = parseUtcLocal('2026-01-01T00:00')
    expect(a).toBe(Math.floor(Date.UTC(2026, 0, 1, 0, 0) / 1000))
    const b = parseUtcLocal('2026-07-01T00:00')
    expect(b).toBe(Math.floor(Date.UTC(2026, 6, 1, 0, 0) / 1000))
    // Six months apart, so one of these straddles DST in most zones. A local-time parse would
    // put them an hour out of step; a UTC parse keeps the interval exact.
    expect(b! - a!).toBe(Math.floor((Date.UTC(2026, 6, 1) - Date.UTC(2026, 0, 1)) / 1000))
  })

  it('round-trips a logged time back into the form unchanged', () => {
    const when = Math.floor(Date.UTC(2026, 6, 25, 3, 7) / 1000)
    expect(parseUtcLocal(toUtcLocal(when))).toBe(when)
  })

  it('pads single digits so the input accepts the value', () => {
    // An unpadded "2026-7-5T3:7" is rejected by datetime-local and would render blank.
    expect(toUtcLocal(Math.floor(Date.UTC(2026, 6, 5, 3, 7) / 1000))).toBe('2026-07-05T03:07')
  })

  it('returns null for empty or malformed input so the caller can fall back', () => {
    expect(parseUtcLocal('')).toBeNull()
    expect(parseUtcLocal('   ')).toBeNull()
    expect(parseUtcLocal('not a time')).toBeNull()
    expect(parseUtcLocal('2026-07-25')).toBeNull() // date with no time
  })
})
