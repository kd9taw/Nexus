// The real-browser harness's pure parts, checked where they run fast: the PNG codec the stored
// pictures go through, the comparator's tolerance, the barcode the cadence probe reads rows by,
// and the wire format the payload sizes are measured in. The browser half runs in its own CI job.
import { describe, expect, it } from 'vitest'
import { comparePixels, TOLERANCE } from './compare.mjs'
import { decodePng, encodePng } from './png.mjs'
import { CODE_SLOTS, codeBits, decodeBits, f32Json, frameJson, frameReply, frameTail } from './frames'

function picture(width, height, fill) {
  const rgba = new Uint8Array(width * height * 4)
  for (let i = 0; i < width * height; i++) rgba.set(fill(i % width, Math.floor(i / width)), i * 4)
  return { width, height, rgba }
}

describe('png', () => {
  it('round-trips opaque and translucent pictures byte for byte', () => {
    const scenes = [
      picture(37, 11, (x, y) => [x * 6, y * 20, 128, 255]), // gradients: the Sub/Up filters
      picture(19, 7, (x, y) => [(x * 7919 + y * 104729) % 256, (x * 31) % 256, y * 3, 255]), // noise: None
      picture(5, 3, (x, y) => [x * 50, y * 80, 7, 40 + x * 30]), // alpha kept: RGBA
    ]
    for (const p of scenes) {
      const back = decodePng(encodePng(p.width, p.height, p.rgba))
      expect([back.width, back.height]).toEqual([p.width, p.height])
      expect(Buffer.from(back.rgba).equals(Buffer.from(p.rgba))).toBe(true)
    }
  })
})

describe('comparePixels', () => {
  const base = picture(100, 100, (x, y) => [x, y, 50, 255])
  const nudged = (count, by) => {
    const p = { ...base, rgba: base.rgba.slice() }
    for (let i = 0; i < count; i++) p.rgba[i * 4] += by
    return p
  }
  it('passes an identical picture, and drift at the channel tolerance', () => {
    expect(comparePixels(base, base).differing).toBe(0)
    expect(comparePixels(nudged(10_000, TOLERANCE.channel), base)).toMatchObject({ match: true, differing: 0 })
  })
  it('counts a pixel one level past the tolerance, and fails once enough of them move', () => {
    const allowed = Math.floor(TOLERANCE.maxFraction * 10_000)
    expect(comparePixels(nudged(allowed, TOLERANCE.channel + 1), base)).toMatchObject({ match: true, differing: allowed })
    expect(comparePixels(nudged(allowed + 1, TOLERANCE.channel + 1), base).match).toBe(false)
  })
  it('refuses pictures of different sizes', () => {
    expect(comparePixels(picture(100, 99, () => [0, 0, 0, 255]), base)).toMatchObject({ match: false, reason: 'size 100x99, stored 100x100' })
  })
})

describe('the cadence barcode', () => {
  it('reads back every sweep number it can carry', () => {
    for (let seq = 0; seq < 1 << 14; seq++) expect(decodeBits(codeBits(seq))).toBe(seq)
    expect(decodeBits(codeBits(null))).toBe('end')
  })
  it('never reads one misread slot as a different sweep', () => {
    for (const seq of [0, 1, 300, 4097, 16383]) {
      for (let s = 0; s < CODE_SLOTS; s++) {
        const bits = codeBits(seq)
        bits[s] = !bits[s]
        expect(decodeBits(bits)).toBeNull()
      }
    }
  })
})

describe('the wire format', () => {
  it('writes f32 the way serde_json does', () => {
    expect([0, 1, 0.1, 0.39375, 4000].map(f32Json)).toEqual(['0.0', '1.0', '0.1', '0.39375', '4000.0'])
  })
  it('parses back to the same f32 values', () => {
    const row = [0.2345678, 0.9, 1 / 3, 0.000123]
    const back = JSON.parse(frameJson({ row, loHz: 0, hiHz: 4000, source: 'audio' }))
    expect(back.row.map(Math.fround)).toEqual(row.map(Math.fround))
    expect([back.loHz, back.hiHz, back.source]).toEqual([0, 4000, 'audio'])
  })
  it('writes a frame byte for byte as the backend does', () => {
    // The same two frames `spectrum_frame_dto_tests` in crates/tempo-app/src/dto.rs pins.
    const civ = { row: [0, 0.5, 1], loHz: 144_975_000, hiHz: 145_025_000, source: 'civ' }
    expect(frameReply(7, 1_700_000_000_123, frameTail(civ))).toBe(
      '{"seq":7,"tMs":1700000000123,"source":"civ","loHz":144975000.0,"hiHz":145025000.0,"scale":{"kind":"relative"},"slice":0,"bins":[0.0,0.5,1.0]}',
    )
    const audio = { row: [0.25], loHz: 0, hiHz: 4000, source: 'audio' }
    expect(frameReply(8, 1_700_000_000_143, frameTail(audio))).toBe(
      '{"seq":8,"tMs":1700000000143,"source":"audio","loHz":0.0,"hiHz":4000.0,"scale":{"kind":"dbfs","loDb":-120.0,"hiDb":0.0},"bins":[0.25]}',
    )
  })
})
