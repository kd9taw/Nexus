import { describe, expect, it } from 'vitest'
import golden from './features/__fixtures__/log-query/qso-edit-fields.json'
import type { QsoEdit } from './api'

/** The field paths of a value, `ota.myRef` style, sorted — the golden's own form. */
function paths(value: object, prefix = ''): string[] {
  return Object.entries(value)
    .flatMap(([k, v]) =>
      v !== null && typeof v === 'object' ? paths(v, `${prefix}${k}.`) : [`${prefix}${k}`],
    )
    .sort()
}

describe('QsoEdit', () => {
  // `tsc -b` holds this literal to EXACTLY the interface — a field missing from it, or one the
  // interface does not have, fails the typecheck — and the test holds its fields to the golden
  // tempo-core's `QsoEdit` is checked against. Together: the edit the UI sends is the edit the
  // backend reads, field for field, so `deny_unknown_fields` never refuses it.
  const sample: QsoEdit = {
    call: 'W1AW',
    grid: null,
    state: null,
    band: '20m',
    freqMhz: 14.074,
    mode: 'FT8',
    rstSent: null,
    rstRcvd: null,
    name: null,
    qth: null,
    comment: null,
    notes: null,
    txPower: null,
    whenUnix: 1_788_000_000,
    timeOffUnix: null,
    ota: { myProgram: null, myRef: null, theirProgram: null, theirRef: null },
    myGrid: null,
    myRig: null,
    qslSentVia: null,
    qslCard: false,
  }

  it('is the golden field list', () => {
    expect(paths(sample)).toEqual(golden)
  })
})
