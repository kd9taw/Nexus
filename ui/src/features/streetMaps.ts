// THE STREET MAP IN THE WEBVIEW: whether its choice is offered, the packs on this computer, and the
// one download the Rust side runs at a time (src-tauri/src/street_map.rs over crates/street-map).
//
// ONE STORE for the map picker (its badge and progress chip), the download sheet and Settings ▸
// Appearance ▸ Map & globe ▸ Street maps. A download outlives the sheet that started it and keeps
// running while the operator uses the app, so its progress lives here, not in a component. Per
// window: a pop-out learns of a pack another window installed when it is focused.
//
// OFFERED ONCE HOSTED. The choice is offered to every desktop operator because
// STREET_MAP_MANIFEST_URL names the hosted index (maps.hamradiotools.io, which
// .github/workflows/street-maps.yml keeps). NEXUS_STREET_MAP=1 in a computer's environment
// (`street_map_info`) adds the bench aid that installs a map file the operator already has. Never on
// the Remote page: a pack lives on the station's disk.
//
// NOTHING REACHES THE NETWORK until the operator asks: the sheet's size (it is open), Download, and
// Check for updates. Listing packs and unfinished downloads reads the maps folder only. The app
// offers an update and never downloads one by itself (operator ruling 2026-10-04, D8).
import { useEffect, useSyncExternalStore } from 'react'
import {
  isTauri,
  streetMapCancel,
  streetMapDownload,
  streetMapInfo,
  streetMapInstallFile,
  streetMapPacks,
  streetMapRemove,
  streetMapUnfinished,
  streetMapUpdates,
} from '../api'
import { webgl2Available } from '../gpu'
import type { LatLon } from '../grid'
import { packCovers } from './streetOverlay'
import type { StreetPack } from './streetPack'

/** The hosted street-map index. While it names the host, the Street choice is offered to every
 *  desktop operator (a test pins it); an empty string hides it from everyone but a bench. The Rust
 *  side fetches the same address: `street_map::default_origin()` + `/streetmaps.json`. */
export const STREET_MAP_MANIFEST_URL: string = 'https://maps.hamradiotools.io/streetmaps.json'

/** The squares offered, in km across (operator ruling 2026-10-04, D2), and the default. */
export const STREET_KMS = [50, 100, 200, 400] as const
export type StreetKm = (typeof STREET_KMS)[number]
export const DEFAULT_KM: StreetKm = 200

/** All streets (tile zoom 14, the default) or Main roads (zoom 12): D3. */
export type StreetDetail = 'streets' | 'roads'

/** What the operator asks for: `crates/street-map` `StreetArea`. */
export interface StreetArea {
  lat: number
  lon: number
  km: StreetKm
  detail: StreetDetail
}

/** The sheet's numbers before anything downloads: `StreetSize`. */
export interface StreetSize {
  packId: string
  bytes: number
  downloadBytes: number
  assetsBytes: number
  tiles: number
  requests: number
  bbox: [number, number, number, number]
  minZoom: number
  maxZoom: number
  detail: StreetDetail
  buildId: string
  dataDate: string
  freeBytes: number | null
  enoughSpace: boolean
  resumeBytes: number
  installed: boolean
}

/** One progress message from a running download: `StreetProgress`. */
export type StreetProgress =
  | { phase: 'assets'; done: number; total: number }
  | { phase: 'tiles'; done: number; total: number; bytesPerSec: number; etaSecs: number | null }
  | { phase: 'retrying'; attempt: number; waitSecs: number; reason: string }
  | { phase: 'verifying'; done: number; total: number }

export interface StreetUnfinished {
  packId: string
  area: StreetArea
  buildId: string
  dataDate: string
  doneBytes: number
  totalBytes: number
}

export interface StreetUpdate {
  packId: string
  area: StreetArea
  dataDate: string
  newBuildId: string
  newDataDate: string
}

export type StreetErrorKind =
  | 'invalidArea'
  | 'badRequest'
  | 'unknownPack'
  | 'notFound'
  | 'busy'
  | 'diskSpace'
  | 'network'
  | 'server'
  | 'paused'
  | 'cancelled'
  | 'buildGone'
  | 'buildChanged'
  | 'rangeIgnored'
  | 'invalidManifest'
  | 'invalidArchive'
  | 'io'

/** A street-map command's refusal. `message` is English diagnostics; the UI says it by `kind`. */
export interface StreetError {
  kind: StreetErrorKind
  message: string
}

export interface StreetInfo {
  folder: string
  bench: boolean
}

export type StreetInstalled = { kind: 'pack'; pack: StreetPack } | { kind: 'assets' }

/** A refusal as `{kind, message}`, whatever shape it arrived in. */
export function asStreetError(e: unknown): StreetError {
  if (e && typeof e === 'object' && typeof (e as StreetError).kind === 'string') {
    return { kind: (e as StreetError).kind, message: String((e as StreetError).message ?? '') }
  }
  return { kind: 'io', message: e instanceof Error ? e.message : String(e) }
}

/** The kinds a Retry answers: the network or the host stopped it, and it resumes where it stopped
 *  (or, for a retired build, starts again on the current one). */
const RETRYABLE: ReadonlySet<StreetErrorKind> = new Set(['paused', 'network', 'server', 'rangeIgnored', 'buildGone', 'buildChanged'])

/** One download, as every view of it sees it. */
export type StreetDownload =
  | { state: 'idle' }
  /** `percent` is the last whole-number progress (0–100), kept through a retry. */
  | { state: 'running'; area: StreetArea; progress: StreetProgress | null; percent: number }
  /** Stopped, saying why. `retry` when pressing Download again resumes it. */
  | { state: 'stopped'; area: StreetArea; error: StreetError; retry: boolean }

export interface StreetMapsState {
  /** Whether the Street choice and the Settings block show; null until known. */
  offered: boolean | null
  /** The maps folder, for removing it by hand; '' until known. */
  folder: string
  /** This run benches the street map (NEXUS_STREET_MAP=1): the bench aid shows. */
  bench: boolean
  /** A WebGL2 context can be made here, which MapLibre needs (gpu.ts; software GL counts). Asked
   *  once, and only once the street map is offered. */
  webgl2: boolean
  /** The packs on this computer; null until listed. */
  packs: StreetPack[] | null
  /** Downloads that stopped before they finished, for Resume. */
  unfinished: StreetUnfinished[]
  download: StreetDownload
}

const INITIAL: StreetMapsState = {
  offered: null,
  folder: '',
  bench: false,
  webgl2: false,
  packs: null,
  unfinished: [],
  download: { state: 'idle' },
}
let state: StreetMapsState = INITIAL
let loading: Promise<void> | null = null
const listeners = new Set<() => void>()

function set(patch: Partial<StreetMapsState>): void {
  state = { ...state, ...patch }
  for (const l of listeners) l()
}

function subscribe(l: () => void): () => void {
  listeners.add(l)
  return () => listeners.delete(l)
}

/** List the packs and the unfinished downloads again: after a change here, and when this window is
 *  focused (another window may have downloaded or removed one). */
export async function refreshStreetMaps(): Promise<void> {
  const [packs, unfinished] = await Promise.all([
    streetMapPacks().catch(() => [] as StreetPack[]),
    streetMapUnfinished().catch(() => [] as StreetUnfinished[]),
  ])
  set({ packs: Array.isArray(packs) ? packs : [], unfinished: Array.isArray(unfinished) ? unfinished : [] })
}

function load(): Promise<void> {
  loading ??= (async () => {
    let info: StreetInfo = { folder: '', bench: false }
    const shell = isTauri() === true
    if (shell) {
      try {
        const got = await streetMapInfo()
        if (got && typeof got.bench === 'boolean') info = got
      } catch {
        /* a shell without the command: not offered */
      }
    }
    const offered = shell && (STREET_MAP_MANIFEST_URL !== '' || info.bench)
    set({ offered, folder: info.folder, bench: info.bench, webgl2: offered && webgl2Available() })
    if (!offered) return
    await refreshStreetMaps()
    window.addEventListener('focus', () => void refreshStreetMaps())
  })()
  return loading
}

/** The street map's state for this window, loaded on first use. */
export function useStreetMaps(): StreetMapsState {
  const s = useSyncExternalStore(subscribe, () => state)
  useEffect(() => {
    void load()
  }, [])
  return s
}

/** The pack Street draws: the one holding the station, else the newest. */
export function packFor(packs: readonly StreetPack[] | null, at: LatLon | null): StreetPack | null {
  if (!packs || packs.length === 0) return null
  return (at && packs.find((p) => packCovers(p, at.lat, at.lon))) || packs[packs.length - 1]
}

const percentOf = (p: StreetProgress, prev: number): number =>
  p.phase === 'tiles' && p.total > 0
    ? Math.min(99, Math.floor((100 * p.done) / p.total))
    : p.phase === 'verifying'
      ? 99
      : p.phase === 'assets'
        ? 0
        : prev

/** Download `area`, or resume an unfinished download of it: the Rust side picks it up. Nothing
 *  happens while one is running. Resolves with the pack, or null when it did not finish (the state
 *  then says why). */
export async function startStreetDownload(area: StreetArea): Promise<StreetPack | null> {
  if (state.download.state === 'running') return null
  set({ download: { state: 'running', area, progress: null, percent: 0 } })
  try {
    const pack = await streetMapDownload(area, (progress) => {
      const d = state.download
      if (d.state === 'running') set({ download: { ...d, progress, percent: percentOf(progress, d.percent) } })
    })
    set({ download: { state: 'idle' } })
    await refreshStreetMaps()
    return pack
  } catch (e) {
    const error = asStreetError(e)
    set({
      download:
        error.kind === 'cancelled' ? { state: 'idle' } : { state: 'stopped', area, error, retry: RETRYABLE.has(error.kind) },
    })
    await refreshStreetMaps()
    return null
  }
}

/** Stop the running download. It keeps what it has, and Download resumes it. */
export async function cancelStreetDownload(): Promise<void> {
  await streetMapCancel()
}

/** Forget a stopped download's message (the sheet closing, or a new choice). */
export function clearStreetStop(): void {
  if (state.download.state === 'stopped') set({ download: { state: 'idle' } })
}

/** Remove a pack, or what an unfinished download of it left. Resolves with the bytes freed. */
export async function removeStreetMap(packId: string): Promise<number> {
  try {
    return await streetMapRemove(packId)
  } finally {
    await refreshStreetMaps()
  }
}

/** The installed packs a newer build could replace. Reads the host's index; downloads nothing. */
export async function checkStreetUpdates(): Promise<StreetUpdate[]> {
  return streetMapUpdates()
}

/** Update a pack: the same square from the newer build, then the old pack goes. */
export async function updateStreetMap(u: StreetUpdate): Promise<StreetPack | null> {
  const pack = await startStreetDownload(u.area)
  if (pack && pack.id !== u.packId) await removeStreetMap(u.packId).catch(() => 0)
  return pack
}

/** THE BENCH AID: the OS file picker, then the file installed as a pack or as the fonts and icons.
 *  Null when the picker was cancelled. */
export async function installStreetMapFile(): Promise<StreetInstalled | null> {
  try {
    return await streetMapInstallFile()
  } finally {
    await refreshStreetMaps()
  }
}

/** Megabytes (10^6 bytes, as the design measures) with their unit: an invariant token. */
export function mb(bytes: number): string {
  return `${(bytes / 1e6).toFixed(bytes < 1e7 ? 2 : 1)} MB`
}

/** A square's size, in km across: an invariant token. */
export function kmText(km: number): string {
  return `${km} km`
}

/** Gigabytes, for free disk space. */
export function gb(bytes: number): string {
  return `${(bytes / 1e9).toFixed(1)} GB`
}

export function __resetStreetMapsForTests(): void {
  state = INITIAL
  loading = null
  listeners.clear()
}
