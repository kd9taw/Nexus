// THE STREET MAP'S READ PATH: an installed pack and the maps folder's assets, handed to MapLibre
// through two custom protocols, each a thin layer over one Tauri command.
//
//   pmtiles://<key>[/z/x/y]   pmtiles' own Protocol, over a Source whose every byte comes from
//                             `street_map_read`. The pack's id names the file and Rust maps it to a
//                             path, so the webview never supplies one.
//   street-asset://<path>     `street_map_asset`: glyph ranges and sprite sheets from `assets/`.
//
// ⛔ NEVER TAURI'S ASSET PROTOCOL OR `convertFileSrc` FOR A PACK. In the Tauri this app ships, the
// asset protocol answers a range request with at most 1000 KiB, and pmtiles' FetchSource never
// checks a short answer: a leaf directory or a tile over 1000 KiB would be cut short and decoded as
// garbage, silently. `street_map_read` answers exactly what was asked (fewer bytes only at the end
// of the file), and PackSource refuses any other answer, so a truncation is an error, never a map.
//
// ZERO NETWORK. Everything comes from the pack and the assets. pmtiles' Protocol treats an archive
// it does not hold as a URL and FETCHES it, so the pack loader refuses a key nobody opened before
// pmtiles sees the request.

import { addProtocol, type AddProtocolResponseData, type GetResourceResponse, type RequestParameters } from 'maplibre-gl'
import { PMTiles, Protocol, type RangeResponse, type Source } from 'pmtiles'
import { streetMapAsset, streetMapRead } from '../api'

/** One installed pack, exactly as `street_map_packs()` lists it. */
export interface StreetPack {
  id: string
  name: string
  /** [west, south, east, north] in degrees. */
  bbox: [number, number, number, number]
  minZoom: number
  maxZoom: number
  detail: 'streets' | 'roads'
  /** The file's size in bytes: a read may come back short only where the file ends. */
  bytes: number
  dataDate: string
  sha256: string
}

/** The most `street_map_read` answers in one call; Rust refuses a larger read. */
export const MAX_READ = 4 * 1024 * 1024

/** pmtiles' Protocol parses exactly this scheme. */
const PACK_SCHEME = 'pmtiles'
const ASSET_SCHEME = 'street-asset'
/** How pmtiles' Protocol splits a tile URL into the archive key and z/x/y. */
const TILE_URL = /pmtiles:\/\/(.+)\/(\d+)\/(\d+)\/(\d+)/

/** A pack's key: its id plus its sha256, so an updated pack is never served from the old one's
 *  cached directories. The id is encoded because the key is the start of a URL. */
export function packKey(pack: StreetPack): string {
  return `${encodeURIComponent(pack.id)}/${pack.sha256}`
}

/** The style's source URL for a pack. */
export function packUrl(pack: StreetPack): string {
  return `${PACK_SCHEME}://${packKey(pack)}`
}

/** The style URL of a file under `assets/`; MapLibre's templates pass through untouched. */
export function assetUrl(path: string): string {
  return `${ASSET_SCHEME}://${path}`
}

/** The `assets/`-relative path a `street-asset://` URL names, as `street_map_asset` takes it. */
export function assetPath(url: string): string {
  return decodeURIComponent(url.slice(ASSET_SCHEME.length + 3))
}

/** pmtiles' view of an installed pack. */
export class PackSource implements Source {
  private readonly pack: StreetPack

  constructor(pack: StreetPack) {
    this.pack = pack
  }

  getKey(): string {
    return packKey(this.pack)
  }

  async getBytes(offset: number, length: number): Promise<RangeResponse> {
    if (length <= MAX_READ) return { data: await this.read(offset, length) }
    // pmtiles knows nothing of Rust's ceiling, so a larger read goes in pieces Rust will answer.
    const out = new Uint8Array(length)
    let got = 0
    while (got < length) {
      const want = Math.min(MAX_READ, length - got)
      const part = await this.read(offset + got, want)
      out.set(new Uint8Array(part), got)
      got += part.byteLength
      if (part.byteLength < want) break // the end of the file
    }
    return { data: got === length ? out.buffer : out.buffer.slice(0, got) }
  }

  /** One `street_map_read`, refused unless it is exactly what was asked for, or a short read that
   *  ends precisely where the file does. */
  private async read(offset: number, length: number): Promise<ArrayBuffer> {
    const data: unknown = await streetMapRead(this.pack.id, offset, length)
    if (!(data instanceof ArrayBuffer)) {
      throw new Error(`street_map_read answered ${Object.prototype.toString.call(data)}, not bytes`)
    }
    const endOfFile = data.byteLength < length && offset + data.byteLength === this.pack.bytes
    if (data.byteLength !== length && !endOfFile) {
      throw new Error(`street_map_read answered ${data.byteLength} of ${length} bytes at ${offset} in ${this.pack.id}`)
    }
    return data
  }
}

/** The archives MapLibre may read, by key: packs this window opened, nothing else. */
const archives = new Protocol()

/** MapLibre's loader for `pmtiles://`, keyed exactly as pmtiles keys it: the whole remainder for
 *  the TileJSON request, everything before `/z/x/y` for a tile. */
function loadPack(params: RequestParameters, abort: AbortController): Promise<GetResourceResponse<AddProtocolResponseData>> {
  const key = params.type === 'json' ? params.url.slice(PACK_SCHEME.length + 3) : TILE_URL.exec(params.url)?.[1]
  if (key === undefined || !archives.get(key)) {
    return Promise.reject(new Error(`no street map pack is open for ${params.url}`))
  }
  return archives.tile(params, abort) as Promise<GetResourceResponse<AddProtocolResponseData>>
}

/** MapLibre's loader for `street-asset://`: glyph ranges and sprite sheets as bytes (MapLibre
 *  decodes the sheet), a sprite index as JSON. */
async function loadAsset(params: RequestParameters): Promise<GetResourceResponse<AddProtocolResponseData>> {
  const bytes = await streetMapAsset(assetPath(params.url))
  return { data: params.type === 'json' ? JSON.parse(new TextDecoder().decode(bytes)) : bytes }
}

/** Let MapLibre read `pack` and its assets. Returns the undo, which drops the pack's cached
 *  directories. The loaders stay registered: the pack loader refuses any pack nobody opened. */
export function openStreetPack(pack: StreetPack): () => void {
  addProtocol(PACK_SCHEME, loadPack)
  addProtocol(ASSET_SCHEME, loadAsset)
  const key = packKey(pack)
  const archive = new PMTiles(new PackSource(pack))
  archives.add(archive)
  return () => {
    if (archives.get(key) === archive) archives.tiles.delete(key)
  }
}
