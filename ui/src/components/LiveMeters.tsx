// Live meter widgets — the display-liveliness fix for the needles (2026-08-01).
//
// WHY. The RX level and the CAT S-meter used to reach the UI only through the 300 ms snapshot
// poll, and the backend's level copy rode the CAT-blocking radio loop — so a needle lagged the
// audio by ~0.4–0.8 s and froze outright during a CAT stall ("even the S meter … feels
// accurate, but slow", operator report 2026-07-30). `get_meters` reads a lock-free meter bus
// (see src-tauri `get_meters` / tempo_app::engine::MeterFeed).
//
// ONE SHARED POLL. A single module-level 100 ms interval feeds every subscriber (the
// `subscribeSnapshot` idiom: one interval, many subscribers) — TopBar + a cockpit + Settings
// no longer stack 3-4 concurrent 10 Hz IPC pollers. The interval runs only while at least one
// ACTIVE subscriber is mounted and the document is visible; a widget in a kept-alive [hidden]
// host passes `active={false}` and costs nothing (the kept-alive host rule).
//
// COST CONTROL. Subscribers select ONE value (`rxLevel` or `smeterDb`), so a host re-renders
// only when the value it displays changes: the cockpits' smeterDb-only subscription never
// re-renders on RX-level churn. Level changes below 0.5 dB (under the bar's pixel resolution)
// don't notify at all. Quantized DEDUPE is not smoothing — every rendered value is a real
// reading; identical-looking readings just don't churn React at 10 Hz.
//
// HONESTY. Values start at rest ({0, null}) and RETURN to rest if `get_meters` stops
// succeeding for METER_STALE_MS — a needle may never freeze at a reading nobody is measuring.
//
// ⚠️ THIS FILE IS ON THE **MIGRATED** LIST (i18n/hardcoded-strings.test.ts). The only word in
// it is the meter's default name, which comes from the catalog; `dB` is a unit symbol and stays
// in the code, and the reading beside it is a measurement this module formats invariantly.
import { useEffect, useRef, useState } from 'react'
import { getMeters } from '../api'
import type { MeterReadout } from '../types'
import { LevelMeter, rxLevelDb } from './LevelMeter'
import { t } from '../i18n'

/** The unit symbol on the text readout — a unit, not a word. */
const DB = 'dB'

/** Meter poll cadence (ms). The RX level is produced every 20 ms and the fast CAT S-meter
 * every ~360 ms (every other fast-dial interval — see service.rs SMETER_FAST_POLL_MS), so
 * 100 ms keeps the needle within a poll of the newest reading without outrunning the
 * producers. */
const METER_POLL_MS = 100

/** How long a failing `get_meters` may show its last reading before the meter falls to rest
 * (ms). Six missed polls — transient IPC hiccups survive; a dead backend reads as silence
 * within ~0.7 s, never as a frozen needle. */
const METER_STALE_MS = 600

const REST: MeterReadout = { rxLevel: 0, smeterDb: null }

/** True when two readouts would RENDER identically (0.5 dB level quantum; exact S-meter). */
function sameReading(a: MeterReadout, b: MeterReadout): boolean {
  return (
    Math.round(rxLevelDb(a.rxLevel) * 2) === Math.round(rxLevelDb(b.rxLevel) * 2) &&
    a.smeterDb === b.smeterDb
  )
}

// ---- the shared poller: ONE interval, many subscribers ----

let current: MeterReadout = REST
let lastOkAt = 0
const subscribers = new Set<() => void>()
let timer: number | null = null

function publish(next: MeterReadout) {
  if (sameReading(current, next)) return
  current = next
  subscribers.forEach((notify) => notify())
}

function tick() {
  try {
    getMeters()
      .then((m) => {
        lastOkAt = Date.now()
        publish(m)
      })
      .catch(() => {
        // The command stopped answering. Bounded staleness: past the window the meters
        // fall to rest — an honest empty meter, never a frozen last reading.
        if (Date.now() - lastOkAt >= METER_STALE_MS) publish(REST)
      })
  } catch {
    /* api unavailable (piecemeal test mocks) — the widgets stay at rest */
  }
}

/** Start/stop the single interval from the current demand: ≥1 subscriber AND a visible
 * document. Called on every subscribe/unsubscribe and visibility flip. */
function reconcilePolling() {
  const wanted = subscribers.size > 0 && document.visibilityState !== 'hidden'
  if (wanted && timer === null) timer = window.setInterval(tick, METER_POLL_MS)
  if (!wanted && timer !== null) {
    window.clearInterval(timer)
    timer = null
  }
}

if (typeof document !== 'undefined') {
  document.addEventListener('visibilitychange', reconcilePolling)
}

/** Subscribe to ONE selected meter value. Re-renders the caller only when that value
 * changes; `active=false` (a hidden kept-alive host) unsubscribes entirely and reads
 * at-rest. */
function useMeterValue<T>(select: (m: MeterReadout) => T, active: boolean): T {
  const [value, setValue] = useState<T>(() => select(active ? current : REST))
  const selectRef = useRef(select)
  selectRef.current = select
  useEffect(() => {
    if (!active) {
      setValue(selectRef.current(REST))
      return
    }
    const notify = () =>
      setValue((prev) => {
        const next = selectRef.current(current)
        return Object.is(prev, next) ? prev : next
      })
    subscribers.add(notify)
    reconcilePolling()
    notify() // adopt what the bus already holds (mounted after another subscriber's poll)
    return () => {
      subscribers.delete(notify)
      reconcilePolling()
    }
  }, [active])
  return value
}

/** The live CAT S-meter reading (dB rel S9; null = "—"), for the cockpit hosts. An
 * smeterDb-only subscription: the host re-renders only when the S-meter value itself
 * changes, never on RX-level churn. */
export function useSmeterDb(active = true): number | null {
  return useMeterValue((m) => m.smeterDb, active)
}

/** Drop-in [`LevelMeter`] on the shared live poll. Subscribes to `rxLevel` only, so the
 * 10 Hz update re-renders THIS tiny component, never the host (TopBar / cockpit /
 * Settings). Pass `active={false}` from a kept-alive hidden host. */
export function LiveLevelMeter({
  label = t('meters.rx.label'),
  variant = 'compact',
  active = true,
}: {
  label?: string
  variant?: 'compact' | 'full'
  active?: boolean
}) {
  const rxLevel = useMeterValue((m) => m.rxLevel, active)
  return <LevelMeter value={rxLevel} label={label} variant={variant} />
}

/** The live "NN dB" text readout beside the Settings RX Level meter — same subscription,
 * same containment reasoning as [`LiveLevelMeter`]. */
export function LiveRxLevelDb({ active = true }: { active?: boolean }) {
  const rxLevel = useMeterValue((m) => m.rxLevel, active)
  return (
    <>
      {Math.round(rxLevelDb(rxLevel))} {DB}
    </>
  )
}
