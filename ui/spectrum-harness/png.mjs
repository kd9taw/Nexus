// PNG in and out for the pixel fixtures, on node:zlib alone — 8-bit RGB/RGBA, not interlaced,
// every row filter on the way in (the files may be re-saved by any tool), adaptive on the way out.
import { crc32, deflateSync, inflateSync } from 'node:zlib'

const SIGNATURE = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])

function chunk(type, data) {
  const len = Buffer.alloc(4)
  len.writeUInt32BE(data.length)
  const body = Buffer.concat([Buffer.from(type, 'ascii'), data])
  const crc = Buffer.alloc(4)
  crc.writeUInt32BE(crc32(body))
  return Buffer.concat([len, body, crc])
}

function paeth(a, b, c) {
  const p = a + b - c
  const pa = Math.abs(p - a)
  const pb = Math.abs(p - b)
  const pc = Math.abs(p - c)
  return pa <= pb && pa <= pc ? a : pb <= pc ? b : c
}

/** Encode RGBA pixels. Stored as RGB when every pixel is opaque (a canvas row always is). */
export function encodePng(width, height, rgba) {
  const opaque = rgba.every((v, i) => i % 4 !== 3 || v === 255)
  const ch = opaque ? 3 : 4
  const stride = width * ch
  const raw = Buffer.alloc(height * (stride + 1))
  const prev = Buffer.alloc(stride)
  const cur = Buffer.alloc(stride)
  const cand = [0, 1, 2].map(() => Buffer.alloc(stride))
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      for (let c = 0; c < ch; c++) cur[x * ch + c] = rgba[(y * width + x) * 4 + c]
    }
    // Filters None, Sub, Up — the one with the smallest absolute sum, the usual heuristic.
    for (let i = 0; i < stride; i++) {
      const left = i >= ch ? cur[i - ch] : 0
      cand[0][i] = cur[i]
      cand[1][i] = (cur[i] - left) & 0xff
      cand[2][i] = (cur[i] - prev[i]) & 0xff
    }
    let best = 0
    let bestSum = Infinity
    for (let f = 0; f < 3; f++) {
      let sum = 0
      for (let i = 0; i < stride; i++) sum += cand[f][i] < 128 ? cand[f][i] : 256 - cand[f][i]
      if (sum < bestSum) {
        bestSum = sum
        best = f
      }
    }
    const o = y * (stride + 1)
    raw[o] = best
    cand[best].copy(raw, o + 1)
    cur.copy(prev)
  }
  const ihdr = Buffer.alloc(13)
  ihdr.writeUInt32BE(width, 0)
  ihdr.writeUInt32BE(height, 4)
  ihdr[8] = 8
  ihdr[9] = opaque ? 2 : 6
  return Buffer.concat([
    SIGNATURE,
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw, { level: 9 })),
    chunk('IEND', Buffer.alloc(0)),
  ])
}

/** Decode to `{ width, height, rgba }`; throws on anything but 8-bit RGB/RGBA, non-interlaced. */
export function decodePng(buf) {
  if (!buf.subarray(0, 8).equals(SIGNATURE)) throw new Error('not a PNG')
  let off = 8
  let width = 0
  let height = 0
  let type = 0
  const idat = []
  while (off < buf.length) {
    const len = buf.readUInt32BE(off)
    const kind = buf.toString('ascii', off + 4, off + 8)
    const data = buf.subarray(off + 8, off + 8 + len)
    if (kind === 'IHDR') {
      width = data.readUInt32BE(0)
      height = data.readUInt32BE(4)
      type = data[9]
      if (data[8] !== 8 || (type !== 2 && type !== 6) || data[12] !== 0) {
        throw new Error(`unsupported PNG (bit depth ${data[8]}, colour type ${type}, interlace ${data[12]})`)
      }
    } else if (kind === 'IDAT') idat.push(data)
    else if (kind === 'IEND') break
    off += 12 + len
  }
  const ch = type === 6 ? 4 : 3
  const stride = width * ch
  const raw = inflateSync(Buffer.concat(idat))
  const rgba = new Uint8Array(width * height * 4)
  let prev = new Uint8Array(stride)
  for (let y = 0; y < height; y++) {
    const f = raw[y * (stride + 1)]
    if (f > 4) throw new Error(`bad PNG row filter ${f}`)
    const src = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1))
    const cur = new Uint8Array(stride)
    for (let i = 0; i < stride; i++) {
      const a = i >= ch ? cur[i - ch] : 0
      const b = prev[i]
      const c = i >= ch ? prev[i - ch] : 0
      const pred = f === 0 ? 0 : f === 1 ? a : f === 2 ? b : f === 3 ? (a + b) >> 1 : paeth(a, b, c)
      cur[i] = (src[i] + pred) & 0xff
    }
    for (let x = 0; x < width; x++) {
      for (let c = 0; c < 3; c++) rgba[(y * width + x) * 4 + c] = cur[x * ch + c]
      rgba[(y * width + x) * 4 + 3] = ch === 4 ? cur[x * ch + 3] : 255
    }
    prev = cur
  }
  return { width, height, rgba }
}
