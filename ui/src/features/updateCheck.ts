import type { UpdateInfo } from '../types'
import { checkForUpdate, openDownloadPage } from '../api'
import { pushToast } from '../toast'

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

/** The non-expiring "update available" toast with a Download button. Marks the version dismissed
 * only AFTER the browser actually opens, so a failed open surfaces an error instead of silently
 * suppressing the prompt forever. */
function promptDownload(info: UpdateInfo): void {
  const latest = info.latest
  if (!latest) return
  pushToast(`Nexus ${latest} is available — you're on ${info.current}`, 'info', 0, {
    prominent: true,
    actionLabel: 'Download',
    action: () => {
      openDownloadPage()
        .then(() => localStorage.setItem(LS_DISMISSED, latest))
        .catch(() => pushToast('Could not open the download page', 'error'))
    },
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
    pushToast('Could not reach the update server to check for updates', 'error')
    return
  }
  if (info.updateAvailable && info.latest) {
    localStorage.removeItem(LS_DISMISSED) // they asked — show it even if previously dismissed
    promptDownload(info)
  } else if (info.latest) {
    pushToast(`You're on the latest Nexus (${info.current})`, 'success')
  } else {
    // Fetch worked but no recognizable version — don't claim up-to-date.
    pushToast("Couldn't read the latest release info", 'info')
  }
}
