// The DXCC entity → representative-location table, fetched once per window.
//
// Why this exists: a bearing needs two points, and most rows only ever carry one.
// `DecodeRow.grid` is populated for CQ/grid messages only, and `NeedAlert`/`SpotRow`
// carry no position at all — so a strictly grid-derived azimuth would be blank on a
// large fraction of exactly the rows a chaser is scanning. cty.dat already has a
// coordinate for every entity it can name, which is the same fallback JTAlert and
// GridTracker use to put a heading on every line.
//
// Fetched ONCE and shared: the table is ~350 static rows that cannot change while
// the app runs, and a dozen panes want it. The memo is on the promise, not on the
// result, so panes mounting in the same tick share one round trip rather than
// racing a dozen of them.
//
// A FAILED fetch is deliberately not cached. Caching the rejection would let one
// transient IPC error disable the fallback for the rest of the session, with the
// only symptom being azimuths quietly missing from half the rows.
import { useEffect, useState } from 'react'
import { getDxccEntityLocations } from '../api'
import type { LatLon } from '../grid'

let pending: Promise<ReadonlyMap<string, LatLon>> | null = null

async function fetchCentroids(): Promise<ReadonlyMap<string, LatLon>> {
  // `async` on purpose: a host with no Tauri bridge (a detached window before its
  // backend answers, a plain browser, a test that stubs the api module) makes the
  // call itself throw synchronously, and this turns that into a rejection the
  // caller's `.catch` can absorb like any other failure.
  const rows = await getDxccEntityLocations()
  const m = new Map<string, LatLon>()
  for (const [name, lat, lon] of rows) m.set(name, { lat, lon })
  return m
}

/** The shared table, fetching it on first ask. Rejects if the backend does. */
export function loadEntityCentroids(): Promise<ReadonlyMap<string, LatLon>> {
  if (!pending) {
    pending = fetchCentroids().catch((e) => {
      pending = null // let the next mount retry — see the header
      throw e
    })
  }
  return pending
}

/**
 * The entity centroids for a pane, or null until they arrive.
 *
 * Null is a working state, not an error: `azimuthTo` simply falls through to
 * grid-derived bearings, so a pane rendered before the table lands (or in a host
 * that has no backend at all) shows fewer azimuths rather than wrong ones.
 */
export function useEntityCentroids(): ReadonlyMap<string, LatLon> | null {
  const [centroids, setCentroids] = useState<ReadonlyMap<string, LatLon> | null>(null)
  useEffect(() => {
    let live = true
    loadEntityCentroids()
      .then((m) => {
        if (live) setCentroids(m)
      })
      .catch(() => {}) // no table ⇒ grid-derived azimuths only
    return () => {
      live = false
    }
  }, [])
  return centroids
}
