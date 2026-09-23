// @vitest-environment node
//
// ⭐ #304 — THE AUTO-CALL TIMING CONTROLS AND THE SEQUENCER MUST AGREE ABOUT WHAT IS LEGAL.
//
// "would it be possible please to add a timer possibility for rtty qso's (example cq, 15s rx,
// cq etc)". The cadence already existed — the RTTY cockpit's Auto call button — with its
// listen window welded to `SeqConfig::default`. Opening it up put the same four numbers in two
// languages: the input's min/max here, and the clamp in `Settings::rtty_seq_config`.
//
// A copy nothing checks is a copy that drifts, and the failure is silent in the worst
// direction: widen only the box and the operator types a number the box accepts, saves it, and
// the sequencer quietly refuses it — a control that looks like it worked and did nothing. So
// this reads the Rust source and fails on disagreement. Node environment, no DOM: it is a
// source-agreement check, not a render.
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, it, expect } from 'vitest'
import {
  RTTY_AUTO_LISTEN_SECS_MIN,
  RTTY_AUTO_LISTEN_SECS_MAX,
  RTTY_AUTO_LISTEN_SECS_DEFAULT,
  RTTY_AUTO_REPEATS_MIN,
  RTTY_AUTO_REPEATS_MAX,
  RTTY_AUTO_REPEATS_DEFAULT,
} from './SettingsPanel'
import { searchSettings } from '../settings/registry'

const rust = (rel: string) => readFileSync(join(__dirname, '../../..', rel), 'utf8')

/** `pub const NAME: u32 = 5;` → 5. Throws rather than returning a default: a rename must be a
 *  loud failure here, not a silently-passing comparison against `undefined`. */
function rustConst(src: string, name: string): number {
  const m = src.match(new RegExp(`pub const ${name}: u32 = (\\d+);`))
  if (!m) throw new Error(`settings.rs no longer declares ${name}`)
  return Number(m[1])
}

describe('the Auto call boxes offer exactly the range the sequencer accepts', () => {
  const src = rust('crates/tempo-app/src/settings.rs')

  it('matches the clamp in Settings::rtty_seq_config', () => {
    expect(RTTY_AUTO_LISTEN_SECS_MIN).toBe(rustConst(src, 'RTTY_AUTO_LISTEN_SECS_MIN'))
    expect(RTTY_AUTO_LISTEN_SECS_MAX).toBe(rustConst(src, 'RTTY_AUTO_LISTEN_SECS_MAX'))
    expect(RTTY_AUTO_REPEATS_MIN).toBe(rustConst(src, 'RTTY_AUTO_REPEATS_MIN'))
    expect(RTTY_AUTO_REPEATS_MAX).toBe(rustConst(src, 'RTTY_AUTO_REPEATS_MAX'))
  })

  it('reads the parser against a value it must NOT find', () => {
    // POSITIVE CONTROL. Every assertion above is an equality against a number this helper
    // produced; if the regex quietly matched nothing the helper would have to throw, and this
    // is the proof that it does rather than yielding a NaN that compares equal to nothing.
    expect(() => rustConst(src, 'RTTY_AUTO_NO_SUCH_BOUND')).toThrow(/no longer declares/)
  })

  it('ships the values the sequencer already had, so the setting is never a prerequisite', () => {
    // The project premise: unconfigured must work. These are what the boxes render with no
    // saved value, and they are `SeqConfig::default` — asserted in Rust by
    // `rtty_auto_call_timing_defaults_wire_keys_and_upgrade`, which reads them off the
    // sequencer rather than restating them.
    const core = rust('crates/tempo-core/src/rtty/seq.rs')
    expect(core).toMatch(new RegExp(`timeout_ms: ${RTTY_AUTO_LISTEN_SECS_DEFAULT}_000,`))
    expect(core).toMatch(new RegExp(`max_repeats: ${RTTY_AUTO_REPEATS_DEFAULT},`))
  })
})

describe('an operator looking for the RTTY timer can find it', () => {
  // #304 called it a "timer". Nothing in "RTTY", in the group title or in either control
  // label contains that word, so without a keyword the search box answers nothing — which is
  // the same findability failure the issue itself is an instance of.
  const ids = (q: string) => searchSettings(q).map((h) => h.section.id)

  it('finds the RTTY section by the words the report used', () => {
    expect(ids('timer')).toContain('rtty')
    expect(ids('auto cq')).toContain('rtty')
    expect(ids('cq repeat')).toContain('rtty')
  })

  it('still answers nothing for a word that is in no section', () => {
    expect(searchSettings('zzzznotasetting')).toEqual([])
  })
})
