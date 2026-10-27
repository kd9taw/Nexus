// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The screen it hosts owns its
// prose; every string here is a catalog key.
//
// THE CONTEST SCREEN WINDOW (`?panel=contestscreen`) — the main window's Contest screen below its log line,
// popped out to a monitor of its own (the operator, 2026-10-12: "the bottom section that has the contest
// mode switch and all the county activity, that whole window needs to pop out". The pick: "A Pop out button
// opens the whole lower section in its own window (put it on another monitor). While it's open, the main
// Contest screen shows just the big entry and last contacts; closing the window brings the section back.").
//
// ⭐ THE SAME SCREEN, NOT A COPY OF IT: `ContestView`, given what App gives it, so everything that works under
// the main window's log line works here — the contest switch, Running / S&P, the exports and the Logger
// window, the merge, the club board, the scoreboard, the bonuses, the log with Remove and Edit. Three things
// differ, each because this is another document:
//   · a save of the contest switch is followed by the main window, which reads its settings and the snapshot
//     again, as after a press of its own (`CONTEST_SCREEN_SAVED_KEY`), so the rail, the top bar and every
//     cockpit there see the change;
//   · a line saying why the contest cannot start has no Open Settings button: Settings is in the main
//     window, which this one cannot send anywhere (the Logger window's screen has none either);
//   · the operator box: while a contest runs it is on the main window's log line, so this screen draws
//     none, as it drew none under that line; with none running, the scoreboard keeps it here.
//
// ⭐ ITS KEYS ARE THE LOGGER WINDOW'S. F1–F12 are held, so an F5 reached for out of habit cannot reload the
// window mid-contest; with Alt, Ctrl or Cmd held a key is the system's, as on the main window's Contest
// screen, so Alt+F4 still closes it. Esc is the station's stop here as in the main window (`halt_tx`, bound by
// `DetachedPanel` for every contest window since the Illinois QSO Party, 2026-10-18, where Esc in a contest
// window did not stop a CW message); on the same press it still closes a dialog and backs out of an edit.
//
// THE MAIN WINDOW'S SIDE is `useContestScreenWindow`, below: whether the section is out, the Pop out, Bring it
// back, and the window's close. Nothing about the window is stored, and Nexus reopens no pop-out when it
// starts, so a restart brings the section back to the main window.

import { useCallback, useEffect, useRef, useState } from 'react'
import { closePanelWindow, openPanelWindow, setMode, type FdRulesetDto } from '../api'
import { useContestSwitch } from '../contestSwitch'
import { contestStartWarning } from '../features/contestLocation'
import { t } from '../i18n'
import { pushToast, withErrorToast } from '../toast'
import type { AppSnapshot, ModeRequest } from '../types'
import { ContestView } from './ContestView'

/** Where the window says it saved the settings, for the main window to read them again. SHARED
 *  (storage-scope.test.ts): the main window hears it as a `storage` event, which only a write from
 *  another document raises. */
export const CONTEST_SCREEN_SAVED_KEY = 'nexus.contestScreen.saved'

/** The window's label: `open_panel_window`'s `panel-<slug>` for the slug `contestscreen`
 *  (`chains::panel_label`, which every window already on an operator's disk pins). */
const WINDOW_LABEL = 'panel-contestscreen'

/** The window's content: the Contest screen, as App draws it under the log line. */
export function ContestScreenWindow({
  snap,
  fdActive,
  fdRuleset,
  onSnap,
  onSettingsSaved,
}: {
  snap: AppSnapshot
  /** The contest switch as this window's settings hold it (`settings.fdActive`). */
  fdActive: boolean
  fdRuleset: FdRulesetDto | null
  /** A command's answer, for the host to show at once instead of at its next poll. */
  onSnap: (snap: AppSnapshot) => void
  /** The host reads its settings and the snapshot again, as App does after its own switch. */
  onSettingsSaved: () => void
}) {
  // THE F-KEYS DO NOTHING HERE, the webview's own included (F5 reloads the page).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (/^F\d{1,2}$/.test(e.key) && !e.altKey && !e.ctrlKey && !e.metaKey) e.preventDefault()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])
  const fd = snap.fieldDay ?? null
  // A contest switched by a pick saved in the main window: this screen says so too.
  const contestSwitch = useContestSwitch(fd)
  // RUNNING / S&P: App's `handleSetMode`, the same request with the same two notices.
  const onSetMode = (mode: ModeRequest) => {
    void withErrorToast(() => setMode(mode), t('shell.error.switchMode')).then((s) => {
      if (s) onSnap(s)
      const warning = contestStartWarning(mode, s)
      if (warning) pushToast(warning, 'info', 15_000, { prominent: true })
    })
  }
  const settingsSaved = () => {
    onSettingsSaved()
    try {
      localStorage.setItem(CONTEST_SCREEN_SAVED_KEY, String(Date.now()))
    } catch {
      /* storage blocked: the main window reads them again at its next save of its own */
    }
  }
  return (
    <main className="layout single">
      <ContestView
        fieldDay={fd}
        clubLink={snap.clubLink}
        onSetMode={onSetMode}
        fdActive={fdActive}
        fdRuleset={fdRuleset}
        tier={snap.link.tier}
        section={snap.radio.operatingMode}
        keyboardMode={snap.radio.keyboardMode}
        onSettingsSaved={settingsSaved}
        contestSwitch={contestSwitch}
        operatorBoxAbove={fd != null}
      />
    </main>
  )
}

/**
 * THE MAIN WINDOW'S SIDE (App): whether the section is out in its window, the Pop out that puts it there,
 * and Bring it back, which closes the window. App keeps it, not the Contest screen, because the operator may
 * be on another screen when the window closes, and the section is back whichever screen they are on.
 *
 * Out only once the window exists (`openPanelWindow` answered), and back the moment it is gone, however it
 * went: its own close button, Alt+F4, Bring it back, or Nexus quitting. That is Tauri's `tauri://destroyed`
 * for that window, which the app sends from the window's Destroyed event. Not a flag the window clears on
 * its way out, as the torn-off waterfall's is: on Windows its webview is closed by WebView2's `Close`, which
 * "is synchronous and won't trigger the beforeunload event", so a window closed from its title bar would
 * leave the section away with nothing to bring it back but the button.
 *
 * `onSettingsSaved` runs when the window saves the settings (its contest switch): App's own
 * `handleSettingsSaved`, so the rail, the top bar and every cockpit follow, as after the switch here.
 */
export function useContestScreenWindow(onSettingsSaved: () => void): {
  out: boolean
  popOut: () => void
  bringBack: () => void
} {
  const [out, setOut] = useState(false)
  useEffect(() => {
    const listen = window.__TAURI__?.event?.listen
    if (!listen) return
    let alive = true
    let unlisten: (() => void) | undefined
    listen('tauri://destroyed', () => setOut(false), { target: { kind: 'WebviewWindow', label: WINDOW_LABEL } })
      .then((un) => {
        if (alive) unlisten = un
        else un()
      })
      .catch(() => {})
    return () => {
      alive = false
      unlisten?.()
    }
  }, [])
  const saved = useRef(onSettingsSaved)
  saved.current = onSettingsSaved
  useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (e.key === CONTEST_SCREEN_SAVED_KEY) saved.current()
    }
    window.addEventListener('storage', onStorage)
    return () => window.removeEventListener('storage', onStorage)
  }, [])
  const popOut = useCallback(() => {
    openPanelWindow('contestscreen').then(
      () => setOut(true),
      () => {},
    )
  }, [])
  const bringBack = useCallback(() => {
    setOut(false)
    void closePanelWindow('contestscreen').catch(() => {})
  }, [])
  return { out, popOut, bringBack }
}

/** Where the section was, while it is out: one line saying so, and the button that brings it back (the
 *  screen's own button, as its ⧉ Logger window is). */
export function ContestScreenAway({ onBringBack }: { onBringBack: () => void }) {
  return (
    <div className="contest-screen-away" role="status">
      <span>{t('contestScreen.away.text')}</span>
      <button type="button" className="export-btn" title={t('contestScreen.away.title')} onClick={onBringBack}>
        {t('contestScreen.away.back')}
      </button>
    </div>
  )
}
