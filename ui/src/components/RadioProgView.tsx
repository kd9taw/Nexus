import { useNavigation } from '../remote-web/useNavigation'
import type { ProgrammingConfiguration } from '../remote-web/configuration'
import { controlFailureMessage } from '../remote-web/control-failure'
import { useStationCapability } from '../stationAccess'
import { sendLogChange, useLogChange, useRemoteOperations } from '../remote-web/operations'
import type { ProgramEdit } from '../remote-web/operation-protocol'
import { downloadProgramExport, programExportName } from '../remote-web/program-export'
import { saveDownload } from '../remote-web/chunked-file'
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog. What does NOT, and this screen is dense with it: every repeater
// callsign, output frequency, offset, CTCSS tone and DTCS code, the band chips, the mode badges
// the directory supplies (DMR / D-STAR / Fusion), the distance and octant `units.ts` formats, the
// city and state of a machine, the channel names as the radio will display them, and the
// per-radio name caps in `features/radioprog.ts` — those name RIG MODELS (FT-60, Baofeng, Yaesu,
// Anytone), which are tokens exactly as a callsign is.
//
// Four more things stay in the code as named constants, each for its own reason:
//   • the DIRECTORY NAMES (`RSGB`, `RepeaterBook`, `hearham`) — proper nouns — and the values a
//     disagreement quotes (a tone, a frequency, a mode list, a colour code), which are tokens, as
//     are the LINK NETWORKS (`AllStar`, `IRLP`, `DMR ID`) a machine's node numbers are named by;
//   • the ATTRIBUTION lines the three directories require, which are also written verbatim into
//     the exported CSV, so they cannot be locale-dependent;
//   • the example grid and frequency the origin field and the by-hand prompt offer;
//   • `My channels`, the persisted project's name inside radioprog.json — a stored value, not a
//     string on screen.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { confirmDialog } from '../confirm'
import type {
  GeoCandidate,
  ProgChannel,
  RadioProgFileNotice,
  RadioProgProject,
  RepeaterDisagreement,
  RepeaterLink,
  RepeaterRecord,
  RepeaterSearchResult,
  RepeaterSearchRow,
} from '../types'
import {
  exportChannels,
  geocodeCity,
  radioprogFileNotice,
  radioprogListProjects,
  radioprogSaveProject,
  repeaterSearch,
  repeaterTune,
  saveTextToDownloads,
} from '../api'
import {
  addMemoryDeduped,
  isSendOnlyDcs,
  memoriesStore,
  starMemory,
  updateMemory,
  useMemories,
} from '../features/memories'
import {
  autoRadiusMi,
  BAND_CHIPS,
  bandOfMhz,
  CORRIDOR_CHIPS_MI,
  DEFAULT_CORRIDOR_MI,
  deriveNames,
  FREQ_MATCH_MHZ,
  frequencyQuery,
  isProgrammable,
  mhzLabel,
  miToKm,
  modeBadge,
  NAME_CAPS,
  octant,
  onFrequency,
  RADIUS_CHIPS_MI,
  repeaterMemory,
  rigRepeaterParams,
  sanitizeName,
  savedMemoryOf,
  saveRepeater,
  splitForMap,
} from '../features/radioprog'
import { RepeaterMap, type MapMarker } from './RepeaterMap'
import { gridToLatLon, isValidGrid } from '../grid'
import { fmtDistanceKm, useUnits } from '../units'
import { Dialog } from './ui/Dialog'
import { useFocusReturn } from '../focusReturn'
import { pushToast } from '../toast'
import { t } from '../i18n'
import { T } from '../i18n/T'
import { parseChirpCsv, type Memory } from '../features/memories'

/** The single auto-saved working project in radioprog.json (named projects can
 * layer on later — the file format already holds many). */
const WORKING_PROJECT_ID = 'working'

/** The three directories' own names, and the attribution each requires. The attribution is not
 * only shown here — `exportChannels` writes it into the CSV — so it is one invariant string in
 * both places, never a translated one in the interface and a different one in the file. */
const SOURCE_REPEATERBOOK = 'RepeaterBook'
const SOURCE_HEARHAM = 'hearham'
const SOURCE_RSGB = 'RSGB'
const ATTRIB_REPEATERBOOK = 'Data courtesy of RepeaterBook.com'
const ATTRIB_HEARHAM = 'Repeater data from hearham.com'
/** The UK coordinator's credit, on the operator's licence call: on screen and in every file. */
const ATTRIB_RSGB = 'Repeater data: RSGB ETCC (ukrepeater.net)'

type Directory = RepeaterRecord['source']
/** Each directory's name, credit and home, in precedence order (the merge's: the coordinator,
 * then RepeaterBook, then hearham). */
const DIRECTORIES: Directory[] = ['rsgb', 'repeaterbook', 'hearham']
const DIRECTORY: Record<Directory, { name: string; credit: string; href: string }> = {
  rsgb: { name: SOURCE_RSGB, credit: ATTRIB_RSGB, href: 'https://ukrepeater.net' },
  repeaterbook: {
    name: SOURCE_REPEATERBOOK,
    credit: ATTRIB_REPEATERBOOK,
    href: 'https://www.repeaterbook.com',
  },
  hearham: { name: SOURCE_HEARHAM, credit: ATTRIB_HEARHAM, href: 'https://hearham.com' },
}
/** The networks a machine's node numbers are named by, as hams write them (tokens). A node whose
 * directory names no network (`node`) is worded instead: `program.row.link.node`. */
const LINK_NETWORK: Record<Exclude<RepeaterLink['network'], 'node'>, string> = {
  allStar: 'AllStar',
  irlp: 'IRLP',
  dmrId: 'DMR ID',
}

/** The frequency search's tolerance as the count line prints it, kHz ("2.5"). */
const FREQ_TOL_KHZ = (FREQ_MATCH_MHZ * 1000).toFixed(1)

const isDirectory = (s: string | undefined): s is Directory =>
  s === 'rsgb' || s === 'repeaterbook' || s === 'hearham'

/** Every channel id a merged machine answers to — one per directory row behind it, the row it
 * programs from first — so a channel saved from any of them shows as already added. */
function rowIds(row: RepeaterSearchRow): string[] {
  return row.sources.length > 0 ? row.sources.map((s) => s.channelId) : [row.channel.id]
}
/** The geocoder's required credit, for the same reason. */
const ATTRIB_OSM = 'Geocoding © OpenStreetMap contributors'

/** Example values drawn from a technical namespace — a locator and a dial frequency. */
const EXAMPLE_GRID = 'FN31'
const EXAMPLE_FREQ_MHZ = '146.940'

/** The mode every programmable channel here is, and the filter chip that names it. */
const MODE_FM = 'FM'

/** localStorage for the last 5 successful query origins (chips). */
const RECENTS_KEY = 'nexus.program.recents.v1'
/** localStorage flag: the operator dismissed the CHIRP how-to dialog. */
const CHIRP_HOWTO_SEEN = 'nexus.program.chirpHowtoSeen.v1'

interface Recent {
  kind: 'grid' | 'city'
  label: string
  lat: number
  lon: number
}

function loadRecents(): Recent[] {
  try {
    const raw = JSON.parse(localStorage.getItem(RECENTS_KEY) ?? '[]')
    if (!Array.isArray(raw)) return []
    return raw
      .filter(
        (r): r is Recent =>
          r &&
          (r.kind === 'grid' || r.kind === 'city') &&
          typeof r.label === 'string' &&
          Number.isFinite(r.lat) &&
          Number.isFinite(r.lon),
      )
      .slice(0, 5)
  } catch {
    return []
  }
}

function pushRecent(r: Recent) {
  const list = [r, ...loadRecents().filter((x) => x.label !== r.label)].slice(0, 5)
  localStorage.setItem(RECENTS_KEY, JSON.stringify(list))
}

/** One builder row: the channel + whether the operator hand-edited its name
 * (edited names are never regenerated by the cap select). */
interface WorkRow {
  channel: ProgChannel
  nameEdited: boolean
}

interface Props {
  /** Station grid from Settings — the zero-input default origin. */
  myGrid: string
  /** CAT link alive — gates the per-row TUNE buttons. */
  catOk?: boolean
}

/**
 * Program — the radio-programming workbench: location → repeater directory →
 * pick → a curated channel list → CHIRP CSV / generic CSV (and, with CAT, tune
 * the rig to a machine right now). The CHANNEL LIST is the artifact; the
 * repeater results are a source feed that populates only on an explicit Fetch
 * (no auto-fetch, no polling — this is a programming tool, not a repeater
 * directory). Its one map, on request, shows hearham's listings alone
 * (RepeaterMap.tsx).
 */
export function RadioProgView({ myGrid, catOk = false }: Props) {
  const configuration=useNavigation<ProgrammingConfiguration>('programming')
  const remote=configuration.remote
  // A browser may Tune (never edit) while the station advertises the repeater transaction.
  const repeaterControl=useStationCapability('repeaterTuning')
  // Exporting from a browser: the station renders the file with the desktop's own CHIRP/CSV
  // writers and this page saves it. Locally it is always available; remotely only while the
  // station advertises the verb, so an older Nexus leaves the buttons dead rather than refused.
  const exportClient=useRemoteOperations(), exportOffered=useLogChange('programExport')
  const exportControl=!remote||(!!exportClient&&exportOffered)
  // Curating that list — rename, reorder, drop a row, clear it. Same shape as the export: always
  // available locally, and remotely only while the station advertises the verb.
  const editOffered=useLogChange('programEdit')
  const editControl=!remote||(!!exportClient&&editOffered)
  if(remote)myGrid=configuration.value?.mygrid??''
  // ── query state ──
  const [originKind, setOriginKind] = useState<'station' | 'grid' | 'city'>('station')
  const [gridInput, setGridInput] = useState('')
  const [cityInput, setCityInput] = useState('')
  const [cityPick, setCityPick] = useState<GeoCandidate | null>(null)
  const [cityCands, setCityCands] = useState<GeoCandidate[]>([])
  const [geoBusy, setGeoBusy] = useState(false)
  const units = useUnits()
  const [radiusMi, setRadiusMi] = useState<number | 'auto'>('auto')
  const [bands, setBands] = useState<string[]>(['2m', '70cm'])
  const [showDigital, setShowDigital] = useState(false)
  const [onAirOnly, setOnAirOnly] = useState(true)
  const [search, setSearch] = useState('')
  const [recents, setRecents] = useState<Recent[]>(loadRecents)
  // Or a route (the operator's pick, 2026-09-30: a route list for a trip): from the origin above to
  // a second place given the same three ways, the machines within a corridor either side of the
  // straight line between them, in the order the route passes them.
  const [area, setArea] = useState<'around' | 'route'>('around')
  const routing = area === 'route'
  const [toKind, setToKind] = useState<'station' | 'grid' | 'city'>('city')
  const [toGrid, setToGrid] = useState('')
  const [toCity, setToCity] = useState('')
  const [toPick, setToPick] = useState<GeoCandidate | null>(null)
  const [toCands, setToCands] = useState<GeoCandidate[]>([])
  const [toBusy, setToBusy] = useState(false)
  const [corridorMi, setCorridorMi] = useState<number>(DEFAULT_CORRIDOR_MI)

  // ── results ──
  const [result, setResult] = useState<RepeaterSearchResult | null>(null)
  /** Where `result` was searched: the map draws the place and the area the list came from, not
   *  what the origin fields say now. */
  const [searched, setSearched] = useState<{
    from: { lat: number; lon: number }
    fromLabel: string
    to: { lat: number; lon: number } | null
    toLabel: string
    km: number
  } | null>(null)
  /** A dot and its row are one machine: the one the pointer is on (in the list or on the map), and the
   *  one selected by a click on either. The map rings both; the list lights both. */
  const [linkedId, setLinkedId] = useState<string | null>(null)
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const rowEls = useRef(new Map<string, HTMLDivElement>())
  /** A dot clicked on the map brings its row into view in the list under it (and only then: a row the
   *  operator clicked is already where they are looking). */
  const scrollToRow = useRef<string | null>(null)
  useEffect(() => {
    const id = scrollToRow.current
    if (!id || id !== selectedId) return
    scrollToRow.current = null
    rowEls.current.get(id)?.scrollIntoView({ block: 'nearest' })
  }, [selectedId])
  const [fetching, setFetching] = useState(false)
  const [fetchErr, setFetchErr] = useState('')

  // ── builder ──
  const [rows, setRows] = useState<WorkRow[]>([])
  const [nameCap, setNameCap] = useState(7)
  const [startAt, setStartAt] = useState(1)
  const [chirpDialog, setChirpDialog] = useState(false)
  // Closed, the keyboard goes back to Export for CHIRP (focusReturn.ts).
  const chirpReturn = useFocusReturn(chirpDialog)
  const loaded = useRef(false)

  useEffect(()=>{
    if(!remote)return
    const project=configuration.value?.projects.find(p=>p.id===WORKING_PROJECT_ID)
    setRows(project?.channels.map(channel=>({channel,nameEdited:true}))??[])
  },[remote,configuration.value?.revision])

  // The saved-projects file this run could not read, when that happened: said out loud with where
  // the file is kept, because the channel list would otherwise just read as empty.
  const [fileNotice, setFileNotice] = useState<RadioProgFileNotice | null>(null)
  const readFileNotice = useCallback(() => {
    radioprogFileNotice()
      .then(setFileNotice)
      .catch(() => {})
  }, [])

  // Load the persisted working list once; save (debounced) on every change after.
  useEffect(() => {
    if(remote)return
    radioprogListProjects()
      .then((projects) => {
        const w = projects.find((p) => p.id === WORKING_PROJECT_ID)
        if (w) setRows(w.channels.map((channel) => ({ channel, nameEdited: true })))
        loaded.current = true
      })
      .catch(() => {
        loaded.current = true
      })
      .finally(readFileNotice)
  }, [])
  const saveTimer = useRef<number | null>(null)
  useEffect(() => {
    if (remote || !loaded.current) return
    if (saveTimer.current) window.clearTimeout(saveTimer.current)
    saveTimer.current = window.setTimeout(() => {
      const project: RadioProgProject = {
        id: WORKING_PROJECT_ID,
        name: 'My channels',
        createdUtc: 0,
        updatedUtc: 0,
        origin: { kind: originKind, grid: myGrid, label: originLabel(), lat: 0, lon: 0 },
        radiusKm: 0,
        channels: rows.map((r) => r.channel),
      }
      // A refused save is the file Program could not read and could not move aside: the notice
      // says so, instead of the list silently not saving.
      void radioprogSaveProject(project).catch(readFileNotice)
    }, 800)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rows])

  const originLabel = useCallback((): string => {
    if (originKind === 'station') return myGrid.toUpperCase() || '—'
    if (originKind === 'grid') return gridInput.toUpperCase()
    return cityPick ? cityPick.displayName.split(',').slice(0, 2).join(',') : cityInput
  }, [originKind, myGrid, gridInput, cityPick, cityInput])

  /** Resolve the active origin to lat/lon (null = not resolvable yet). */
  const origin = useMemo((): { lat: number; lon: number } | null => {
    if (originKind === 'city') return cityPick ? { lat: cityPick.lat, lon: cityPick.lon } : null
    const g = (originKind === 'station' ? myGrid : gridInput).trim()
    if (!isValidGrid(g)) return null
    return gridToLatLon(g)
  }, [originKind, myGrid, gridInput, cityPick])

  const destLabel = useCallback((): string => {
    if (toKind === 'station') return myGrid.toUpperCase()
    if (toKind === 'grid') return toGrid.toUpperCase()
    return toPick ? toPick.displayName.split(',').slice(0, 2).join(',') : toCity
  }, [toKind, myGrid, toGrid, toPick, toCity])

  /** The route's other end, resolved the same way. */
  const dest = useMemo((): { lat: number; lon: number } | null => {
    if (toKind === 'city') return toPick ? { lat: toPick.lat, lon: toPick.lon } : null
    const g = (toKind === 'station' ? myGrid : toGrid).trim()
    if (!isValidGrid(g)) return null
    return gridToLatLon(g)
  }, [toKind, myGrid, toGrid, toPick])

  const effRadiusMi = radiusMi === 'auto' ? autoRadiusMi(bands) : radiusMi

  const doFetch = useCallback(() => {
    if (!origin || fetching || (routing && !dest)) return
    setFetching(true)
    setFetchErr('')
    const query = routing && dest
      ? repeaterSearch(origin.lat, origin.lon, miToKm(corridorMi), dest)
      : repeaterSearch(origin.lat, origin.lon, miToKm(effRadiusMi))
    const area = {
      from: origin,
      fromLabel: originLabel(),
      to: routing && dest ? dest : null,
      toLabel: routing && dest ? destLabel() : '',
      km: miToKm(routing ? corridorMi : effRadiusMi),
    }
    query
      .then((res) => {
        setResult(res)
        setSearched(area)
        setSelectedId(null)
        setLinkedId(null)
        const label = originLabel()
        if (label && label !== '—') {
          pushRecent({
            kind: originKind === 'city' ? 'city' : 'grid',
            label,
            lat: origin.lat,
            lon: origin.lon,
          })
          setRecents(loadRecents())
        }
      })
      .catch((e) => setFetchErr(String(e)))
      .finally(() => setFetching(false))
  }, [origin, fetching, routing, dest, corridorMi, effRadiusMi, originKind, originLabel, destLabel])

  const searchCity = useCallback(() => {
    const q = cityInput.trim()
    if (!q || geoBusy) return
    setGeoBusy(true)
    setCityCands([])
    geocodeCity(q)
      .then((cands) => {
        setCityCands(cands)
        if (cands.length === 0) pushToast(t('program.city.noMatch'), 'info', 3000)
        if (cands.length === 1) setCityPick(cands[0])
      })
      .catch((e) => pushToast(String(e), 'error'))
      .finally(() => setGeoBusy(false))
  }, [cityInput, geoBusy])

  const searchToCity = useCallback(() => {
    const q = toCity.trim()
    if (!q || toBusy) return
    setToBusy(true)
    setToCands([])
    geocodeCity(q)
      .then((cands) => {
        setToCands(cands)
        if (cands.length === 0) pushToast(t('program.city.noMatch'), 'info', 3000)
        if (cands.length === 1) setToPick(cands[0])
      })
      .catch((e) => pushToast(String(e), 'error'))
      .finally(() => setToBusy(false))
  }, [toCity, toBusy])

  const useRecent = (r: Recent) => {
    // Recents carry resolved coordinates — re-fetch immediately from them.
    setOriginKind(r.kind === 'city' ? 'city' : 'grid')
    if (r.kind === 'grid') setGridInput(r.label)
    else {
      setCityInput(r.label)
      setCityPick({ displayName: r.label, lat: r.lat, lon: r.lon })
    }
  }

  // ── filtered picker rows ──
  // A frequency in the search box finds every machine ON it, whatever the band, digital and
  // on-air filters say: "what is on 147.18?" has one answer, and the default filters (FM only, on
  // the air) would hide a DMR or an off-air machine there and read as "nothing on it".
  const searchMhz = useMemo(() => frequencyQuery(search), [search])
  const shown = useMemo(() => {
    if (!result) return []
    if (searchMhz !== null) {
      return result.rows.filter((row) => onFrequency(row.record.outputMhz, searchMhz))
    }
    const q = search.trim().toUpperCase()
    return result.rows.filter((row) => {
      const r = row.record
      const band = bandOfMhz(r.outputMhz)
      if (bands.length > 0 && !bands.includes(band)) return false
      if (!showDigital && !r.fm) return false
      if (onAirOnly && !r.operational) return false
      if (q && !r.callsign.toUpperCase().includes(q) && !r.city.toUpperCase().includes(q))
        return false
      return true
    })
  }, [result, bands, showDigital, onAirOnly, search, searchMhz])

  const inList = useMemo(() => new Set(rows.map((r) => r.channel.id)), [rows])
  const isAdded = (row: RepeaterSearchRow) => rowIds(row).some((id) => inList.has(id))

  // The memory each shown machine already is, live from the shared bank, so the badge and the
  // star stay right when the operator edits, unstars or deletes one in the Memories section. "The
  // same machine" is the merge's own rule (`sameMachine`), the one identity Save to Memories, Save
  // all shown and the ★ share: a machine is saved once, whichever of them saved it.
  const bank = useMemories()
  const savedById = useMemo(() => {
    const out = new Map<string, Memory>()
    for (const row of shown) {
      const hit = savedMemoryOf(bank, row)
      if (hit) out.set(row.channel.id, hit)
    }
    return out
  }, [shown, bank])
  const starredIds = useMemo(
    () => new Set([...savedById].filter(([, m]) => m.favorite).map(([id]) => id)),
    [savedById],
  )

  // The map (the operator's pick, 2026-09-30: "hearham-only map now"): the shown rows hearham
  // lists, each where its hearham row places it, and a count of the rest by why they are off it.
  const mapSplit = useMemo(() => splitForMap(shown), [shown])
  const markers = useMemo(
    (): MapMarker[] =>
      mapSplit.mapped.map((row) => {
        const added = rowIds(row).some((id) => inList.has(id))
        return {
          id: row.channel.id,
          lat: row.map!.lat,
          lon: row.map!.lon,
          call: row.map!.callsign,
          mhz: row.map!.outputMhz,
          city: row.map!.city,
          added,
          pickable: added || isProgrammable(row.record),
          savable: isProgrammable(row.record),
          saved: savedById.has(row.channel.id),
          savedAs: savedById.get(row.channel.id)?.name,
        }
      }),
    [mapSplit, inList, savedById],
  )


  // ── builder ops ──
  /** A Memory (a CHIRP CSV row) as a Program channel — the reverse of `saveToBank`. Manual
   * and imported rows carry a `manual:` id so re-imports dedup. */
  const memoryToChannel = (m: Memory): ProgChannel => ({
    id: `manual:${m.rxMhz.toFixed(4)}:${m.name}`,
    name: m.name,
    rxMhz: m.rxMhz,
    duplex:
      m.offsetDir === 'split' ? 'split' : m.offsetDir === 'plus' ? 'plus' : m.offsetDir === 'minus' ? 'minus' : 'simplex',
    offsetMhz: m.offsetDir === 'split' ? (m.txMhz ?? m.rxMhz) - m.rxMhz : (m.offsetMhz ?? 0),
    toneMode:
      m.toneMode === 'tone' || m.toneMode === 'tsql' || m.toneMode === 'dtcs'
        ? m.toneMode
        : isSendOnlyDcs(m)
          ? 'dtcs'
          : 'none',
    dtcsTxOnly: isSendOnlyDcs(m) || undefined,
    rtoneHz: m.ctcssEncHz ?? 0,
    ctoneHz: m.ctcssDecHz ?? m.ctcssEncHz ?? 0,
    dtcsCode: m.dtcsCode ?? 0,
    mode: m.mode.trim().toLowerCase() === 'nfm' ? 'nfm' : m.mode.trim().toLowerCase() === 'am' ? 'am' : 'fm',
    comment: m.notes ?? '',
    source: null,
  })

  /** Manual channel entry (operator ask, 2026-08-16): the directory is a FEED, and a repeater
   * it has wrong or missing must be enterable by hand — HearHam's coverage is thin in places
   * and the shared RepeaterBook path is still pending approval. */
  const addManual = () => {
    const freq = window.prompt(t('program.manual.freq', { example: EXAMPLE_FREQ_MHZ }))
    if (!freq) return
    const rxMhz = Number(freq)
    if (!Number.isFinite(rxMhz) || rxMhz <= 0) return
    const name = window.prompt(t('program.manual.name')) ?? ''
    const off = window.prompt(t('program.manual.offset')) ?? ''
    const tone = window.prompt(t('program.manual.tone')) ?? ''
    const toneHz = Number(tone)
    const ch: ProgChannel = {
      id: `manual:${rxMhz.toFixed(4)}:${name || 'manual'}`,
      name: name || `${rxMhz.toFixed(3)} FM`,
      rxMhz,
      duplex: off.trim() === '+' ? 'plus' : off.trim() === '-' ? 'minus' : 'simplex',
      offsetMhz: 0,
      toneMode: Number.isFinite(toneHz) && toneHz > 0 ? 'tone' : 'none',
      rtoneHz: Number.isFinite(toneHz) && toneHz > 0 ? toneHz : 0,
      ctoneHz: 0,
      dtcsCode: 0,
      mode: 'fm',
      comment: '',
      source: null,
    }
    setRows((rs) =>
      rs.some((r) => r.channel.id === ch.id) ? rs : [...rs, { channel: ch, nameEdited: name !== '' }],
    )
  }

  const importInputRef = useRef<HTMLInputElement>(null)
  const importCsv = async (file: File) => {
    const text = await file.text()
    const mems = parseChirpCsv(text)
    if (mems.length === 0) {
      pushToast(t('program.import.notChirp'), 'error')
      return
    }
    setRows((rs) => {
      const have = new Set(rs.map((r) => r.channel.id))
      const add = mems.map(memoryToChannel).filter((c) => !have.has(c.id))
      return [...rs, ...add.map((channel) => ({ channel, nameEdited: true }))]
    })
    pushToast(t('program.import.done', { count: mems.length }), 'success')
  }

  const addRow = (row: RepeaterSearchRow) => {
    if (isAdded(row)) {
      const ids = new Set(rowIds(row))
      setRows((rs) => rs.filter((r) => !ids.has(r.channel.id)))
      return
    }
    setRows((rs) => [...rs, { channel: { ...row.channel }, nameEdited: false }])
  }
  const addAllShown = async () => {
    const candidates = shown.filter((row) => isProgrammable(row.record) && !isAdded(row))
    if (candidates.length === 0) return
    if (
      candidates.length > 50 &&
      !(await confirmDialog({
        title: t('program.addAll.confirm.title', { count: candidates.length }),
        confirmLabel: t('program.addAll.confirm.ok'),
      }))
    )
      return
    setRows((rs) => [
      ...rs,
      ...candidates.slice(0, 200).map((row) => ({ channel: { ...row.channel }, nameEdited: false })),
    ])
  }
  /** One curation gesture at the station, against the document revision this page is SHOWING.
   *  Whatever the station answers, re-read the list rather than guessing what it now holds: a
   *  stale revision, a row somebody else removed and a half-written file all end the same way. */
  const sendEdit = async (edit: ProgramEdit) => {
    const revision = configuration.value?.revision
    if (!exportClient || !revision) return
    await sendLogChange(exportClient, { kind: 'programEdit', revision, edit })
    configuration.refresh()
  }
  const move = (i: number, d: -1 | 1) => {
    if (remote) { void sendEdit({ action: 'move', id: rows[i]!.channel.id, by: d }); return }
    setRows((rs) => {
      const j = i + d
      if (j < 0 || j >= rs.length) return rs
      const next = rs.slice()
      ;[next[i], next[j]] = [next[j], next[i]]
      return next
    })
  }
  const remove = (i: number) => {
    if (remote) { void sendEdit({ action: 'remove', id: rows[i]!.channel.id }); return }
    setRows((rs) => rs.filter((_, k) => k !== i))
  }
  const rename = (i: number, name: string) =>
    setRows((rs) =>
      rs.map((r, k) => (k === i ? { ...r, channel: { ...r.channel, name }, nameEdited: true } : r)),
    )
  /** The name being typed in a browser, held OUTSIDE `rows`: the station's document is re-read on a
   *  timer, and a refresh landing mid-word would otherwise take the half-typed name away. It is
   *  sent on blur or Enter — one gesture, one transaction — the way the Memories fields commit.
   *  Escape drops it, and nothing is sent.
   *
   *  ⚠️ Mirrored in a ref, and the ref is what commit reads. Escape clears the draft and then
   *  leaves the field, and a `useState` value is still the OLD one inside the blur handler's
   *  closure — which committed the name the operator had just abandoned. */
  const [draftName, setDraftName] = useState<{ id: string; name: string } | null>(null)
  const draftRef = useRef<{ id: string; name: string } | null>(null)
  const setDraft = (draft: { id: string; name: string } | null) => { draftRef.current = draft; setDraftName(draft) }
  const commitName = (id: string, was: string) => {
    const draft = draftRef.current
    setDraft(null)
    if (draft?.id === id && draft.name !== was) void sendEdit({ action: 'rename', id, name: draft.name })
  }

  // Auto-derived names for rows the operator hasn't touched (cap-aware).
  const displayRows = useMemo(() => {
    const auto = deriveNames(
      rows.map((r) => ({
        callsign: r.channel.source?.callsign ?? r.channel.name,
        city: r.channel.comment,
        outputMhz: r.channel.rxMhz,
      })),
      nameCap,
    )
    return rows.map((r, i) => ({
      ...r,
      displayName: r.nameEdited ? r.channel.name : auto[i],
    }))
  }, [rows, nameCap])

  const dupNames = useMemo(() => {
    const seen = new Map<string, number>()
    for (const r of displayRows) {
      const n = sanitizeName(r.displayName, nameCap)
      seen.set(n, (seen.get(n) ?? 0) + 1)
    }
    return new Set([...seen.entries()].filter(([, c]) => c > 1).map(([n]) => n))
  }, [displayRows, nameCap])

  // Every directory the file's rows could carry data from, one credit line each: the lists this
  // search read (a merged row takes fields from several) and the source of every channel in the
  // list, which may have been added from an earlier search. hearham's alone when nothing says.
  const attribution = useMemo(() => {
    const used = new Set<Directory>(result ? result.lists.map((l) => l.source) : [])
    for (const r of displayRows) {
      const s = r.channel.source?.source
      if (isDirectory(s)) used.add(s)
    }
    if (used.size === 0) used.add('hearham')
    return DIRECTORIES.filter((d) => used.has(d))
      .map((d) => DIRECTORY[d].credit)
      .join('\n')
  }, [result, displayRows])

  const exportList = (format: 'chirp' | 'csv') => {
    if(remote&&!exportClient)return
    const channels = displayRows.map((r) => ({ ...r.channel, name: r.displayName }))
    const analog = channels.filter((c) => c.mode === 'fm' || c.mode === 'nfm' || c.mode === 'am')
    // Checked here for BOTH paths: the browser is holding the same rows the station would render,
    // so an empty CHIRP export is refused without a round trip, in the same words.
    if (format === 'chirp' && analog.length === 0) {
      pushToast(t('program.export.noFm'), 'info')
      return
    }
    const name = programExportName(format)
    if (remote) {
      // The file is built at the station — a second CHIRP writer in this browser would drift from
      // the one the desktop uses — and lands in THIS machine's downloads.
      void downloadProgramExport(exportClient!, format, nameCap)
        .then((blob) => {
          saveDownload(name, blob)
          pushToast(format === 'chirp' ? t('program.export.browser.savedChirp', { name })
            : t('program.export.browser.saved', { name }), 'success', 6000)
        })
        .catch((e) => {
          const code = e instanceof Error ? e.message : ''
          pushToast(code === 'notFound' ? t('program.export.browser.empty')
            : code === 'tooLarge' ? t('program.export.browser.tooLarge')
              : code === 'invalidProgramFile' ? t('program.export.browser.failed')
                : controlFailureMessage(e), 'error', 6000)
        })
      return
    }
    void exportChannels(channels, format, nameCap, attribution)
      .then((text) =>
        saveTextToDownloads(name, text).then((path) => {
          pushToast(
            format === 'chirp'
              ? t('program.export.saved.chirp', { path })
              : t('program.export.saved', { path }),
            'success',
            6000,
          )
        }),
      )
      .catch((e) => pushToast(String(e), 'error'))
  }

  const onExportChirp = () => {
    if (localStorage.getItem(CHIRP_HOWTO_SEEN) === '1') exportList('chirp')
    else setChirpDialog(true)
  }

  /** Tune the CAT rig to a repeater NOW — one atomic backend step that routes on
   * FM (so 2 m repeater work reaches the FM radio even when the operator came
   * from a digital section) and lands the shift/offset/tone with the QSY. */
  const tuneTo = (c: ProgChannel) => {
    if(remote&&!repeaterControl)return
    const { shift, offsetHz, toneHz } = rigRepeaterParams(c)
    void repeaterTune(c.rxMhz, shift, offsetHz, toneHz)
      .then(() => {
        // The shift is a sign and a number of MHz — a token, assembled here; only the word
        // "simplex" and the tone clause are prose.
        const shiftLabel =
          shift === 'simplex'
            ? t('program.tune.simplex')
            : `${shift === 'plus' ? '+' : '−'}${(offsetHz / 1e6).toFixed(2)} MHz`
        pushToast(
          t('program.tune.done', {
            freq: c.rxMhz.toFixed(4),
            mode: MODE_FM,
            shift: shiftLabel,
            tone: toneHz ? t('program.tune.tone', { hz: toneHz.toFixed(1) }) : '',
          }),
          'success',
          4000,
        )
      })
      // A browser's refusal carries data, not operator text: name it plainly.
      .catch((e) => pushToast(remote ? controlFailureMessage(e) : String(e), 'error'))
  }

  /** ★ one machine straight into the favorites list — the whole point of the
   * feature: fetch, star, and it's on the cockpit MEM strip and in Memories
   * without a trip through the channel-list builder. Starring a machine the bank
   * already holds (the merge's rule, `savedMemoryOf`) stars THAT memory rather than
   * adding a second, and the star toggles back off. */
  const toggleStar = (row: RepeaterSearchRow) => {
    let msg = ''
    memoriesStore.update((bank) => {
      const existing = savedMemoryOf(bank, row)
      if (existing?.favorite) {
        msg = t('program.star.unstarred', { name: existing.name })
        return updateMemory(bank, existing.id, { favorite: false })
      }
      if (existing) {
        msg = t('program.star.starred', { name: existing.name })
        return starMemory(bank, existing.id)
      }
      const saved = saveRepeater(bank, row)
      if (!saved.memory) return bank
      msg = t('program.star.saved', { name: saved.memory.name })
      return starMemory(saved.bank, saved.memory.id)
    })
    if (msg) pushToast(msg, 'success', 4000)
  }

  /** Save to Memories (the operator's pick, 2026-10-02: "Save buttons + all fields"): the machine
   * into the bank as a memory, not starred, with every field `repeaterMemory` maps — frequency,
   * offset, tone or DCS, narrow, the callsign, and the town, links and colour code in its notes. A
   * machine the bank already holds is never saved twice: its row shows the badge instead. */
  const saveRow = (row: RepeaterSearchRow) => {
    let name = ''
    memoriesStore.update((bank) => {
      const saved = saveRepeater(bank, row)
      if (saved.result === 'saved') name = saved.memory!.name
      return saved.bank
    })
    if (name) pushToast(t('program.save.done', { name }), 'success', 4000)
  }

  /** Save all shown: every FM machine the list shows into Memories, each once, in list order. More
   * than 50 asks first and at most 200 go in one press, as ＋ Add all shown does. */
  const saveAllShown = async () => {
    const candidates = shown.filter((row) => isProgrammable(row.record))
    if (candidates.length === 0) {
      pushToast(t('program.saveBank.noFm'), 'info', 5000)
      return
    }
    const fresh = candidates.filter((row) => !savedMemoryOf(memoriesStore.get(), row))
    if (
      fresh.length > 50 &&
      !(await confirmDialog({
        title: t('program.saveAll.confirm.title', { count: fresh.length }),
        confirmLabel: t('program.saveAll.confirm.ok'),
      }))
    )
      return
    let n = 0
    let dup = 0
    memoriesStore.update((bank) => {
      let next = bank
      for (const row of candidates) {
        if (n >= 200) break
        const saved = saveRepeater(next, row)
        next = saved.bank
        if (saved.result === 'saved') n += 1
        else if (saved.result === 'exists') dup += 1
      }
      return next
    })
    pushToast(
      n
        ? t('program.saveBank.done', {
            count: n,
            dupes: dup ? t('program.saveBank.dupes', { count: dup }) : '',
          })
        : t('program.saveBank.allDupes'),
      n ? 'success' : 'info',
      5000,
    )
  }

  /** Save the whole list into the shared Memories store (with the FM repeater
   * fields, so recall keys the machine — not just the output). Deduped on
   * freq+mode+tone so re-saving the same list never piles duplicates; the
   * SHARED store means the Memories section + cockpit MEM strips update live
   * (the old direct-to-storage write went stale until reload). */
  const saveToBank = () => {
    let n = 0
    let dup = 0
    memoriesStore.update((bank) => {
      let next = bank
      for (const r of displayRows) {
        const c = r.channel
        if (!(c.mode === 'fm' || c.mode === 'nfm' || c.mode === 'am')) continue
        // No site coordinates here: the builder list persists channels only
        // (radioprog.json), so a reloaded row has no record to read lat/lon from.
        // ★-ing from the results list is the path that carries them.
        const res = addMemoryDeduped(
          next,
          repeaterMemory(c, sanitizeName(r.displayName, nameCap) || c.name),
        )
        next = res.bank
        if (res.added) n += 1
        else dup += 1
      }
      return next
    })
    // The already-there clause is interpolated INTO the sentence rather than glued onto it: it
    // carries the second count, which one message cannot pluralise alongside the first, and a
    // language that words the pair differently can still place it.
    pushToast(
      n
        ? t('program.saveBank.done', {
            count: n,
            dupes: dup ? t('program.saveBank.dupes', { count: dup }) : '',
          })
        : dup
          ? t('program.saveBank.allDupes')
          : t('program.saveBank.noFm'),
      n ? 'success' : 'info',
      5000,
    )
  }

  // The unit letter rides inside the message with its number, so a translation can never
  // separate the two.
  const fmtAge = (unix: number): string => {
    const mins = Math.max(0, Math.round((Date.now() / 1000 - unix) / 60))
    if (mins < 60) return t('program.age.mins', { mins })
    if (mins < 60 * 48) return t('program.age.hours', { hours: Math.round(mins / 60) })
    return t('program.age.days', { days: Math.round(mins / 1440) })
  }

  const offsetLabel = (c: ProgChannel): string => {
    if (c.duplex === 'simplex') return '—'
    if (c.duplex === 'split') return `→${c.offsetMhz.toFixed(3)}`
    return `${c.duplex === 'plus' ? '+' : '-'}${c.offsetMhz.toFixed(1)}`
  }
  /** A row's directories and its date: the top row's own date when its directory gives one,
   * otherwise "no date" and the age of the list it came in. */
  const sourceLine = (row: RepeaterSearchRow): string => {
    const names = [...new Set(row.sources.map((s) => DIRECTORY[s.source].name))].join(' + ')
    const sources = names || DIRECTORY[row.record.source].name
    const updated = row.sources[0]?.updated
    if (updated) return t('program.row.source.updated', { sources, date: updated })
    const list = result?.lists.find((l) => l.source === row.record.source)
    return t('program.row.source.noDate', { sources, age: list ? fmtAge(list.fetchedUtc) : '—' })
  }
  /** One disagreement: the value the row programs, then what the others listed instead. */
  const differText = (d: RepeaterDisagreement): string => {
    const quote = (s: RepeaterDisagreement['said'][number]) =>
      `${DIRECTORY[s.source].name} ${s.value}`
    const [first, ...rest] = d.said
    const used = first ? quote(first) : ''
    const others = rest
      .filter((s) => s.value !== first?.value)
      .map(quote)
      .join(' · ')
    switch (d.field) {
      case 'tone':
        return t('program.row.differ.tone', { used, others })
      case 'input':
        return t('program.row.differ.input', { used, others })
      case 'mode':
        return t('program.row.differ.mode', { used, others })
      case 'colorCode':
        return t('program.row.differ.colorCode', { used, others })
    }
  }

  /** A machine's links and its DMR colour code, as hams write them ("IRLP 3570", "DMR ID 314158",
   * "CC1"); a node whose network its directory does not name is worded. Empty: no line. */
  const linkParts = (r: RepeaterRecord): string[] => [
    ...(r.links ?? []).map((l) =>
      l.network === 'node'
        ? t('program.row.link.node', { node: l.node })
        : `${LINK_NETWORK[l.network]} ${l.node}`,
    ),
    ...(r.dmrColorCode != null ? [`CC${r.dmrColorCode}`] : []),
  ]

  /** Where a machine is: from the origin ("12 mi NE"), or on a route how far along it ("120 mi"),
   * with how far off it on the line under the row (`routeOff`) and its town in the tooltip. Both
   * figures in the cell wrapped it at 1366×768 (Chrome, 2026-10-02). */
  const distLabel = (row: RepeaterSearchRow): string => {
    const r = row.record
    if (row.alongKm != null) return fmtDistanceKm(row.alongKm, units)
    return `${fmtDistanceKm(r.distanceKm, units)} ${octant(r.bearingDeg)}`
  }
  const routeOff = (row: RepeaterSearchRow): string =>
    t('program.row.route.off', {
      off: fmtDistanceKm(row.record.distanceKm, units),
      dir: octant(row.record.bearingDeg),
    })
  const distTitle = (row: RepeaterSearchRow): string => {
    const r = row.record
    const town = `${r.city}${r.state ? `, ${r.state}` : ''}`
    if (row.alongKm == null) return town
    const place = t('program.row.route.title', {
      along: fmtDistanceKm(row.alongKm, units),
      off: fmtDistanceKm(r.distanceKm, units),
      dir: octant(r.bearingDeg),
    })
    return town ? `${place} · ${town}` : place
  }
  /** The next wider reach an empty list offers, miles: the next corridor on a route; else 100 mi
   * from Auto and 200 mi from any chip. None from the widest. */
  const widerMi = result?.route
    ? (CORRIDOR_CHIPS_MI.find((mi) => mi > corridorMi) ?? null)
    : radiusMi === 200
      ? null
      : radiusMi === 'auto'
        ? 100
        : 200
  /** The reach the shown list was searched with, as the empty list words it. */
  const reachMi = result?.route ? corridorMi : effRadiusMi

  const toneLabel = (c: ProgChannel): string => {
    if (c.toneMode === 'none') return '—'
    if (c.toneMode === 'dtcs') return `D${String(c.dtcsCode).padStart(3, '0')}`
    return c.rtoneHz.toFixed(1)
  }

  return (
    <section className="radioprog panel" data-remote-configuration={remote || undefined}>
      <div className="panel-header">
        <h2>{t('program.title')}</h2>
        <span className="awards-sub">{t('program.sub')}</span>
      </div>

      {remote && <p className="settings-note" role="status">{configuration.value ? t('remote.programmingObserver') : configuration.loading ? t('remote.collectionLoading') : t('remote.collectionUnavailable')}</p>}
      {/* Above the body, not inside it: .rp-body is a two-column grid, and a note as its first
          child would take the source column's cell. */}
      {fileNotice && (
        <p className="settings-note" role="status">
          {fileNotice.keptInPlace ? (
            <T k="program.projectsFile.keptInPlace" tags={{ code: <code /> }} vals={{ path: fileNotice.path }} />
          ) : (
            <T k="program.projectsFile.setAside" tags={{ code: <code /> }} vals={{ path: fileNotice.path }} />
          )}
        </p>
      )}
      <div className="rp-body" hidden={remote&&!configuration.value}>
        {/* ── SOURCE pane: the query tool ── */}
        <div className="rp-source">
          {/* THE CONDITIONS LOOK (the operator's pick, 2026-10-02: "Conditions' look"): each block is a card with
              a section header, as the dashboard's boxes are. The cards are content, not panes: the view keeps
              its one grid, its two columns and its scrollers. */}
          <section className="rp-card rp-query" aria-label={t('program.card.search')}>
          <header className="rp-card-head">
            <span className="rp-card-title">{t('program.card.search')}</span>
          </header>
          <div className="rp-card-body">
          <div className="rp-origin" role="group" aria-label={t('program.origin.aria')}>
            <span className="rp-lbl">{routing ? t('program.route.from') : t('program.origin.label')}</span>
            <button disabled={remote}
              type="button"
              className={`filter-chip${originKind === 'station' ? ' active' : ''}`}
              onClick={() => setOriginKind('station')}
              title={t('program.origin.station.title')}
            >
              {t('program.origin.station.label', {
                grid: myGrid ? `· ${myGrid.toUpperCase()}` : '',
              })}
            </button>
            <button disabled={remote}
              type="button"
              className={`filter-chip${originKind === 'grid' ? ' active' : ''}`}
              onClick={() => setOriginKind('grid')}
            >
              {t('program.origin.grid.label')}
            </button>
            <button disabled={remote}
              type="button"
              className={`filter-chip${originKind === 'city' ? ' active' : ''}`}
              onClick={() => setOriginKind('city')}
            >
              {t('program.origin.city.label')}
            </button>
            {originKind === 'grid' && (
              <input disabled={remote}
                type="text"
                className={`settings-input mono rp-grid${gridInput && !isValidGrid(gridInput.trim()) ? ' invalid' : ''}`}
                value={gridInput}
                maxLength={6}
                placeholder={EXAMPLE_GRID}
                aria-label={t('program.origin.grid.aria')}
                onChange={(e) => setGridInput(e.target.value.toUpperCase())}
              />
            )}
            {originKind === 'city' && (
              <span className="rp-city">
                <input disabled={remote}
                  type="text"
                  className="settings-input rp-city-input"
                  value={cityInput}
                  placeholder={t('program.origin.city.placeholder')}
                  aria-label={t('program.origin.city.aria')}
                  onChange={(e) => {
                    setCityInput(e.target.value)
                    setCityPick(null)
                  }}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') searchCity()
                  }}
                />
                <button
                  type="button"
                  className="filter-chip"
                  onClick={searchCity}
                  disabled={remote || (geoBusy || !cityInput.trim())}
                >
                  {geoBusy ? t('program.city.searching') : t('program.city.search')}
                </button>
              </span>
            )}
            {!routing && <span className="rp-filter-gap" />}
            {!routing && (
              <button disabled={remote}
                type="button"
                className="filter-chip rp-route-add"
                onClick={() => setArea('route')}
                title={t('program.route.add.title')}
              >
                {t('program.route.add')}
              </button>
            )}
          </div>
          {originKind === 'city' && cityCands.length > 1 && !cityPick && (
            <div className="rp-city-cands" role="listbox" aria-label={t('program.city.matches.aria')}>
              {cityCands.map((c) => (
                <button disabled={remote}
                  key={c.displayName}
                  type="button"
                  className="filter-chip"
                  onClick={() => setCityPick(c)}
                >
                  {c.displayName}
                </button>
              ))}
            </div>
          )}
          {routing && (
            <div className="rp-origin rp-dest" role="group" aria-label={t('program.route.to.aria')}>
              <span className="rp-lbl">{t('program.route.to')}</span>
              <button disabled={remote}
                type="button"
                className={`filter-chip${toKind === 'station' ? ' active' : ''}`}
                onClick={() => setToKind('station')}
                title={t('program.origin.station.title')}
              >
                {t('program.origin.station.label', {
                  grid: myGrid ? `· ${myGrid.toUpperCase()}` : '',
                })}
              </button>
              <button disabled={remote}
                type="button"
                className={`filter-chip${toKind === 'grid' ? ' active' : ''}`}
                onClick={() => setToKind('grid')}
              >
                {t('program.origin.grid.label')}
              </button>
              <button disabled={remote}
                type="button"
                className={`filter-chip${toKind === 'city' ? ' active' : ''}`}
                onClick={() => setToKind('city')}
              >
                {t('program.origin.city.label')}
              </button>
              {toKind === 'grid' && (
                <input disabled={remote}
                  type="text"
                  className={`settings-input mono rp-grid${toGrid && !isValidGrid(toGrid.trim()) ? ' invalid' : ''}`}
                  value={toGrid}
                  maxLength={6}
                  placeholder={EXAMPLE_GRID}
                  aria-label={t('program.origin.grid.aria')}
                  onChange={(e) => setToGrid(e.target.value.toUpperCase())}
                />
              )}
              {toKind === 'city' && (
                <span className="rp-city">
                  <input disabled={remote}
                    type="text"
                    className="settings-input rp-city-input"
                    value={toCity}
                    placeholder={t('program.origin.city.placeholder')}
                    aria-label={t('program.origin.city.aria')}
                    onChange={(e) => {
                      setToCity(e.target.value)
                      setToPick(null)
                    }}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') searchToCity()
                    }}
                  />
                  <button
                    type="button"
                    className="filter-chip"
                    onClick={searchToCity}
                    disabled={remote || (toBusy || !toCity.trim())}
                  >
                    {toBusy ? t('program.city.searching') : t('program.city.search')}
                  </button>
                </span>
              )}
              <button disabled={remote}
                type="button"
                className="filter-chip rp-route-remove"
                onClick={() => setArea('around')}
                title={t('program.route.remove.title')}
                aria-label={t('program.route.remove.title')}
              >
                ✕
              </button>
            </div>
          )}
          {routing && toKind === 'city' && toCands.length > 1 && !toPick && (
            <div className="rp-city-cands" role="listbox" aria-label={t('program.city.matches.aria')}>
              {toCands.map((c) => (
                <button disabled={remote}
                  key={c.displayName}
                  type="button"
                  className="filter-chip"
                  onClick={() => setToPick(c)}
                >
                  {c.displayName}
                </button>
              ))}
            </div>
          )}
          {recents.length > 0 && (
            <div className="rp-recents" role="group" aria-label={t('program.recent.aria')}>
              <span className="rp-lbl">{t('program.recent.label')}</span>
              {recents.map((r) => (
                <button disabled={remote}
                  key={r.label}
                  type="button"
                  className="filter-chip rp-recent"
                  onClick={() => useRecent(r)}
                  title={t('program.recent.chip.title')}
                >
                  {r.label}
                </button>
              ))}
            </div>
          )}

          {routing ? (
            <div className="rp-radius rp-corridor" role="group" aria-label={t('program.corridor.aria')}>
              <span className="rp-lbl">{t('program.corridor.label')}</span>
              {CORRIDOR_CHIPS_MI.map((mi) => (
                <button disabled={remote}
                  key={mi}
                  type="button"
                  className={`filter-chip${corridorMi === mi ? ' active' : ''}`}
                  onClick={() => setCorridorMi(mi)}
                >
                  {fmtDistanceKm(mi * 1.609344, units)}
                </button>
              ))}
              <span className="rp-hint">{t('program.corridor.hint')}</span>
            </div>
          ) : (
            <div className="rp-radius" role="group" aria-label={t('program.radius.aria')}>
              <span className="rp-lbl">{t('program.radius.label')}</span>
              {RADIUS_CHIPS_MI.map((mi) => (
                <button disabled={remote}
                  key={mi}
                  type="button"
                  className={`filter-chip${radiusMi === mi ? ' active' : ''}`}
                  onClick={() => setRadiusMi(mi)}
                >
                  {fmtDistanceKm(mi * 1.609344, units)}
                </button>
              ))}
              <button disabled={remote}
                type="button"
                className={`filter-chip${radiusMi === 'auto' ? ' active' : ''}`}
                onClick={() => setRadiusMi('auto')}
                title={t('program.radius.auto.title')}
              >
                {t('program.radius.auto.label')}
              </button>
              {radiusMi === 'auto' && (
                <span className="rp-hint">
                  {t('program.radius.auto.hint', {
                    radius: fmtDistanceKm(effRadiusMi * 1.609344, units),
                    bands: bands.length ? bands.join('+') : t('program.radius.auto.allBands'),
                  })}
                </span>
              )}
            </div>
          )}

          <div className="rp-fetch-row">
            <button
              type="button"
              className="filter-chip pota-refresh-btn rp-fetch"
              onClick={doFetch}
              disabled={remote || (!origin || fetching || (routing && !dest))}
              title={
                routing
                  ? origin && dest
                    ? t('program.fetch.title.route', {
                        radius: fmtDistanceKm(corridorMi * 1.609344, units),
                      })
                    : t('program.fetch.title.noRoute')
                  : origin
                    ? t('program.fetch.title', {
                        radius: fmtDistanceKm(effRadiusMi * 1.609344, units),
                      })
                    : t('program.fetch.title.noOrigin')
              }
            >
              {fetching ? t('program.fetch.busy') : t('program.fetch.label')}
            </button>
            {result && (
              <span className="rp-stamp" title={t('program.stamp.title')}>
                {result.lists.map((l, i) => (
                  <span key={l.source} className="rp-stamp-list">
                    {i > 0 && ' '}
                    {DIRECTORY[l.source].name} · {fmtAge(l.fetchedUtc)}
                    {l.stale ? t('program.stamp.stale') : ''}
                  </span>
                ))}
              </span>
            )}
          </div>
          </div>
          </section>
          {fetchErr && (
            <div className="rp-error" role="alert">
              {fetchErr}{' '}
              <button disabled={remote} type="button" className="filter-chip" onClick={doFetch}>
                {t('program.fetch.retry')}
              </button>
            </div>
          )}
          {result?.coverageGap && (
            <div className="rp-note" role="status">
              <T
                k="program.coverageGap"
                tags={{ b: <strong /> }}
                vals={{ source: SOURCE_HEARHAM, band: result.coverageGap }}
              />
            </div>
          )}
          {/* A state the search planned but never heard from. Said out loud, because a
              short list is otherwise indistinguishable from a quiet area (#241). */}
          {result && result.missingStates.length > 0 && (
            <div className="rp-note" role="status">
              <T
                k="program.missingStates"
                tags={{ b: <strong /> }}
                vals={{ states: result.missingStates.join(', ') }}
              />
            </div>
          )}
          {/* The coordinator's list is a BETA endpoint: when it cannot be read, the list is
              hearham's alone, and the panel says so rather than showing fewer machines quietly. */}
          {result?.rsgbUnavailable && (
            <div className="rp-note" role="status">
              <T
                k="program.rsgb.unavailable"
                tags={{ b: <strong /> }}
                vals={{ rsgb: SOURCE_RSGB, hearham: SOURCE_HEARHAM }}
              />
            </div>
          )}
          {/* hearham is the list under the others: when it could not be read, the machines shown
              are the others' alone, and that is said rather than shown as fewer repeaters. With
              nothing shown it is said in the list's place instead (below). */}
          {result?.hearhamUnavailable && shown.length > 0 && (
            <div className="rp-note" role="status">
              <T
                k="program.hearham.unavailable"
                tags={{ b: <strong /> }}
                vals={{ hearham: SOURCE_HEARHAM }}
              />
            </div>
          )}
          {/* A route asks RepeaterBook about the nine states it reaches first, no more than a radius
              search can, so the states past them are named rather than left to read as empty. */}
          {result?.rbBeyond && result.rbBeyond.length > 0 && (
            <div className="rp-note" role="status">
              <T
                k="program.route.rbBeyond"
                tags={{ b: <strong /> }}
                vals={{
                  rb: SOURCE_REPEATERBOOK,
                  hearham: SOURCE_HEARHAM,
                  states: result.rbBeyond.join(', '),
                }}
              />
            </div>
          )}
          {result && result.rsgbBeyond.length > 0 && (
            <div className="rp-note" role="status">
              {result.route ? (
                <T
                  k="program.rsgb.beyond.route"
                  tags={{ b: <strong /> }}
                  vals={{
                    rsgb: SOURCE_RSGB,
                    hearham: SOURCE_HEARHAM,
                    squares: result.rsgbBeyond.join(', '),
                  }}
                />
              ) : (
                <T
                  k="program.rsgb.beyond"
                  tags={{ b: <strong /> }}
                  vals={{
                    rsgb: SOURCE_RSGB,
                    hearham: SOURCE_HEARHAM,
                    squares: result.rsgbBeyond.join(', '),
                  }}
                />
              )}
            </div>
          )}

          <section className="rp-card rp-found" aria-label={t('program.card.results')}>
          <header className="rp-card-head">
            <span className="rp-card-title">{t('program.card.results')}</span>
          {result && (
            <div className="rp-count">
              {searchMhz !== null
                ? result.route
                  ? t('program.count.freq.route', {
                      shown: shown.length,
                      freq: mhzLabel(searchMhz),
                      tol: FREQ_TOL_KHZ,
                    })
                  : t('program.count.freq', {
                      shown: shown.length,
                      freq: mhzLabel(searchMhz),
                      tol: FREQ_TOL_KHZ,
                    })
                : result.route
                  ? t('program.count.route', { shown: shown.length, total: result.rows.length })
                  : t('program.count', { shown: shown.length, total: result.rows.length })}
              {shown.some((r) => isProgrammable(r.record) && !isAdded(r)) && (
                <button disabled={remote} type="button" className="filter-chip" onClick={addAllShown}>
                  {t('program.addAll.label')}
                </button>
              )}
              {shown.some((r) => isProgrammable(r.record) && !savedById.has(r.channel.id)) && (
                <button
                  disabled={remote}
                  type="button"
                  className="filter-chip rp-save-all"
                  onClick={() => void saveAllShown()}
                  title={t('program.saveAll.title')}
                >
                  {t('program.saveAll.label')}
                </button>
              )}
            </div>
          )}
          </header>
          <div className="rp-card-body rp-found-body">
          <div className="rp-filters" role="group" aria-label={t('program.filters.aria')}>
            <button disabled={remote}
              type="button"
              className={`filter-chip${bands.length === 0 ? ' active' : ''}`}
              onClick={() => setBands([])}
            >
              {t('program.filters.allBands')}
            </button>
            {BAND_CHIPS.map((b) => (
              <button disabled={remote}
                key={b}
                type="button"
                className={`filter-chip${bands.includes(b) ? ' active' : ''}`}
                onClick={() =>
                  setBands((bs) => (bs.includes(b) ? bs.filter((x) => x !== b) : [...bs, b]))
                }
              >
                {b}
              </button>
            ))}
            <span className="rp-filter-gap" />
            <button disabled={remote}
              type="button"
              className={`filter-chip${!showDigital ? ' active' : ''}`}
              onClick={() => setShowDigital(false)}
              title={t('program.filters.fm.title')}
            >
              {MODE_FM}
            </button>
            <button disabled={remote}
              type="button"
              className={`filter-chip${showDigital ? ' active' : ''}`}
              onClick={() => setShowDigital(true)}
              title={t('program.filters.digital.title')}
            >
              {t('program.filters.digital.label')}
            </button>
            <button disabled={remote}
              type="button"
              className={`filter-chip${onAirOnly ? ' active' : ''}`}
              onClick={() => setOnAirOnly((v) => !v)}
              title={t('program.filters.onAir.title')}
            >
              {t('program.filters.onAir.label')}
            </button>
            <input disabled={remote}
              type="search"
              className="settings-input rp-search"
              value={search}
              placeholder={t('program.filters.search.placeholder')}
              title={t('program.filters.search.title')}
              aria-label={t('program.filters.search.aria')}
              onChange={(e) => setSearch(e.target.value)}
            />
          </div>

          {/* Map first, the list under it (the operator's pick, 2026-10-02): after a fetch with something
              shown, the map of hearham's listings above the list of every machine. With nothing
              shown, the list's own empty words (and its Try wider) stand alone. */}
          {result && searched && shown.length > 0 && (
            <div className="rp-map">
              <RepeaterMap
                markers={markers}
                from={searched.from}
                fromLabel={searched.fromLabel}
                to={searched.to}
                toLabel={searched.toLabel}
                reachKm={searched.km}
                linkedId={linkedId ?? selectedId}
                selectedId={selectedId}
                onHover={setLinkedId}
                onSelect={(id) => {
                  scrollToRow.current = id
                  setSelectedId(id)
                }}
                onPick={(id) => {
                  const row = mapSplit.mapped.find((r) => r.channel.id === id)
                  if (row) addRow(row)
                }}
                onSave={(id) => {
                  const row = mapSplit.mapped.find((r) => r.channel.id === id)
                  if (row) saveRow(row)
                }}
              />
              {/* What the map shows and what it leaves off, in words, under it. */}
              <div className="rp-map-note" role="status">
                <span>{t('program.map.shows', { count: markers.length, hearham: SOURCE_HEARHAM })}</span>
                {(mapSplit.leftOffRb > 0 || result.lists.some((l) => l.source === 'repeaterbook')) && (
                  <span>
                    {mapSplit.leftOffRb > 0
                      ? t('program.map.rb', { count: mapSplit.leftOffRb, rb: SOURCE_REPEATERBOOK })
                      : t('program.map.rb.none', { rb: SOURCE_REPEATERBOOK, hearham: SOURCE_HEARHAM })}
                  </span>
                )}
                {(mapSplit.leftOffRsgb > 0 || result.lists.some((l) => l.source === 'rsgb')) && (
                  <span>
                    {mapSplit.leftOffRsgb > 0
                      ? t('program.map.rsgb', { count: mapSplit.leftOffRsgb, rsgb: SOURCE_RSGB })
                      : t('program.map.rsgb.none', { rsgb: SOURCE_RSGB, hearham: SOURCE_HEARHAM })}
                  </span>
                )}
                <span>{t('program.map.hint')}</span>
              </div>
            </div>
          )}
          <div className="rp-results" role="table" aria-label={t('program.results.aria')}>
            {!result && !fetching && (
              <p className="aw-empty">
                {routing ? (
                  <T k="program.results.prompt.route" tags={{ b: <strong /> }} />
                ) : (
                  <T k="program.results.prompt" tags={{ b: <strong /> }} />
                )}
              </p>
            )}
            {result && shown.length === 0 && (
              <p className="aw-empty">
                {/* Two whole sentences, not one with an "FM " fragment spliced in: where the
                    mode word sits in the sentence is the translator's to decide. Never "No
                    repeaters" while hearham's list is missing: the area is not known to be empty. */}
                {result.hearhamUnavailable ? (
                  <T
                    k="program.hearham.unavailable"
                    tags={{ b: <strong /> }}
                    vals={{ hearham: SOURCE_HEARHAM }}
                  />
                ) : searchMhz !== null
                  ? result.route
                    ? t('program.results.none.freq.route', {
                        freq: mhzLabel(searchMhz),
                        tol: FREQ_TOL_KHZ,
                        radius: fmtDistanceKm(reachMi * 1.609344, units),
                      })
                    : t('program.results.none.freq', {
                        freq: mhzLabel(searchMhz),
                        tol: FREQ_TOL_KHZ,
                        radius: fmtDistanceKm(reachMi * 1.609344, units),
                      })
                  : showDigital
                    ? result.route
                      ? t('program.results.none.route', { radius: fmtDistanceKm(reachMi * 1.609344, units) })
                      : t('program.results.none', { radius: fmtDistanceKm(reachMi * 1.609344, units) })
                    : result.route
                      ? t('program.results.none.fm.route', { radius: fmtDistanceKm(reachMi * 1.609344, units) })
                      : t('program.results.none.fm', { radius: fmtDistanceKm(reachMi * 1.609344, units) })}
                {widerMi !== null && (
                  <button disabled={remote}
                    type="button"
                    className="filter-chip"
                    onClick={() => (result.route ? setCorridorMi(widerMi) : setRadiusMi(widerMi))}
                  >
                    {t('program.results.tryWider', {
                      radius: fmtDistanceKm(widerMi * 1.609344, units),
                    })}
                  </button>
                )}
                {!showDigital && searchMhz === null && (
                  <button disabled={remote} type="button" className="filter-chip" onClick={() => setShowDigital(true)}>
                    {t('program.results.showDigital')}
                  </button>
                )}
              </p>
            )}
            {shown.map((row) => {
              const r = row.record
              const c = row.channel
              const added = isAdded(row)
              const programmable = isProgrammable(r)
              const badge = modeBadge(r)
              const links = linkParts(r)
              const saved = savedById.get(c.id)
              return (
                <div
                  key={c.id}
                  ref={(el) => {
                    if (el) rowEls.current.set(c.id, el)
                    else rowEls.current.delete(c.id)
                  }}
                  role="row"
                  className={`rp-row${!r.operational ? ' offair' : ''}${!programmable ? ' digital' : ''}${
                    c.id === selectedId ? ' selected' : c.id === linkedId ? ' linked' : ''
                  }`}
                  onPointerEnter={() => setLinkedId(c.id)}
                  onPointerLeave={() => setLinkedId((cur) => (cur === c.id ? null : cur))}
                  // A click on the row (not on one of its buttons) selects the machine: its dot is ringed
                  // and carries its card. A second click lets it go.
                  onClick={(e) => {
                    if ((e.target as HTMLElement).closest('button, a, input, select')) return
                    setSelectedId((cur) => (cur === c.id ? null : c.id))
                  }}
                >
                  <span className="rp-call mono" role="cell">
                    {r.callsign || '—'}
                  </span>
                  <span className="rp-freq mono" role="cell">
                    {mhzLabel(r.outputMhz)}
                  </span>
                  <span className="rp-off mono" role="cell">
                    {offsetLabel(c)}
                  </span>
                  <span className="rp-tone mono" role="cell">
                    {toneLabel(c)}
                  </span>
                  <span className="rp-dist mono" role="cell" title={distTitle(row)}>
                    {distLabel(row)}
                  </span>
                  <span className="rp-badges" role="cell">
                    {badge && <span className="pota-badge rp-mode-badge">{badge}</span>}
                    {!r.operational && (
                      <span className="pota-badge rp-offair-badge">{t('program.row.offAir')}</span>
                    )}
                  </span>
                  <span className="rp-actions" role="cell">
                    {programmable && (
                      <button disabled={remote}
                        type="button"
                        className={`rp-star${starredIds.has(c.id) ? ' on' : ''}`}
                        onClick={() => toggleStar(row)}
                        aria-pressed={starredIds.has(c.id)}
                        title={
                          starredIds.has(c.id)
                            ? t('program.row.unstar.title')
                            : t('program.row.star.title')
                        }
                      >
                        {starredIds.has(c.id) ? '★' : '☆'}
                      </button>
                    )}
                    {/* Saved: the badge stands where the button was, so the row keeps its shape. It
                        is no button: the memory is the operator's now, and only Memories edits or
                        deletes it. */}
                    {programmable &&
                      (saved ? (
                        <span
                          className="pota-badge rp-saved-badge"
                          title={t('program.row.saved.title', { name: saved.name })}
                        >
                          {t('program.row.saved.label')}
                        </span>
                      ) : (
                        <button disabled={remote}
                          type="button"
                          className="pota-hunt-btn rp-save"
                          onClick={() => saveRow(row)}
                          title={t('program.row.save.title')}
                        >
                          {t('program.row.save.label')}
                        </button>
                      ))}
                    {catOk && programmable && (
                      <button disabled={remote && !repeaterControl}
                        type="button"
                        className="pota-hunt-btn rp-tune"
                        onClick={() => tuneTo(c)}
                        title={t('program.row.tune.title')}
                      >
                        {t('program.row.tune.label')}
                      </button>
                    )}
                    <button
                      type="button"
                      className={`pota-hunt-btn rp-add${added ? ' added' : ''}`}
                      disabled={remote || (!programmable && !added)}
                      onClick={() => addRow(row)}
                      title={
                        programmable
                          ? added
                            ? t('program.row.remove.title')
                            : t('program.row.add.title')
                          : t('program.row.add.digital.title')
                      }
                    >
                      {added ? t('program.row.added.label') : t('program.row.add.label')}
                    </button>
                  </span>
                  {links.length > 0 && (
                    <span className="rp-links mono" role="cell" title={t('program.row.links.title')}>
                      {links.join(' · ')}
                    </span>
                  )}
                  <span className="rp-src" role="cell">
                    {row.alongKm != null ? `${routeOff(row)} · ${sourceLine(row)}` : sourceLine(row)}
                  </span>
                  {/* The flag leads its own line, under the row: in the badge column it widened
                      the row past the list at 1024 (48 px, measured). */}
                  {row.disagreements.length > 0 && (
                    <span className="rp-differ" role="cell">
                      <span className="pota-badge rp-differ-badge" title={t('program.row.differ.title')}>
                        {t('program.row.differ.label')}
                      </span>
                      {row.disagreements.map((d) => (
                        <span key={d.field}>{differText(d)}</span>
                      ))}
                    </span>
                  )}
                </div>
              )
            })}
          </div>
          </div>
          </section>

          <div className="settings-hint rp-attribution">
            {(result ? result.lists.map((l) => l.source) : (['hearham'] as Directory[])).map(
              (d, i) => (
                <span key={d}>
                  {i > 0 && <span> · </span>}
                  <a href={DIRECTORY[d].href} target="_blank" rel="noreferrer">
                    {DIRECTORY[d].credit}
                  </a>
                </span>
              ),
            )}
            {(originKind === 'city' || (routing && toKind === 'city')) && <span> · {ATTRIB_OSM}</span>}
          </div>
        </div>

        {/* ── CHANNEL LIST pane: the artifact ── */}
        <aside className="rp-builder rp-card">
          <div className="rp-builder-head rp-card-head">
            <span className="rp-builder-title rp-card-title">
              {t('program.builder.title')} <span className="np-count">{rows.length}</span>
            </span>
            <label className="rp-cap">
              {t('program.builder.nameCap.label')}
              {/* The option labels name RIG MODELS (`features/radioprog.ts`) — tokens. */}
              {/* The rig's name cap is part of the EXPORT, not of the saved list: it only
                  truncates the names CHIRP is handed, and it is browser-local state either way.
                  So it follows the export's own control rather than staying dead in a browser. */}
              <select disabled={!exportControl}
                className="settings-input"
                value={nameCap}
                onChange={(e) => setNameCap(Number(e.target.value))}
                title={t('program.builder.nameCap.title')}
              >
                {NAME_CAPS.map((c) => (
                  <option key={c.cap} value={c.cap}>
                    {c.label}
                  </option>
                ))}
              </select>
            </label>
            <label className="rp-startat" title={t('program.builder.startAt.title')}>
              {t('program.builder.startAt.label')}
              <input disabled={remote}
                type="number"
                className="settings-input"
                min={1}
                max={999}
                value={startAt}
                onChange={(e) => setStartAt(Math.max(1, Math.round(Number(e.target.value) || 1)))}
              />
            </label>
          </div>

          <div className="rp-chan-rows">
            {displayRows.length === 0 && <p className="aw-empty">{t('program.builder.empty')}</p>}
            {displayRows.map((r, i) => {
              const clean = sanitizeName(r.displayName, nameCap)
              const dup = dupNames.has(clean)
              const over = r.displayName.trim().length > nameCap
              return (
                <div key={r.channel.id} className={`rp-chan-row${dup ? ' dup' : ''}`}>
                  <span className="rp-chan-num mono">{startAt + i}</span>
                  <input disabled={!editControl}
                    type="text"
                    className={`settings-input mono rp-chan-name${dup || over ? ' invalid' : ''}`}
                    value={draftName?.id === r.channel.id ? draftName.name : r.displayName}
                    maxLength={24}
                    aria-label={t('program.chan.name.aria', { n: startAt + i })}
                    onChange={(e) => { if (remote) setDraft({ id: r.channel.id, name: e.target.value }); else rename(i, e.target.value) }}
                    onBlur={() => { if (remote) commitName(r.channel.id, r.displayName) }}
                    onKeyDown={(e) => {
                      if (!remote) return
                      // Enter commits through the blur; Escape drops the draft and sends nothing.
                      if (e.key === 'Enter') e.currentTarget.blur()
                      else if (e.key === 'Escape') { setDraft(null); e.currentTarget.blur() }
                    }}
                    title={
                      dup
                        ? t('program.chan.dup.title')
                        : over
                          ? t('program.chan.over.title', { cap: nameCap, name: clean })
                          : undefined
                    }
                  />
                  <span className="rp-chan-freq mono">{r.channel.rxMhz.toFixed(4)}</span>
                  <span className="rp-chan-off mono">{offsetLabel(r.channel)}</span>
                  <span className="rp-chan-tone mono">{toneLabel(r.channel)}</span>
                  <span className="rp-chan-btns">
                    {/* A browser cannot search RepeaterBook (the key stays at the shack), so the
                        station's own list is where it tunes an FM machine from. */}
                    {remote && catOk && (r.channel.mode === 'fm' || r.channel.mode === 'nfm') && (
                      <button
                        type="button"
                        className="rp-tune"
                        disabled={!repeaterControl}
                        onClick={() => tuneTo(r.channel)}
                        title={t('program.row.tune.title')}
                      >
                        {t('program.row.tune.label')}
                      </button>
                    )}
                    <button
                      type="button"
                      onClick={() => move(i, -1)}
                      disabled={!editControl || i === 0}
                      aria-label={t('program.chan.moveUp.aria')}
                    >
                      ▲
                    </button>
                    <button
                      type="button"
                      onClick={() => move(i, 1)}
                      disabled={!editControl || i === displayRows.length - 1}
                      aria-label={t('program.chan.moveDown.aria')}
                    >
                      ▼
                    </button>
                    <button disabled={!editControl} type="button" onClick={() => remove(i)} aria-label={t('program.chan.remove.aria')}>
                      ✕
                    </button>
                  </span>
                </div>
              )
            })}
          </div>

          <div className="rp-deliver">
            <button disabled={remote}
              type="button"
              className="settings-refresh"
              onClick={addManual}
              title={t('program.deliver.byHand.title')}
            >
              {t('program.deliver.byHand.label')}
            </button>
            <button disabled={remote}
              type="button"
              className="settings-refresh"
              onClick={() => importInputRef.current?.click()}
              title={t('program.deliver.import.title')}
            >
              {t('program.deliver.import.label')}
            </button>
            <input disabled={remote}
              ref={importInputRef}
              type="file"
              accept=".csv"
              hidden
              onChange={(e) => {
                const f = e.target.files?.[0]
                if (f) void importCsv(f)
                e.target.value = ''
              }}
            />
            <button
              type="button"
              className="settings-save rp-export-chirp"
              disabled={!exportControl || rows.length === 0}
              onClick={onExportChirp}
              title={t('program.deliver.exportChirp.title')}
            >
              {t('program.deliver.exportChirp.label')}
            </button>
            <button
              type="button"
              className="settings-refresh"
              disabled={!exportControl || rows.length === 0}
              onClick={() => exportList('csv')}
              title={t('program.deliver.exportCsv.title')}
            >
              {t('program.deliver.exportCsv.label')}
            </button>
            <button
              type="button"
              className="settings-refresh"
              disabled={remote || (rows.length === 0)}
              onClick={saveToBank}
              title={t('program.deliver.saveBank.title')}
            >
              {t('program.deliver.saveBank.label')}
            </button>
            <button
              type="button"
              className="settings-refresh rp-clear"
              disabled={!editControl || rows.length === 0}
              onClick={() => {
                void (async () => {
                  if (
                    !(await confirmDialog({
                      title: t('program.clear.confirm.title'),
                      confirmLabel: t('program.clear.confirm.ok'),
                      danger: true,
                    }))
                  )
                    return
                  if (remote) await sendEdit({ action: 'clear' })
                  else setRows([])
                })()
              }}
            >
              {t('program.deliver.clear.label')}
            </button>
          </div>
        </aside>
      </div>

      <Dialog
        open={chirpDialog}
        onOpenChange={setChirpDialog}
        title={t('program.chirp.title')}
        description={t('program.chirp.description')}
        onCloseAutoFocus={chirpReturn}
      >
        {/* CHIRP's own menu path stays inside the sentence, marked but not translated apart. */}
        <ol className="rp-chirp-steps">
          <li>{t('program.chirp.step.save')}</li>
          <li>
            <T k="program.chirp.step.import" tags={{ b: <strong /> }} />
          </li>
          <li>
            <T k="program.chirp.step.upload" tags={{ b: <strong /> }} />
          </li>
        </ol>
        <p className="settings-hint">
          <a href="https://chirpmyradio.com" target="_blank" rel="noreferrer">
            {t('program.chirp.link')}
          </a>
        </p>
        <div className="rp-chirp-actions">
          <label className="settings-hint rp-chirp-skip">
            <input disabled={remote}
              type="checkbox"
              onChange={(e) => {
                if (e.target.checked) localStorage.setItem(CHIRP_HOWTO_SEEN, '1')
                else localStorage.removeItem(CHIRP_HOWTO_SEEN)
              }}
            />
            {t('program.chirp.dontShow')}
          </label>
          <button disabled={remote}
            type="button"
            className="settings-save"
            onClick={() => {
              setChirpDialog(false)
              exportList('chirp')
            }}
          >
            {t('program.chirp.save')}
          </button>
        </div>
      </Dialog>
    </section>
  )
}
