// THE SSTV COMPOSER PIPELINE — orient → crop → resample, the pure half.
//
// These four modules exist as pure functions precisely so they can be tested here: this
// project's jsdom has NO canvas at all (`getContext('2d')` returns null, and there is no
// `createImageBitmap`, `OffscreenCanvas`, `ImageData`, `URL.createObjectURL` or
// `DataTransfer`), which is why `SstvView`'s `recrop` silently no-ops in every unit test
// today. Anything that can be decided without pixels is decided here; the pixels
// themselves are proved in Rust (`crates/tempo-sstv/tests/id_legibility.rs`) and on the
// real webview.

import { describe, it, expect } from 'vitest'
import { readExifOrientation, readIntrinsicSize, sniffImageKind } from './sstvExif'
import { effectiveOrientation, orientTransform, orientationSwapsAxes } from './sstvOrient'
import {
  CENTRE,
  clampCentre,
  coverScale,
  cropWindow,
  dragCentre,
  freeAxis,
  isExactFit,
  isUpscale,
  nudgeCentre,
  windowSize,
} from './sstvCrop'
import { halvingChain, peakStagePixels } from './sstvResample'

// ---------------------------------------------------------------------------
// Synthetic files. Small enough to read, real enough that the walks are exercised.
// ---------------------------------------------------------------------------

/** A minimal JPEG: SOI, an APP1/Exif with an IFD0 carrying Orientation, an SOF0 with the
 *  stored (unrotated) frame size, then SOS. */
function jpegWithExif(orientation: number, w: number, h: number, little = true): Uint8Array {
  const ifdEntries = 1
  const exif: number[] = []
  const put16 = (v: number) =>
    little ? exif.push(v & 0xff, (v >> 8) & 0xff) : exif.push((v >> 8) & 0xff, v & 0xff)
  const put32 = (v: number) =>
    little
      ? exif.push(v & 0xff, (v >> 8) & 0xff, (v >> 16) & 0xff, (v >> 24) & 0xff)
      : exif.push((v >> 24) & 0xff, (v >> 16) & 0xff, (v >> 8) & 0xff, v & 0xff)
  // TIFF header
  if (little) exif.push(0x49, 0x49)
  else exif.push(0x4d, 0x4d)
  put16(0x002a)
  put32(8) // IFD0 at offset 8 from the TIFF header
  put16(ifdEntries)
  put16(0x0112) // Orientation
  put16(3) // SHORT
  put32(1) // count
  put16(orientation)
  put16(0) // pad the 4-byte value field
  put32(0) // next IFD

  const app1Payload = [0x45, 0x78, 0x69, 0x66, 0x00, 0x00, ...exif] // "Exif\0\0" + TIFF
  const app1Len = app1Payload.length + 2
  return new Uint8Array([
    0xff, 0xd8, // SOI
    0xff, 0xe1, (app1Len >> 8) & 0xff, app1Len & 0xff, ...app1Payload,
    // SOF0: len=17, precision=8, height, width, 3 components
    0xff, 0xc0, 0x00, 0x11, 0x08,
    (h >> 8) & 0xff, h & 0xff,
    (w >> 8) & 0xff, w & 0xff,
    0x03, 0x01, 0x22, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01,
    0xff, 0xda, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3f, 0x00,
  ])
}

function png(w: number, h: number): Uint8Array {
  const b = new Uint8Array(24)
  b.set([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a], 0)
  b.set([0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52], 8)
  new DataView(b.buffer).setUint32(16, w)
  new DataView(b.buffer).setUint32(20, h)
  return b
}

/** An ISO-BMFF header with the given brand — how HEIC is recognised before any decode. */
function ftyp(brand: string): Uint8Array {
  const b = new Uint8Array(16)
  b.set([0, 0, 0, 0x10], 0)
  b.set([...'ftyp'].map((c) => c.charCodeAt(0)), 4)
  b.set([...brand].map((c) => c.charCodeAt(0)), 8)
  return b
}

// ---------------------------------------------------------------------------

describe('sniffImageKind — the format is decided by MAGIC, never by the extension', () => {
  it('names the formats we accept', () => {
    expect(sniffImageKind(jpegWithExif(1, 8, 8))).toBe('jpeg')
    expect(sniffImageKind(png(8, 8))).toBe('png')
    expect(sniffImageKind(new Uint8Array([0x42, 0x4d, ...new Array(20).fill(0)]))).toBe('bmp')
    expect(
      sniffImageKind(new Uint8Array([...'GIF89a'].map((c) => c.charCodeAt(0)).concat(new Array(10).fill(0)))),
    ).toBe('gif')
    const webp = new Uint8Array(20)
    webp.set([...'RIFF'].map((c) => c.charCodeAt(0)), 0)
    webp.set([...'WEBP'].map((c) => c.charCodeAt(0)), 8)
    expect(sniffImageKind(webp)).toBe('webp')
  })

  it('⭐ recognises iPhone HEIC by brand, so it can be refused BY NAME before a decode', () => {
    for (const brand of ['heic', 'heix', 'hevc', 'mif1', 'msf1']) {
      expect(sniffImageKind(ftyp(brand)), brand).toBe('heic')
    }
    // A HEIC renamed .jpg is still HEIC — which is exactly why the extension is not
    // consulted anywhere in this path.
    expect(sniffImageKind(ftyp('heic'))).not.toBe('jpeg')
  })

  it('names the other refusals rather than calling them all "not an image"', () => {
    expect(sniffImageKind(ftyp('avif'))).toBe('avif')
    expect(sniffImageKind(new Uint8Array([0x49, 0x49, 0x2a, 0x00, ...new Array(12).fill(0)]))).toBe('tiff')
    const cr2 = new Uint8Array(16)
    cr2.set([0x49, 0x49, 0x2a, 0x00], 0)
    cr2.set([...'CR'].map((c) => c.charCodeAt(0)), 8)
    expect(sniffImageKind(cr2)).toBe('raw')
    expect(sniffImageKind(new Uint8Array([...'8BPS'].map((c) => c.charCodeAt(0)).concat(new Array(12).fill(0))))).toBe('psd')
    expect(sniffImageKind(new Uint8Array([...'<svg xmlns='].map((c) => c.charCodeAt(0)).concat([0])))).toBe('svg')
    expect(sniffImageKind(new Uint8Array(20))).toBe('unknown')
  })

  it('a truncated file is unknown, not a crash', () => {
    expect(sniffImageKind(new Uint8Array([0xff, 0xd8]))).toBe('unknown')
    expect(sniffImageKind(new Uint8Array(0))).toBe('unknown')
  })
})

describe('readExifOrientation', () => {
  it('⭐ reads the iPhone portrait case (orientation 6) out of a JPEG APP1', () => {
    expect(readExifOrientation(jpegWithExif(6, 4032, 3024))).toBe(6)
  })

  it('reads big-endian (Motorola) TIFF too — half the cameras in the world', () => {
    expect(readExifOrientation(jpegWithExif(8, 4032, 3024, false))).toBe(8)
  })

  it('every orientation 1–8 survives the round trip', () => {
    for (let o = 1; o <= 8; o++) {
      expect(readExifOrientation(jpegWithExif(o, 100, 50)), `orientation ${o}`).toBe(o)
    }
  })

  it('an out-of-range tag is treated as 1 (matching Orientation::from_exif returning None)', () => {
    expect(readExifOrientation(jpegWithExif(0, 100, 50))).toBe(1)
    expect(readExifOrientation(jpegWithExif(9, 100, 50))).toBe(1)
  })

  it('no EXIF at all (PNG, most BMP/GIF) is orientation 1', () => {
    expect(readExifOrientation(png(320, 240))).toBe(1)
    expect(readExifOrientation(new Uint8Array(40))).toBe(1)
  })
})

describe('readIntrinsicSize — the UNROTATED size the file declares', () => {
  it('reads the JPEG SOF frame header (the double-rotation guard rests on this)', () => {
    expect(readIntrinsicSize(jpegWithExif(6, 4032, 3024))).toEqual({ w: 4032, h: 3024 })
  })

  it('reads the PNG IHDR', () => {
    expect(readIntrinsicSize(png(1920, 1080))).toEqual({ w: 1920, h: 1080 })
  })

  it('returns null rather than guessing for a format whose header we do not walk', () => {
    expect(readIntrinsicSize(new Uint8Array([0x42, 0x4d, ...new Array(30).fill(0)]))).toBeNull()
  })
})

describe('orientTransform — the eight-row table', () => {
  const W = 4032
  const H = 3024
  const TABLE: Record<number, { w: number; h: number; m: number[] }> = {
    1: { w: W, h: H, m: [1, 0, 0, 1, 0, 0] },
    2: { w: W, h: H, m: [-1, 0, 0, 1, W, 0] },
    3: { w: W, h: H, m: [-1, 0, 0, -1, W, H] },
    4: { w: W, h: H, m: [1, 0, 0, -1, 0, H] },
    5: { w: H, h: W, m: [0, 1, 1, 0, 0, 0] },
    6: { w: H, h: W, m: [0, 1, -1, 0, H, 0] },
    7: { w: H, h: W, m: [0, -1, -1, 0, H, W] },
    8: { w: H, h: W, m: [0, -1, 1, 0, 0, W] },
  }

  it('matches the table for every orientation', () => {
    for (let o = 1; o <= 8; o++) {
      expect(orientTransform(o, W, H), `orientation ${o}`).toEqual(TABLE[o])
    }
  })

  it('⭐ 5–8 SWAP the destination canvas — the half that gets forgotten', () => {
    for (const o of [5, 6, 7, 8]) {
      const t = orientTransform(o, W, H)
      expect(t.w, `orientation ${o} width`).toBe(H)
      expect(t.h, `orientation ${o} height`).toBe(W)
      expect(orientationSwapsAxes(o)).toBe(true)
    }
    for (const o of [1, 2, 3, 4]) expect(orientationSwapsAxes(o)).toBe(false)
  })

  it('a nonsense orientation is the identity, not a crash', () => {
    expect(orientTransform(0, 10, 20)).toEqual({ w: 10, h: 20, m: [1, 0, 0, 1, 0, 0] })
    expect(orientTransform(99, 10, 20)).toEqual({ w: 10, h: 20, m: [1, 0, 0, 1, 0, 0] })
  })

  it('the matrix actually puts the picture upright (corner mapping, orientation 6)', () => {
    // Orientation 6 is "rotate 90 CW". The source's TOP-LEFT must land at the
    // destination's TOP-RIGHT. Apply the affine by hand: x' = a·x + c·y + e.
    const { w, h, m } = orientTransform(6, W, H)
    const [a, b, c, d, e, f] = m
    const map = (x: number, y: number) => [a * x + c * y + e, b * x + d * y + f]
    expect(map(0, 0)).toEqual([w, 0]) // source top-left → destination top-right
    expect(map(W, 0)).toEqual([w, h]) // source top-right → destination bottom-right
    expect(map(0, H)).toEqual([0, 0]) // source bottom-left → destination top-left
  })
})

describe('effectiveOrientation — the double-rotation guard', () => {
  const intrinsic = { w: 4032, h: 3024 }

  it('applies the tag when the decoder returned the picture as stored', () => {
    expect(effectiveOrientation(6, intrinsic, { w: 4032, h: 3024 })).toBe(6)
  })

  it('⭐ SKIPS the matrix when the decoder already rotated (dimensions came back swapped)', () => {
    // This is the case that double-rotates: <img> with image-orientation:from-image, or
    // createImageBitmap on an engine that honours EXIF by default.
    expect(effectiveOrientation(6, intrinsic, { w: 3024, h: 4032 })).toBe(1)
  })

  it('never second-guesses orientations 1–4 (there is nothing to double-apply)', () => {
    for (const o of [1, 2, 3, 4]) {
      expect(effectiveOrientation(o, intrinsic, { w: 3024, h: 4032 })).toBe(o)
    }
  })

  it('trusts the tag when the file header could not be read', () => {
    expect(effectiveOrientation(6, null, { w: 3024, h: 4032 })).toBe(6)
  })

  it('trusts the tag when the decode is neither the same nor a clean swap', () => {
    // A square picture is both at once, and a scaled decode is neither: in both cases the
    // measurement carries no information, so the tag stands.
    expect(effectiveOrientation(6, { w: 512, h: 512 }, { w: 512, h: 512 })).toBe(6)
    expect(effectiveOrientation(6, intrinsic, { w: 1000, h: 800 })).toBe(6)
  })
})

describe('cover-crop geometry', () => {
  it('a 4032×3024 phone photo covers Scottie 1 by pinning the HEIGHT', () => {
    const s = coverScale(4032, 3024, 320, 256)
    expect(s).toBeCloseTo(256 / 3024, 10)
    const { cw, ch } = windowSize(4032, 3024, 320, 256)
    expect(ch).toBe(3024) // the short axis is full
    expect(cw).toBe(3780) // 320 / (256/3024)
    expect(freeAxis(4032, 3024, 320, 256)).toBe('x')
  })

  it('⭐ a PORTRAIT phone photo (the common iPhone case) leaves the VERTICAL axis free', () => {
    // 3024×4032 after orientation 6 — cover pins the WIDTH, and dragging up/down picks
    // which horizontal band of the picture survives. This is where the drag earns its keep.
    expect(freeAxis(3024, 4032, 320, 256)).toBe('y')
    const { cw, ch } = windowSize(3024, 4032, 320, 256)
    expect(cw).toBe(3024)
    expect(ch).toBe(2419)
  })

  it('a pre-sized drop needs no crop at all — both axes pinned, chain empty, 1:1 draw', () => {
    expect(freeAxis(320, 256, 320, 256)).toBe('none')
    expect(isExactFit(320, 256, 320, 256)).toBe(true)
    expect(halvingChain(320, 256, 320, 256)).toEqual([{ w: 320, h: 256 }])
  })

  it('the window never leaves the source, so there is never a black edge', () => {
    for (const c of [
      { cx: -5, cy: 0.5 },
      { cx: 9, cy: 0.5 },
      { cx: 0.5, cy: -1 },
      { cx: 0.5, cy: 2 },
      { cx: NaN, cy: NaN },
    ]) {
      const win = cropWindow(4032, 3024, 320, 256, c)
      expect(win.sx).toBeGreaterThanOrEqual(0)
      expect(win.sy).toBeGreaterThanOrEqual(0)
      expect(win.sx + win.sw).toBeLessThanOrEqual(4032)
      expect(win.sy + win.sh).toBeLessThanOrEqual(3024)
    }
  })

  it('a pinned axis collapses to the single point 0.5 — the drag is inert with no special case', () => {
    const c = clampCentre(4032, 3024, 320, 256, { cx: 0.9, cy: 0.9 })
    expect(c.cy).toBe(0.5) // height is full: no travel
    expect(c.cx).toBeLessThan(0.9) // width has travel, but it is bounded
    expect(c.cx).toBeGreaterThan(0.5)
  })

  it('⭐ the crop origin is EVEN, which is what keeps the 2:1 halving phase exact', () => {
    for (let i = 0; i < 40; i++) {
      const w = cropWindow(4033, 3025, 320, 256, { cx: i / 40, cy: 0.5 })
      expect(w.sx % 2, `cx=${i / 40} gave sx=${w.sx}`).toBe(0)
      expect(w.sy % 2, `cy gave sy=${w.sy}`).toBe(0)
    }
  })

  it('centre is the default and matches the old cover-crop exactly', () => {
    const w = cropWindow(4032, 3024, 320, 256, CENTRE)
    expect(w.sy).toBe(0)
    expect(w.sh).toBe(3024)
    // Centred horizontally: (4032 − 3780)/2 = 126, already even.
    expect(w.sx).toBe(126)
    expect(w.sw).toBe(3780)
  })
})

describe('drag scaling', () => {
  it('⭐ a drag moves the same PICTURE CONTENT at 1× and at 3× preview', () => {
    // The classic "drag feels three times too fast" bug: the delta has to be divided by
    // the preview upscale as well as the cover scale.
    const at1 = dragCentre(4032, 3024, 320, 256, CENTRE, 30, 0, 1)
    const at3 = dragCentre(4032, 3024, 320, 256, CENTRE, 90, 0, 3)
    expect(at3.cx).toBeCloseTo(at1.cx, 12)
  })

  it('dragging right reveals more of the picture LEFT (the image moves under a fixed frame)', () => {
    expect(dragCentre(4032, 3024, 320, 256, CENTRE, 40, 0, 1).cx).toBeLessThan(0.5)
    expect(dragCentre(3024, 4032, 320, 256, CENTRE, 0, 40, 1).cy).toBeLessThan(0.5)
  })

  it('a drag on the pinned axis does nothing, however hard it is dragged', () => {
    const c = dragCentre(4032, 3024, 320, 256, CENTRE, 0, 100000, 1)
    expect(c.cy).toBe(0.5)
  })

  it('a keyboard nudge moves one TARGET-RASTER pixel', () => {
    const c = nudgeCentre(4032, 3024, 320, 256, CENTRE, 1, 0)
    const before = cropWindow(4032, 3024, 320, 256, CENTRE)
    const after = cropWindow(4032, 3024, 320, 256, c)
    // One raster pixel is 3024/256 ≈ 11.8 source pixels; the even-rounding lands it on 12.
    expect(Math.abs(after.sx - before.sx)).toBe(12)
  })
})

describe('the halving chain', () => {
  it('⭐ plans the worked example: 3780×3024 → Scottie 1 in three halvings and a finish', () => {
    expect(halvingChain(3780, 3024, 320, 256)).toEqual([
      { w: 1890, h: 1512 },
      { w: 945, h: 756 },
      { w: 472, h: 378 },
      { w: 320, h: 256 },
    ])
  })

  it('every step is an exact halving or a 1:1 pass, and never overshoots the target', () => {
    for (const [sw, sh, tw, th] of [
      [4032, 3024, 320, 256],
      [3024, 4032, 320, 240],
      [8000, 6000, 800, 616],
      [1000, 1000, 512, 400],
      [4000, 300, 320, 256],
    ]) {
      const steps = halvingChain(sw, sh, tw, th)
      let [w, h] = [sw, sh]
      for (const s of steps.slice(0, -1)) {
        // Each axis either halved exactly (the box-average identity) or held (a row copy).
        expect([Math.floor(w / 2), w], `width step from ${w}`).toContain(s.w)
        expect([Math.floor(h / 2), h], `height step from ${h}`).toContain(s.h)
        expect(s.w).toBeGreaterThanOrEqual(tw)
        expect(s.h).toBeGreaterThanOrEqual(th)
        ;[w, h] = [s.w, s.h]
      }
      const last = steps[steps.length - 1]
      expect(last).toEqual({ w: tw, h: th })
      // ⭐ THE POINT OF THE WHOLE CHAIN: the final bilinear step is never more than a 2×
      // reduction on either axis, so it reads at least a quarter of the contributing
      // pixels instead of two of a hundred and fifty.
      expect(w / last.w, `residual width factor for ${sw}×${sh}`).toBeLessThanOrEqual(2.0000001)
      expect(h / last.h, `residual height factor for ${sw}×${sh}`).toBeLessThanOrEqual(2.0000001)
    }
  })

  it('⭐ halves PER AXIS — a panorama gets its width reduced properly, not dumped on one step', () => {
    // 4000×300 into 320×256: 12.5× to give on the width, nothing on the height. Halving
    // in lockstep would refuse to halve at all and hand the whole 12.5× to one bilinear
    // step, which is precisely the aliasing this module exists to prevent.
    expect(halvingChain(4000, 300, 320, 256)).toEqual([
      { w: 2000, h: 300 },
      { w: 1000, h: 300 },
      { w: 500, h: 300 },
      { w: 320, h: 256 },
    ])
  })

  it('an upscale is one bilinear step — blocky is worse than soft on an analogue line scan', () => {
    expect(halvingChain(160, 120, 320, 256)).toEqual([{ w: 320, h: 256 }])
    expect(isUpscale(160, 120, 320, 256)).toBe(true)
    expect(isUpscale(4032, 3024, 320, 256)).toBe(false)
  })

  it('degenerate inputs still produce a usable plan', () => {
    expect(halvingChain(0, 0, 320, 256)).toEqual([{ w: 320, h: 256 }])
    expect(halvingChain(NaN, 100, 320, 256)).toEqual([{ w: 320, h: 256 }])
  })

  it('reports the peak intermediate cost (PD-290 from a phone photo stays sub-frame)', () => {
    const steps = halvingChain(3924, 3024, 800, 616)
    // Largest intermediate is 1962×1512 ≈ 3.0 Mpx — a few ms of drawImage, not a stall.
    expect(peakStagePixels(steps)).toBeLessThan(4_000_000)
  })
})
