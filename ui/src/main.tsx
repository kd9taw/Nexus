import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import App from './App'
import { DetachedPanel } from './DetachedPanel'
import { OPERATE_PANELS, redockStalePopouts } from './features/panelState'
import './styles.css'

// A torn-off window (created by open_panel_window) loads the app at `?panel=<name>`
// and renders just that panel for multi-monitor use.
const panel = new URLSearchParams(window.location.search).get('panel')

// Tag the document so per-panel CSS can target a torn-off window (e.g. the Needed
// window bumps its font/line size — the operator reads it from across the shack).
if (panel) document.documentElement.dataset.panel = panel

// Fresh main-window boot: clear any stale "popped out" state. A detached panel window never
// survives an app restart (only the main window is restored), so a leftover pop-out — e.g. from
// a crash while popped out — would otherwise hide the docked panel with no window to re-dock it.
// Panels the operator explicitly REMOVED are untouched; those are meant to stay gone.
if (!panel) {
  redockStalePopouts(OPERATE_PANELS)
  try {
    localStorage.removeItem('nexus.waterfall.detached')
  } catch {
    /* localStorage unavailable — nothing to clear */
  }
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>{panel ? <DetachedPanel panel={panel} /> : <App />}</StrictMode>,
)
