// The self-update prompt.
//
// Quiet on purpose. A new build is not urgent and never interrupts operating: this sits at the
// bottom of the window, does not steal focus, and can be dismissed for the session. Contrast
// with PounceBanner, which is loud and top-of-screen because a rare DX spot IS urgent — the two
// exist at deliberately different volumes.
//
// The Install button never installs by surprise. It refuses while the radio is busy and says
// why, because installing restarts the app and a restart mid-QSO loses the contact.

import type { SelfUpdate } from '../useSelfUpdate'

function pct(done: number, total: number): number {
  return total > 0 ? Math.min(100, Math.round((done / total) * 100)) : 0
}

export function UpdateBanner({ update }: { update: SelfUpdate }) {
  const { phase, version, blockReason, progress, error, install, dismiss } = update
  // 'available'/'downloading' stay silent: nothing is asked of the operator until it is ready to
  // go, and a progress bar for something they did not request is just noise.
  if (phase !== 'ready' && phase !== 'installing' && phase !== 'error') return null

  return (
    <div className="update-banner" role="status" aria-live="polite">
      {phase === 'error' ? (
        <>
          <span className="update-what">Update failed</span>
          <span className="update-detail" title={error ?? ''}>
            {error ? error.slice(0, 120) : 'Unknown error'}
          </span>
          <button type="button" className="update-close" onClick={dismiss} aria-label="Dismiss">
            ✕
          </button>
        </>
      ) : phase === 'installing' ? (
        <span className="update-what">Installing {version ?? ''} — Nexus will restart…</span>
      ) : (
        <>
          <span className="update-what">
            Nexus {version ?? ''} is ready to install
            {progress && progress.total > 0 ? ` (${pct(progress.done, progress.total)}% downloaded)` : ''}
          </span>
          <button
            type="button"
            className="update-install"
            onClick={install}
            disabled={!!blockReason}
            title={blockReason ?? 'Install the update and restart Nexus'}
          >
            {blockReason ?? 'Install and restart'}
          </button>
          <button
            type="button"
            className="update-close"
            onClick={dismiss}
            aria-label="Not now"
            title="Not now — the update stays downloaded"
          >
            ✕
          </button>
        </>
      )}
    </div>
  )
}
