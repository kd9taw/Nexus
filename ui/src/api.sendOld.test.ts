// Saving a QRZ Logbook, ClubLog or eQSL login must state what becomes of the contacts never sent
// there. An omitted answer reaches the backend as "nothing was asked", which sends the whole log,
// so the answer is a required parameter: a new caller that skips the question fails `tsc -b`
// instead of silently uploading every old contact. These checks are type-level; `tsc -b` is
// their gate (vitest only runs the file).
import { describe, expect, expectTypeOf, it } from 'vitest'
import * as api from './api'

describe('a login save always states whether old contacts are sent', () => {
  it('takes the answer as a required parameter', () => {
    expectTypeOf(api.setQrzLogbookKey).parameters.toEqualTypeOf<[string, boolean | undefined]>()
    expectTypeOf(api.setClublogPassword).parameters.toEqualTypeOf<[string, boolean | undefined]>()
    expectTypeOf(api.setEqslPassword).parameters.toEqualTypeOf<[string, boolean | undefined]>()
  })

  it('refuses a call that leaves the answer out', () => {
    // Never called: each line only has to fail to type-check. An unused `@ts-expect-error`
    // is itself an error, so a parameter made optional again turns `tsc -b` red here.
    const omitted = [
      // @ts-expect-error the old-contacts answer is required
      () => api.setQrzLogbookKey('k'),
      // @ts-expect-error the old-contacts answer is required
      () => api.setClublogPassword('p'),
      // @ts-expect-error the old-contacts answer is required
      () => api.setEqslPassword('p'),
    ]
    expect(omitted).toHaveLength(3)
  })
})
