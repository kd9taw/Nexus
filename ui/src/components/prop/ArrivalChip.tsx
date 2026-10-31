// THE "COMING" CHIP — a countdown to what NOAA expects to reach Earth.
//
// Three ways of knowing, three lines, never merged (the server keeps them apart too — see
// src-tauri arrival_outlook.rs and crates/propagation arrival.rs):
//   1. NOAA's WATCH — the forecasters' call, by the UT day. The chip's headline and its countdown:
//      "G1 watch · NOAA expects 31 Oct–1 Nov (in 21 h)", then "in force" inside the window.
//   2. The MODEL — the WSA-Enlil run, a second line under a watch only ("Model (WSA-Enlil): …").
//      Alone it draws no chip: a raw model run is louder than NOAA's own call, and NOAA has not
//      made one.
//   3. The MEASUREMENT — a step in the solar wind at L1, propagated to Earth by NOAA. The only
//      line that may say "arrived", and it draws a chip on its own (it is a fact).
// The hover says where each line comes from and when it was issued, and the one sentence the map
// owes the operator: a flare is never "coming" — its X-rays are here the instant they are seen.
//
// Shown only while there is something to say; nothing at all on a quiet sky or with no feed.
import { useEffect, useState } from 'react'
import type { ArrivalOutlook } from '../../types'
import { wxStampLabel } from '../../propViz'
import { dayLabel } from './SolarTrends'
import { ARRIVAL_OUTLOOK, useFeed } from '../../features/connectFeeds'
import { t } from '../../i18n'

const DAY = 86_400

/** "in 40 min", "in 21 h", "in 2 d 3 h" — the countdown to a time ahead. */
export function inLabel(secs: number): string {
  const mins = Math.max(1, Math.round(secs / 60))
  if (mins < 90) return t('map.arrival.in.mins', { n: mins })
  const hours = Math.round(secs / 3600)
  if (hours < 48) return t('map.arrival.in.hours', { n: hours })
  return t('map.arrival.in.days', { d: Math.floor(hours / 24), h: hours % 24 })
}

/** The watch's stormy days: "31 Oct" or "31 Oct–1 Nov" (the window's end is the next 00Z). */
export function watchDays(window: [number, number]): string {
  const [from, to] = window
  const last = to - DAY
  return last <= from ? dayLabel(from) : `${dayLabel(from)}–${dayLabel(last)}`
}

export interface ArrivalChipText {
  lines: string[]
  title: string
}

/** What the chip says at `now` (Unix seconds), or null when it has nothing to say. */
export function arrivalChipText(o: ArrivalOutlook | null | undefined, now: number): ArrivalChipText | null {
  if (!o) return null
  const watch = o.watch?.window && o.watch.window[1] > now ? o.watch : null
  const jump = o.l1?.jump ?? null
  if (!watch && !jump) return null
  const lines: string[] = []
  const title: string[] = []
  if (watch?.window) {
    const [start, end] = watch.window
    lines.push(
      now < start
        ? t('map.arrival.watch.ahead', { g: watch.maxG, days: watchDays(watch.window), in: inLabel(start - now) })
        : t('map.arrival.watch.now', { g: watch.maxG, last: dayLabel(end - DAY) }),
    )
    title.push(t('map.arrival.title.watch', { code: watch.code, serial: watch.serial, issued: wxStampLabel(watch.issued) }))
    if (watch.comment) title.push(t('map.arrival.title.comment', { comment: watch.comment }))
    const m = o.model
    if (m) {
      if (m.arrival) {
        lines.push(
          t('map.arrival.model.arrival', {
            from: wxStampLabel(m.arrival.start),
            to: wxStampLabel(m.arrival.end),
            speed: Math.round(m.arrival.speedTo),
          }),
        )
      } else if (m.peak) {
        lines.push(t('map.arrival.model.peak', { speed: Math.round(m.peak.speed), when: wxStampLabel(m.peak.at) }))
      }
      title.push(
        m.runAt != null ? t('map.arrival.title.model', { run: wxStampLabel(m.runAt) }) : t('map.arrival.title.modelNoRun'),
      )
    }
  }
  if (jump) {
    lines.push(t('map.arrival.l1.jump', { at: wxStampLabel(jump.at), earth: wxStampLabel(jump.arrives) }))
    title.push(t('map.arrival.title.l1'))
  }
  const latest = o.l1?.latest
  if (latest?.speed != null) {
    title.push(
      t('map.arrival.title.l1Now', {
        speed: Math.round(latest.speed),
        at: wxStampLabel(latest.at),
        earth: wxStampLabel(latest.arrives),
      }),
    )
  }
  title.push(t('map.arrival.title.why'))
  return { lines, title: title.join('\n') }
}

/** The chip itself, on the window's one poll of the outlook (features/connectFeeds). Re-reads the
 *  clock each minute so the countdown moves between polls. `compact`: the map is too narrow for the
 *  Layers panel and the Conditions rail side by side (the 1024 floor), so the chip draws its first
 *  line only, clear of the rail, and the rest is on hover. */
export function ArrivalChip({ compact = false }: { compact?: boolean }) {
  const outlook = useFeed(ARRIVAL_OUTLOOK).value
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000))
  useEffect(() => {
    const id = window.setInterval(() => setNow(Math.floor(Date.now() / 1000)), 60_000)
    return () => window.clearInterval(id)
  }, [])
  const text = arrivalChipText(outlook, now)
  if (!text) return null
  const shown = compact ? text.lines.slice(0, 1) : text.lines
  const title = compact && text.lines.length > 1 ? `${text.lines.slice(1).join('\n')}\n${text.title}` : text.title
  return (
    <div className={compact ? 'arrival-chip compact' : 'arrival-chip'} role="status" title={title}>
      {shown.map((l, i) => (
        <div key={i} className={i === 0 ? 'arrival-chip-head' : 'arrival-chip-line'}>
          {l}
        </div>
      ))}
    </div>
  )
}
