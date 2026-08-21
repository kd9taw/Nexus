// The self-update prompt.
//
// Quiet on purpose. A new build is not urgent and never interrupts operating: this sits at the
// bottom of the window, does not steal focus, and can be dismissed for the session. Contrast
// with PounceBanner, which is loud and top-of-screen because a rare DX spot IS urgent — the two
// exist at deliberately different volumes.
//
// The Install button never installs by surprise. It refuses while the radio is busy and says
// why, because installing restarts the app and a restart mid-QSO loses the contact.

//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The version string and
// the download percentage are invariant technical values — they are interpolated, never
// formatted. `blockReason` and `error` arrive already-worded from the update hook.

import type { SelfUpdate } from '../useSelfUpdate'
import { t } from '../i18n'

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
          <span className="update-what">{t('update.failed')}</span>
          <span className="update-detail" title={error ?? ''}>
            {error ? error.slice(0, 120) : t('update.unknownError')}
          </span>
          <button
            type="button"
            className="update-close"
            onClick={dismiss}
            aria-label={t('common.dismiss')}
          >
            ✕
          </button>
        </>
      ) : phase === 'installing' ? (
        <span className="update-what">{t('update.installing', { version: version ?? '' })}</span>
      ) : (
        <>
          {/* Two whole sentences, not one plus an appended fragment: a language may want the
              download percentage somewhere else entirely. */}
          <span className="update-what">
            {progress && progress.total > 0
              ? t('update.readyDownloaded', {
                  version: version ?? '',
                  percent: pct(progress.done, progress.total),
                })
              : t('update.ready', { version: version ?? '' })}
          </span>
          <button
            type="button"
            className="update-install"
            onClick={install}
            disabled={!!blockReason}
            title={blockReason ?? t('update.install.title')}
          >
            {blockReason ?? t('update.install.label')}
          </button>
          <button
            type="button"
            className="update-close"
            onClick={dismiss}
            aria-label={t('update.notNow.label')}
            title={t('update.notNow.title')}
          >
            ✕
          </button>
        </>
      )}
    </div>
  )
}
