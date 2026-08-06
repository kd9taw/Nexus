// EXIF ORIENTATION + INTRINSIC DIMENSIONS, read straight off the file's bytes.
//
// ⚠️ THE TRAP THIS EXISTS FOR. A phone photo that looks upright in a file browser
// transmits SIDEWAYS if the orientation tag is ignored — an iPhone shot in portrait is
// stored 4032×3024 landscape with `Orientation = 6` (rotate 90° CW). That is the single
// most likely "it worked in testing, it is broken for a real operator" failure in the
// whole resizer, and an SSTV over is up to ~290 s of key-down to discover it in.
//
// ⚠️ AND THE SECOND TRAP, WHICH IS WORSE. A modern <img> defaults to
// `image-orientation: from-image`, so drawImage() USUALLY yields an upright picture and
// naturalWidth/Height come back already swapped for orientations 5–8.
// `createImageBitmap(blob)` with no options has had BOTH defaults across engine
// generations. So a pipeline that reads the tag AND applies the matrix double-rotates on
// one engine and is correct on the other — which is right in dev and wrong for the
// operator, or the reverse. Guessing from a browser version is not good enough.
//
// So we do not guess. `readExifOrientation` gets the tag, `readIntrinsicSize` gets the
// dimensions AS STORED IN THE FILE (the JPEG SOF frame header, the PNG IHDR, the WebP
// VP8/VP8L/VP8X header). The caller compares the stored dimensions with the ones the
// decoder handed back: if they came back swapped, the engine already applied the
// orientation and our matrix must be skipped. That is a fact about THIS decode rather
// than a bet on a browser.
//
// No dependency: this is a byte walk over a header we have already read into memory.

/** EXIF orientation, 1–8. 1 means "as shot"; 5–8 swap width and height. */
export type ExifOrientation = 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8

/** The container formats we accept, plus the ones we refuse BY NAME. */
export type ImageKind =
  | 'jpeg'
  | 'png'
  | 'webp'
  | 'gif'
  | 'bmp'
  | 'heic'
  | 'tiff'
  | 'avif'
  | 'svg'
  | 'psd'
  | 'raw'
  | 'unknown'

/** Magic-number sniff. Never trusts the file extension or the browser's `File.type`:
 *  an iPhone HEIC renamed `.jpg` has to be caught before a decode is attempted, because
 *  the failure we want to give the operator names their fix. */
export function sniffImageKind(buf: Uint8Array): ImageKind {
  const b = buf
  // `>>> 0` is load-bearing: JS `<<` yields a SIGNED 32-bit int, so the PNG magic
  // 0x89504E47 comes out negative and every comparison against the literal fails. That
  // made PNG sniff as "unknown" and PNG dimensions read as null.
  const u32 = (o: number) => (((b[o] << 24) | (b[o + 1] << 16) | (b[o + 2] << 8) | b[o + 3]) >>> 0)
  const ascii = (o: number, n: number) =>
    String.fromCharCode(...Array.from(b.subarray(o, o + n)))
  if (b.length < 12) return 'unknown'
  if (b[0] === 0xff && b[1] === 0xd8 && b[2] === 0xff) return 'jpeg'
  if (u32(0) === 0x89504e47 && u32(4) === 0x0d0a1a0a) return 'png'
  if (ascii(0, 4) === 'RIFF' && ascii(8, 4) === 'WEBP') return 'webp'
  if (ascii(0, 3) === 'GIF') return 'gif'
  if (b[0] === 0x42 && b[1] === 0x4d) return 'bmp'
  if (ascii(0, 4) === '8BPS') return 'psd'
  // ISO-BMFF families share the `ftyp` box; the brand is what separates them.
  if (ascii(4, 4) === 'ftyp') {
    const brand = ascii(8, 4)
    if (/^(heic|heix|heim|heis|hevc|hevx|mif1|msf1)$/.test(brand)) return 'heic'
    if (/^(avif|avis)$/.test(brand)) return 'avif'
  }
  // TIFF, and with it most camera RAW (CR2, NEF, DNG, ARW are TIFF-derived).
  if ((u32(0) === 0x49492a00 || u32(0) === 0x4d4d002a) && b.length > 12) {
    // CR2 stamps its own marker at byte 8; the rest we call TIFF and name as such.
    return ascii(8, 2) === 'CR' ? 'raw' : 'tiff'
  }
  if (/^\s*<(\?xml|svg)/i.test(ascii(0, 12))) return 'svg'
  return 'unknown'
}

/** The intrinsic (pre-rotation) pixel size the FILE declares, or null if we cannot read
 *  it from the header. Used for the double-rotation guard: compare it with what the
 *  decoder returned. */
export function readIntrinsicSize(buf: Uint8Array): { w: number; h: number } | null {
  switch (sniffImageKind(buf)) {
    case 'jpeg':
      return jpegFrameSize(buf)
    case 'png':
      return buf.length >= 24
        ? { w: be32(buf, 16), h: be32(buf, 20) }
        : null
    case 'webp':
      return webpSize(buf)
    default:
      return null
  }
}

/** EXIF orientation from a JPEG APP1 or a WebP `EXIF` chunk. Anything else — PNG, BMP,
 *  most GIF, a JPEG with no APP1, a tag outside 1–8 — is orientation 1, which matches
 *  what the `image` crate's `Orientation::from_exif` does with an unparseable value. */
export function readExifOrientation(buf: Uint8Array): ExifOrientation {
  const kind = sniffImageKind(buf)
  if (kind === 'jpeg') {
    const app1 = findJpegApp1(buf)
    return app1 ? parseTiffOrientation(buf, app1.start, app1.end) : 1
  }
  if (kind === 'webp') {
    const chunk = findRiffChunk(buf, 'EXIF')
    return chunk ? parseTiffOrientation(buf, chunk.start, chunk.end) : 1
  }
  return 1
}

// ---------------------------------------------------------------------------
// JPEG / RIFF / TIFF walks. All bounds-checked: an operator's file is untrusted
// input, and a truncated header must return null, never read past the buffer.
// ---------------------------------------------------------------------------

const be16 = (b: Uint8Array, o: number) => (b[o] << 8) | b[o + 1]
const be32 = (b: Uint8Array, o: number) =>
  ((b[o] << 24) | (b[o + 1] << 16) | (b[o + 2] << 8) | b[o + 3]) >>> 0

/** Walk the JPEG marker chain to the Start-Of-Frame and read its stored size. This is
 *  the UNROTATED size — the double-rotation guard's whole basis. */
function jpegFrameSize(b: Uint8Array): { w: number; h: number } | null {
  let o = 2
  while (o + 9 < b.length) {
    if (b[o] !== 0xff) {
      o++
      continue
    }
    const marker = b[o + 1]
    if (marker === 0xd8 || marker === 0x01 || (marker >= 0xd0 && marker <= 0xd7)) {
      o += 2
      continue
    }
    const len = be16(b, o + 2)
    if (len < 2) return null
    // SOF0..SOF15 carry the frame geometry; SOF4/SOF8/SOF12 are not frame headers.
    const isSof =
      marker >= 0xc0 && marker <= 0xcf && marker !== 0xc4 && marker !== 0xc8 && marker !== 0xcc
    if (isSof) return { h: be16(b, o + 5), w: be16(b, o + 7) }
    if (marker === 0xda) return null // start of scan — no frame header found
    o += 2 + len
  }
  return null
}

/** The APP1 segment's Exif payload (the byte range of the TIFF header onward). */
function findJpegApp1(b: Uint8Array): { start: number; end: number } | null {
  let o = 2
  while (o + 4 < b.length) {
    if (b[o] !== 0xff) {
      o++
      continue
    }
    const marker = b[o + 1]
    if (marker === 0xd8 || marker === 0x01 || (marker >= 0xd0 && marker <= 0xd7)) {
      o += 2
      continue
    }
    if (marker === 0xda) return null
    const len = be16(b, o + 2)
    if (len < 2) return null
    if (marker === 0xe1 && o + 4 + 6 <= b.length) {
      // "Exif\0\0" then the TIFF header.
      const tag = String.fromCharCode(...Array.from(b.subarray(o + 4, o + 8)))
      if (tag === 'Exif') return { start: o + 10, end: Math.min(o + 2 + len, b.length) }
    }
    o += 2 + len
  }
  return null
}

/** A named RIFF chunk's payload range (WebP is RIFF). */
function findRiffChunk(b: Uint8Array, want: string): { start: number; end: number } | null {
  let o = 12
  while (o + 8 <= b.length) {
    const id = String.fromCharCode(...Array.from(b.subarray(o, o + 4)))
    const size = le32(b, o + 4)
    const start = o + 8
    const end = Math.min(start + size, b.length)
    if (id === want) return { start, end }
    // Chunks are padded to even length.
    o = start + size + (size & 1)
    if (size === 0) break
  }
  return null
}

const le16 = (b: Uint8Array, o: number) => b[o] | (b[o + 1] << 8)
const le32 = (b: Uint8Array, o: number) =>
  (b[o] | (b[o + 1] << 8) | (b[o + 2] << 16) | (b[o + 3] << 24)) >>> 0

/** WebP intrinsic size: simple lossy (`VP8 `), lossless (`VP8L`) or extended (`VP8X`). */
function webpSize(b: Uint8Array): { w: number; h: number } | null {
  const x = findRiffChunk(b, 'VP8X')
  if (x && x.start + 10 <= b.length) {
    const w = 1 + (b[x.start + 4] | (b[x.start + 5] << 8) | (b[x.start + 6] << 16))
    const h = 1 + (b[x.start + 7] | (b[x.start + 8] << 8) | (b[x.start + 9] << 16))
    return { w, h }
  }
  const lossy = findRiffChunk(b, 'VP8 ')
  if (lossy && lossy.start + 10 <= b.length) {
    return { w: le16(b, lossy.start + 6) & 0x3fff, h: le16(b, lossy.start + 8) & 0x3fff }
  }
  const lossless = findRiffChunk(b, 'VP8L')
  if (lossless && lossless.start + 5 <= b.length) {
    const bits = le32(b, lossless.start + 1)
    return { w: (bits & 0x3fff) + 1, h: ((bits >> 14) & 0x3fff) + 1 }
  }
  return null
}

/** Orientation tag (0x0112) out of a TIFF/Exif IFD0. */
function parseTiffOrientation(b: Uint8Array, start: number, end: number): ExifOrientation {
  if (start + 8 > end) return 1
  const le = be16(b, start) === 0x4949
  if (!le && be16(b, start) !== 0x4d4d) return 1
  const u16 = (o: number) => (le ? le16(b, o) : be16(b, o))
  const u32 = (o: number) => (le ? le32(b, o) : be32(b, o))
  if (u16(start + 2) !== 0x002a) return 1
  const ifd0 = start + u32(start + 4)
  if (ifd0 + 2 > end) return 1
  const count = u16(ifd0)
  for (let i = 0; i < count; i++) {
    const entry = ifd0 + 2 + i * 12
    if (entry + 12 > end) break
    if (u16(entry) !== 0x0112) continue
    // SHORT value, left-aligned in the 4-byte value field.
    const v = u16(entry + 8)
    return v >= 1 && v <= 8 ? (v as ExifOrientation) : 1
  }
  return 1
}
