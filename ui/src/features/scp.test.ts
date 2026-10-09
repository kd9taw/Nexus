import { describe, expect, it } from 'vitest'
import { near, partial, scpLine, SCP_MAX_HITS } from './scp'

describe('Super Check Partial matching', () => {
  it('lists the calls that contain what is typed, in list order', () => {
    expect(partial('K9A', ['K9AAA', 'W9XYZ', 'K9AB'])).toEqual(['K9AAA', 'K9AB'])
    // Anywhere in the call, not only at the start: that is what makes it Super Check PARTIAL.
    expect(partial('9AB', ['K9AAA', 'W9ABC', 'K9AB'])).toEqual(['W9ABC', 'K9AB'])
    expect(partial('k9a', ['K9AAA'])).toEqual(['K9AAA'])
  })

  it('lists the calls one character away, never the typed call itself', () => {
    expect(near('K9AAB', ['K9AAA', 'K9AAB', 'K9ABB'])).toEqual(['K9AAA', 'K9ABB'])
    // An inserted and a deleted character count as one away too.
    expect(near('K9AAB', ['K9AAAB', 'K9AB', 'K9BBAB', 'W1AW'])).toEqual(['K9AAAB', 'K9AB'])
  })

  it('starts at the third character, and near matches at the fourth', () => {
    expect(partial('K9', ['K9AAA'])).toEqual([])
    expect(scpLine('K9', ['K9AAA'], ['K9AAA'])).toEqual([])
    expect(near('K9A', ['K9B'])).toEqual([])
  })

  it('puts a call already in this log first, marked worked, and shows each call once', () => {
    expect(scpLine('K9AA', ['K9AAA', 'K9AAB', 'K9ABA'], ['K9AAB'])).toEqual([
      { call: 'K9AAB', kind: 'worked' },
      { call: 'K9AAA', kind: 'partial' },
      { call: 'K9ABA', kind: 'near' },
    ])
  })

  it('never offers more than the line is given', () => {
    const calls = Array.from({ length: 200 }, (_, i) => `K9A${String(i).padStart(3, '0')}`)
    expect(scpLine('K9A', calls, [])).toHaveLength(SCP_MAX_HITS)
  })
})
