// Entry for the TV page (connect-tv.html) — a separate Vite entry, NOT the app's
// main.tsx: no external-link interceptor, no DPI seeding, no pop-out plumbing. The
// RPC base is declared BEFORE anything can invoke, which is what routes api.ts's
// invoke() over HTTP instead of the (absent) desktop bridge.
import React from 'react'
import ReactDOM from 'react-dom/client'
import { ConnectTv } from './ConnectTv'
import '../styles.css'

/** Outermost net, same reason the app's main.tsx has one: a throw above the view
 *  otherwise takes the root down to a bare black screen on a TV nobody can debug.
 *  Recovery is a reload — at this level there is nothing else to fall back to. */
class TvBoundary extends React.Component<{ children: React.ReactNode }, { err: unknown }> {
  state = { err: null as unknown }
  static getDerivedStateFromError(err: unknown) {
    return { err }
  }
  render() {
    if (this.state.err != null) {
      return (
        <div className="tv-wait" role="alert">
          {String(this.state.err)} — reload the page
        </div>
      )
    }
    return this.props.children
  }
}

window.__NEXUS_TV_RPC__ = '/connect/rpc'
// A shack TV is a dark room; the app's dark theme is the design's home. (The desktop
// stores the operator's choice; a TV has no operator to ask.)
document.documentElement.setAttribute('data-theme', 'dark')

// ---- Wall-display defaults, seeded ONLY where this browser has never chosen ----
// The TV browser's storage is its own (per origin), so none of this touches the
// desktop; and every seed is a first-run default, not an override — change a pane or
// expand the conditions rail on the TV and the choice sticks, exactly as it would in
// the app. The first render came out CRAMPED (operator, on the screenshot): two cards
// parked over the globe and a Chase pane that can never fill here, because the needs
// board is deliberately never served. A wall display is map-first.
import { surfaceGet, surfaceSet } from '../features/windowScope'
if (surfaceGet('nexus.connect.insights.collapsed') == null) {
  // The conditions card over the globe starts collapsed — its numbers live in the
  // panes anyway; the chevron brings it back.
  surfaceSet('nexus.connect.insights.collapsed', '1')
}
if (surfaceGet('nexus.connect.config') == null) {
  // Panes that FILL from public weather. Chase and Selection need the needs board /
  // a click-through workflow, which the read-only page never has — an empty pane on
  // a wall reads as broken, not as waiting.
  surfaceSet(
    'nexus.connect.config',
    JSON.stringify({
      slots: {
        left1: 'advisory',
        left2: 'bandAdvisor',
        right1: 'insights',
        right2: 'kpOutlook',
        bottom1: 'openings',
        bottom2: 'spacewx',
        bottom3: 'beacons',
      },
      overlays: {},
    }),
  )
}
// Room-distance type: a mild default magnification, overridable per TV with ?zoom=1.3
// (and persisted nowhere — the URL is the setting, which a TV bookmark keeps).
const zoom = Number(new URLSearchParams(location.search).get('zoom')) || 1.15
document.documentElement.style.setProperty('--ui-zoom', String(zoom))

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <TvBoundary>
      <ConnectTv />
    </TvBoundary>
  </React.StrictMode>,
)
