// The app's one error boundary — a render throw must never black the whole window again.
//
// Field incident (0.24.6): a rules-of-hooks bug in a Connect pane threw on re-render,
// and with no boundary anywhere React unmounted the entire main-window root — pure
// background, nav rail included, re-armed every launch by the (valid!) persisted view.
// This boundary is the structural fix, mounted at two depths:
//   - App.tsx, INSIDE `.shell` around the workspace + keep-alive hosts, so a view
//     crash costs the view alone — the nav rail, top bar and toasts survive and the
//     operator can always navigate away;
//   - main.tsx, around each React root (the main window AND every pop-out — a pop-out
//     is its own root and gets nothing from App's), catching a throw above `.shell`.
//
// Honesty rules (notify, never hide):
// - the fallback names the failed surface and shows the real error + component stack,
//   selectable, for a bug report (the frontend twin of 0.20.0's nexus-crash.txt);
// - `console.error` fires on every catch — nothing is swallowed silently;
// - recovery is EXPLICIT: navigating to another section (`resetKey` change) or the
//   fallback's action button. A crash that recurs on retry is caught and shown again.
//
// Deliberately NOT a React `key={view}` reset: keying this wrapper would remount the
// keep-alive hosts (Operate/RTTY/SSTV/APRS accumulate decodes across navigation) on
// every section change. Instead the boundary clears its OWN error state when
// `resetKey` changes; in the no-error steady state it renders children with no
// wrapper element at all, so the `.shell` flex layout and the hosts are untouched.
import { Component, type ErrorInfo, type ReactNode } from 'react'

interface Props {
  /** What crashed, operator-facing ("Connect", "Needed pop-out"). */
  label: string
  /** Clearing signal: when this changes while showing the fallback, retry the children
   * (App passes the current view — navigating away always works). */
  resetKey?: string
  /** The fallback's escape hatch ("Back to Operate" / "Reload panel"). Clicking it
   * also retries the children, so it recovers even when the crashed view IS the
   * fallback target. */
  action?: { label: string; onClick: () => void }
  children: ReactNode
}

interface State {
  /** The caught error's message; null = healthy. */
  error: string | null
  /** React component stack of the crash, when available. */
  stack: string | null
}

export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null, stack: null }

  static getDerivedStateFromError(error: unknown): Partial<State> {
    return { error: error instanceof Error ? error.message : String(error) }
  }

  componentDidCatch(error: unknown, info: ErrorInfo) {
    // Loud by contract — a boundary must never turn a crash into silence.
    console.error(`[nexus] ${this.props.label} crashed:`, error, info.componentStack)
    this.setState({ stack: info.componentStack ?? null })
  }

  componentDidUpdate(prev: Props) {
    if (this.state.error !== null && prev.resetKey !== this.props.resetKey) {
      this.setState({ error: null, stack: null })
    }
  }

  render() {
    const { error, stack } = this.state
    if (error === null) return this.props.children
    const { label, action } = this.props
    return (
      // role="alert" — announced assertively (a11y is always-on; a crashed section
      // must not be silent to a screen-reader operator).
      <div className="view-crash" role="alert">
        <h2 className="view-crash-title">{label} hit an error</h2>
        <p className="view-crash-hint">
          The rest of Nexus is still running — the radio was not touched. Pick another
          section from the rail, or copy the details below into a bug report.
        </p>
        {/* The escape hatch sits ABOVE the details deliberately: a component stack is
            long, and the way out must be in the first screenful at every window size
            (the layout contract's "nothing unreachable"). */}
        {action && (
          <button
            type="button"
            className="view-crash-back"
            onClick={() => {
              action.onClick()
              // Retry even when the action lands on this same view (no resetKey change).
              this.setState({ error: null, stack: null })
            }}
          >
            {action.label}
          </button>
        )}
        <pre className="view-crash-detail">
          {error}
          {stack ? `\n${stack}` : ''}
        </pre>
      </div>
    )
  }
}
