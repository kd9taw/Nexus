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

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <TvBoundary>
      <ConnectTv />
    </TvBoundary>
  </React.StrictMode>,
)
