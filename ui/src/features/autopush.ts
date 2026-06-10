// Shared auto-upload for a freshly-logged QSO (QRZ Logbook / ClubLog / eQSL).
// ONE implementation for every log path — the manual Logbook form, the cockpit's
// "Log QSO" button, and the prompt-to-log confirm — so the Settings auto-upload
// toggles can never be silently dead for one path (they were: only the manual
// form pushed; cockpit-logged QSOs never reached any service).
import type { LoggedQso } from '../types'
import { clublogPushQso, eqslPushQso, qrzPushQso } from '../api'
import { pushToast, withErrorToast } from '../toast'

export interface AutoPushFlags {
  qrz: boolean
  clublog: boolean
  eqsl: boolean
}

/** Best-effort pushes (the QSO is already logged locally); each outcome toasts. */
export async function autoPushQso(record: LoggedQso, flags: AutoPushFlags): Promise<void> {
  if (flags.qrz) {
    const r = await withErrorToast(() => qrzPushQso(record), 'QRZ upload failed')
    if (r) {
      const msg =
        r.result === 'ok'
          ? `Uploaded ${record.call} to QRZ`
          : r.result === 'replace'
            ? `Updated existing ${record.call} in your QRZ logbook`
            : r.result === 'duplicate'
              ? `${record.call} already in your QRZ logbook`
              : r.result === 'authFail'
                ? 'QRZ Logbook key invalid — check Settings'
                : `QRZ upload: ${r.reason ?? 'failed'}`
      pushToast(msg, r.result === 'fail' || r.result === 'authFail' ? 'error' : 'success')
    }
  }
  if (flags.clublog) {
    const c = await withErrorToast(() => clublogPushQso(record), 'ClubLog upload failed')
    if (c) {
      const msg =
        c.result === 'ok' || c.result === 'modified'
          ? `Uploaded ${record.call} to ClubLog`
          : c.result === 'duplicate'
            ? `${record.call} already on ClubLog`
            : c.result === 'authFail'
              ? 'ClubLog credentials invalid — auto-upload paused; fix in Settings'
              : c.result === 'serverError'
                ? 'ClubLog busy — try again later'
                : `ClubLog: ${c.message ?? 'rejected'}`
      const ok = c.result === 'ok' || c.result === 'modified' || c.result === 'duplicate'
      pushToast(msg, ok ? 'success' : 'error')
    }
  }
  if (flags.eqsl) {
    const e = await withErrorToast(() => eqslPushQso(record), 'eQSL upload failed')
    if (e) {
      const msg =
        e.outcome === 'accepted'
          ? `Uploaded ${record.call} to eQSL`
          : e.outcome === 'duplicate'
            ? `${record.call} already on eQSL`
            : e.outcome === 'authfail'
              ? 'eQSL login invalid — check Settings'
              : e.outcome === 'retry'
                ? 'eQSL unavailable — try again later'
                : `eQSL upload rejected${e.detail ? `: ${e.detail}` : ''}`
      const ok = e.outcome === 'accepted' || e.outcome === 'duplicate'
      pushToast(msg, ok ? 'success' : 'error')
    }
  }
}
