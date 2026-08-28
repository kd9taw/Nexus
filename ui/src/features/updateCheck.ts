// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every toast below
// comes from the catalog; the version strings interpolated into them are invariant.

import type { UpdateInfo } from '../types'
import { checkForUpdate, openDownloadPage } from '../api'
import { pushToast } from '../toast'
import { t } from '../i18n'

// The dismissal lives client-side so a check never routes through the heavyweight set_settings
// path (which restarts feeds). The backend just fetches + compares.
const LS_DISMISSED = 'nexus.update.dismissedVersion'

/**
 * On app launch: check for a newer release and surface a single non-expiring "update available"
 * toast (with a Download button) whenever the latest build is newer than THIS one and the operator
 * hasn't already dismissed THAT version via Download.
 *
 * The check runs on EVERY launch. It's one small JSON GET, and the backend returns the real running
 * version, so there's never a stale or phantom nag. (The old code throttled to once/day AND gated
 * the *display* on that throttle — so the prompt got a single, easily-missed shot per 24 h, and any
 * prior launch or manual "Check for updates" reset the timer and suppressed it. Running per-launch
 * makes the notice reliably reappear until the operator acts.) Silent on any failure (offline).
 */
export async function maybeCheckForUpdate(): Promise<void> {
  const info = await checkForUpdate().catch(() => null)
  if (!info) return // offline / fetch error — stay silent
  if (!info.updateAvailable || !info.latest) return
  if (localStorage.getItem(LS_DISMISSED) === info.latest) return
  promptDownload(info)
}

/** The non-expiring "update available" toast with a Download button.
 *
 * ⚠️ THE ACTION RETURNS ITS PROMISE, and that is the contract with `Toasts.tsx`: an action that
 * resolves retires its toast, an action that REJECTS leaves it on screen. Both halves matter
 * here (R3, shipped in 1.6.x). On Linux the opener used to return Ok the instant `xdg-open` was
 * spawned, so a failed open reported success — and this function wrote `dismissedVersion` on it,
 * which `maybeCheckForUpdate` reads on every subsequent launch. One click on a button that did
 * nothing silenced update notices permanently. The backend now reports the real outcome; nothing
 * below records a dismissal or closes the prompt on a result we cannot trust, and a failure hands
 * over the URL rather than a dead end. */
function promptDownload(info: UpdateInfo): void {
  const latest = info.latest
  if (!latest) return
  pushToast(t('update.available', { latest, current: info.current }), 'info', 0, {
    prominent: true,
    actionLabel: t('update.download'),
    action: () =>
      openDownloadPage().then(
        () => {
          localStorage.setItem(LS_DISMISSED, latest)
        },
        (err: unknown) => {
          // The operator still needs the build. Give them the address, non-expiring (a URL you
          // have to read must not time out) and copyable — the copy is the action, so this toast
          // in turn only closes once the link is really on the clipboard.
          const url = info.downloadUrl
          pushToast(t('update.downloadFailed', { url }), 'error', 0, {
            actionLabel: t('update.copyLink'),
            action: () => navigator.clipboard?.writeText(url),
          })
          throw err // keep the update prompt up — they have not got the download yet
        },
      ),
  })
}

/**
 * Manual "Check for updates" (Settings button) — bypasses the once/day throttle and always gives
 * feedback: the update prompt, an "up to date" note, or an explicit "couldn't read the release
 * info" (never a false "you're on the latest" when the fetch succeeded but the parse failed).
 * Because the operator explicitly asked, it clears any prior dismissal of the offered version.
 */
export async function checkForUpdateManual(): Promise<void> {
  const info = await checkForUpdate().catch(() => null)
  if (!info) {
    pushToast(t('update.checkFailed'), 'error')
    return
  }
  if (info.updateAvailable && info.latest) {
    localStorage.removeItem(LS_DISMISSED) // they asked — show it even if previously dismissed
    promptDownload(info)
  } else if (info.latest) {
    pushToast(t('update.upToDate', { current: info.current }), 'success')
  } else {
    // Fetch worked but no recognizable version — don't claim up-to-date.
    pushToast(t('update.unreadable'), 'info')
  }
}
