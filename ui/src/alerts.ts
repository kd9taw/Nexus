// Decode alerts: a WebAudio beep + a visual toast, fired from the live decode
// feed and gated by user settings.
//
// - alertMyCall → any decode directed at my callsign
// - alertCq     → any decode that is a CQ
// - alertNew    → a station not seen before this browser session
//
// Each unique decode (from + message + freq) alerts at most once, and the
// "new station" set persists for the whole session so a call only alerts once.

import type { DecodeRow, Settings } from './types'
import { pushToast } from './toast'

const seenStations = new Set<string>()
const alertedDecodes = new Set<string>()

let audioCtx: AudioContext | null = null

/** Lazily create / resume the shared AudioContext (needs a user gesture first). */
function ensureCtx(): AudioContext | null {
  try {
    if (!audioCtx) {
      const Ctor =
        window.AudioContext ||
        (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext
      if (!Ctor) return null
      audioCtx = new Ctor()
    }
    if (audioCtx.state === 'suspended') void audioCtx.resume()
    return audioCtx
  } catch {
    return null
  }
}

/** Short two-tone beep. Frequencies differ by alert kind so they're distinguishable. */
function beep(freq: number): void {
  const ctx = ensureCtx()
  if (!ctx) return
  const now = ctx.currentTime
  const osc = ctx.createOscillator()
  const gain = ctx.createGain()
  osc.type = 'sine'
  osc.frequency.value = freq
  gain.gain.setValueAtTime(0.0001, now)
  gain.gain.exponentialRampToValueAtTime(0.18, now + 0.01)
  gain.gain.exponentialRampToValueAtTime(0.0001, now + 0.22)
  osc.connect(gain)
  gain.connect(ctx.destination)
  osc.start(now)
  osc.stop(now + 0.24)
}

function decodeKey(d: DecodeRow): string {
  return `${d.from ?? '?'}|${d.message}|${Math.round(d.freqHz)}`
}

type AlertKind = 'mycall' | 'cq' | 'new'

const BEEP_HZ: Record<AlertKind, number> = { mycall: 880, cq: 620, new: 740 }

/**
 * Inspect the latest decode rows and fire alerts for any that match the
 * enabled settings and haven't alerted before. Marks every row as seen so the
 * "new station" detection stays correct even when alerts are off.
 */
export function processDecodes(decodes: DecodeRow[], settings: Settings): void {
  for (const d of decodes) {
    const call = d.from
    const isNewStation = call != null && !seenStations.has(call)
    if (call != null) seenStations.add(call)

    // Decide whether this row should alert (highest priority first).
    let kind: AlertKind | null = null
    if (settings.alertMyCall && d.directedToMe) kind = 'mycall'
    else if (settings.alertCq && d.isCq) kind = 'cq'
    else if (settings.alertNew && isNewStation) kind = 'new'
    if (!kind) continue

    const key = decodeKey(d)
    if (alertedDecodes.has(key)) continue
    alertedDecodes.add(key)

    beep(BEEP_HZ[kind])
    const who = call ?? 'station'
    const text =
      kind === 'mycall'
        ? `${who} is calling you`
        : kind === 'cq'
          ? `CQ from ${who}`
          : `New station ${who}`
    pushToast(text, kind === 'mycall' ? 'success' : 'info', 3500)
  }

  // Keep the dedup set from growing unbounded over a long session.
  if (alertedDecodes.size > 400) alertedDecodes.clear()
}
