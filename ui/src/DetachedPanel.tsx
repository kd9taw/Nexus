// Standalone-window renderer: when the app is loaded at `?panel=<name>` (a torn-off
// window created by open_panel_window), render JUST that panel — chrome-less, with
// its own polling — against the same shared engine the main window uses. Multi-
// monitor tear-off. First target: the Needed/spots board; the switch extends to
// other panels.
import { useEffect, useState } from 'react'
import type { BandChannel, NeedAlert } from './types'
import { getBandPlan, getNeedAlerts, selectPeer, setFrequency } from './api'
import { NeededPanel } from './components/NeededPanel'

export function DetachedPanel({ panel }: { panel: string }) {
  const [alerts, setAlerts] = useState<NeedAlert[]>([])
  const [bandPlan, setBandPlan] = useState<BandChannel[]>([])
  const [selected, setSelected] = useState<string | null>(null)

  useEffect(() => {
    let live = true
    const load = () =>
      getNeedAlerts()
        .then((a) => live && setAlerts(a))
        .catch(() => {})
    load()
    const id = setInterval(load, 15_000)
    getBandPlan()
      .then((b) => live && setBandPlan(b))
      .catch(() => {})
    return () => {
      live = false
      clearInterval(id)
    }
  }, [])

  if (panel === 'needed') {
    return (
      <div className="app detached">
        <NeededPanel
          alerts={alerts}
          bandPlan={bandPlan}
          selectedCall={selected}
          onQsy={(band) => {
            const ch = bandPlan.find((c) => c.band === band)
            if (ch) void setFrequency(ch.dialMhz, ch.band, ch.mode).catch(() => {})
          }}
          onSelect={(call) => {
            setSelected(call)
            void selectPeer(call).catch(() => {})
          }}
        />
      </div>
    )
  }

  return (
    <div className="app detached">
      <div className="app loading">
        <span>Panel “{panel}” isn’t available as a standalone window yet.</span>
      </div>
    </div>
  )
}
