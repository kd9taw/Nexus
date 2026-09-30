// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every toast below
// comes from the catalog; the version strings interpolated into them are invariant.
//
// ONE UPDATE PROMPT (operator, 2026-09-29). Where the signed self-updater (useSelfUpdate) can
// replace this install, its banner is the update prompt. The notice here, whose Download button
// opens the release page, speaks only where it cannot: the .deb packages, a page with no updater
// at all, and a launch or a check the self-updater could not answer. Both prompts on one screen
// was the bug.

import type { UpdateInfo } from '../types'
import { appVersion, checkForUpdate, openDownloadPage } from '../api'
import { dismissToast, pushToast } from '../toast'
import { t } from '../i18n'

// The dismissal lives client-side so a check never routes through the heavyweight set_settings
// path (which restarts feeds). The backend just fetches + compares.
const LS_DISMISSED = 'nexus.update.dismissedVersion'

/** The notice on screen, so the self-updater's banner can take its place when it arrives. */
let promptId: number | null = null

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
 *
 * `selfUpdated` is the self-updater's answer for this launch: true once it has answered itself
 * (up to date, or an update downloaded and offered), false when it could not. The feed is still
 * read every launch, because its answer is the launch's line in the diagnostic log; only the
 * prompt waits for the self-updater.
 */
export async function maybeCheckForUpdate(
  selfUpdated: Promise<boolean> = Promise.resolve(false),
): Promise<void> {
  const info = await checkForUpdate().catch(() => null)
  if (!info) return // offline / fetch error — stay silent
  if (!info.updateAvailable || !info.latest) return
  if (localStorage.getItem(LS_DISMISSED) === info.latest) return
  if (await selfUpdated) return // its banner is the prompt
  promptDownload(info)
}

/** Take the notice down: the self-updater's banner has the update now. */
export function retireDownloadPrompt(): void {
  if (promptId == null) return
  dismissToast(promptId)
  promptId = null
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
  promptId = pushToast(t('update.available', { latest, current: info.current }), 'info', 0, {
    prominent: true,
    actionLabel: t('update.download'),
    action: () =>
      openDownloadPage().then(
        () => {
          localStorage.setItem(LS_DISMISSED, latest)
        },
        (err: unknown) => {
          handOverDownloadUrl(info.downloadUrl)
          throw err // keep the update prompt up — they have not got the download yet
        },
      ),
  })
}

/** The browser did not open, and the operator still needs the build. Give them the address,
 * non-expiring (a URL you have to read must not time out) and copyable — the copy is the action,
 * so this toast in turn only closes once the link is really on the clipboard. */
export function handOverDownloadUrl(url: string): void {
  pushToast(t('update.downloadFailed', { url }), 'error', 0, {
    actionLabel: t('update.copyLink'),
    action: () => navigator.clipboard?.writeText(url),
  })
}

/** What the self-updater answered when Settings asked it to check (useSelfUpdate). */
export type SelfUpdateAnswer =
  /** An update is on screen in the banner, or downloading on its way there. */
  | { kind: 'ready' | 'downloading'; version: string | null }
  /** Up to date; the check failed; or it cannot work here at all (a .deb, no updater). */
  | { kind: 'upToDate' | 'failed' | 'unsupported' }

let selfUpdate: (() => Promise<SelfUpdateAnswer>) | null = null

/** The self-updater offers its check here while it is mounted: the hook lives in App and the
 * button in SettingsPanel, the way `installApplicationTransport` offers a transport. Returns
 * the disposer. */
export function installSelfUpdate(check: () => Promise<SelfUpdateAnswer>): () => void {
  selfUpdate = check
  return () => {
    if (selfUpdate === check) selfUpdate = null
  }
}

/**
 * Manual "Check for updates" (Settings button) — bypasses the once/day throttle and always gives
 * feedback: the update prompt, an "up to date" note, or an explicit "couldn't read the release
 * info" (never a false "you're on the latest" when the fetch succeeded but the parse failed).
 * Because the operator explicitly asked, it clears any prior dismissal of the offered version.
 *
 * The self-updater answers first wherever it can replace this install: its banner offers the
 * update (with a note while it downloads), or "up to date" names the running version. When it
 * could not answer, or cannot work here, the notice's own check below answers as it always has.
 */
export async function checkForUpdateManual(): Promise<void> {
  const answer = selfUpdate ? await selfUpdate().catch(() => null) : null
  if (answer?.kind === 'ready') return // the banner on screen is the answer
  if (answer?.kind === 'downloading') {
    pushToast(t('update.downloading', { version: answer.version ?? '' }), 'info')
    return
  }
  if (answer?.kind === 'upToDate') {
    const current = await appVersion().catch(() => null)
    if (current) {
      pushToast(t('update.upToDate', { current }), 'success')
      return
    }
  }
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
