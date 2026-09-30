// The FULL Connect view, in a plain browser — the shack-TV wall-display page.
//
// This renders the SAME `ConnectView` the desktop app renders: the real map with every
// layer (grayline, spots, MUF, aurora, satellites, parks), the real panes, the real
// styles. Nothing is a copy; a Connect improvement lands on the TV the same release.
// Data arrives over the read-only LAN RPC (`window.__NEXUS_TV_RPC__` → api.ts's
// httpInvoke), which serves exactly the allowlist in
// `crates/tempo-app/src/connect_web.rs` and 404s everything else.
//
// ⚠️ WHAT THIS PAGE DELIBERATELY DOES NOT HAVE — and what stands in for it:
//   • stations = []          — the live FT8 roster is operating state, not weather.
//   • needByCall = empty     — the needs board is never served (module threat model).
//   • onWorkSpot omitted     — nothing on a TV may reach a transmitter; without the
//                              handler ConnectView renders no work affordance.
//   • Panes whose feed is not allowlisted (rotator, amplifier) degrade exactly as the
//     desktop does offline: their fetches reject, their catch paths render the empty
//     state. Honest emptiness, never fabricated data.
import { useEffect, useRef, useState } from 'react'
import { ConnectView } from '../components/ConnectView'
import { DashboardBar } from '../components/DashboardBar'
import { getPropagation, getTvStation } from '../api'
import type { NeedTag, PropagationSnapshot } from '../types'
import { useViewport } from '../useViewport'
import { t } from '../i18n'

/** Matches the app's own 30 s propagation cadence (PSK Reporter's rate limit is
 *  enforced server-side by the cache, so a second viewer costs nothing extra). */
const POLL_MS = 30_000

export function ConnectTv() {
  useViewport()
  const [prop, setProp] = useState<PropagationSnapshot | null>(null)
  const [station, setStation] = useState<{ call: string; grid: string } | null>(null)
  const [selectedCall, setSelectedCall] = useState<string | null>(null)
  // Wall-clock of the last successful poll — the honesty chip. A wall display that
  // cannot say its data is stale is worse than a blank one.
  const [lastOk, setLastOk] = useState<number | null>(null)
  const [linkDown, setLinkDown] = useState(false)
  const emptyNeeds = useRef(new Map<string, NeedTag>())

  useEffect(() => {
    let live = true
    const load = () =>
      getPropagation()
        .then((p) => {
          if (!live) return
          setProp(p)
          setLastOk(Date.now())
          setLinkDown(false)
        })
        .catch(() => live && setLinkDown(true))
    load()
    const id = window.setInterval(load, POLL_MS)
    return () => {
      live = false
      window.clearInterval(id)
    }
  }, [])

  useEffect(() => {
    let live = true
    getTvStation()
      .then((s) => live && setStation(s))
      .catch(() => {})
    return () => {
      live = false
    }
  }, [])

  // The link's age is counted in whole minutes, so a ten-second tick is plenty for the chip.
  // The clocks are the dashboard bar's own.
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 10_000)
    return () => window.clearInterval(id)
  }, [])

  const staleMin = lastOk == null ? null : Math.floor((now - lastOk) / 60_000)

  return (
    <div className="app tv-app">
      {/* The same bar as the Connect pop-out: station, clocks, indices. */}
      <DashboardBar call={station?.call ?? ''} grid={station?.grid ?? ''} prop={prop}>
        {/* Read-only is a promise the server keeps (GET/HEAD only, allowlisted reads);
            the chip just says it out loud so nobody hunts for controls. */}
        <span className="tv-chip">{t('tv.readonly')}</span>
        {linkDown ? (
          <span className="tv-chip tv-bad">{t('tv.noLink')}</span>
        ) : staleMin != null && staleMin >= 3 ? (
          <span className="tv-chip tv-warn">{t('tv.stale', { min: staleMin })}</span>
        ) : null}
      </DashboardBar>
      {prop == null && !linkDown ? (
        <div className="tv-wait">{t('tv.waiting')}</div>
      ) : (
        <ConnectView
          myGrid={station?.grid ?? ''}
          theme="dark"
          stations={[]}
          prop={prop}
          selectedCall={selectedCall}
          onSelectCall={setSelectedCall}
          needByCall={emptyNeeds.current}
          // A wall display: a slot's tabs may rotate (⋯ ▸ Rotate the tabs), as in the dashboard window.
          autoRotate
        />
      )}
    </div>
  )
}
