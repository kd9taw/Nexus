// Signed self-update: download quietly, install only when the operator says so.
//
// THE RULE THIS ENCODES (operator decision, 2026-07-25): Nexus never installs on its own
// schedule and never on launch. Installing restarts the app, and this app keys a transmitter —
// a restart at the wrong instant can strand PTT, abandon a QSO mid-sequence, or drop a run. So
// the download happens silently in the background (it is inert, nothing restarts), and the
// install waits for an explicit press that is REFUSED, with a reason, while the radio is busy.
//
// The refusal reason comes from the backend (`update_install_block`), not from the UI, because
// the engine is the only thing that actually knows whether TX is armed or a QSO is live. Asking
// it at press time rather than caching means the answer cannot be stale.
//
// ONE UPDATE PROMPT (operator, 2026-09-29). This hook is the update prompt wherever the updater
// can replace this install (`update_route`: the Windows setup, the AppImage, the macOS app), and
// it decides when the old notice in features/updateCheck.ts speaks: only when this hook has
// nothing to show, on a .deb, on a page with no updater, or after a check or download that
// failed quietly. Settings' "Check for updates" reaches it through `installSelfUpdate`.

import { useCallback, useEffect, useRef, useState } from 'react'
import {
  checkBetaUpdate,
  installBetaUpdate,
  openDownloadPage,
  prepareUpdateInstall,
  restartApp,
  updateInstallBlock,
  updateRoute,
  type UpdateRoute,
} from './api'
import {
  handOverDownloadUrl,
  installSelfUpdate,
  maybeCheckForUpdate,
  retireDownloadPrompt,
  type SelfUpdateAnswer,
} from './features/updateCheck'
import { pollSingleFlight } from './singleFlight'

type Phase = 'idle' | 'available' | 'downloading' | 'ready' | 'installing' | 'error'

export interface SelfUpdate {
  phase: Phase
  /** The version waiting to be installed, when there is one. */
  version: string | null
  /** Why installing is refused right now, or null when it is allowed. */
  blockReason: string | null
  /** Bytes downloaded so far / total, for the progress readout. */
  progress: { done: number; total: number } | null
  error: string | null
  /** Install and restart. No-op unless phase is 'ready' and nothing blocks. */
  install: () => void
  /** Hide the prompt for this session (the update stays downloaded). */
  dismiss: () => void
  /** The way out of a failed update: open the download page, then put this version away for
   *  the session as "Not now" does; hand over the page's address when no browser opens. */
  downloadInstead: () => void
}

/** Minimal shape of the updater plugin's JS surface, reached through the same global bridge
 * `api.ts` uses — see usePounce for why we do not add @tauri-apps/api as a dependency. */
interface UpdaterHandle {
  available: boolean
  version?: string
  downloadAndInstall?: (cb?: (ev: { event: string; data?: { contentLength?: number; chunkLength?: number } }) => void) => Promise<void>
  download?: (cb?: (ev: { event: string; data?: { contentLength?: number; chunkLength?: number } }) => void) => Promise<void>
  install?: () => Promise<void>
}

function updaterApi(): { check: () => Promise<UpdaterHandle | null> } | null {
  const u = (window as unknown as { __TAURI__?: { updater?: { check?: () => Promise<UpdaterHandle | null> } } })
    .__TAURI__?.updater
  return u?.check ? { check: u.check } : null
}

/** How a check ended, for the old notice. `answered`: this hook showed something, or found
 * nothing to show. `quiet`: it failed and showed nothing, the one ending that leaves the notice
 * to speak. `cancelled`: the channel changed under it, and the next run answers instead. */
type Ending = 'answered' | 'quiet' | 'cancelled'

export function useSelfUpdate(betaEnabled: boolean): SelfUpdate {
  const [phase, setPhase] = useState<Phase>('idle')
  const [version, setVersion] = useState<string | null>(null)
  const [blockReason, setBlockReason] = useState<string | null>(null)
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null)
  const [error, setError] = useState<string | null>(null)
  const handle = useRef<UpdaterHandle | null>(null)
  // Which channel produced the pending update — so `install` uses the matching install path. The
  // beta path (Rust `check_beta_update`/`install_beta_update`) points the updater at a
  // pre-release manifest; the stable path (the plugin's own JS `handle`) is left exactly as it
  // was, byte-for-byte, so every non-beta user's proven flow is unchanged.
  const isBeta = useRef(false)
  /// The version the operator dismissed. Written by `dismiss`, and READ by the hourly re-check
  /// — it was a plain boolean that nothing read, so "Not now" lasted until the next hourly
  /// tick re-ran the whole flow, re-downloaded, and put the banner straight back. Dismissal is
  /// per-VERSION, deliberately: "not 1.4.1 now" must not also swallow 1.5.0 next month.
  const dismissed = useRef<string | null>(null)
  /// The operator pressed "Check for updates" while a download was already on its way, so its
  /// failure is theirs to see, as a failed Install is. Cleared when a stable run ends. (A run
  /// the press starts itself knows it by its `told`.)
  const asked = useRef(false)
  /// This install's route, asked once: `update_route` reads the package stamped into the
  /// binary, which cannot change while the app runs. Asked only where the updater's API exists.
  const route = useRef<Promise<UpdateRoute | null> | null>(null)
  const routeOnce = useCallback(() => (route.current ??= updateRoute().catch(() => null)), [])
  /// The run the check effect set up, for Settings' check to reuse. Null where there is none:
  /// no updater, a package it cannot replace, or before the route has answered.
  const runner = useRef<((told?: (a: SelfUpdateAnswer) => void) => Promise<Ending>) | null>(null)
  /// This launch's answer for the old notice: true once a check has answered, false when none
  /// can. The first run to finish settles it; a promise settles once.
  const [launch] = useState(() => {
    let settle: (answered: boolean) => void = () => {}
    const answered = new Promise<boolean>((resolve) => (settle = resolve))
    return { answered, settle }
  })

  // The old notice's launch check. Its feed is read at every launch, as it always was; the
  // notice itself waits for this hook's answer and speaks only when that answer is "could not".
  useEffect(() => {
    void maybeCheckForUpdate(launch.answered)
  }, [launch])

  // Check once at startup, then hourly. Deliberately NOT aggressive: a new build is not urgent,
  // and the check costs a network round-trip.
  useEffect(() => {
    let alive = true
    let hourly: number | undefined
    const api = updaterApi()
    if (!api) {
      launch.settle(false) // no updater (the Remote page, a plain browser)
      return
    }

    // `told` hears the check's answer the moment there is one, before a download that can take
    // minutes, so "Check for updates" can say what it found. Only that press passes it.
    const run = async (told?: (a: SelfUpdateAnswer) => void): Promise<Ending> => {
      // BETA channel: resolve the newest pre-release via the backend and stash it for install.
      // No silent pre-download (that's the stable path's luxury) — the beta build downloads when
      // the operator presses Install, shown as an indeterminate 'installing' state.
      if (betaEnabled) {
        try {
          const info = await checkBetaUpdate()
          if (!alive) return 'cancelled'
          if (!info) {
            told?.({ kind: 'upToDate' })
            return 'answered'
          }
          if (dismissed.current != null && info.version === dismissed.current) return 'answered'
          isBeta.current = true
          setVersion(info.version)
          setPhase('ready')
          retireDownloadPrompt()
          told?.({ kind: 'ready', version: info.version })
          return 'answered'
        } catch (e) {
          // SILENT, exactly like the stable check below: a background beta check failing (no
          // pre-release manifest yet, GitHub unreachable, a blocked network) is not news.
          if (!alive) return 'cancelled'
          // eslint-disable-next-line no-console
          console.warn('nexus: beta update check failed (silent):', e)
          told?.({ kind: 'failed' })
          return 'quiet'
        }
      }
      isBeta.current = false
      let found = false
      try {
        const up = await api.check()
        if (!alive) return 'cancelled'
        if (!up?.available) {
          told?.({ kind: 'upToDate' })
          return 'answered'
        }
        // Honour a dismissal for THIS version — the hourly tick must not resurrect a banner
        // the operator just closed. A NEWER version than the dismissed one still lands.
        if (dismissed.current != null && (up.version ?? '') === dismissed.current) return 'answered'
        found = true
        handle.current = up
        setVersion(up.version ?? null)
        setPhase('available')
        // Download IMMEDIATELY and silently. Downloading changes nothing on disk that matters
        // and restarts nothing, so there is no reason to make the operator wait for it later —
        // the whole point is that when they press install, it is instant.
        setPhase('downloading')
        const dl = up.download ?? up.downloadAndInstall
        if (!dl) {
          setPhase('available')
          told?.({ kind: 'failed' })
          return 'quiet'
        }
        told?.({ kind: 'downloading', version: up.version ?? null })
        let total = 0
        let done = 0
        await dl((ev) => {
          if (ev.event === 'Started') total = ev.data?.contentLength ?? 0
          if (ev.event === 'Progress') {
            done += ev.data?.chunkLength ?? 0
            setProgress({ done, total })
          }
        })
        if (!alive) return 'cancelled'
        setPhase('ready')
        retireDownloadPrompt()
        return 'answered'
      } catch (e) {
        // SILENT. A background check or download failing is NOT news the operator asked for or
        // can act on: no release published a manifest yet, GitHub is unreachable, a corporate
        // network blocks it, the connection dropped mid-fetch. Surfacing "Update failed" for any
        // of those puts an error in front of someone who did nothing wrong — and before the first
        // manifest ships it would fire on EVERY launch for EVERY user. Only a failure of
        // something they explicitly pressed is worth their attention (see `install` below), and
        // "Check for updates" is such a press once it has found an update.
        if (!alive) return 'cancelled'
        // eslint-disable-next-line no-console
        console.warn('nexus: update check failed (silent):', e)
        if (found && (told || asked.current)) {
          setError(String(e))
          setPhase('error')
          return 'answered'
        }
        setPhase('idle')
        told?.({ kind: 'failed' })
        return 'quiet'
      } finally {
        asked.current = false
      }
    }
    const answer = (end: Ending) => {
      if (end !== 'cancelled') launch.settle(end === 'answered')
    }

    void routeOnce().then((r) => {
      if (!alive) return
      if (!r?.selfUpdate) {
        // A package the updater cannot replace (a .deb): no check, and so no download of an
        // installer it could never use. The notice speaks for it.
        launch.settle(false)
        return
      }
      runner.current = run
      void run().then(answer)
      hourly = window.setInterval(() => void run().then(answer), 60 * 60 * 1000)
    })
    return () => {
      alive = false
      window.clearInterval(hourly)
      if (runner.current === run) runner.current = null
    }
    // Re-run when the channel changes: flipping the beta toggle switches which feed is checked.
  }, [betaEnabled, launch, routeOnce])

  // Keep the refusal reason current while an update is waiting, so the button explains itself
  // the moment the radio goes idle rather than after the next click.
  useEffect(() => {
    if (phase !== 'ready') return
    // Single-flight (#335): `update_install_block` takes the engine mutex, so a tick skips while
    // the last read is still out.
    return pollSingleFlight('update block', 2000, (owns) =>
      updateInstallBlock()
        .then((r) => owns() && setBlockReason(r))
        .catch(() => {}),
    )
  }, [phase])

  const install = useCallback(() => {
    if (phase !== 'ready') return
    // Re-ask at the instant of the press. The polled value is for DISPLAY; this is the decision,
    // because the radio can go busy between the last poll and the click.
    void updateInstallBlock()
      .then(async (why) => {
        if (why) {
          setBlockReason(why)
          return
        }
        setPhase('installing')
        try {
          // FLUSH FIRST — on Windows there is no "after". The plugin's install() runs the
          // NSIS installer and calls exit(0) without unwinding, so nothing below this line
          // executes there and no exit event ever reaches the backend's quit cleanup: the
          // conversations, the Field Day log, an open propagation episode and the tail of the
          // diagnostic log would all go unwritten. A no-op on macOS/Linux, where install()
          // returns and restartApp() takes the ordinary path.
          // Best-effort, deliberately: the operator pressed Install, and a bookkeeping step
          // that could not complete must not be what stops the update. Every write behind it
          // is best-effort on the Rust side too.
          await prepareUpdateInstall().catch(() => {})
          // Same choreography for both channels — only the install call differs. The beta path
          // downloads THEN installs here (no pre-download), so this press covers both; on Windows
          // it execs the installer and exits the process, on macOS/Linux it returns and the
          // restartApp() below takes over — identical to the stable path.
          if (isBeta.current) {
            await installBetaUpdate()
          } else {
            await handle.current?.install?.()
          }
          // The plugin does NOT restart the app on macOS or the Linux AppImage — install()
          // swaps the bundle on disk and resolves with the OLD build still running, so
          // without this call the banner sat at "Nexus will restart…" forever (mac QA
          // audit, 2026-08-17). The restart is ours to do, and it goes through the
          // backend's ordinary quit cleanup (TX unkey wait, journal flushes, geometry) —
          // the same path a window close takes. On Windows the NSIS installer exits the
          // process mid-install(), so this line is never reached there.
          await restartApp()
        } catch (e) {
          setError(String(e))
          setPhase('error')
        }
      })
      .catch((e) => {
        // The operator pressed Install — a failure here IS theirs to know about.
        setError(String(e))
        setPhase('error')
      })
  }, [phase])

  const dismiss = useCallback(() => {
    dismissed.current = version
    setPhase('idle')
  }, [version])

  const downloadInstead = useCallback(() => {
    void openDownloadPage().then(
      () => dismiss(), // they have the page: this version is put away for the session
      async () => {
        const url = (await routeOnce())?.downloadPage
        if (url) handOverDownloadUrl(url)
      },
    )
  }, [dismiss, routeOnce])

  // Settings' "Check for updates". It answers at once where an update is already on screen or
  // on its way, and otherwise runs a check now, answering as soon as the check does. It never
  // installs: finding an update only ever ends at the banner's Install button.
  const checkNow = useCallback(async (): Promise<SelfUpdateAnswer> => {
    if (updaterApi()) await routeOnce()
    const run = runner.current
    if (!run) return { kind: 'unsupported' }
    if (phase === 'ready' || phase === 'installing') return { kind: 'ready', version }
    if (phase === 'available' || phase === 'downloading') {
      asked.current = true // the download on its way is now one they asked for
      return { kind: 'downloading', version }
    }
    dismissed.current = null // they asked: offer it even if they said "not now" to this version
    return new Promise<SelfUpdateAnswer>((resolve) => {
      // A run that ends without an answer (the channel changed under it) hands the question to
      // the notice's own check, which always answers.
      void run(resolve).then(() => resolve({ kind: 'failed' }))
    })
  }, [phase, routeOnce, version])
  useEffect(() => installSelfUpdate(checkNow), [checkNow])

  return { phase, version, blockReason, progress, error, install, dismiss, downloadInstead }
}
