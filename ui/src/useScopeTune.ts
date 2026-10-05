import { useRef } from 'react'
import type { AppSnapshot } from './types'
import { setFrequency } from './api'
import { bandLabelForMhz } from './band'
import { clampPassband, type PassbandLimits } from './spectrum/markers'

/** Trailing-flush window while dragging: at most one CAT write per this many ms
 * (the same cadence as useWheelTune's coalescer). */
const FLUSH_MS = 120

/** THE COALESCING both scope writers share — the dial drag and the filter edge. The first report
 * arms one timer, every report until it fires replaces the target, and the flush delivers the
 * LAST: one write per FLUSH_MS however fast the reports come, never a burst. */
function coalescer<T>(deliver: (v: T) => void): (v: T) => void {
  let target: { v: T } | null = null
  let timer: number | null = null
  return (v: T) => {
    target = { v }
    if (timer == null) {
      timer = window.setTimeout(() => {
        timer = null
        const t = target
        target = null
        if (t) deliver(t.v)
      }, FLUSH_MS)
    }
  }
}

/** A tune request reported by PhoneScope — already resolved to an absolute dial. */
export interface ScopeTuneRequest {
  dialHz: number
  kind: 'click' | 'drag'
}

interface ScopeTuneOpts {
  /** Sideband to preserve so a scope tune never flips the mode. */
  sideband: string
  /** Only tune when CAT is up and we're not transmitting. */
  enabled: boolean
  /** Receive the flushed set_frequency snapshot so the UI updates promptly. */
  onSnap?: (s: AppSnapshot) => void
}

/**
 * The CAT side of scope click/drag tuning: clicks command immediately; drag reports
 * are COALESCED — latest target wins, one `set_frequency` per ~120 ms — mirroring
 * useWheelTune (which stays untouched; drag is simpler since every report is an
 * absolute target, so there's no accumulator and no idle-reseed). The pointerup's
 * final drag report rides the pending timer, so no explicit drag-end signal exists.
 * A wheel racing a drag is benign: the backend pushes only the latest dial and defers
 * read-back a full poll past a QSY, so latest-wins with no snap-back.
 */
export function useScopeTune(opts: ScopeTuneOpts): (t: ScopeTuneRequest) => void {
  const stateRef = useRef(opts)
  stateRef.current = opts
  const cbRef = useRef<((t: ScopeTuneRequest) => void) | null>(null)

  if (!cbRef.current) {
    const send = (hz: number) => {
      const { sideband, onSnap } = stateRef.current
      const mhz = Math.round(hz) / 1e6
      // A dial has to BE one — the band lookup was doing this job too (it answers '' for NaN),
      // and a degenerate scope span must not put NaN or a negative frequency on the wire.
      if (!Number.isFinite(mhz) || mhz <= 0) return
      // An EMPTY band label is fine: listening off the ham bands is first-class (operator,
      // 2026-08-13), so a click on the part of a band map that names no band tunes there instead
      // of doing nothing at all. The engine routes a bandless dial.
      void setFrequency(mhz, bandLabelForMhz(mhz), sideband || 'USB')
        .then((s) => s && onSnap?.(s))
        .catch(() => {})
    }
    const drag = coalescer(send)
    cbRef.current = (t: ScopeTuneRequest) => {
      if (!stateRef.current.enabled) return
      if (t.kind === 'click') {
        send(t.dialHz)
        return
      }
      drag(t.dialHz)
    }
  }
  return cbRef.current
}

interface ScopePassbandOpts {
  /** The same hold as tuning — CAT up, not transmitting, a width the rig reports and can take —
   *  asked when a report arrives AND again when the flush fires, so an edge still being dragged
   *  when the transmitter keys writes nothing. (The dial's flush is not re-asked: its write is held
   *  by the radio loop while keyed, and its behaviour is not this hook's to change.) */
  enabled: boolean
  /** The cockpit's range and step (`PASSBAND_LIMITS`); every write is clamped to it. */
  limits: PassbandLimits
  /** The write itself — useReceiverFilter's `setWidth`, the ± stepper's own. */
  send: (hz: number) => Promise<AppSnapshot | undefined>
  /** Receive the snapshot the write returns, so the marker settles on the width at once. */
  onSnap?: (s: AppSnapshot) => void
  /** A write that failed — the cockpit's toast. */
  onError?: () => void
}

/**
 * The CAT side of the scope's filter edge and its [ ] keys: every report is a WIDTH, coalesced
 * exactly as a dial drag is — one write per flush, the last width wins — and clamped to the
 * cockpit's range on the way out.
 */
export function useScopePassband(opts: ScopePassbandOpts): (widthHz: number) => void {
  const stateRef = useRef(opts)
  stateRef.current = opts
  const cbRef = useRef<((widthHz: number) => void) | null>(null)
  if (!cbRef.current) {
    const send = (hz: number) => {
      const { enabled, limits, send: write, onSnap, onError } = stateRef.current
      if (!enabled) return
      void write(clampPassband(hz, limits))
        .then((s) => s && onSnap?.(s))
        .catch(() => onError?.())
    }
    const drag = coalescer(send)
    cbRef.current = (widthHz: number) => {
      if (!stateRef.current.enabled) return
      drag(widthHz)
    }
  }
  return cbRef.current
}
