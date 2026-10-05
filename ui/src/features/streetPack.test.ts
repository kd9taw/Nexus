// @vitest-environment jsdom
//
// The street map's read path, over the real api.ts wrappers with the Tauri bridge stubbed (the
// api.js8.test.ts seam): every byte of a pack must come from `street_map_read` with exactly the
// arguments Rust destructures, come back untouched, and be refused when it is not what was asked
// for — pmtiles itself never notices a short read, which is the failure this path exists to stop.
//
// The archive is SYNTHETIC, written here at test time: no map data ever enters the repository.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { addProtocol } from 'maplibre-gl'
import { assetPath, assetUrl, MAX_READ, openStreetPack, PackSource, packKey, packUrl, type StreetPack } from './streetPack'

vi.mock('maplibre-gl', () => ({ addProtocol: vi.fn() }))

type Call = { cmd: string; args: Record<string, unknown> }
let calls: Call[] = []
/** What the stubbed bridge answers: the test sets it per case. */
let answer: (cmd: string, args: Record<string, unknown>) => unknown

beforeEach(() => {
  calls = []
  ;(window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
    invoke: async (cmd: string, args: Record<string, unknown>) => {
      calls.push({ cmd, args })
      return answer(cmd, args)
    },
  }
  // Any network request would land here; the read path must never make one.
  vi.stubGlobal('fetch', vi.fn(() => Promise.reject(new Error('network'))))
})
afterEach(() => {
  delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
  vi.unstubAllGlobals()
  vi.mocked(addProtocol).mockClear()
})

const pack = (over: Partial<StreetPack> = {}): StreetPack => ({
  id: 'home-200km',
  name: 'Home',
  bbox: [-98.2, 38.4, -97.0, 39.3],
  minZoom: 0,
  maxZoom: 14,
  detail: 'streets',
  bytes: 64,
  dataDate: '2026-10-04',
  sha256: 'ab'.repeat(32),
  ...over,
})

/** Bytes whose value says where they came from, so a misplaced range cannot pass. */
function patterned(offset: number, length: number): ArrayBuffer {
  const out = new Uint8Array(length)
  for (let i = 0; i < length; i++) out[i] = (offset + i) % 251
  return out.buffer
}

/** Serve `file` the way Rust does: exactly what was asked, or what is left at the end. */
function serveFile(file: ArrayBuffer) {
  answer = (cmd, args) => {
    if (cmd !== 'street_map_read') throw new Error(`unexpected ${cmd}`)
    const offset = args.offset as number
    return file.slice(offset, offset + (args.length as number))
  }
}

describe('PackSource', () => {
  it('reads through street_map_read with the pack id, offset and length, and passes the ArrayBuffer through untouched', async () => {
    const bytes = patterned(16, 32)
    answer = () => bytes
    const got = await new PackSource(pack()).getBytes(16, 32)
    expect(calls).toEqual([{ cmd: 'street_map_read', args: { packId: 'home-200km', offset: 16, length: 32 } }])
    expect(got.data).toBe(bytes)
  })

  it('accepts a short read only where the file ends', async () => {
    serveFile(patterned(0, 64))
    const got = await new PackSource(pack({ bytes: 64 })).getBytes(48, 100)
    expect(got.data.byteLength).toBe(16)
  })

  it('refuses a short read that does not end at the end of the file (a truncation)', async () => {
    answer = () => patterned(0, 10)
    await expect(new PackSource(pack({ bytes: 64 })).getBytes(0, 32)).rejects.toThrow(/answered 10 of 32 bytes at 0/)
  })

  it('refuses a read longer than asked', async () => {
    answer = () => patterned(0, 40)
    await expect(new PackSource(pack()).getBytes(0, 32)).rejects.toThrow(/answered 40 of 32 bytes/)
  })

  it('refuses an answer that is not bytes', async () => {
    answer = () => [1, 2, 3]
    await expect(new PackSource(pack()).getBytes(0, 3)).rejects.toThrow(/not bytes/)
  })

  it("passes the bridge's own error on", async () => {
    answer = () => {
      throw new Error('no such pack: home-200km')
    }
    await expect(new PackSource(pack()).getBytes(0, 16)).rejects.toThrow('no such pack: home-200km')
  })

  it('splits a read over 4 MiB into reads Rust will answer, and joins them in order', async () => {
    const size = 2 * MAX_READ + 1000
    serveFile(patterned(0, size + 5000))
    const got = await new PackSource(pack({ bytes: size + 5000 })).getBytes(7, size)
    expect(calls.map((c) => [c.args.offset, c.args.length])).toEqual([
      [7, MAX_READ],
      [7 + MAX_READ, MAX_READ],
      [7 + 2 * MAX_READ, 1000],
    ])
    // Compared byte by byte: toEqual walks an 8 MB typed array key by key and exhausts the heap.
    const bytes = new Uint8Array(got.data)
    expect(bytes.length).toBe(size)
    expect(bytes.findIndex((b, i) => b !== (7 + i) % 251)).toBe(-1)
  })

  it('stops a split read at the end of the file', async () => {
    const file = MAX_READ + 300
    serveFile(patterned(0, file))
    const got = await new PackSource(pack({ bytes: file })).getBytes(100, 2 * MAX_READ)
    expect(got.data.byteLength).toBe(file - 100)
    expect(calls).toHaveLength(2)
  })

  it('keys an archive by the pack id plus its sha256, so an updated pack is a different archive', () => {
    const a = new PackSource(pack()).getKey()
    expect(a).toBe(`home-200km/${'ab'.repeat(32)}`)
    expect(new PackSource(pack({ sha256: 'cd'.repeat(32) })).getKey()).not.toBe(a)
    // The key starts a URL, so an id with a slash or a space cannot split it.
    expect(packKey(pack({ id: 'a b/c' }))).toBe(`a%20b%2Fc/${'ab'.repeat(32)}`)
  })
})

describe('asset paths', () => {
  it('names the glyph range MapLibre asks for as street_map_asset takes it', () => {
    // MapLibre fills the style's template by plain substitution, spaces and all.
    const url = assetUrl('fonts/{fontstack}/{range}.pbf').replace('{fontstack}', 'Noto Sans Regular').replace('{range}', '0-255')
    expect(assetPath(url)).toBe('fonts/Noto Sans Regular/0-255.pbf')
  })

  it('names the sprite sheet MapLibre asks for as street_map_asset takes it', () => {
    // MapLibre appends the pixel ratio and extension to the sprite URL's path with `new URL`.
    const url = new URL(assetUrl('sprites/light'))
    url.pathname += '@2x.png'
    expect(assetPath(url.toString())).toBe('sprites/light@2x.png')
  })
})

// ---------------------------------------------------------------------------------------------
// A synthetic PMTiles v3 archive: header, a one-entry root directory, empty metadata, one tile,
// nothing compressed. The layout is the spec's (and pmtiles' bytesToHeader/deserializeIndex).
// ---------------------------------------------------------------------------------------------
function syntheticArchive(tile: Uint8Array): ArrayBuffer {
  const dir = Uint8Array.of(1, 0, 1, tile.length, 1) // 1 entry: tile id 0 (z0), run 1, length, offset 0 (+1)
  const meta = new TextEncoder().encode('{}')
  const rootAt = 127
  const metaAt = rootAt + dir.length
  const dataAt = metaAt + meta.length
  const out = new Uint8Array(dataAt + tile.length)
  const v = new DataView(out.buffer)
  out.set(new TextEncoder().encode('PMTiles'), 0)
  v.setUint8(7, 3)
  const u64 = (at: number, n: number) => v.setBigUint64(at, BigInt(n), true)
  u64(8, rootAt)
  u64(16, dir.length)
  u64(24, metaAt)
  u64(32, meta.length)
  u64(40, dataAt) // no leaf directories
  u64(48, 0)
  u64(56, dataAt)
  u64(64, tile.length)
  u64(72, 1)
  u64(80, 1)
  u64(88, 1)
  v.setUint8(96, 1) // clustered
  v.setUint8(97, 1) // internal compression: none
  v.setUint8(98, 1) // tile compression: none
  v.setUint8(99, 1) // MVT
  v.setUint8(100, 0) // min zoom
  v.setUint8(101, 0) // max zoom
  v.setInt32(102, -10_000_000, true)
  v.setInt32(106, -10_000_000, true)
  v.setInt32(110, 10_000_000, true)
  v.setInt32(114, 10_000_000, true)
  out.set(dir, rootAt)
  out.set(meta, metaAt)
  out.set(tile, dataAt)
  return out.buffer
}

type Loader = (params: { url: string; type?: string }, abort: AbortController) => Promise<{ data: unknown }>
const loaderFor = (scheme: string): Loader => {
  const call = vi.mocked(addProtocol).mock.calls.find(([s]) => s === scheme)
  if (!call) throw new Error(`no ${scheme} loader registered`)
  return call[1] as unknown as Loader
}

describe('openStreetPack — the loaders MapLibre calls', () => {
  const tile = Uint8Array.of(9, 8, 7, 6, 5)
  const file = syntheticArchive(tile)
  const opened = () => pack({ bytes: file.byteLength })

  it('registers pmtiles:// through pmtiles, and street-asset://', () => {
    serveFile(file)
    const close = openStreetPack(opened())
    expect(vi.mocked(addProtocol).mock.calls.map(([s]) => s).sort()).toEqual(['pmtiles', 'street-asset'])
    close()
  })

  it("serves an open pack's TileJSON and tiles, every byte from street_map_read", async () => {
    serveFile(file)
    const p = opened()
    const close = openStreetPack(p)
    const loadPack = loaderFor('pmtiles')

    const tileJson = await loadPack({ url: packUrl(p), type: 'json' }, new AbortController())
    expect(tileJson.data).toEqual({
      tiles: [`${packUrl(p)}/{z}/{x}/{y}`],
      minzoom: 0,
      maxzoom: 0,
      bounds: [-1, -1, 1, 1],
    })
    const got = await loadPack({ url: `${packUrl(p)}/0/0/0`, type: 'arrayBuffer' }, new AbortController())
    expect(new Uint8Array(got.data as ArrayBufferLike)).toEqual(tile)

    expect(calls.length).toBeGreaterThan(0)
    expect(calls.every((c) => c.cmd === 'street_map_read' && c.args.packId === p.id)).toBe(true)
    expect(fetch).not.toHaveBeenCalled()
    close()
  })

  it('refuses a pack nobody opened, and a closed one, without touching the network', async () => {
    serveFile(file)
    const p = opened()
    const close = openStreetPack(p)
    const loadPack = loaderFor('pmtiles')
    // pmtiles' own Protocol would FETCH an unknown key as a URL; this must never reach it.
    const stranger = 'https://example.invalid/planet.pmtiles'
    await expect(loadPack({ url: `pmtiles://${stranger}`, type: 'json' }, new AbortController())).rejects.toThrow(/no street map pack/)
    await expect(loadPack({ url: `pmtiles://${stranger}/0/0/0` }, new AbortController())).rejects.toThrow(/no street map pack/)
    close()
    await expect(loadPack({ url: packUrl(p), type: 'json' }, new AbortController())).rejects.toThrow(/no street map pack/)
    expect(fetch).not.toHaveBeenCalled()
    expect(calls).toEqual([])
  })

  it('reads glyphs and sprite sheets as bytes and a sprite index as JSON, through street_map_asset', async () => {
    const glyphs = Uint8Array.of(1, 2, 3).buffer
    const index = new TextEncoder().encode('{"arrow":{"x":0,"y":0,"width":8,"height":8,"pixelRatio":2}}').buffer
    answer = (_cmd, args) => (String(args.path).endsWith('.json') ? index : glyphs)
    const close = openStreetPack(opened())
    const loadAsset = loaderFor('street-asset')

    const glyph = await loadAsset({ url: 'street-asset://fonts/Noto Sans Regular/0-255.pbf', type: 'arrayBuffer' }, new AbortController())
    expect(glyph.data).toBe(glyphs)
    const sprite = await loadAsset({ url: 'street-asset://sprites/light@2x.json', type: 'json' }, new AbortController())
    expect(sprite.data).toEqual({ arrow: { x: 0, y: 0, width: 8, height: 8, pixelRatio: 2 } })
    expect(calls).toEqual([
      { cmd: 'street_map_asset', args: { path: 'fonts/Noto Sans Regular/0-255.pbf' } },
      { cmd: 'street_map_asset', args: { path: 'sprites/light@2x.json' } },
    ])
    expect(fetch).not.toHaveBeenCalled()
    close()
  })
})
