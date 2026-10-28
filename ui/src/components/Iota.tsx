// IOTA ON SCREEN — an island group's name beside its reference, the one-line consent to download the
// list the names come from, and the chip that OFFERS a reference to a contact being logged.
//
// The names are IOTA Ltd's (© RSGB and © Islands On The Air Ltd): every name shown carries a link
// to IOTA's site, and the list is on this machine only because the operator said yes to the
// consent line (`features/iotaGroups`). Without it a reference shows bare, as it always did.
//
// An offer is never a write: the chip says where the reference came from (the spot's comment, or the
// callbook) and the operator takes it or not. The log-time half that decides what to offer is the
// engine's (`tempo_core::logbook::offer::iota_offer`); this is what it shows.
import { useEffect, useState, useSyncExternalStore } from 'react'
import { t } from '../i18n'
import { withErrorToast } from '../toast'
import {
  IOTA_SHAPE,
  IOTA_SITE,
  answerIotaConsent,
  iotaKnown,
  iotaGroupsVersion,
  iotaListHeld,
  iotaListStatus,
  iotaName,
  loadIotaGroups,
  subscribeIotaGroups,
} from '../features/iotaGroups'

/** Read the station's list once and re-render when it changes. */
export function useIotaGroups(): number {
  useEffect(() => {
    void loadIotaGroups()
  }, [])
  return useSyncExternalStore(subscribeIotaGroups, iotaGroupsVersion)
}

/** The credit every island name carries: IOTA Ltd's, linking to its site. */
function IotaCredit() {
  return (
    <a className="iota-credit" href={IOTA_SITE} target="_blank" rel="noreferrer" title={t('iota.credit.title')}>
      {t('iota.credit')}
    </a>
  )
}

/** The group's name for `reference`, with IOTA Ltd's credit; nothing without the list. */
export function IotaGroupName({ reference }: { reference: string | null | undefined }) {
  useIotaGroups()
  const name = iotaName(reference)
  if (!name) return null
  return (
    <span className="iota-name">
      <span className="iota-name-text" title={name}>
        {name}
      </span>{' '}
      <IotaCredit />
    </span>
  )
}

/**
 * The one line that asks whether to download the island-group list: what it is, whose it is, where
 * it comes from. Shown only while the operator has never answered; Settings ▸ Logbook ▸ IOTA island
 * list changes the answer later.
 */
export function IotaListConsent() {
  useIotaGroups()
  const [busy, setBusy] = useState(false)
  const st = iotaListStatus()
  if (!st || st.consent !== null) return null
  const answer = (yes: boolean) => {
    setBusy(true)
    void withErrorToast(() => answerIotaConsent(yes), t('iota.consent.failed')).finally(() => setBusy(false))
  }
  return (
    <div className="iota-consent" role="group" aria-label={t('iota.consent.aria')}>
      <span className="iota-consent-line">{t('iota.consent.line')}</span>
      <button type="button" className="settings-test-btn" disabled={busy} onClick={() => answer(true)}>
        {t('iota.consent.yes')}
      </button>
      <button type="button" className="settings-test-btn" disabled={busy} onClick={() => answer(false)}>
        {t('iota.consent.no')}
      </button>
    </div>
  )
}

/**
 * Beside a reference: its group's name when the list is held and has it, a warning when the list is
 * held and does not ("EU-999" is no island group), and without the list the consent line, once.
 * A reference of the wrong shape says nothing here: the box's own check refuses it.
 */
export function IotaReferenceNote({ reference }: { reference: string | null | undefined }) {
  useIotaGroups()
  const r = (reference ?? '').trim().toUpperCase()
  if (!IOTA_SHAPE.test(r)) return null
  if (!iotaListHeld()) return <IotaListConsent />
  if (!iotaKnown(r)) {
    return (
      <span className="iota-unknown" role="note">
        {t('iota.notOnList', { ref: r })} <IotaCredit />
      </span>
    )
  }
  return <IotaGroupName reference={r} />
}

/** What the engine offers a contact being logged (`tempo_core::logbook::offer::IotaOffer`). */
export interface IotaOfferData {
  reference: string
  source: 'spot' | 'callbook'
}

/**
 * The offer chip: "IOTA EU-131 · Veneto Region group — from the spot  [Use] [×]". Use hands the
 * reference to the log; × leaves the field as it is. A spot's reference stays out of every upload
 * until the operator confirms it on the contact card, and the chip says so.
 */
export function IotaOffer({
  offer,
  onAccept,
  onDismiss,
}: {
  offer: IotaOfferData
  onAccept: (reference: string) => void
  onDismiss: () => void
}) {
  useIotaGroups()
  const from = offer.source === 'spot' ? t('iota.offer.fromSpot') : t('iota.offer.fromCallbook')
  return (
    <div className="iota-offer" role="group" aria-label={t('iota.offer.aria', { ref: offer.reference })}>
      <span className="need-chip need-iota">{t('iota.offer.chip', { ref: offer.reference })}</span>
      <IotaGroupName reference={offer.reference} />
      <span className="iota-offer-from" title={offer.source === 'spot' ? t('iota.offer.spotHeld') : undefined}>
        {from}
      </span>
      <button type="button" className="settings-test-btn" onClick={() => onAccept(offer.reference)}>
        {t('iota.offer.use')}
      </button>
      <button type="button" className="settings-test-btn" aria-label={t('iota.offer.dismiss')} title={t('iota.offer.dismiss')} onClick={onDismiss}>
        ×
      </button>
    </div>
  )
}

/**
 * Settings ▸ Logbook ▸ IOTA island list: the same yes or no as the consent line, changeable at any
 * time, with what the station holds. Off deletes the copy held.
 */
export function IotaListSettings() {
  useIotaGroups()
  const [busy, setBusy] = useState(false)
  const st = iotaListStatus()
  const on = st?.consent === true
  const flip = () => {
    setBusy(true)
    void withErrorToast(() => answerIotaConsent(!on), t('iota.consent.failed')).finally(() => setBusy(false))
  }
  return (
    <div className="settings-field">
      <label className="settings-field">
        <span className="settings-label">{t('settings.iotaList.consent.label')}</span>
        <button type="button" role="switch" aria-checked={on} className={`toggle${on ? ' on' : ''}`} disabled={busy || !st} onClick={flip}>
          <span className="toggle-knob" />
        </button>
      </label>
      <span className="settings-hint">{t('settings.iotaList.consent.hint')}</span>
      <span className="settings-hint">
        {st && st.count > 0
          ? t('settings.iotaList.status', {
              count: st.count.toLocaleString(),
              date: new Date(st.fetchedAt * 1000).toISOString().slice(0, 10),
            })
          : t('settings.iotaList.empty')}{' '}
        <IotaCredit />
      </span>
      {st?.lastError && on && <span className="settings-hint">{t('settings.iotaList.failed', { detail: st.lastError })}</span>}
    </div>
  )
}
