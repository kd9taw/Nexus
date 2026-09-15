// "Spot me": the operator's own activation, posted to pota.app AND the DX cluster in one press.
// Shared by the desktop's POTA board and the Remote page's, which send the same press two ways
// (a Tauri command, a station log change) and get back the same two results.
import { confirmDialog } from './confirm'
import { pushToast } from './toast'
import { t } from './i18n'
// The two result lists live in their own leaf module: the protocol validator that reads them is
// also compiled by the Cloudflare Worker, which has neither DOM nor JSX. See selfSpotReport.ts.
import type { SelfSpotReport } from './selfSpotReport'

export { POTA_SPOT_RESULTS, CLUSTER_SPOT_RESULTS } from './selfSpotReport'
export type { SelfSpotReport } from './selfSpotReport'

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

/** A public post: ask on every press, never remember the answer, and show exactly the park and
 *  dial that will be sent. */
export function confirmSelfSpot(reference: string, dialHz: number): Promise<boolean> {
  return confirmDialog({
    title: t('ota.selfSpot.confirm.title'),
    confirmLabel: t('ota.selfSpot.confirm.post'),
    body: t('ota.selfSpot.confirm.body', { reference, freq: (dialHz / 1e6).toFixed(4) }),
  })
}

/** One message when both targets took the spot. Otherwise one per target, cluster first, so a
 *  failure of one never hides what the other did. */
export function announceSelfSpot(report: SelfSpotReport): void {
  if (report.pota === 'posted' && report.cluster === 'queued') {
    pushToast(t('ota.selfSpot.both'), 'success')
    return
  }
  const say = (message: string, ok: boolean, notice: boolean) =>
    ok ? pushToast(message, 'success') : pushToast(message, notice ? 'info' : 'error', 8000)
  say(clusterMessage(report.cluster), report.cluster === 'queued', report.cluster === 'throttled')
  say(potaMessage(report.pota), report.pota === 'posted',
    report.pota === 'throttled' || report.pota === 'notPark' || report.pota === 'unknownMode')
}
