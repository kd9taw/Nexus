// @vitest-environment jsdom
// #441: WHICH RADIOS GET PHONE'S TUNE NOTE, AND WHEN. Phone keys plain USB/LSB, so a Yaesu with built-in USB
// audio takes its transmit audio from its SSB MOD SOURCE menu (MIC from the factory: the front jack only),
// and Tune's tone over USB audio then goes out as no RF. The note is for those radios only, and it comes up
// only when a Tune shows the trouble: under 2 W for 2 s while keyed. Then it stays for the session until
// dismissed (for this computer). With no power reading at all during a Tune, an ⓘ holds it instead.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import { NO_POWER_MS, TUNE_NOTE_KEY, dismissTuneNote, phoneTuneNeedsRearSource, resetTuneNote, useTuneNote } from './yaesuTuneNote'

beforeEach(() => {
  localStorage.clear()
  resetTuneNote()
  vi.useFakeTimers()
})
afterEach(() => vi.useRealTimers())

describe('the radios (#441)', () => {
  it('are the five Yaesus with built-in USB audio, on their own CAT link', () => {
    // FT-991/991A, FTDX101D, FTDX10, FTDX101MP, FT-710 (rigmodels.rs names each).
    for (const rigModel of [1035, 1040, 1042, 1044, 1049]) {
      expect(phoneTuneNeedsRearSource({ rigModel, rigConn: 'serial' }), String(rigModel)).toBe(true)
      expect(phoneTuneNeedsRearSource({ rigModel, rigConn: 'network' }), String(rigModel)).toBe(true)
    }
    for (const rigModel of [
      1036, // FT-891: USB for CAT only
      1027, // FT-450
      1046, // FT-450D
      1028, // FT-950
      1029, // FT-2000
      1030, // FTDX9000
      1032, // FTDX5000
      1034, // FTDX1200
      1037, // FTDX3000
      1051, // FTX-1
      1001, // FT-847
      3073, // IC-7300
      2036, // Flex SmartSDR CAT
      0,
    ]) {
      expect(phoneTuneNeedsRearSource({ rigModel, rigConn: 'serial' }), String(rigModel)).toBe(false)
    }
    // A TCI program stores a model of its own; that number is not the radio's CAT.
    expect(phoneTuneNeedsRearSource({ rigModel: 1042, rigConn: 'tci' })).toBe(false)
  })
})

type R = { tuning: boolean; txPoW?: number | null }
function tune(applies = true, first: R = { tuning: false }) {
  const h = renderHook(({ r }: { r: R }) => useTuneNote(applies, r as never), { initialProps: { r: first } })
  const set = (r: R) => h.rerender({ r })
  const wait = (ms: number) => act(() => void vi.advanceTimersByTime(ms))
  return { set, wait, now: () => h.result.current }
}

describe('when the note shows (#441)', () => {
  it('nothing before a Tune, and nothing for a Tune that reads power', () => {
    const t = tune()
    expect(t.now()).toBeNull()
    t.set({ tuning: true, txPoW: 0.4 })
    t.set({ tuning: true, txPoW: 45 })
    t.wait(NO_POWER_MS + 1000)
    t.set({ tuning: false, txPoW: 45 })
    expect(t.now(), 'a radio putting out power got the note').toBeNull()
  })

  it('the line, after 2 s of about no power while keyed; it stays for the session after Tune ends', () => {
    const t = tune()
    t.set({ tuning: true, txPoW: 0 })
    t.wait(NO_POWER_MS - 500)
    expect(t.now(), 'too early: the carrier may still be building').toBeNull()
    t.wait(800)
    expect(t.now()).toBe('line')
    t.set({ tuning: false, txPoW: 0 })
    t.wait(60_000)
    expect(t.now()).toBe('line')
    // Another Phone mounting in this window (a remount) still has it.
    expect(tune(true).now()).toBe('line')
  })

  it('a short Tune, or one that comes up to power, does not bring it', () => {
    const t = tune()
    t.set({ tuning: true, txPoW: 0 })
    t.wait(1200)
    t.set({ tuning: false, txPoW: 0 })
    t.wait(5000)
    expect(t.now(), 'a 1.2 s Tune').toBeNull()
    t.set({ tuning: true, txPoW: 0.5 })
    t.wait(1500)
    t.set({ tuning: true, txPoW: 30 })
    t.wait(1500)
    t.set({ tuning: true, txPoW: 31 })
    t.wait(1500)
    expect(t.now(), 'a Tune that reached 30 W').toBeNull()
  })

  it('the ⓘ when the radio reports no power at all during a Tune; a later no-power Tune brings the line', () => {
    const t = tune()
    t.set({ tuning: true, txPoW: null })
    t.wait(NO_POWER_MS + 300)
    expect(t.now()).toBe('icon')
    t.set({ tuning: false, txPoW: null })
    t.set({ tuning: true, txPoW: 0 })
    t.wait(NO_POWER_MS + 300)
    expect(t.now()).toBe('line')
  })

  it('dismissed, it is gone for this computer', () => {
    const t = tune()
    t.set({ tuning: true, txPoW: 0 })
    t.wait(NO_POWER_MS + 300)
    expect(t.now()).toBe('line')
    act(() => dismissTuneNote())
    expect(t.now()).toBeNull()
    expect(localStorage.getItem(TUNE_NOTE_KEY)).toBe('1')
    // A fresh window reads the dismissal back.
    resetTuneNote()
    const u = tune()
    u.set({ tuning: true, txPoW: 0 })
    u.wait(NO_POWER_MS + 300)
    expect(u.now()).toBeNull()
  })

  it('never for another radio', () => {
    const t = tune(false)
    t.set({ tuning: true, txPoW: 0 })
    t.wait(NO_POWER_MS + 1000)
    expect(t.now()).toBeNull()
  })
})
