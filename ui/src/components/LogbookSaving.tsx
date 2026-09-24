// The quit, when the logbook still has changes on their way to disk (SPEC-1's C10).
//
// Closing the main window used to take it off the screen at once and leave the process behind,
// invisible, saving the log for up to ten seconds — and then quitting regardless, with whatever
// had not reached the disk still lost. Now the station holds the window while it saves, and this
// host is what the operator sees meanwhile. It shows nothing until the save has taken longer than
// `SAVING_GRACE_MS`, so an ordinary quit is as quick as it always was. It asks a question only
// when the station cannot finish on its own: a minute has passed, or the logbook refused a change.
//
// The station drives it through three events (src-tauri `quit.rs`), and never the other way
// round — the only thing sent back is the operator's answer:
//
//   `logbook-saving`       { pending, radioLive }  — saving; `pending` counts down as changes land.
//                          0 means only the log.adi copy is left, and the line names no count.
//   `logbook-save-failed`  { pending, retryable, refused, reason, retryReason, radioLive } — the
//                          question. `pending` changes may still land if the operator waits;
//                          `retryable` ones the logbook refused for a reason that can pass, and
//                          Keep trying sends them again from memory; `refused` ones it refused for
//                          what they are, which nothing will save. So Keep trying is offered only
//                          while something is pending or can be sent again.
//   `logbook-save-done`    { saved } — the station has finished with the logbook; hide.
//
// ⛔ THE STOP LINE. This is a modal over the whole window, so while it shows, every stop control
// in every cockpit is beneath it. On the quit and restart paths that costs nothing — the station
// has already stopped the radio loop, which unkeys, before it waits on anything. Before a Windows
// update the radio is deliberately left running (a failed install must leave a working station),
// and the station says so with `radioLive`; the dialog then carries its own Stop TX, which is the
// universal `halt_tx`.
//
// Inert without the desktop event bridge: the hosted Remote page reuses App, and a browser has no
// quit to report.
import { useEffect, useRef, useState, type ReactNode } from 'react'
import { Dialog } from './ui/Dialog'
import { haltTx, logbookSaveChoice } from '../api'
import { t } from '../i18n'
import { useLocale } from '../i18n/useLocale'

export const LOGBOOK_SAVING = 'logbook-saving'
export const LOGBOOK_SAVE_FAILED = 'logbook-save-failed'
export const LOGBOOK_SAVE_DONE = 'logbook-save-done'

/** How long a save may take before the operator is told about it. Below this the window closes
 *  with nothing drawn, which is what a quit looked like before the logbook database. */
export const SAVING_GRACE_MS = 300

/** `logbook-saving`: how many changes are still on their way, and whether the radio still runs. */
export interface LogbookSavingEvent {
  pending: number
  radioLive: boolean
}

/** `logbook-save-failed`: what is left, what the logbook refused, and the station's reasons
 *  (its diagnostic wording, shown untranslated): `reason` for the changes refused for good,
 *  `retryReason` for those Keep trying sends again. */
export interface LogbookSaveFailedEvent {
  pending: number
  /** Absent from a station that predates the re-send: read as 0. */
  retryable?: number
  refused: number
  reason: string | null
  retryReason?: string | null
  radioLive: boolean
}

type Shown =
  | { kind: 'saving'; ev: LogbookSavingEvent }
  | { kind: 'failed'; ev: LogbookSaveFailedEvent; answered: boolean }

export function LogbookSaving() {
  useLocale()
  const [shown, setShown] = useState<Shown | null>(null)
  // The latest progress while the grace runs, read when it expires.
  const latest = useRef<LogbookSavingEvent | null>(null)
  const grace = useRef<number | null>(null)
  // Whether the dialog is on screen, readable from the listeners without a stale closure.
  const visible = useRef(false)

  useEffect(() => {
    const listen = window.__TAURI__?.event?.listen
    if (!listen) return
    let alive = true
    const unlisten: Array<() => void> = []
    const stopGrace = () => {
      if (grace.current !== null) window.clearTimeout(grace.current)
      grace.current = null
    }
    const show = (next: Shown | null) => {
      visible.current = next !== null
      setShown(next)
    }
    const onSaving = (ev: LogbookSavingEvent) => {
      latest.current = ev
      if (visible.current) {
        // Already on screen (a slow save, or the operator chose to keep trying): no second grace.
        show({ kind: 'saving', ev })
        return
      }
      // The grace starts at the FIRST report and is never restarted: the station reports every
      // 250 ms, and a restarting timer would keep a slow save off the screen forever.
      if (grace.current === null) {
        grace.current = window.setTimeout(() => {
          grace.current = null
          if (latest.current) show({ kind: 'saving', ev: latest.current })
        }, SAVING_GRACE_MS)
      }
    }
    const onFailed = (ev: LogbookSaveFailedEvent) => {
      stopGrace()
      show({ kind: 'failed', ev, answered: false })
    }
    const onDone = () => {
      stopGrace()
      latest.current = null
      show(null)
    }
    const register = async <P,>(name: string, handle: (p: P) => void) => {
      try {
        const un = await listen<P>(name, (e) => {
          if (e?.payload) handle(e.payload)
        })
        if (alive) unlisten.push(un)
        else un()
      } catch {
        // A listener that fails to register must not take the app down.
      }
    }
    void register<LogbookSavingEvent>(LOGBOOK_SAVING, onSaving)
    void register<LogbookSaveFailedEvent>(LOGBOOK_SAVE_FAILED, onFailed)
    void register<{ saved: boolean }>(LOGBOOK_SAVE_DONE, onDone)
    return () => {
      alive = false
      stopGrace()
      for (const un of unlisten) un()
    }
  }, [])

  const answer = (keepTrying: boolean) => {
    setShown((s) => (s?.kind === 'failed' ? { ...s, answered: true } : s))
    void logbookSaveChoice(keepTrying).catch(() => {})
  }

  const radioLive = shown?.ev.radioLive ?? false
  const stop = radioLive ? (
    <button type="button" className="settings-refresh" onClick={() => void haltTx().catch(() => {})}>
      {t('quit.logbook.stopTx')}
    </button>
  ) : null

  // The first sentence is the dialog's DESCRIPTION, so a screen reader announces it with the
  // title; a second one (a refusal AND changes still pending) follows as a plain paragraph.
  let title = ''
  let description: string | undefined
  let more: ReactNode = null
  let actions: ReactNode = stop
  if (shown?.kind === 'saving') {
    title = t('quit.logbook.saving.title')
    description =
      shown.ev.pending > 0 ? t('quit.logbook.saving.pending', { count: shown.ev.pending }) : undefined
  } else if (shown?.kind === 'failed') {
    const { pending, refused, reason } = shown.ev
    const retryable = shown.ev.retryable ?? 0
    const lines = [
      retryable > 0
        ? t('quit.logbook.retry.body', { count: retryable, reason: shown.ev.retryReason ?? '' })
        : null,
      refused > 0 ? t('quit.logbook.refused.body', { count: refused, reason: reason ?? '' }) : null,
      pending > 0 ? t('quit.logbook.slow.body', { count: pending }) : null,
    ].filter((line): line is string => line !== null)
    title = pending > 0 ? t('quit.logbook.slow.title') : t('quit.logbook.refused.title')
    description = lines[0]
    more = lines.slice(1).map((line) => (
      <p className="ui-dialog-desc" key={line}>
        {line}
      </p>
    ))
    actions = (
      <>
        {stop}
        {pending + retryable > 0 && (
          <button
            type="button"
            className="settings-refresh"
            disabled={shown.answered}
            onClick={() => answer(true)}
            autoFocus
          >
            {t('quit.logbook.keepTrying')}
          </button>
        )}
        <button
          type="button"
          className="settings-save danger"
          disabled={shown.answered}
          onClick={() => answer(false)}
        >
          {t('quit.logbook.quitWithout', { count: pending + retryable + refused })}
        </button>
      </>
    )
  }

  return (
    <Dialog
      open={shown !== null}
      // Not dismissable: Escape and a click outside are the station's question left unanswered,
      // and the station keeps the window until it has an answer or the logbook is saved.
      onOpenChange={() => {}}
      title={title}
      description={description}
    >
      {more}
      {actions && <div className="confirm-actions">{actions}</div>}
    </Dialog>
  )
}
