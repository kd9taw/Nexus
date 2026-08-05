// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from 'vitest'
import {
  FT_PALETTE_SCOPE,
  WF_PALETTE_KEY,
  getWaterfallPalette,
  setWaterfallPalette,
} from './waterfallPalette'

describe('waterfall palette scoping (operator: "FT modes should start on Turbo")', () => {
  beforeEach(() => localStorage.clear())

  it('defaults to Turbo everywhere on a fresh install', () => {
    expect(getWaterfallPalette()).toBe('turbo')
    expect(getWaterfallPalette(FT_PALETTE_SCOPE)).toBe('turbo')
  })

  // ⭐ THE REPORT. Turbo has been the coded default all along and nothing overrides it on the
  // FT surfaces — so the operator seeing something else means an explicit older pick, made in
  // some other mode, reaching FT through the one global key. FT now reads its own key, which
  // is unset, so it starts on Turbo.
  it('FT starts on Turbo even when the master palette holds an older pick', () => {
    setWaterfallPalette('grayscale') // a pick made years ago in CW or Phone
    expect(getWaterfallPalette()).toBe('grayscale')
    expect(getWaterfallPalette(FT_PALETTE_SCOPE)).toBe('turbo')
  })

  // ⭐ AND THE THING THAT MUST NOT BREAK. The picker is the store's only writer, so a stored
  // value is always a real operator choice. Satisfying the request by resetting the master key
  // would take away a setting they made on the CW and Phone scopes and the RTTY/SSTV
  // waterfalls — those keep reading the bare key and are untouched by any of this.
  it('leaves the master value alone — other surfaces keep what they chose', () => {
    setWaterfallPalette('sdr-green')
    setWaterfallPalette('viridis', FT_PALETTE_SCOPE)
    expect(getWaterfallPalette()).toBe('sdr-green')
    expect(localStorage.getItem(WF_PALETTE_KEY)).toBe('sdr-green')
  })

  it('keeps an explicit FT pick, and keeps the two independent in both directions', () => {
    setWaterfallPalette('inferno', FT_PALETTE_SCOPE)
    expect(getWaterfallPalette(FT_PALETTE_SCOPE)).toBe('inferno')
    expect(getWaterfallPalette()).toBe('turbo') // master untouched, still its own default
    setWaterfallPalette('blue')
    expect(getWaterfallPalette(FT_PALETTE_SCOPE)).toBe('inferno') // FT unmoved
  })

  it('scopes to a distinct key, so nothing collides with the pre-existing master key', () => {
    setWaterfallPalette('amber-crt', FT_PALETTE_SCOPE)
    expect(localStorage.getItem(WF_PALETTE_KEY)).toBeNull()
    expect(localStorage.getItem(`${WF_PALETTE_KEY}.${FT_PALETTE_SCOPE}`)).toBe('amber-crt')
  })

  it('survives blocked storage by falling back to the default rather than throwing', () => {
    const real = localStorage.getItem
    localStorage.getItem = () => {
      throw new Error('blocked')
    }
    try {
      expect(getWaterfallPalette(FT_PALETTE_SCOPE)).toBe('turbo')
    } finally {
      localStorage.getItem = real
    }
  })
})
