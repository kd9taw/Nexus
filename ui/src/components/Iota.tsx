// IOTA ON SCREEN — an island group's name beside its reference, the one-line consent to download the
// list the names come from, and the chip that OFFERS a reference to a contact being logged.
//
// The names are IOTA Ltd's (© RSGB and © Islands On The Air Ltd): every name shown carries a link
// to IOTA's site, and the list is on this machine only because the operator said yes to the
// consent line (`features/iotaGroups`). Without it a reference shows bare, as it always did.
//
// An offer is never a write: the chip says where the reference came from (the spot's comment, or the
// callbook) and the operator takes it or not. The log-time half that decides what to offer is the
// station's (`get_iota_offer`, `tempo_core::logbook::offer::iota_offer`); this is what it shows, and
// `useIotaOffer` is what a log form keeps of it until the contact is logged.
import { useEffect, useState, useSyncExternalStore } from 'react'
import { t } from '../i18n'
import { withErrorToast } from '../toast'
import { getIotaOffer, type IotaOfferData } from '../api'
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

export type { IotaOfferData }

/**
 * The offer chip: "IOTA EU-131 · Veneto Region group — from the spot  [Use] [×]". Use hands the
 * reference to the log (pressed again, takes it back); × leaves the field as it is. A reference
 * taken from an offer, either source, stays out of every upload until the operator confirms it on
 * the contact card, and the chip says so.
 */
export function IotaOffer({
  offer,
  accepted = false,
  onAccept,
  onDismiss,
}: {
  offer: IotaOfferData
  /** The operator pressed Use: the reference goes into the log with the contact. */
  accepted?: boolean
  onAccept: (reference: string) => void
  onDismiss: () => void
}) {
  useIotaGroups()
  const from = offer.source === 'spot' ? t('iota.offer.fromSpot') : t('iota.offer.fromCallbook')
  return (
    <div className="iota-offer" role="group" aria-label={t('iota.offer.aria', { ref: offer.reference })}>
      <span className="need-chip need-iota">{t('iota.offer.chip', { ref: offer.reference })}</span>
      <IotaGroupName reference={offer.reference} />
      <span className="iota-offer-from" title={t('iota.offer.spotHeld')}>
        {from}
      </span>
      <button
        type="button"
        className="settings-test-btn"
        aria-pressed={accepted}
        title={t('iota.offer.spotHeld')}
        onClick={() => onAccept(offer.reference)}
      >
        {t('iota.offer.use')}
      </button>
      <button type="button" className="settings-test-btn" aria-label={t('iota.offer.dismiss')} title={t('iota.offer.dismiss')} onClick={onDismiss}>
        ×
      </button>
    </div>
  )
}

/** A form's offer before any call: nothing asked, taken or dismissed. */
const FRESH_OFFER = { call: '', offer: null, taken: false, dismissed: false }

/** What a log form keeps of the IOTA offer for the call it holds ([`useIotaOffer`]). */
export interface IotaOfferState {
  /** The offer to show, or null (none, dismissed, or the form holds a reference of its own). */
  offer: IotaOfferData | null
  /** The offer the operator took with Use: what the form logs with the contact. */
  taken: IotaOfferData | null
  /** Use: take the offer, or, taken, give it back. */
  toggle: () => void
  /** ×: no offer for this call. */
  dismiss: () => void
  /** The contact is logged: what was taken or dismissed was for it, and the next starts over. */
  reset: () => void
}

/**
 * The IOTA offer for the contact a log form holds for `call` (`get_iota_offer`; operator decision,
 * 2026-10-27: offered from the spot comment and the callbook, the operator takes a chip, nothing is
 * stamped by itself). Asked again when the call changes and when `asked` does (a callbook lookup
 * that answered), never while the form holds a reference of its own (`held`: a typed one is never
 * replaced) and never while `off`. A new call starts over: nothing taken, nothing dismissed, and so
 * does the next contact once the form logs one ([`IotaOfferState.reset`]), even with the same call:
 * a Use press is for one contact.
 */
export function useIotaOffer(call: string, held?: string | null, asked?: unknown, off = false): IotaOfferState {
  const key = call.trim().toUpperCase()
  const holding = (held ?? '').trim() !== ''
  const [kept, setKept] = useState<{ call: string; offer: IotaOfferData | null; taken: boolean; dismissed: boolean }>(FRESH_OFFER)
  useEffect(() => {
    if (off || holding || key.length < 3) return
    let live = true
    const id = window.setTimeout(() => {
      // Inside the chain, so a station or a window with no such command (nothing to call at all)
      // lands in the catch below rather than throwing out of the timer.
      Promise.resolve()
        .then(() => getIotaOffer(key))
        .then((offer) => {
          if (!live) return
          setKept((k) =>
            k.call !== key
              ? { call: key, offer: offer ?? null, taken: false, dismissed: false }
              : // A reference taken stays what it was, even if its spot ages out meanwhile.
                k.taken
                ? k
                : { ...k, offer: offer ?? null },
          )
        })
        .catch(() => {}) // a station without the command offers nothing
    }, 250)
    return () => {
      live = false
      window.clearTimeout(id)
    }
  }, [key, holding, asked, off])
  const mine = !off && !holding && kept.call === key && !kept.dismissed
  const offer = mine ? kept.offer : null
  return {
    offer,
    taken: offer && kept.taken ? offer : null,
    toggle: () => setKept((k) => ({ ...k, taken: !k.taken })),
    dismiss: () => setKept((k) => ({ ...k, taken: false, dismissed: true })),
    reset: () => setKept(FRESH_OFFER),
  }
}

/** The offer chip for a log form's [`useIotaOffer`], or nothing. */
export function IotaOfferChip({ state }: { state: IotaOfferState }) {
  if (!state.offer) return null
  return <IotaOffer offer={state.offer} accepted={!!state.taken} onAccept={state.toggle} onDismiss={state.dismiss} />
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
