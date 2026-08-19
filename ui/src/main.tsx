import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import App from './App'
import { ErrorBoundary } from './components/ErrorBoundary'
import { DetachedPanel } from './DetachedPanel'
import { redockAllStalePopouts } from './features/panelState'
import { loadDurable } from './features/durableStore'
import { installExternalLinkInterceptor } from './externalLinks'
import { isTauri, openExternalUrl } from './api'
import { pushToast } from './toast'
import { t } from './i18n'
import './styles.css'
// AFTER styles.css, deliberately: the cockpit pane grid's structural rules are all flat
// single-class selectors, so an equal-specificity tie with anything in styles.css must
// resolve in the structural sheet's favour (source order breaks the tie). See the header
// of cockpit-panes.css; cockpit-panes.test.ts guards both the order and the isolation.
import './cockpit-panes.css'

// A torn-off window (created by open_panel_window) loads the app at `?panel=<name>`
// and renders just that panel for multi-monitor use.
const panel = new URLSearchParams(window.location.search).get('panel')

// Tag the document so per-panel CSS can target one torn-off window. No rule uses it
// today (the Needed window's font bump was removed when pop-outs stopped being pinned to
// the 65% zoom floor — see useScale.ts); the hook stays because a per-panel override has
// nowhere else to live, and because the attribute is what tells you, in the inspector,
// which surface a window is.
if (panel) document.documentElement.dataset.panel = panel

// Fresh main-window boot: clear any stale "popped out" state. A detached panel window never
// survives an app restart (only the main window is restored), so a leftover pop-out — e.g. from
// a crash while popped out — would otherwise hide the docked panel with no window to re-dock it.
// Panels the operator explicitly REMOVED are untouched; those are meant to stay gone.
//
// EVERY vocabulary, not just Operate's — this ran on OPERATE_PANELS alone, which left a stored
// 'popped' in any other cockpit's record to persist across launches. See redockAllStalePopouts.
if (!panel) {
  redockAllStalePopouts()
  try {
    localStorage.removeItem('nexus.waterfall.detached')
  } catch {
    /* localStorage unavailable — nothing to clear */
  }
}

// Every `<a target="_blank">` needs routing through Rust (externalLinks.ts has the whole
// story: the opener plugin's injected handler swallows the click and its invoke is
// ACL-denied). Main window AND pop-outs — panels render external anchors too (APRS cards).
// In a plain browser (vite dev preview) there is no bridge and no plugin interceptor, so the
// native `_blank` behavior is correct and we install nothing.
if (isTauri()) {
  installExternalLinkInterceptor((url) => {
    openExternalUrl(url).catch((e) => {
      // A dead link failing SILENTLY is the exact bug class this fixes — say so instead.
      pushToast(t('externalLink.failed', { detail: e instanceof Error ? e.message : String(e) }), 'error')
    })
  })
}

// Outermost net. App carries its own boundary around the workspace (so a view crash
// leaves the rail usable), but a throw ABOVE `.shell` — the top bar, the Now Bar, App's
// own render — would still take the root down to a bare black window with no way out,
// which is exactly the 0.24.6 field report. A pop-out is a separate React root and gets
// nothing from the main window's boundary, so it needs its own. Recovery here is a
// window reload: at this level there is no navigation left to fall back to.
const reload = { label: t('crash.reload'), onClick: () => window.location.reload() }

// The product name, not prose: it is the same six characters in every language, and a catalog
// entry for it is an invitation to "localise" a brand. Same escape hatch as STATION_EXAMPLES.
const APP_NAME = 'Nexus'

const tree = (
  <StrictMode>
    {panel ? (
      <ErrorBoundary label={t('crash.panelWindow', { panel })} action={reload}>
        <DetachedPanel panel={panel} />
      </ErrorBoundary>
    ) : (
      <ErrorBoundary label={APP_NAME} action={reload}>
        <App />
      </ErrorBoundary>
    )}
  </StrictMode>
)

// Load the durable store BEFORE the first render, because the modules it backs — the memory
// bank, the watchlist, the chase sets and their alarms — read synchronously during their own
// module init and on first paint. Reading them before the store is loaded would fall through to
// `localStorage` and, worse, a subsequent write would then persist that stale answer over the
// durable copy. See features/durableStore.ts.
//
// `loadDurable` never rejects: with no bridge it leaves the cache empty and every call site
// falls back to `localStorage` exactly as before. `.finally` rather than `.then` so a future
// change that does let it throw still renders the app rather than a blank window — the 0.24.6
// failure mode, and not one to re-create over a preferences file.
void loadDurable().finally(() => {
  createRoot(document.getElementById('root')!).render(tree)
})
