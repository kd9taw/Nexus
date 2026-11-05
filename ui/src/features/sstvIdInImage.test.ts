// @vitest-environment jsdom
//
// The remembered "my picture already shows my callsign" tick: keyed by the picture's content, only
// for that picture, forgotten on an untick, bounded, and never assumed without a key. The view's
// half (a remembered card comes back ticked and sends without the plate) is SstvView.test.tsx.
import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest'
import { MAX_REMEMBERED, SSTV_ID_IN_IMAGE_KEY, idInImageRemembered, pictureKey, rememberIdInImage } from './sstvIdInImage'

beforeEach(() => localStorage.clear())
afterEach(() => vi.unstubAllGlobals())

describe('the picture key', () => {
  it('is the hex SHA-256 of the bytes: the same picture under any name, a different one differs', async () => {
    const a = await pictureKey(new File([new Uint8Array([1, 2, 3])], 'card.png'))
    expect(a).toBe('039058c6f2c0cb492c533b0a4d14ef77cc0f78abccced5287d84a1a2011cfb81')
    expect(await pictureKey(new File([new Uint8Array([1, 2, 3])], 'other name.jpg'))).toBe(a)
    expect(await pictureKey(new File([new Uint8Array([1, 2, 4])], 'card.png'))).not.toBe(a)
  })

  it('is null where the browser cannot hash', async () => {
    vi.stubGlobal('crypto', {})
    expect(await pictureKey(new File([new Uint8Array([1])], 'card.png'))).toBeNull()
  })
})

describe('the remembered tick', () => {
  it('holds for the picture ticked and no other; an untick forgets it', () => {
    rememberIdInImage('K1ABC', 'aa', true)
    expect(idInImageRemembered('K1ABC', 'aa')).toBe(true)
    expect(idInImageRemembered('K1ABC', 'bb')).toBe(false)
    rememberIdInImage('K1ABC', 'aa', false)
    expect(idInImageRemembered('K1ABC', 'aa')).toBe(false)
  })

  it('holds only under the call it was ticked under (the picture shows THAT call)', () => {
    rememberIdInImage('K1ABC', 'aa', true)
    expect(idInImageRemembered('W1AW', 'aa'), 'a tick crossed to another station call').toBe(false)
    expect(idInImageRemembered('k1abc ', 'aa'), 'the same call, spelled loosely').toBe(true)
  })

  it('without a key or a call nothing is stored and nothing is remembered', () => {
    rememberIdInImage('K1ABC', null, true)
    rememberIdInImage('', 'aa', true)
    expect(localStorage.getItem(SSTV_ID_IN_IMAGE_KEY)).toBeNull()
    expect(idInImageRemembered('K1ABC', null)).toBe(false)
    expect(idInImageRemembered('', 'aa')).toBe(false)
  })

  it('keeps the most recent pictures, forgetting the oldest past the cap; a re-tick is recent again', () => {
    for (let i = 0; i < MAX_REMEMBERED; i++) rememberIdInImage('K1ABC', `k${i}`, true)
    rememberIdInImage('K1ABC', 'k0', true) // ticked again: now the newest
    rememberIdInImage('K1ABC', 'new', true)
    expect(idInImageRemembered('K1ABC', 'k1'), 'the oldest was not forgotten').toBe(false)
    expect(idInImageRemembered('K1ABC', 'k0')).toBe(true)
    expect(idInImageRemembered('K1ABC', 'new')).toBe(true)
    expect(JSON.parse(localStorage.getItem(SSTV_ID_IN_IMAGE_KEY)!)).toHaveLength(MAX_REMEMBERED)
  })

  it('a damaged store remembers nothing and is replaced by the next tick', () => {
    localStorage.setItem(SSTV_ID_IN_IMAGE_KEY, '{not json')
    expect(idInImageRemembered('K1ABC', 'aa')).toBe(false)
    rememberIdInImage('K1ABC', 'aa', true)
    expect(idInImageRemembered('K1ABC', 'aa')).toBe(true)
    localStorage.setItem(SSTV_ID_IN_IMAGE_KEY, JSON.stringify({ 'K1ABC:aa': true }))
    expect(idInImageRemembered('K1ABC', 'aa'), 'a non-list store must not read as a tick').toBe(false)
  })
})
