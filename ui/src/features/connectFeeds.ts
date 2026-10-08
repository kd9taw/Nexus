// CONNECT'S FEEDS — ONE POLL PER WINDOW, HOWEVER MANY SURFACES SHOW THEM.
//
// Connect's boxes read eight live feeds beside the propagation snapshot (App polls that one): the
// band outlook, who hears me, the NOAA scales and alerts, the ionosonde MUF, the X-ray fast lane,
// the DXpedition windows, NOAA's daily solar indices and the Kp forecast, plus the path outlook for
// a selection. Each used to be a `useEffect` in whichever component drew it, and three of them had
// TWO pollers in one window: App polls the X-ray lane, the DXpedition windows and the Kp forecast
// for its own alerts while Connect (or its Kp box) polled the same three, so opening Connect asked
// for each twice a cycle. The dashboard rail beside the cockpits would have made a third. The
// POTA/SOTA board's two lists are feeds here too (2026-10-07): the view, a Conditions box and the
// rail's box each fetched them once a minute for themselves, and now ask once between them.
//
// A MODULE STORE, NOT A PROP (bandConditions.ts's reasoning; features/logSource's shape): a feed is
// WANTED while at least one surface shows it — Connect, the dashboard rail, the pop-out, or one of
// App's alert watchers — and exactly one timer polls it then. `wantFeed` returns the release;
// `useFeed` wants a feed while a component is mounted and re-renders it on each answer.
//
// WHAT A FEED HOLDS, and the three rules that keep it honest:
//   · the value is the last answer, and a FAILED refresh keeps it: every one of these boxes already
//     said how old its data is (the files carry their own dates; the old effects all did the same);
//   · `settled` turns true at the first answer OR the first failure, so a box can tell "still
//     asking" from "asked and got nothing" (the 30-day solar lines: nothing drawn yet, versus the
//     sentence that says there is no file);
//   · when the LAST surface lets go, the poll stops and the value is FORGOTTEN, so the next surface
//     to show it asks afresh on arrival, exactly as a mount always did. A value kept through a gap
//     would be shown as current by a box that has no way to know how long nobody was looking. An
//     answer still on its way when everyone left is dropped for the same reason.
// A slow answer does not stop the poll — one request that never comes back must not freeze a feed —
// and an answer is applied only if it is newer than the one on screen: two requests in flight can
// land in either order, and the older one must never overwrite the newer.
import { useCallback, useEffect, useSyncExternalStore } from 'react'
import {
  getBandOutlook,
  getDxpedWindows,
  getGettingOut,
  getKc2gMuf,
  getKpForecast,
  getOtaSpots,
  getPathOutlook,
  getSolarIndices,
  getSpaceWxScales,
  getXrayNow,
} from '../api'
import type {
  AlertView,
  DailySolarIndices,
  DxpedWindow,
  GettingOut,
  KpForecast,
  MufStation,
  NoaaScalesView,
  OtaSpot,
  PathPrediction,
  XrayNow,
} from '../types'
import { t } from '../i18n'
import { withErrorToast } from '../toast'

/** A polled feed. `everyMs: null` is asked once per stretch of being shown and never polled. */
export interface Feed<T> {
  /** For a failing test's message, and nothing else. */
  readonly name: string
  readonly load: () => Promise<T>
  readonly everyMs: number | null
}

/** A feed asked for one KEY at a time — the path outlook for the selected grid. Never polled. */
export interface KeyedFeed<T> {
  readonly name: string
  readonly load: (key: string) => Promise<T>
}

/** What a feed holds (see the header). */
export interface Held<T> {
  readonly value: T | undefined
  readonly settled: boolean
}

// The cadences are the ones each old effect used — see the notes beside them. The loads call the
// api at CALL time, so a test's spy on an api function is the one asked.
/** Modelled per-band workability to a long-haul ring; a plain minute (ConnectView kept it warm
 *  unconditionally because three boxes read it whatever is selected). */
export const BAND_OUTLOOK: Feed<PathPrediction> = { name: 'bandOutlook', load: () => getBandOutlook(), everyMs: 60_000 }
/** Who hears me now: the backend reads the live PSK Reporter / RBN firehose on every call. */
export const GETTING_OUT: Feed<GettingOut> = { name: 'gettingOut', load: () => getGettingOut(), everyMs: 30_000 }
/** NOAA's R/S/G scales and alerts: a 15-minute server cache, asked every 5 minutes (cheap). */
export const SPACE_WX_SCALES: Feed<{ scales: NoaaScalesView; alerts: AlertView[] }> = {
  name: 'spaceWxScales',
  load: () => getSpaceWxScales(),
  everyMs: 300_000,
}
/** The ionosonde MUF map: the kc2g cache's 5 minutes. */
export const KC2G_MUF: Feed<MufStation[]> = { name: 'kc2gMuf', load: () => getKc2gMuf(), everyMs: 300_000 }
/** The X-ray fast lane (60 s): App's flare watcher and the map's flare layer read the same one. */
export const XRAY_NOW: Feed<XrayNow> = { name: 'xrayNow', load: () => getXrayNow(), everyMs: 60_000 }
/** DXpedition best-shot windows (server-cached climatology; 10 minutes is generous): App's chase
 *  alerts and Connect's selection and chase feed. */
export const DXPED_WINDOWS: Feed<DxpedWindow[]> = { name: 'dxpedWindows', load: () => getDxpedWindows(), everyMs: 600_000 }
/** NOAA's daily solar indices: the server's one-hour cache; the file changes about once a day. */
export const SOLAR_INDICES: Feed<DailySolarIndices> = { name: 'solarIndices', load: () => getSolarIndices(), everyMs: 3_600_000 }
/** The 3-day Kp forecast: the server's 15-minute cache (SWPC republishes every 30). App's storm
 *  heads-up and the Kp outlook box. */
export const KP_FORECAST: Feed<KpForecast> = { name: 'kpForecast', load: () => getKpForecast(), everyMs: 900_000 }
/** The outlook along the path to the selected station's grid, asked once per selection. */
export const PATH_OUTLOOK: KeyedFeed<PathPrediction> = { name: 'pathOutlook', load: (grid) => getPathOutlook(grid) }

/** A programme's activators on the air now, with when the answer came (the board's "Updated" line). */
export interface OtaAnswer {
  readonly spots: OtaSpot[]
  readonly at: number
}
/** pota.app's or SOTAwatch's list, a plain minute (the board's own timer before it was a feed); one feed
 *  per programme, so a board on Both and a board on POTA share the POTA poll. A failed fetch says so in a
 *  toast, once a window, and answers an EMPTY list: what the board has always shown after one, so this
 *  feed answers where the others keep their last value. */
function otaSpots(program: 'POTA' | 'SOTA'): Feed<OtaAnswer> {
  return {
    name: `otaSpots:${program}`,
    load: async () => ({
      spots: (await withErrorToast(() => getOtaSpots(program), t('ota.spots.failed', { program }))) ?? [],
      at: Date.now(),
    }),
    everyMs: 60_000,
  }
}
export const POTA_SPOTS = otaSpots('POTA')
export const SOTA_SPOTS = otaSpots('SOTA')

interface Entry<T> {
  held: Held<T>
  wants: number
  timer: ReturnType<typeof setInterval> | null
  /** Every request's number, in the order asked… */
  asked: number
  /** …and the newest one whose answer is on screen. An answer at or below it is late. */
  shown: number
  /** Moves when the last surface lets go, so an answer asked for before that is dropped. */
  gen: number
  listeners: Set<() => void>
  /** Requests on their way now, and who hears it go between none and some (`useFeedAsking`). Apart from
   *  `listeners`, because a watcher hears every notice there as a new answer. */
  asking: number
  askingListeners: Set<() => void>
}

const NOTHING: Held<never> = Object.freeze({ value: undefined, settled: false })

let entries = new Map<Feed<unknown>, Entry<unknown>>()
let keyed = new Map<KeyedFeed<unknown>, Map<string, Feed<unknown>>>()

function entryOf<T>(feed: Feed<T>): Entry<T> {
  let e = entries.get(feed as Feed<unknown>)
  if (!e) {
    e = {
      held: NOTHING, wants: 0, timer: null, asked: 0, shown: 0, gen: 0, listeners: new Set(),
      asking: 0, askingListeners: new Set(),
    }
    entries.set(feed as Feed<unknown>, e)
  }
  return e as Entry<T>
}

function publish<T>(e: Entry<T>, held: Held<T>): void {
  e.held = held
  for (const l of [...e.listeners]) l()
}

/** Set how many requests are on their way, telling the asking listeners only when that goes between
 *  none and some. */
function setAsking<T>(e: Entry<T>, n: number): void {
  const was = e.asking > 0
  e.asking = n
  if (was !== n > 0) for (const l of [...e.askingListeners]) l()
}

/** One request. Resolves once it has settled, with an answer or a failure. */
function ask<T>(feed: Feed<T>, e: Entry<T>): Promise<void> {
  const gen = e.gen
  const n = (e.asked += 1)
  let answer: Promise<T>
  try {
    answer = feed.load()
  } catch (err) {
    // A host with no bridge throws before it returns a promise: a failure like any other.
    answer = Promise.reject(err)
  }
  setAsking(e, e.asking + 1)
  // Counted back in only within its own stretch: the last release already counted nothing on its way.
  const landed = () => {
    if (gen === e.gen) setAsking(e, e.asking - 1)
  }
  return answer.then(
    (value) => {
      try {
        if (gen === e.gen && n > e.shown) {
          e.shown = n
          publish(e, { value, settled: true })
        }
      } finally {
        landed()
      }
    },
    () => {
      // A failure changes no value, so it never marks one shown: an older answer landing after it
      // is still newer than what is on screen.
      try {
        if (gen === e.gen && !e.held.settled) publish(e, { value: e.held.value, settled: true })
      } finally {
        landed()
      }
    },
  )
}

/** Show a feed: the first surface starts its poll (asking at once), the rest share it. Returns the
 *  release; the last release stops the poll and forgets the value. A second call of one release is
 *  a no-op. */
export function wantFeed<T>(feed: Feed<T>): () => void {
  const e = entryOf(feed)
  e.wants += 1
  if (e.wants === 1) {
    ask(feed, e)
    if (feed.everyMs != null) e.timer = setInterval(() => ask(feed, e), feed.everyMs)
  }
  let released = false
  return () => {
    if (released) return
    released = true
    e.wants -= 1
    if (e.wants > 0) return
    if (e.timer != null) clearInterval(e.timer)
    e.timer = null
    e.gen += 1
    e.asked = 0
    e.shown = 0
    publish(e, NOTHING)
    setAsking(e, 0)
  }
}

/** Ask a feed NOW — a board's Refresh — and share the answer with every surface showing it, as a poll's
 *  is shared; the poll's own timer is left as it was. A feed nobody shows asks nothing. Resolves once this
 *  request has settled. */
export function refreshFeed<T>(feed: Feed<T>): Promise<void> {
  const e = entries.get(feed as Feed<unknown>) as Entry<T> | undefined
  return e && e.wants > 0 ? ask(feed, e) : Promise.resolve()
}

/** Whether a feed has a request on its way: what a board's Refresh spins and greys on — the first
 *  request, each poll and each refresh, as a board that fetched for itself showed. `null` never is. */
export function useFeedAsking(feed: Feed<unknown> | null): boolean {
  const subscribe = useCallback(
    (l: () => void) => {
      if (!feed) return () => {}
      const e = entryOf(feed)
      e.askingListeners.add(l)
      return () => {
        e.askingListeners.delete(l)
      }
    },
    [feed],
  )
  return useSyncExternalStore(subscribe, () => (feed ? (entries.get(feed)?.asking ?? 0) > 0 : false))
}

/** What a feed holds right now: the same object until it changes (useSyncExternalStore's rule). */
export function peekFeed<T>(feed: Feed<T>): Held<T> {
  return (entries.get(feed as Feed<unknown>)?.held as Held<T> | undefined) ?? NOTHING
}

function subscribeFeed<T>(feed: Feed<T>, listener: () => void): () => void {
  const e = entryOf(feed)
  e.listeners.add(listener)
  return () => {
    e.listeners.delete(listener)
  }
}

/** A feed while this component is mounted — or nothing, and no request, while `enabled` is false
 *  (the hosted Remote page, whose copies come from the station). */
export function useFeed<T>(feed: Feed<T> | null, enabled = true): Held<T> {
  const on = enabled && feed != null
  const subscribe = useCallback((l: () => void) => (feed ? subscribeFeed(feed, l) : () => {}), [feed])
  const held = useSyncExternalStore(subscribe, () => (on && feed ? peekFeed(feed) : NOTHING))
  useEffect(() => (on && feed ? wantFeed(feed) : undefined), [feed, on])
  return held
}

/** The one-key feed `key` names, made once and kept (a handful of grids a session). */
function feedFor<T>(kf: KeyedFeed<T>, key: string): Feed<T> {
  let byKey = keyed.get(kf as KeyedFeed<unknown>)
  if (!byKey) {
    byKey = new Map()
    keyed.set(kf as KeyedFeed<unknown>, byKey)
  }
  let feed = byKey.get(key)
  if (!feed) {
    feed = { name: `${kf.name}:${key}`, load: () => kf.load(key), everyMs: null }
    byKey.set(key, feed)
  }
  return feed as Feed<T>
}

/** A keyed feed for `key` while this component is mounted; `null` asks for nothing. */
export function useKeyedFeed<T>(kf: KeyedFeed<T>, key: string | null, enabled = true): Held<T> {
  return useFeed(key == null ? null : feedFor(kf, key), enabled)
}

/**
 * Hear every answer of a feed without rendering — App's alert watchers (the flare onset, the chase
 * alerts' windows, the storm heads-up). It wants the feed like any surface, so it shares the poll,
 * and `onValue` runs once per answer (never for a failure), starting with the one already held.
 * Returns the stop.
 */
export function watchFeed<T>(feed: Feed<T>, onValue: (value: T) => void): () => void {
  // A watcher's own failure is its own: it must not reject the feed's request (the old per-watcher
  // effects' `.catch` kept it there) nor cost the listeners after it their answer. Loud, not silent.
  const hear = (v: T) => {
    try {
      onValue(v)
    } catch (err) {
      console.error(`[nexus] a watcher of the ${feed.name} feed failed:`, err)
    }
  }
  const held = peekFeed(feed).value
  if (held !== undefined) hear(held)
  // Every VALUE the store publishes is a new answer: a failure publishes only while a feed holds
  // nothing, and the last release publishes nothing — so a value here is heard exactly once.
  const unsubscribe = subscribeFeed(feed, () => {
    const v = peekFeed(feed).value
    if (v !== undefined) hear(v)
  })
  const release = wantFeed(feed)
  return () => {
    release()
    unsubscribe()
  }
}

/** Tests: nothing polled, nothing held, after every test (src/test-setup.ts). */
export function __resetConnectFeedsForTests(): void {
  for (const e of entries.values()) if (e.timer != null) clearInterval(e.timer)
  entries = new Map()
  keyed = new Map()
}
;(globalThis as { __nexusTestResets?: Set<() => void> }).__nexusTestResets?.add(__resetConnectFeedsForTests)
