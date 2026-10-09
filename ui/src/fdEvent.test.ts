// The event-date MATH left this module (it lives in Rust — fd_rules data +
// FdRuleset::next_or_running; the deleted TS walk hardcoded 24 h and dropped
// WFD's final six hours). What remains here is pure labeling over a window the
// backend supplies, so these tests feed explicit windows.
import { describe, it, expect } from 'vitest'
import { CONTESTS, clubSyncRefusal, clubSyncRefusalText, contestClubRefusal, fdEventFromWindow, fdCountdownLabel, fdHeaderSubtitle } from './fdEvent'

// The real 2026 windows, as the Rust side computes them (pinned there in
// crates/tempo-core/src/fd_rules.rs::event_windows_are_algorithmic_and_dodge_the_feb_spill).
const SFD_START = Date.UTC(2026, 5, 27, 18, 0, 0) / 1000 // Sat Jun 27 1800Z
const SFD_END = SFD_START + 27 * 3600 // Sun Jun 28 2100Z
const WFD_START = Date.UTC(2026, 0, 24, 16, 0, 0) / 1000 // Sat Jan 24 1600Z
const WFD_END = WFD_START + 30 * 3600 // Sun Jan 25 2200Z

describe('fdEventFromWindow', () => {
  it('builds the event from the backend window, label + year included', () => {
    const ev = fdEventFromWindow('wfd', WFD_START, WFD_END)!
    expect(ev.label).toBe('Winter Field Day')
    expect(ev.year).toBe(2026)
    expect(ev.endUnix - ev.startUnix).toBe(30 * 3600)

    const sfd = fdEventFromWindow('arrlfd', SFD_START, SFD_END)!
    expect(sfd.label).toBe('ARRL Field Day')
    expect(sfd.endUnix - sfd.startUnix).toBe(27 * 3600)
  })

  it('returns null without a window (older backend) — no client-side dates invented', () => {
    expect(fdEventFromWindow('arrlfd', undefined, undefined)).toBeNull()
    expect(fdEventFromWindow('wfd', 0, 0)).toBeNull()
  })
})

describe('fdCountdownLabel', () => {
  it('returns null when the event is active — including the final six WFD hours', () => {
    const ev = fdEventFromWindow('wfd', WFD_START, WFD_END)!
    const midEvent = new Date(Date.UTC(2026, 0, 24, 20, 0, 0))
    expect(fdCountdownLabel(midEvent, ev)).toBeNull()
    // Hour 28 of 30 — the slice the old 24 h math treated as "over".
    const lateEvent = new Date(Date.UTC(2026, 0, 25, 20, 0, 0))
    expect(fdCountdownLabel(lateEvent, ev)).toBeNull()
  })

  it('returns "starts in N days" when more than 1 day away', () => {
    const ev = fdEventFromWindow('arrlfd', SFD_START, SFD_END)!
    const now = new Date(Date.UTC(2026, 5, 1, 0, 0, 0)) // Jun 1 → Jun 27 = 26 days
    expect(fdCountdownLabel(now, ev)).toBe('starts in 26 days')
  })

  it('returns "starts tomorrow" when 1 day away', () => {
    const ev = fdEventFromWindow('arrlfd', SFD_START, SFD_END)!
    // 25 h before the start = days=1 → "starts tomorrow"
    const now = new Date(Date.UTC(2026, 5, 26, 17, 0, 0))
    expect(fdCountdownLabel(now, ev)).toBe('starts tomorrow')
  })
})

describe('fdHeaderSubtitle', () => {
  it('formats the ARRL FD subtitle with the 27 h date range and countdown', () => {
    const ev = fdEventFromWindow('arrlfd', SFD_START, SFD_END)!
    const now = new Date(Date.UTC(2026, 5, 1, 0, 0, 0))
    const sub = fdHeaderSubtitle(now, ev)
    expect(sub).toContain('ARRL Field Day')
    expect(sub).toContain('Jun 27–28')
    expect(sub).toContain('starts in')
  })

  it('says active through the WHOLE 30 h WFD window', () => {
    const ev = fdEventFromWindow('wfd', WFD_START, WFD_END)!
    const now = new Date(Date.UTC(2026, 0, 25, 20, 0, 0)) // hour 28
    const sub = fdHeaderSubtitle(now, ev)
    expect(sub).toContain('Winter Field Day')
    expect(sub).toContain('Jan 24–25')
    expect(sub).toContain('active')
  })

  it('spans months when a window does (date formatting, not a schedule claim)', () => {
    // A hypothetical override window crossing a month boundary must not render
    // as a single-month range.
    const start = Date.UTC(2027, 0, 31, 16, 0, 0) / 1000
    const ev = fdEventFromWindow('wfd', start, start + 30 * 3600)!
    const sub = fdHeaderSubtitle(new Date(Date.UTC(2027, 0, 1)), ev)
    expect(sub).toContain('Jan 31–Feb 1')
  })
})

// Club sync runs the picked contest's own rules and refuses only the contests whose merged log
// would be wrong whoever built it (`Engine::club_sync_refusal`, pinned in Rust by
// `club_sync_runs_the_pickers_contest_and_refuses_only_what_it_cannot_run`). The screen says why
// from this, so it has to read the same settings the same way, the trim and the blank default
// included.
describe('clubSyncRefusal', () => {
  it('names the reason for hosting or joining a serial-number or transmitter-column contest', () => {
    expect(clubSyncRefusal({ fdEvent: 'arrlss_cw', fdHostEnable: true })).toBe('serial')
    expect(clubSyncRefusal({ fdEvent: 'cqp', fdHostEnable: true })).toBe('serial')
    expect(clubSyncRefusal({ fdEvent: 'cqww_cw', fdJoinAddr: '192.168.1.10:42073' })).toBe('transmitter')
    expect(clubSyncRefusal({ fdEvent: ' cqww_rtty ', fdHostEnable: true })).toBe('transmitter')
  })
  it('is null for the Illinois QSO Party, both Field Days and the blank default — every contest the club runs', () => {
    for (const fdEvent of ['ilqp', 'nyqp', 'tnqp', 'ohqp', 'txqp', 'arrlvhf_jun', 'arrlfd', 'wfd', '', ' wfd ', undefined]) {
      expect(clubSyncRefusal({ fdEvent, fdHostEnable: true })).toBeNull()
    }
  })
  it('is null when club sync is not switched on at all', () => {
    expect(clubSyncRefusal({ fdEvent: 'cqww_cw', fdHostEnable: false, fdJoinAddr: '   ' })).toBeNull()
    expect(clubSyncRefusal(null)).toBeNull()
  })
  it('answers for every contest on the picker, refused or not, and an inherited key is not a contest', () => {
    const refused = CONTESTS.filter((c) => contestClubRefusal(c.id) !== null).map((c) => c.id)
    expect(refused.sort()).toEqual(
      ['arrlss_cw', 'arrlss_ssb', 'cqp', 'cqwpx_cw', 'cqwpx_ssb', 'cqww_cw', 'cqww_rtty', 'cqww_ssb'],
    )
    expect(contestClubRefusal('constructor')).toBeNull()
  })
  it('says the reason in the sentence it shows', () => {
    expect(clubSyncRefusalText('serial', 'CQ WPX')).toMatch(/serial numbers must run in one sequence/)
    expect(clubSyncRefusalText('transmitter', 'CQ WW')).toMatch(/which transmitter made each contact/)
    expect(clubSyncRefusalText('serial', 'CQ WPX')).toContain('CQ WPX')
  })
  // A rules file the station downloaded can DROP a contest. The engine refuses club sync for
  // it then (`ClubRefusal::NoRuleset`, code "unknown"), and the station's preview of the picked
  // contest shows it by resolving the id to the ARRL Field Day that mode entry falls back to.
  it('names a contest the loaded rules do not carry, read off the station preview', () => {
    const fellBack = { event: 'arrlfd' }
    expect(clubSyncRefusal({ fdEvent: 'ilqp', fdHostEnable: true }, fellBack)).toBe('unknown')
    expect(clubSyncRefusal({ fdEvent: ' ilqp ', fdJoinAddr: '10.0.0.2:42073' }, fellBack)).toBe('unknown')
    expect(contestClubRefusal('ilqp', fellBack)).toBe('unknown')
    // The engine asks for the rules first, so a dropped serial contest is refused for that.
    expect(clubSyncRefusal({ fdEvent: 'arrlss_cw', fdHostEnable: true }, fellBack)).toBe('unknown')
    // CONTROLS: carried, the blank default, no preview yet, and not switched on.
    expect(clubSyncRefusal({ fdEvent: 'ilqp', fdHostEnable: true }, { event: 'ilqp' })).toBeNull()
    expect(clubSyncRefusal({ fdEvent: '', fdHostEnable: true }, fellBack)).toBeNull()
    expect(clubSyncRefusal({ fdEvent: 'arrlfd', fdHostEnable: true }, fellBack)).toBeNull()
    expect(clubSyncRefusal({ fdEvent: 'ilqp', fdHostEnable: true }, null)).toBeNull()
    expect(clubSyncRefusal({ fdEvent: 'ilqp', fdHostEnable: false }, fellBack)).toBeNull()
  })
  it('says the missing-rules reason by the sponsor name, and where to fix it', () => {
    const said = clubSyncRefusalText('unknown', 'Illinois QSO Party')
    expect(said).toContain('Illinois QSO Party')
    expect(said).toMatch(/rules this Nexus loaded do not include it/)
    expect(said).toMatch(/Check for rules updates/)
  })
})
