// IOTA ON SCREEN — an island group's name beside its reference, the Settings switch for the list the
// names come from, and the box a log form fills with the reference a spot or the callbook names.
//
// The names are IOTA Ltd's (© RSGB and © Islands On The Air Ltd), shipped with Nexus by its
// permission: every name shown carries IOTA Ltd's credit and a link to its site, as a courtesy. With
// the list switched off (`features/iotaGroups`) a reference shows bare, as it did before the list.
//
// The reference is logged automatically (operator ruling, 2026-11-10): a log form puts it in its IOTA
// box, says where it came from (the spot's comment, or the callbook), and logs what the box holds, so
// the operator can change or clear it first. What to fill is the station's (`get_iota_offer`,
// `tempo_core::logbook::offer::iota_offer`); `useIotaPrefill` is what a form keeps of it.
//
// The ISLAND within the group (ADIF's IOTA_ISLAND_ID) is only ever the operator's pick (operator
// ruling, 2026-11-10): no spot or callbook names one. With a group in the box, `IotaIslandPick` lists
// that group's islands from IOTA's islands list (shipped with the group list under the same written
// permission); nothing is picked until the operator picks, and a new group lets go of the pick.
import { useEffect, useRef, useState, useSyncExternalStore } from 'react'
import { t } from '../i18n'
import { withErrorToast } from '../toast'
import { getIotaOffer, type IotaIsland, type IotaOfferData } from '../api'
import {
  IOTA_SHAPE,
  IOTA_SITE,
  iotaKnown,
  iotaGroupsVersion,
  iotaListHeld,
  iotaListStatus,
  iotaName,
  loadIotaGroups,
  loadIotaIslands,
  normalizeIota,
  setIotaListOn,
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
 * Beside a reference: its group's name when the list is in use and has it, a warning when it does
 * not ("EU-999" is no island group), and nothing with the list off. A reference of the wrong shape
 * says nothing here: the box's own check refuses it.
 */
export function IotaReferenceNote({ reference }: { reference: string | null | undefined }) {
  useIotaGroups()
  const r = (reference ?? '').trim().toUpperCase()
  if (!IOTA_SHAPE.test(r)) return null
  if (!iotaListHeld()) return null
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

/** What a log form keeps of the IOTA reference it filled its box with ([`useIotaPrefill`]). */
export interface IotaPrefillState {
  /** What the station names for the call (a spot's comment, or the callbook's), or null. */
  offer: IotaOfferData | null
  /** `offer` while the box still holds exactly its reference: what the form logs it as, so the
   *  log marks it by its source. A reference the operator changed is theirs. */
  taken: IotaOfferData | null
  /** The operator typed in the box: what they typed stays, and nothing fills it again. */
  edit: (value: string) => void
  /** ×: the box emptied, and nothing fills it again for this contact. */
  clear: () => void
  /** The contact is logged or let go of: the next one starts over, even with the same call. */
  reset: () => void
}

/**
 * Fill a log form's IOTA box (`value`, set through `setValue`) with the reference the station names
 * for `call` (`get_iota_offer`; operator ruling, 2026-11-10: logged automatically, shown first so it
 * can be changed or cleared). Asked again when the call changes and when `asked` does (a callbook
 * lookup that answered); never while `off`. Fills only an empty box the operator has not touched for
 * this contact, and a reference it filled goes again when the call changes before it is touched.
 * `seededFill` is a reference the box opened with that the station filled before the form was shown
 * (the popup's held contact, marked as Nexus's fill): it is treated as this hook's own fill, so a
 * corrected call empties it and takes its own. A seeded reference the operator typed is theirs.
 */
export function useIotaPrefill(
  call: string,
  value: string,
  setValue: (value: string) => void,
  asked?: unknown,
  off = false,
  seededFill: string | null = null,
): IotaPrefillState {
  const key = call.trim().toUpperCase()
  const [kept, setKept] = useState<{ call: string; offer: IotaOfferData | null }>({ call: '', offer: null })
  // What this contact's box holds by the operator's hand (`touched`) or by this fill (`filled`).
  const touched = useRef(false)
  const filled = useRef<string | null>(seededFill ? seededFill.trim().toUpperCase() : null)
  const latest = useRef({ value, setValue })
  latest.current = { value, setValue }
  const keyRef = useRef(key)
  // A new call: what this hook put in the box was for the old one.
  useEffect(() => {
    if (keyRef.current === key) return
    keyRef.current = key
    const { value: v, setValue: set } = latest.current
    if (!touched.current && filled.current !== null && (v ?? '').trim().toUpperCase() === filled.current) set('')
    touched.current = false
    filled.current = null
  }, [key])
  useEffect(() => {
    if (off || key.length < 3) return
    let live = true
    const id = window.setTimeout(() => {
      // Inside the chain, so a station or a window with no such command (nothing to call at all)
      // lands in the catch below rather than throwing out of the timer.
      Promise.resolve()
        .then(() => getIotaOffer(key))
        .then((answer) => {
          if (!live) return
          // Only an answer that IS an offer: a reference by its shape and a known source. Anything
          // else (a station that answers `{}`, an older one, a stub) fills nothing and breaks nothing.
          const offer = asOffer(answer)
          setKept({ call: key, offer })
          const { value: v, setValue: set } = latest.current
          if (offer && !touched.current && (v ?? '').trim() === '') {
            filled.current = offer.reference
            set(offer.reference)
          }
        })
        .catch(() => {}) // a station without the command fills nothing
    }, 250)
    return () => {
      live = false
      window.clearTimeout(id)
    }
  }, [key, asked, off])
  const offer = !off && kept.call === key ? kept.offer : null
  return {
    offer,
    taken: offer && normalizeIota(value) === offer.reference ? offer : null,
    edit: (v: string) => {
      touched.current = true
      latest.current.setValue(v)
    },
    clear: () => {
      touched.current = true
      latest.current.setValue('')
    },
    reset: () => {
      touched.current = false
      filled.current = null
    },
  }
}

/** The islands of the group `reference` the station holds, for a picker; empty until they are read,
 *  and for no group, a group the list lacks, or no islands list. */
export function useIotaIslands(reference: string | null | undefined): IotaIsland[] {
  const version = useIotaGroups()
  const r = normalizeIota(reference) ?? ''
  const [held, setHeld] = useState<{ ref: string; list: IotaIsland[] }>({ ref: '', list: [] })
  useEffect(() => {
    if (!r) return
    let live = true
    void loadIotaIslands(r).then((list) => {
      if (live) setHeld({ ref: r, list })
    })
    return () => {
      live = false
    }
  }, [r, version])
  return r && held.ref === r ? held.list : []
}

/** The island a form has picked for the group its IOTA box holds (`reference`): none until the
 *  operator picks one, and none again whenever the group changes, since an island is one group's. */
export function useIotaIslandPick(reference: string | null | undefined): [number | null, (id: number | null) => void] {
  const r = normalizeIota(reference) ?? ''
  const [pick, setPick] = useState<{ ref: string; id: number | null }>({ ref: r, id: null })
  // A pick is forgotten with its group, not kept for a return to it.
  useEffect(() => {
    setPick((p) => (p.ref === r ? p : { ref: r, id: null }))
  }, [r])
  const id = pick.ref === r ? pick.id : null
  return [id, (next) => setPick({ ref: r, id: next })]
}

/** An island's line in the picker: its name, and IOTA's note on it when it has one. */
const islandLabel = (i: IotaIsland) => (i.comment ? `${i.name} (${i.comment})` : i.name)

/**
 * The island picker for the group `reference`: "Island [— none —|Brownsea (in England)|…]", listing
 * only that group's islands, shown while the station has any for it. `value` is the island picked
 * (null: none); `onChange` gets the pick or null.
 */
export function IotaIslandPick({
  reference,
  value,
  onChange,
  disabled = false,
  className,
}: {
  reference: string | null | undefined
  value: number | null
  onChange: (id: number | null) => void
  disabled?: boolean
  /** The host form's field class, where the picker is a field of its own. */
  className?: string
}) {
  const list = useIotaIslands(reference)
  if (list.length === 0) return null
  const picked = list.find((i) => i.id === value)
  return (
    <label className={className ? `iota-island ${className}` : 'iota-island'}>
      <span>{t('iota.island.label')}</span>
      <select
        className="settings-input iota-island-pick"
        value={picked ? String(picked.id) : ''}
        title={picked ? islandLabel(picked) : t('iota.island.none')}
        disabled={disabled}
        onChange={(e) => onChange(e.target.value === '' ? null : Number(e.target.value))}
      >
        <option value="">{t('iota.island.none')}</option>
        {list.map((i) => (
          <option key={i.id} value={String(i.id)}>
            {islandLabel(i)}
          </option>
        ))}
      </select>
    </label>
  )
}

/** Where the box's reference came from, while it is the station's fill: "from the spot". */
export function IotaPrefillSource({ state }: { state: IotaPrefillState }) {
  if (!state.taken) return null
  return (
    <span className="iota-offer-from">
      {state.taken.source === 'spot' ? t('iota.offer.fromSpot') : t('iota.offer.fromCallbook')}
    </span>
  )
}

/**
 * The IOTA box of a log form with no box of its own (the log strip, the confirm-before-log popup):
 * "IOTA [EU-131] · Veneto Region group — from the spot  [×]", shown while it holds a reference or
 * the station names one. The reference is logged with the contact as the box holds it; × empties it.
 */
export function IotaPrefillField({
  state,
  value: given,
  island,
}: {
  state: IotaPrefillState
  value: string | null | undefined
  /** The form's island pick for the reference in the box ([`useIotaIslandPick`]). */
  island?: { value: number | null; onChange: (id: number | null) => void }
}) {
  useIotaGroups()
  const value = given ?? ''
  if (!state.offer && value.trim() === '') return null
  const r = value.trim().toUpperCase()
  const invalid = iotaInvalid(value)
  return (
    <div className="iota-offer" role="group" aria-label={t('iota.prefill.aria')}>
      <span className="need-chip need-iota" aria-hidden="true">
        {t('iota.prefill.label')}
      </span>
      <input
        className="settings-input mono"
        value={value}
        onChange={(e) => state.edit(e.target.value.toUpperCase())}
        maxLength={6}
        size={6}
        autoComplete="off"
        spellCheck={false}
        aria-label={t('iota.prefill.label')}
        aria-invalid={invalid}
        title={t('iota.prefill.hint')}
      />
      <IotaGroupName reference={normalizeIota(r) ?? r} />
      <IotaPrefillSource state={state} />
      {invalid && (
        <span className="iota-unknown" role="alert">
          {t('logbook.form.iotaInvalid')}
        </span>
      )}
      {r !== '' && (
        <button type="button" className="settings-test-btn" aria-label={t('iota.offer.dismiss')} title={t('iota.offer.dismiss')} onClick={state.clear}>
          ×
        </button>
      )}
      {island && <IotaIslandPick reference={r} value={island.value} onChange={island.onChange} />}
    </div>
  )
}

/** The box's reference as the log takes it, in its one spelling (`normalizeIota`: "eu-5" is
 *  EU-005); none for an empty box. A box that holds something else is [`iotaInvalid`], and the form
 *  says so and does not log, never logging it as nothing. */
export function iotaToLog(value: string | null | undefined): string | null {
  return normalizeIota(value)
}

/** The box holds something that is not a reference in any spelling. */
export function iotaInvalid(value: string | null | undefined): boolean {
  return (value ?? '').trim() !== '' && normalizeIota(value) === null
}

/** `answer` when it is an offer: a reference by its shape and a source the log knows; else null. */
function asOffer(answer: unknown): IotaOfferData | null {
  const o = answer as Partial<IotaOfferData> | null | undefined
  if (!o || typeof o !== 'object' || typeof o.reference !== 'string') return null
  const reference = normalizeIota(o.reference)
  if (!reference || (o.source !== 'spot' && o.source !== 'callbook')) return null
  return { reference, source: o.source }
}

/**
 * The Logbook's IOTA cell: the contact's reference and, with the list, its group's name beside it
 * ("EU-005 Great Britain"); the name's credit is on the cell's hover. A dash for none.
 */
export function IotaCell({ reference }: { reference: string | null | undefined }) {
  useIotaGroups()
  const r = (reference ?? '').trim()
  const name = iotaName(r)
  const title = name ? `${r} ${name} — ${t('iota.credit.title')}` : r
  return (
    <span className="log-cell mono" title={title}>
      {r || '—'}
      {name && <span className="iota-cell-name"> {name}</span>}
    </span>
  )
}

/** A Unix time as its UTC day, `2026-10-29`. */
const isoDay = (secs: number) => new Date(secs * 1000).toISOString().slice(0, 10)

/**
 * Settings ▸ Logbook ▸ IOTA island list: on (the default) shows names from the copy that shipped with
 * Nexus, or a newer one the monthly check fetched; off shows none, stops the check and deletes any
 * downloaded copy.
 */
export function IotaListSettings() {
  useIotaGroups()
  const [busy, setBusy] = useState(false)
  const st = iotaListStatus()
  const on = st?.enabled === true
  const flip = () => {
    setBusy(true)
    void withErrorToast(() => setIotaListOn(!on), t('iota.list.failed')).finally(() => setBusy(false))
  }
  return (
    <div className="settings-field">
      <label className="settings-field">
        <span className="settings-label">{t('settings.iotaList.label')}</span>
        <button type="button" role="switch" aria-checked={on} className={`toggle${on ? ' on' : ''}`} disabled={busy || !st} onClick={flip}>
          <span className="toggle-knob" />
        </button>
      </label>
      <span className="settings-hint">{t('settings.iotaList.hint')}</span>
      <span className="settings-hint">
        {!st || !on
          ? t('settings.iotaList.empty')
          : st.count === 0
            ? t('settings.iotaList.none')
            : st.bundled
              ? t('settings.iotaList.statusShipped', { count: st.count.toLocaleString(), date: isoDay(st.fetchedAt) })
              : t('settings.iotaList.status', { count: st.count.toLocaleString(), date: isoDay(st.fetchedAt) })}{' '}
        <IotaCredit />
      </span>
      {st?.lastError && on && <span className="settings-hint">{t('settings.iotaList.failed', { detail: st.lastError })}</span>}
    </div>
  )
}
