// #441: WHICH RADIOS GET THE TUNE NOTE IN PHONE. Phone keys plain USB/LSB, so a Yaesu on Hamlib's
// newcat backend takes its transmit audio from its SSB modulation source menu (MIC from the factory:
// the front jack only), and Tune's tone over USB audio then goes out as no RF. The set is Rust's own
// newcat ASCII set (`raw_mode_query` in crates/tempo-audio/src/rigmodels.rs), read out of that file,
// so the two lists cannot drift apart.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { NEWCAT_MODELS, phoneTuneNeedsRearSource } from './yaesuTuneNote'

describe('the newcat Yaesus (#441)', () => {
  it('are exactly the models Rust names as Hamlib newcat ASCII', () => {
    const rs = readFileSync(resolve(process.cwd(), '../crates/tempo-audio/src/rigmodels.rs'), 'utf8')
    const fn = rs.slice(rs.indexOf('pub(crate) fn raw_mode_query'))
    const body = fn.slice(fn.indexOf('matches!('), fn.indexOf('.then_some('))
    const rust = [...body.matchAll(/\b(\d{4})\b/g)].map((m) => Number(m[1]))
    expect(rust.length, 'the Rust list was not found').toBeGreaterThan(10)
    expect([...NEWCAT_MODELS].sort()).toEqual(rust.sort())
  })

  it('get the note on a serial or network CAT link, and no other radio does', () => {
    for (const rigModel of [1042, 1035, 1049, 1036, 1040, 1044]) {
      expect(phoneTuneNeedsRearSource({ rigModel, rigConn: 'serial' }), String(rigModel)).toBe(true)
      expect(phoneTuneNeedsRearSource({ rigModel, rigConn: 'network' }), String(rigModel)).toBe(true)
    }
    for (const rigModel of [
      1051, // FTX-1: its own backend, not newcat's
      1001, // FT-847: binary CAT
      1020, // FT-817
      3073, // IC-7300
      2036, // Flex SmartSDR CAT
      0, // no radio
    ]) {
      expect(phoneTuneNeedsRearSource({ rigModel, rigConn: 'serial' }), String(rigModel)).toBe(false)
    }
    // A TCI program stores a model of its own; a newcat number there is not the radio's CAT.
    expect(phoneTuneNeedsRearSource({ rigModel: 1042, rigConn: 'tci' })).toBe(false)
  })
})
