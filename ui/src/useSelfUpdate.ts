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

import { useCallback, useEffect, useRef, useState } from 'react'
import { prepareUpdateInstall, restartApp, updateInstallBlock } from './api'

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

export function useSelfUpdate(): SelfUpdate {
  const [phase, setPhase] = useState<Phase>('idle')
  const [version, setVersion] = useState<string | null>(null)
  const [blockReason, setBlockReason] = useState<string | null>(null)
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null)
  const [error, setError] = useState<string | null>(null)
  const handle = useRef<UpdaterHandle | null>(null)
  /// The version the operator dismissed. Written by `dismiss`, and READ by the hourly re-check
  /// — it was a plain boolean that nothing read, so "Not now" lasted until the next hourly
  /// tick re-ran the whole flow, re-downloaded, and put the banner straight back. Dismissal is
  /// per-VERSION, deliberately: "not 1.4.1 now" must not also swallow 1.5.0 next month.
  const dismissed = useRef<string | null>(null)

  // Check once at startup, then hourly. Deliberately NOT aggressive: a new build is not urgent,
  // and the check costs a network round-trip.
  useEffect(() => {
    let alive = true
    const api = updaterApi()
    if (!api) return // no updater (dev view, or a .deb build that cannot self-update)

    const run = async () => {
      try {
        const up = await api.check()
        if (!alive || !up?.available) return
        // Honour a dismissal for THIS version — the hourly tick must not resurrect a banner
        // the operator just closed. A NEWER version than the dismissed one still lands.
        if (dismissed.current != null && (up.version ?? '') === dismissed.current) return
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
          return
        }
        let total = 0
        let done = 0
        await dl((ev) => {
          if (ev.event === 'Started') total = ev.data?.contentLength ?? 0
          if (ev.event === 'Progress') {
            done += ev.data?.chunkLength ?? 0
            setProgress({ done, total })
          }
        })
        if (alive) setPhase('ready')
      } catch (e) {
        // SILENT. A background check or download failing is NOT news the operator asked for or
        // can act on: no release published a manifest yet, GitHub is unreachable, a corporate
        // network blocks it, the connection dropped mid-fetch. Surfacing "Update failed" for any
        // of those puts an error in front of someone who did nothing wrong — and before the first
        // manifest ships it would fire on EVERY launch for EVERY user. Only a failure of
        // something they explicitly pressed is worth their attention (see `install` below).
        if (!alive) return
        // eslint-disable-next-line no-console
        console.warn('nexus: update check failed (silent):', e)
        setPhase('idle')
      }
    }
    void run()
    const id = window.setInterval(run, 60 * 60 * 1000)
    return () => {
      alive = false
      window.clearInterval(id)
    }
  }, [])

  // Keep the refusal reason current while an update is waiting, so the button explains itself
  // the moment the radio goes idle rather than after the next click.
  useEffect(() => {
    if (phase !== 'ready') return
    let alive = true
    const poll = () => {
      updateInstallBlock()
        .then((r) => alive && setBlockReason(r))
        .catch(() => {})
    }
    poll()
    const id = window.setInterval(poll, 2000)
    return () => {
      alive = false
      window.clearInterval(id)
    }
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
          await handle.current?.install?.()
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

  return { phase, version, blockReason, progress, error, install, dismiss }
}
