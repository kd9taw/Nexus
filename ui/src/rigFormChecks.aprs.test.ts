// "APRS uses DATA-FM" (#393) is offered from the ENGINE's model list, not a second opinion: the
// engine obeys the switch only on the models `aprs_data_fm_available` names, so a switch offered
// anywhere else would be a control that does nothing. This reads the Rust source and fails if the
// two lists drift.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { APRS_DATA_FM_MODELS } from './rigFormChecks'

const RUST = fileURLToPath(new URL('../../crates/tempo-app/src/settings.rs', import.meta.url))

describe('APRS DATA-FM model list', () => {
  it('matches aprs_data_fm_available in the engine', () => {
    const src = readFileSync(RUST, 'utf8')
    const fn = src.slice(src.indexOf('pub fn aprs_data_fm_available'))
    const body = fn.slice(fn.indexOf('{'), fn.indexOf('\n}'))
    const models = [...body.matchAll(/\b(\d{3,5})\b/g)].map((m) => Number(m[1])).sort((a, b) => a - b)
    expect(models.length, 'control: the Rust body was actually parsed').toBeGreaterThan(0)
    expect([...APRS_DATA_FM_MODELS].sort((a, b) => a - b)).toEqual(models)
  })
})
