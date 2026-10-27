// "Spot me": the operator's own activation, posted to pota.app AND the DX cluster in one press.
// Shared by the desktop's POTA board and the Remote page's, which send the same press two ways
// (a Tauri command, a station log change) and get back the same two results.
import { confirmDialog } from './confirm'
import { pushToast } from './toast'
import { t } from './i18n'
// The two result lists live in their own leaf module: the protocol validator that reads them is
// also compiled by the Cloudflare Worker, which has neither DOM nor JSX. See selfSpotReport.ts.
import type { ActivationSpotReport, SelfSpotReport } from './selfSpotReport'

export { POTA_SPOT_RESULTS, CLUSTER_SPOT_RESULTS } from './selfSpotReport'
export type { ActivationSpotReport, SelfSpotReport } from './selfSpotReport'

function potaMessage(result: SelfSpotReport['pota']): string {
  switch (result) {
    case 'posted': return t('ota.selfSpot.pota.posted')
    case 'loginRequired': return t('ota.selfSpot.pota.loginRequired')
    case 'failed': return t('ota.selfSpot.pota.failed')
    case 'throttled': return t('ota.selfSpot.pota.throttled')
    case 'notPark': return t('ota.selfSpot.pota.notPark')
    case 'unknownMode': return t('ota.selfSpot.pota.unknownMode')
    case 'invalid': return t('ota.selfSpot.pota.invalid')
  }
}
function clusterMessage(result: SelfSpotReport['cluster']): string {
  switch (result) {
    case 'queued': return t('ota.selfSpot.cluster.queued')
    case 'unavailable': return t('ota.selfSpot.cluster.unavailable')
    case 'failed': return t('ota.selfSpot.cluster.failed')
    case 'throttled': return t('ota.selfSpot.cluster.throttled')
    case 'invalid': return t('ota.selfSpot.cluster.invalid')
  }
}

/** What a spot's comment says when the operator leaves it blank: on the air. */
export const DEFAULT_SPOT_COMMENT = 'QRV'
/** What a spot's comment says to end the activation's spot. */
export const QRT_COMMENT = 'QRT'
/** The longest comment the station takes (`self_spot::COMMENT_MAX`). */
export const SPOT_COMMENT_MAX = 30

/** The comment as the station sends it: upper case, one space between words, QRV when blank. */
export function spotComment(raw: string): string {
  return raw.trim().split(/ +/).join(' ').toUpperCase() || DEFAULT_SPOT_COMMENT
}

/** A public post: ask on every press, never remember the answer, and show exactly the parks, dial
 *  and comment that will be sent. `reference` is the activation's, a two-fer's parks as a comma
 *  list: one confirm names them all, and each is then spotted on its own. */
export function confirmSelfSpot(reference: string, dialHz: number, comment: string): Promise<boolean> {
  const parks = reference.split(',').map((p) => p.trim()).filter(Boolean).join(', ')
  return confirmDialog({
    title: t('ota.selfSpot.confirm.title'),
    confirmLabel: t('ota.selfSpot.confirm.post'),
    body: t('ota.selfSpot.confirm.body', { reference: parks, freq: (dialHz / 1e6).toFixed(4), comment: spotComment(comment) }),
  })
}

// RE-SPOT ON A QSY: A PROMPT, NEVER A POST. Hunters find an activator by the spot's frequency, so
// one who moves needs a new spot — but a spot is a public post, and only the operator's press
// sends one. Once the dial has sat away from the spotted frequency for RESPOT_DWELL_MS, by at least
// a filter's width for the mode, the board offers one button; that button is Spot me, confirm and
// repeat limit and all.

/** How far the dial must move from the spotted frequency to be a new one: a phone signal is about
 *  3 kHz wide, a CW or digital one well under 1 kHz. */
export function respotThresholdHz(operatingMode: string | undefined): number {
  return operatingMode === 'phone' ? 3000 : 1000
}

/** How long the dial must stay on the new frequency before the prompt: tuning across the band is
 *  not a move. */
export const RESPOT_DWELL_MS = 20_000

/** Where the dial is against the last spot. `awayHz` is where it settled off the spotted
 *  frequency, since `awaySince`; `promptedHz` is the frequency already offered, so each move is
 *  offered once. */
export interface RespotWatch {
  spotHz: number
  awayHz: number | null
  awaySince: number
  promptedHz: number | null
}

export function respotWatch(spotHz: number): RespotWatch {
  return { spotHz, awayHz: null, awaySince: 0, promptedHz: null }
}

/** One look at the dial: the watch to keep, and whether to offer a re-spot now. Pure. */
export function respotStep(
  watch: RespotWatch,
  dialHz: number,
  nowMs: number,
  thresholdHz: number,
): { watch: RespotWatch; prompt: boolean } {
  if (Math.abs(dialHz - watch.spotHz) < thresholdHz) {
    return { watch: respotWatch(watch.spotHz), prompt: false }
  }
  // A new resting place restarts the dwell; a nudge within the threshold does not.
  const settled = watch.awayHz !== null && Math.abs(dialHz - watch.awayHz) < thresholdHz
  const next = settled ? watch : { ...watch, awayHz: dialHz, awaySince: nowMs }
  const due = next.awayHz !== null && nowMs - next.awaySince >= RESPOT_DWELL_MS && next.promptedHz !== next.awayHz
  return { watch: due ? { ...next, promptedHz: next.awayHz } : next, prompt: due }
}

/** One message when every target took the spot. Otherwise one for the cluster, then one per park
 *  (named when there are two or more), so a failure of one never hides what another did. */
export function announceSelfSpot(report: ActivationSpotReport): void {
  if (report.parks.every((p) => p.pota === 'posted') && report.cluster === 'queued') {
    pushToast(t('ota.selfSpot.both'), 'success')
    return
  }
  const say = (message: string, ok: boolean, notice: boolean) =>
    ok ? pushToast(message, 'success') : pushToast(message, notice ? 'info' : 'error', 8000)
  say(clusterMessage(report.cluster), report.cluster === 'queued', report.cluster === 'throttled')
  const named = report.parks.length > 1
  for (const { reference, pota } of report.parks) {
    const message = potaMessage(pota)
    say(named ? t('ota.selfSpot.park', { reference, message }) : message, pota === 'posted',
      pota === 'throttled' || pota === 'notPark' || pota === 'unknownMode')
  }
}
