// Browser alerts for new needs on the station's Needed board. NOTIFY ONLY: nothing here
// tunes, works, logs or moves the radio, and clicking an alert only brings this tab forward.
// Reads ride the same bounded `needs` collection the board already polls, so enabling alerts
// adds no station command. The operator turns them on with a click; the browser asks first.
import { useCallback, useEffect, useRef, useState } from 'react'
import type { NeedAlert, Settings } from '../types'
import { visibleNeeds } from '../features/needs'
import { readEnabledModes } from '../useFeatures'
import { t } from '../i18n'
import type { RemoteCollections } from './collections'

const STORAGE_KEY = 'nexus.remote.needAlerts'
export const NEED_ALERT_POLL_MS = 30_000
/** Alerts shown one by one per read; the rest are counted in a single summary. */
const INDIVIDUAL = 3
const SEEN_LIMIT = 4096

export type NeedAlertsControl = { supported: boolean; blocked: boolean; enabled: boolean; toggle: () => void }
export const needKey = (a: NeedAlert): string => `${a.call.trim().toUpperCase()}|${a.band}|${a.mode}`

/** The first read after alerts start, or after the station link comes back, is a baseline:
 * what is already on the board is not news, so a replayed board never notifies. After that
 * a call/band/mode notifies once for the session, strongest need first. */
export function newNeeds(seen: Set<string> | null, rows: NeedAlert[]): { fresh: NeedAlert[]; seen: Set<string> } {
  const next = new Set(seen ?? [])
  const fresh: NeedAlert[] = []
  for (const row of rows) {
    const key = needKey(row)
    if (next.has(key)) continue
    next.add(key)
    if (seen) fresh.push(row)
  }
  for (const key of next) { if (next.size <= SEEN_LIMIT) break; next.delete(key) }
  return { fresh: fresh.sort((a, b) => b.priority - a.priority), seen: next }
}

function post(title: string, body: string, tag: string): void {
  try {
    const note = new Notification(title, { body, tag })
    note.onclick = () => { window.focus(); note.close() }
  } catch { /* a refused notification never takes the page down */ }
}
function announce(fresh: NeedAlert[]): void {
  for (const a of fresh.slice(0, INDIVIDUAL)) {
    post(t('remote.b3.needAlertTitle', { station: a.entity || a.call }),
      a.freqMhz ? t('remote.b3.needAlertBodyFreq', { call: a.call, freq: a.freqMhz.toFixed(3), mode: a.mode, need: a.headline })
        : t('remote.b3.needAlertBodyBand', { call: a.call, band: a.band, mode: a.mode, need: a.headline }),
      `nexus-need-${needKey(a)}`)
  }
  if (fresh.length > INDIVIDUAL) post(t('remote.b3.needAlertMoreTitle'), t('remote.b3.needAlertMore', { count: fresh.length - INDIVIDUAL }), 'nexus-need-more')
}
const stored = (): boolean => { try { return localStorage.getItem(STORAGE_KEY) === 'on' } catch { return false } }
const store = (on: boolean): void => { try { localStorage.setItem(STORAGE_KEY, on ? 'on' : 'off') } catch { /* this session only */ } }

/** `ready` is true only while the station link is current and offers the board. */
export function useNeedAlerts(source: RemoteCollections | null, ready: boolean, settings: Settings | null): NeedAlertsControl {
  const supported = typeof Notification !== 'undefined'
  const [permission, setPermission] = useState<NotificationPermission>(() => supported ? Notification.permission : 'denied')
  const [wanted, setWanted] = useState(stored)
  const enabled = supported && wanted && permission === 'granted'
  const seen = useRef<Set<string> | null>(null)
  // The same band scopes the board and roster honour (Settings ▸ Spots & Alerts).
  const scopes = useRef({ dxcc: settings?.alertDxccBands, grid: settings?.alertGridBands, rareGrid: settings?.alertRareGridBands })
  scopes.current = { dxcc: settings?.alertDxccBands, grid: settings?.alertGridBands, rareGrid: settings?.alertRareGridBands }
  useEffect(() => {
    seen.current = null
    if (!enabled || !ready || !source) return
    let live = true, timer: ReturnType<typeof setTimeout> | undefined
    const poll = async () => {
      try {
        const { rows } = await source.read('needs')
        if (!live) return
        const result = newNeeds(seen.current, visibleNeeds(rows as unknown as NeedAlert[], readEnabledModes(), scopes.current))
        seen.current = result.seen
        announce(result.fresh)
      } catch { /* the board shows the failure; the next poll tries again */ }
      if (live) timer = setTimeout(() => void poll(), NEED_ALERT_POLL_MS)
    }
    void poll()
    return () => { live = false; clearTimeout(timer) }
  }, [enabled, ready, source])
  const toggle = useCallback(() => {
    if (!supported) return
    if (enabled) { setWanted(false); store(false); return }
    void Promise.resolve(Notification.requestPermission()).then(result => {
      setPermission(result)
      if (result === 'granted') { setWanted(true); store(true) }
    }).catch(() => {})
  }, [supported, enabled])
  return { supported, blocked: supported && permission === 'denied', enabled, toggle }
}
