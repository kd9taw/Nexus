// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog; a hardcoded one fails CI. What does NOT come from it: the
// AWARD NAMES in AWARD_NAMES below (DXCC, Honor Roll, WAZ, VUCC, IOTA… are the names of ARRL/CQ
// programmes, not words), the DXCC entity names, band names, mode names, CQ zones and US state
// codes the tables print (data), and the achievement titles/details and diagnosis explanations,
// which the backend writes. See the invariant-token rule in `i18n/index.ts`.

import { Fragment, useEffect, useRef, useState } from 'react'
import { Trophy, CheckCircle2, Radio, Target, Layers, Send, Globe2, Award, Flag, UploadCloud, Grid3x3, Satellite } from 'lucide-react'
import type { AwardBandNeeds, AwardSummary, BandNeed, EntityNeed, DiagnosticsReport, DiagAction, DiagActionBucket, DxccDisagreement, FfmaProgress, QsoDiagnosis, UploadReport, WaeProgress } from '../types'
import {
  getAwards,
  getConfirmationDiagnostics,
  uploadLotwReportByIds,
  qrzPushQso,
  clublogPushQso,
  eqslPushQso,
} from '../api'
import { t } from '../i18n'
import { logSource } from '../features/logSource'
import { utcDate } from '../features/utcLog'
import { askAgainWhileSaving } from '../features/notAnswered'
import { PaneSeam } from './PaneSeam'
import { surfaceGet, surfaceSet } from '../features/windowScope'
import { VIEW_COLUMN_FLOOR, columnShareStyle, parseColumnShare } from '../features/viewColumns'
import { LOTW_SKIP_TOAST_MS, lotwSkipNote } from '../features/lotwSkips'
import { pushToast } from '../toast'
import { StateBlock } from './StateBlock'
import { CustomAwardsPanel, DxccChartPanel, PotaActivatorPanel, WhatToQslPanel } from './AwardsMore'

/** The programmes' own names. Award names are invariant tokens — DXCC is DXCC in every
 * language, and a translated one names no award anybody can apply for. */
const AWARD_NAMES = {
  dxcc: 'DXCC',
  honorRoll: 'Honor Roll',
  challenge: 'Challenge',
  fiveBandDxcc: '5-Band DXCC',
  waz: 'WAZ',
  was: 'WAS',
  vucc: 'VUCC',
  ffma: 'FFMA',
  satVucc: 'Sat VUCC',
  iota: 'IOTA',
  wap: 'WAP',
  wpx: 'WPX',
  wae: 'WAE',
  wwff: 'WWFF',
}

/** Confirmed entities for the basic DXCC award. */
const DXCC_BASIC = 100
/** Confirmed entity×band slots for the DXCC Challenge award. */
const CHALLENGE_AWARD = 1000
/** CQ zones for the Worked All Zones (WAZ) award. */
const WAZ_ZONES = 40
/** US states for the Worked All States (WAS) award. */
const WAS_STATES = 50
/** Grid squares for VUCC (6m/2m — the headline VHF grid award). The Satellite
 * VUCC category needs the same 100; ARRL counts a satellite contact toward it
 * ONLY, so the terrestrial and satellite tallies are separate cards. */
const VUCC_GRIDS = 100
/** Island groups for basic IOTA (Islands On The Air). */
const IOTA_ISLANDS = 100
/** Canadian provinces and territories for WAP. */
const WAP_PROVINCES = 13

type NeedSort = 'entity' | 'bands'

/** The awards NEEDED BY BAND lists, in the order the operator asked for them. */
const BAND_NEED_AWARDS = ['dxcc', 'was', 'waz'] as const
type BandNeedAward = (typeof BAND_NEED_AWARDS)[number]
/** What each award's needs per band count, and what confirms one. */
function bandNeedNote(award: BandNeedAward): string {
  switch (award) {
    case 'was':
      return t('awards.bandNeeds.note.was')
    case 'waz':
      return t('awards.bandNeeds.note.waz')
    default:
      return t('awards.bandNeeds.note.dxcc')
  }
}
type BandNeedKind = 'unworked' | 'unconfirmed'

/** NEEDED BY BAND: for DXCC, WAS or WAZ, each band's count never worked there and worked there but not
 *  confirmed as the award credits it — the station's own count (`AwardSummary.bandNeeds`). Each count
 *  opens its list under the row. */
function BandNeedsPanel({ needs }: { needs: AwardBandNeeds }) {
  const [award, setAward] = useState<BandNeedAward>('dxcc')
  const [open, setOpen] = useState<{ band: string; kind: BandNeedKind } | null>(null)
  const count = (row: BandNeed, kind: BandNeedKind) => {
    const n = row[kind].length
    const isOpen = open?.band === row.band && open.kind === kind
    return (
      <button
        type="button"
        className="aw-bn-count"
        data-kind={kind}
        aria-expanded={isOpen}
        disabled={n === 0}
        title={isOpen ? t('awards.bandNeeds.hide') : t('awards.bandNeeds.show', { count: n, band: row.band })}
        onClick={() => setOpen(isOpen ? null : { band: row.band, kind })}
      >
        {n}
      </button>
    )
  }
  return (
    <div className="aw-panel aw-bandneeds">
      <h3>{t('awards.bandNeeds.head')}</h3>
      <div className="aw-needctl" role="group" aria-label={t('awards.bandNeeds.award.label')}>
        {BAND_NEED_AWARDS.map((a) => (
          <button
            type="button"
            key={a}
            className={`aw-needsort${award === a ? ' active' : ''}`}
            aria-pressed={award === a}
            onClick={() => {
              setAward(a)
              setOpen(null)
            }}
          >
            {AWARD_NAMES[a]}
          </button>
        ))}
      </div>
      <p className="aw-empty">{bandNeedNote(award)}</p>
      <table className="aw-bn-table">
        <thead>
          <tr>
            <th scope="col">{t('awards.bandNeeds.col.band')}</th>
            <th scope="col">{t('awards.bandNeeds.col.unworked')}</th>
            <th scope="col">{t('awards.bandNeeds.col.unconfirmed')}</th>
          </tr>
        </thead>
        <tbody>
          {needs[award].map((row) => (
            <Fragment key={row.band}>
              <tr>
                <th scope="row" className="aw-band">
                  {row.band}
                </th>
                <td>{count(row, 'unworked')}</td>
                <td>{count(row, 'unconfirmed')}</td>
              </tr>
              {open?.band === row.band && (
                <tr>
                  <td colSpan={3}>
                    <p className="aw-bn-list" data-kind={open.kind}>
                      <strong>
                        {open.kind === 'unworked'
                          ? t('awards.bandNeeds.list.unworked', { band: row.band, count: row.unworked.length })
                          : t('awards.bandNeeds.list.unconfirmed', { band: row.band, count: row.unconfirmed.length })}
                      </strong>{' '}
                      {row[open.kind].join(' · ')}
                    </p>
                  </td>
                </tr>
              )}
            </Fragment>
          ))}
        </tbody>
      </table>
    </div>
  )
}

/** The FFMA chase: the squares worked on 6 m but not confirmed, then the ones never worked there
 *  (folded away by default — on a new log that is all 488). Grid squares are data. */
function FfmaNeeded({ ffma }: { ffma: FfmaProgress }) {
  const chips = (grids: string[]) => (
    <span className="aw-needbands">
      {grids.map((g) => (
        <span className="aw-chip" key={g}>
          {g}
        </span>
      ))}
    </span>
  )
  const needed = ffma.unworked.length + ffma.unconfirmed.length
  return (
    <div className="aw-panel aw-ffma">
      <h3>
        <Grid3x3 size={14} aria-hidden="true" /> {t('awards.chase.ffma.head', { count: needed })}
      </h3>
      {needed === 0 ? (
        <p className="aw-empty">{t('awards.chase.ffma.empty', { total: ffma.total })}</p>
      ) : (
        <>
          <p className="aw-empty">{t('awards.chase.ffma.note')}</p>
          {ffma.unconfirmed.length > 0 && (
            <p className="aw-bn-list" data-kind="unconfirmed">
              <strong>{t('awards.chase.ffma.unconfirmed', { count: ffma.unconfirmed.length })}</strong>{' '}
              {chips(ffma.unconfirmed)}
            </p>
          )}
          {ffma.unworked.length > 0 && (
            <details className="aw-ffma-unworked">
              <summary>{t('awards.chase.ffma.unworked', { count: ffma.unworked.length })}</summary>
              {chips(ffma.unworked)}
            </details>
          )}
        </>
      )}
    </div>
  )
}

/** The WAE chase: countries worked but not confirmed, then the ones never worked (folded away).
 *  DARC's prefixes and the country names are data. */
function WaeNeeded({ wae }: { wae: WaeProgress }) {
  const chips = (cs: WaeProgress['unworked']) => (
    <span className="aw-needbands">
      {cs.map((c) => (
        <span className="aw-chip" key={c.prefix} title={c.name}>
          {c.prefix}
        </span>
      ))}
    </span>
  )
  const needed = wae.unworked.length + wae.unconfirmed.length
  return (
    <div className="aw-panel aw-wae">
      <h3>
        <Globe2 size={14} aria-hidden="true" /> {t('awards.chase.wae.head', { count: needed })}
      </h3>
      {needed === 0 ? (
        <p className="aw-empty">{t('awards.chase.wae.empty', { total: wae.total })}</p>
      ) : (
        <>
          <p className="aw-empty">{t('awards.chase.wae.note')}</p>
          {wae.unconfirmed.length > 0 && (
            <p className="aw-bn-list" data-kind="unconfirmed">
              <strong>{t('awards.chase.wae.unconfirmed', { count: wae.unconfirmed.length })}</strong>{' '}
              {chips(wae.unconfirmed)}
            </p>
          )}
          {wae.unworked.length > 0 && (
            <details className="aw-wae-unworked">
              <summary>{t('awards.chase.wae.unworked', { count: wae.unworked.length })}</summary>
              {chips(wae.unworked)}
            </details>
          )}
        </>
      )}
    </div>
  )
}

/** A DXCC entity as a disagreement names it: its name and number, or the number alone when the
 * station's table does not know it. */
function dxccLabel(code: number, name?: string | null): string {
  return name ? `${name} (${code})` : t('awards.dxccDiffer.unknown', { code })
}

/** The contacts LoTW confirmed under another DXCC entity than the log records: counted by
 * LoTW's, listed so the operator sees every one. The log's own value is never changed. */
function DxccDifferList({ items }: { items: DxccDisagreement[] }) {
  return (
    <>
      <p className="aw-empty">{t('awards.dxccDiffer.note')}</p>
      <ul className="aw-needed">
        {items.map((d, i) => (
          <li key={`${d.call}-${d.whenUnix}-${i}`}>
            <span className="aw-entity">
              {d.call} · {utcDate(d.whenUnix)} · {d.band} {d.mode}
            </span>
            <span className="aw-needbands">
              {t('awards.dxccDiffer.row', {
                logged: dxccLabel(d.loggedDxcc, d.loggedEntity),
                lotw: dxccLabel(d.lotwDxcc, d.lotwEntity),
              })}
            </span>
          </li>
        ))}
      </ul>
    </>
  )
}

/** Render a chase list (entity + the bands to confirm) with a quick filter + a
 * sort (A–Z or by how many bands are needed), or an empty note. */
function NeedList({ items, empty }: { items: EntityNeed[]; empty: string }) {
  const [sort, setSort] = useState<NeedSort>('entity')
  const [q, setQ] = useState('')
  if (items.length === 0) return <p className="aw-empty">{empty}</p>
  const needle = q.trim().toLowerCase()
  const rows = items
    .filter((n) => !needle || n.entity.toLowerCase().includes(needle))
    .sort((a, b) =>
      sort === 'bands'
        ? b.bands.length - a.bands.length || a.entity.localeCompare(b.entity)
        : a.entity.localeCompare(b.entity),
    )
  return (
    <>
      <div className="aw-needctl">
        <input
          className="aw-needfilter"
          type="text"
          value={q}
          placeholder={t('awards.needList.filter.placeholder')}
          aria-label={t('awards.needList.filter.label')}
          onChange={(e) => setQ(e.target.value)}
        />
        <button
          type="button"
          className={`aw-needsort${sort === 'entity' ? ' active' : ''}`}
          onClick={() => setSort('entity')}
          title={t('awards.needList.sort.alpha.title')}
        >
          {t('awards.needList.sort.alpha.label')}
        </button>
        <button
          type="button"
          className={`aw-needsort${sort === 'bands' ? ' active' : ''}`}
          onClick={() => setSort('bands')}
          title={t('awards.needList.sort.bands.title')}
        >
          {t('awards.needList.sort.bands.label')}
        </button>
      </div>
      {rows.length === 0 ? (
        <p className="aw-empty">{t('awards.needList.noMatch', { query: q.trim() })}</p>
      ) : (
        <ul className="aw-needed">
          {rows.map((n) => (
            <li key={n.entity}>
              <span className="aw-entity">{n.entity}</span>
              <span className="aw-needbands">
                {n.bands.map((b) => (
                  <span className="aw-chip" key={b}>
                    {b}
                  </span>
                ))}
              </span>
            </li>
          ))}
        </ul>
      )}
    </>
  )
}

/**
 * Awards dashboard — DXCC-first, computed from the operator's log (cty.dat
 * entity resolution). Headline DXCC (toward 100), DXCC Challenge (toward 1000
 * entity×band slots), and confirmation rate; a per-band entity breakdown; and
 * two chases — "confirm for a new entity" (DXCC) and "confirm for a Challenge
 * slot" (a band on an entity you already have). Online LoTW/eQSL sync (which
 * flips `confirmed`) is a later increment; this is all from the log.
 */
/** `showGamification` (the `gamification` feature) gates the celebratory badge
 * grid; the award math + tables always render. */

/** True iff this action maps to the one-click LoTW (re)upload via TQSL — the only
 * service with an in-app bulk upload path. A `reUpload` for QRZ/ClubLog
 * carries a non-LoTW `source` and must NOT drive the LoTW upload button. */
function isLotwUpload(a?: DiagAction): boolean {
  if (!a) return false
  return a.kind === 'uploadToLotw' || (a.kind === 'reUpload' && (a.source ?? 'LoTW') === 'LoTW')
}

/** Human-readable result of an upload attempt, for the panel status line. */
function uploadMessage(r: UploadReport): string {
  const n = r.dispatched
  switch (r.outcome) {
    case 'pending':
      return t('awards.upload.pending', { count: n })
    case 'duplicate':
      return t('awards.upload.duplicate', { count: n })
    case 'rejected':
      // Two whole sentences rather than one with an optional ": detail" tail — where the
      // server's own words belong is a decision for each language.
      return r.detail
        ? t('awards.upload.rejectedDetail', { detail: r.detail })
        : t('awards.upload.rejected')
    case 'authfail':
      return t('awards.upload.authFailed')
    case 'retry':
      return t('awards.upload.retry')
    case 'none':
      return t('awards.upload.none')
    default:
      return t('awards.upload.finished', { outcome: r.outcome })
  }
}

/** Diagnoses the list shows. */
const SHOWN_DIAGNOSES = 50

/** The per-QSO push targets the diagnostics can drive (each has a single-QSO
 * push command; LoTW goes through the TQSL upload of the contacts' ids instead). */
type PushService = 'QRZ' | 'ClubLog' | 'eQSL'

/** The contacts a bucket names, by id — or null when the report does not name every one of them.
 *  A bucket also lists LOG POSITIONS, and those are never acted on: another window's delete moves
 *  a position onto the neighbouring contact between the diagnosis and the press (see `upload`). */
function bucketIds(b: DiagActionBucket): string[] | null {
  const ids = b.qsoIds
  if (!ids || ids.length === 0 || ids.length !== b.qsoIndices.length) return null
  return ids.every((id): id is string => !!id) ? ids : null
}

/** The push service a row action maps to, or null when it isn't a push. */
function pushService(a: DiagAction): PushService | null {
  if (a.kind === 'uploadToQrz') return 'QRZ'
  if (a.kind === 'uploadToClublog') return 'ClubLog'
  if (a.kind === 'uploadToEqsl') return 'eQSL'
  if (a.kind === 'reUpload' && (a.source === 'QRZ' || a.source === 'ClubLog' || a.source === 'eQSL'))
    return a.source
  return null
}

/** Per-QSO action affordance: a live button for the upload/push kinds, a static
 * guidance chip for the rest (field/dup/call fixes + partner-side waits are 1a). */
function RowAction({
  d,
  busyKey,
  onUpload,
  onPush,
  onOpenSettings,
  observed = false,
}: {
  d: QsoDiagnosis
  busyKey: string | null
  onUpload: (ids: string[], key: string) => void
  onPush: (id: string, service: PushService, key: string) => void
  /** Open Settings at a section id. Absent ⇒ the re-login row stays a guidance chip. */
  onOpenSettings?: (target: string) => void
  /** A station report read from a browser: every action is guidance, never a button. */
  observed?: boolean
}) {
  const a = d.reasons[0]?.action
  if (!a) return null
  const key = `row-${d.index}`
  // The contact this row is about, by its id — the only way a button here names one. Its log
  // position (`d.index`) can name the neighbouring contact by the time the button is pressed.
  // A contact the report does not name by id gets the guidance chip, never a button.
  const id = d.id
  // Only LoTW has an in-app one-click (re)upload (via TQSL) — show the live button.
  if (isLotwUpload(a)) {
    if (observed || !id)
      return (
        <span className="conf-act">
          {a.kind === 'reUpload' ? t('awards.conf.reupload') : t('awards.conf.uploadToLotw')}
        </span>
      )
    return (
      <button
        className="conf-btn"
        disabled={busyKey !== null}
        onClick={() => onUpload([id], key)}
      >
        {busyKey === key
          ? t('awards.conf.uploading')
          : a.kind === 'reUpload'
            ? t('awards.conf.reupload')
            : t('awards.conf.uploadToLotw')}
      </button>
    )
  }
  // QRZ/ClubLog/eQSL: one-click per-row push via the existing single-QSO commands.
  // Muted styling + tooltip keep the house rule visible: these serve the personal
  // logbook, NOT ARRL DXCC/WAS credit — only the LoTW button is the award pill.
  const svc = pushService(a)
  if (svc) {
    const label =
      a.kind === 'reUpload'
        ? t('awards.conf.repush', { service: svc })
        : t('awards.conf.push', { service: svc })
    if (observed || !id) return <span className="conf-act">{label}</span>
    return (
      <button
        className="conf-btn conf-btn-push"
        disabled={busyKey !== null}
        title={t('awards.conf.push.title', { service: svc })}
        onClick={() => onPush(id, svc, key)}
      >
        {busyKey === key ? t('awards.conf.pushing') : label}
      </button>
    )
  }
  // Re-login: LoTW's certificate lives in TQSL, outside the app, so that one stays a chip.
  // Every other service's credentials are in Confirmations, and this row is where the
  // operator is when they learn the login is stale — so it takes them there.
  if (a.kind === 'reauthenticate') {
    const src = a.source ?? 'LoTW'
    if (src === 'LoTW') return <span className="conf-act">{t('awards.conf.fixCert')}</span>
    if (!onOpenSettings)
      return (
        <span className="conf-act">{t('awards.conf.fixLoginInSettings', { service: src })}</span>
      )
    return (
      <button
        className="conf-btn"
        title={t('awards.conf.fixLogin.title', { service: src })}
        onClick={() => onOpenSettings('confirmations')}
      >
        {t('awards.conf.fixLogin', { service: src })}
      </button>
    )
  }
  if (a.kind === 'nudgePartner')
    return <span className="conf-act">{t('awards.conf.waitingOn', { call: a.call ?? '' })}</span>
  if (a.kind === 'mergeDuplicate')
    return (
      <span className="conf-act">
        {t('awards.conf.reviewDup', { number: (a.otherIndex ?? 0) + 1 })}
      </span>
    )
  if (a.kind === 'fixField')
    return <span className="conf-act">{t('awards.conf.fixField', { field: a.field ?? '' })}</span>
  if (a.kind === 'correctBustedCall')
    return (
      <span className="conf-act">{t('awards.conf.bustedCall', { call: a.suggested ?? '' })}</span>
    )
  return null
}

/** The two fr tokens the Awards grid reads for its columns (layout L7): the progress column's and
 *  the chase lists'. */
const AWARDS_COLUMN_VARS = ['--awards-col-a', '--awards-col-b'] as const

export function AwardsView({
  showGamification = true,
  onOpenSettings,
  onOpenLogbook,
  observation,
  diagnostics,
}: {
  showGamification?: boolean
  /** Open a diagnosed contact in the Logbook's editor (by its id), as its ✎ there opens it. */
  onOpenLogbook?: (call: string, id?: string | null) => void
  /** A complete station-owned summary; observation never starts upload/diagnostic reads. */
  observation?: AwardSummary
  /** The station's confirmation diagnostics for an observed summary, shown read only. */
  diagnostics?: DiagnosticsReport
  /** Open Settings at a section id (see settings/registry.ts). */
  onOpenSettings?: (target: string) => void
}) {
  const [nativeAwards, setAw] = useState<AwardSummary | null>(null)
  const aw = observation ?? nativeAwards
  const [diag, setDiag] = useState<DiagnosticsReport | null>(null)
  const shownDiag = observation ? (diagnostics ?? null) : diag
  const [err, setErr] = useState(false)
  // Grid list: VUCC bands only by default — see the grids panel for why. Declared up here
  // with the other hooks rather than beside its own derivations, because AwardsView early-
  // returns for the loading, error and empty-log states and a hook below those is not
  // called on every render path.
  const [gridsVuccOnly, setGridsVuccOnly] = useState(true)
  // THE TWO COLUMNS' SPLIT (layout L7), up here for the same reason: the divider between the
  // progress column and the chase lists stores the first one's share of 2, per window, and the
  // grid reads it as two fr tokens.
  const bodyRef = useRef<HTMLDivElement>(null)
  const leftRef = useRef<HTMLDivElement>(null)
  const chasesRef = useRef<HTMLDivElement>(null)
  const [colShare, setColShare] = useState(() => parseColumnShare(surfaceGet('nexus.awards.columns')))
  const commitCols = (share: number | null) => {
    setColShare(share)
    // '' reads back as "never set" (parseColumnShare): the sheet's stock split.
    surfaceSet('nexus.awards.columns', share == null ? '' : String(share))
  }
  const [busyKey, setBusyKey] = useState<string | null>(null)
  const [uploadMsg, setUploadMsg] = useState<string | null>(null)
  // Guards post-await setState in upload() — TQSL signing can take seconds, during
  // which the operator may switch tabs and unmount this view.
  const mounted = useRef(true)
  useEffect(() => {
    mounted.current = true
    if (observation) return () => { mounted.current = false }
    let live = true
    // Asked again while a contact logged just before is still being saved (features/notAnswered).
    askAgainWhileSaving(getAwards, () => live)
      .then((a) => live && setAw(a))
      .catch(() => live && setErr(true))
    askAgainWhileSaving(getConfirmationDiagnostics, () => live)
      .then((d) => live && setDiag(d))
      .catch(() => {}) // diagnostics are a best-effort add-on; never block the dashboard
    return () => {
      live = false
      mounted.current = false
    }
  }, [observation])

  /** Sign + upload the given QSOs via TQSL, then re-diagnose so the panel reflects
   * the new state (uploaded rows drop to Pending/waiting; bounced ones show R9).
   *
   * ⚠️ BY ID, NEVER BY POSITION. The diagnosis names each contact by its log position too, and
   * a position is only right while nothing above it changes: another window's delete (a Remote
   * browser, the Logbook) between the diagnosis and the press moves it onto the neighbouring
   * contact — which TQSL would then sign under the operator's certificate. An id names the
   * contact or, once it is gone, nothing (the upload skips it). */
  async function upload(ids: string[], key: string) {
    if (observation) return
    setBusyKey(key)
    setUploadMsg(null)
    try {
      const r = await uploadLotwReportByIds(ids)
      // Contacts that changed while TQSL signed are not marked (features/lotwSkips). A toast, as
      // the Logbook's upload says it — raised before the mount check, so it still reaches an
      // operator who left this view while TQSL worked.
      const skipped = lotwSkipNote(r)
      if (skipped) pushToast(skipped, 'info', LOTW_SKIP_TOAST_MS)
      const fresh = await getConfirmationDiagnostics().catch(() => null)
      if (!mounted.current) return
      setUploadMsg(uploadMessage(r))
      if (fresh) setDiag(fresh)
    } catch (e) {
      if (mounted.current) setUploadMsg(e instanceof Error ? e.message : String(e))
    } finally {
      if (mounted.current) setBusyKey(null)
    }
  }

  /** Push one QSO to QRZ/ClubLog/eQSL (the never-uploaded and bounced-re-push
   * cases), then re-diagnose so the row reflects the new upload state.
   *
   * The contact is read by its id when the button is pressed — as it stands then, and never the
   * one at its diagnosed position, which another window's delete can have moved (see `upload`).
   * One that was deleted meanwhile is not pushed. */
  async function push(id: string, service: PushService, key: string) {
    if (observation) return
    setBusyKey(key)
    setUploadMsg(null)
    try {
      const q = await logSource()
        .ask({ kind: 'row', id })
        .catch(() => null)
      if (!q) {
        if (mounted.current) setUploadMsg(t('awards.push.noQso'))
        return
      }
      // One whole sentence per outcome — the shipped text spliced " (already there)" into
      // the middle of one, which no language with another word order can reproduce.
      let msg: string
      if (service === 'QRZ') {
        const r = await qrzPushQso(q)
        msg =
          r.result === 'ok' || r.result === 'replace'
            ? t('awards.push.qrz.ok', { call: q.call })
            : r.result === 'duplicate'
              ? t('awards.push.qrz.duplicate', { call: q.call })
              : t('awards.push.qrz.rejected', { call: q.call, reason: r.reason ?? r.result })
      } else if (service === 'ClubLog') {
        const r = await clublogPushQso(q)
        msg =
          r.result === 'duplicate'
            ? t('awards.push.clublog.duplicate', { call: q.call })
            : r.result === 'ok' || r.result === 'modified'
              ? t('awards.push.clublog.ok', { call: q.call })
              : t('awards.push.clublog.rejected', { call: q.call, reason: r.message ?? r.result })
      } else {
        // eQSL's classify_upload returns 'accepted' on success (never 'pending' —
        // that's the LoTW/TQSL batch convention on the shared DTO).
        const r = await eqslPushQso(q)
        msg =
          r.outcome === 'duplicate'
            ? t('awards.push.eqsl.duplicate', { call: q.call })
            : r.outcome === 'accepted'
              ? t('awards.push.eqsl.ok', { call: q.call })
              : t('awards.push.eqsl.rejected', { detail: r.detail ?? r.outcome })
      }
      const fresh = await getConfirmationDiagnostics().catch(() => null)
      if (!mounted.current) return
      setUploadMsg(msg)
      if (fresh) setDiag(fresh)
    } catch (e) {
      if (mounted.current)
        setUploadMsg(
          t('awards.push.failed', {
            service,
            detail: e instanceof Error ? e.message : String(e),
          }),
        )
    } finally {
      if (mounted.current) setBusyKey(null)
    }
  }

  if (!observation && err) {
    return (
      <section className="awards">
        <StateBlock
          kind="error"
          title={t('awards.load.failed.title')}
          detail={t('awards.load.failed.detail')}
        />
      </section>
    )
  }
  if (!aw) {
    return (
      <section className="awards">
        <StateBlock
          kind="empty"
          title={t('awards.loading.title')}
          detail={t('awards.loading.detail')}
        />
      </section>
    )
  }
  if (aw.qsos === 0) {
    return (
      <section className="awards">
        <StateBlock
          kind="empty"
          title={t('awards.empty.title')}
          detail={t('awards.empty.detail')}
        />
      </section>
    )
  }

  const confRate = Math.round((aw.confirmedQsos / aw.qsos) * 100)
  const hr = aw.honorRoll
  const hrPct = hr.currentTotal > 0 ? Math.min(100, (hr.confirmed / hr.currentTotal) * 100) : 0
  const hrNote = hr.numberOne
    ? t('awards.honorRoll.note.numberOne', { total: hr.currentTotal })
    : hr.achieved
      ? t('awards.honorRoll.note.achieved', { needed: hr.numberOneNeeded })
      : t('awards.honorRoll.note.toGo', { needed: hr.needed, threshold: hr.threshold })
  const dxccPct = Math.min(100, Math.round((aw.dxccConfirmed / DXCC_BASIC) * 100))
  const challengePct = Math.min(100, Math.round((aw.slotsConfirmed / CHALLENGE_AWARD) * 100))
  const bandMax = Math.max(1, ...aw.bands.map((b) => b.worked))
  const modeMax = Math.max(1, ...aw.modes.map((m) => m.worked))
  /** The bands ARRL actually awards grids on, taken from the standings Rust already
   *  filtered against VUCC_THRESHOLDS. Never a second band list maintained here. */
  const vuccBandSet = new Set((aw.vucc.awards ?? []).map((a) => a.band))
  const gridRows = gridsVuccOnly
    ? aw.vucc.bands.filter((b) => vuccBandSet.has(b.band))
    : aw.vucc.bands
  // ⚠️ Deliberately NOT a max over every band. A scale taken from an HF FT8 grid pile
  // would render every VHF bar as a sliver — technically correct and visually useless,
  // the opposite of what filtering to VHF is for. So it tracks what is SHOWN.
  const gridBandMax = Math.max(1, ...gridRows.map((b) => b.worked))

  return (
    <section className="awards">
      <div className="awards-head">
        <h2>
          <Trophy size={16} aria-hidden="true" /> {t('awards.title')}
        </h2>
        <span className="awards-sub">{t('awards.subtitle')}</span>
      </div>

      <div className="awards-cards">
        <div className="aw-card">
          <span className="aw-k">
            <Trophy size={13} aria-hidden="true" /> {AWARD_NAMES.dxcc}
          </span>
          <span className="aw-v">
            {aw.dxccConfirmed}
            <span className="aw-of"> / {DXCC_BASIC}</span>
          </span>
          <div className="aw-bar">
            <div className="aw-fill good" style={{ width: `${dxccPct}%` }} />
          </div>
          <span className="aw-note">
            {aw.dxccConfirmed >= DXCC_BASIC
              ? t('awards.dxcc.note.achieved', {
                  confirmed: aw.dxccConfirmed,
                  worked: aw.dxccWorked,
                  credited: aw.dxccCredited,
                })
              : t('awards.dxcc.note.toGo', {
                  remaining: DXCC_BASIC - aw.dxccConfirmed,
                  worked: aw.dxccWorked,
                  credited: aw.dxccCredited,
                })}
            {aw.readyToSubmit > 0 &&
              t('awards.dxcc.note.readyToSubmit', { count: aw.readyToSubmit })}
          </span>
          {aw.dxccConfirmedTotal !== undefined && (
            <span className="aw-note">
              {t('awards.dxcc.note.currentTotal', {
                current: aw.dxccConfirmed,
                total: aw.dxccConfirmedTotal,
              })}
            </span>
          )}
        </div>

        <div className={`aw-card${hr.achieved ? ' aw-card-elite' : ''}`}>
          <span className="aw-k">
            <Award size={13} aria-hidden="true" /> {AWARD_NAMES.honorRoll}
          </span>
          <span className="aw-v">
            {hr.confirmed}
            <span className="aw-of"> / {hr.currentTotal}</span>
          </span>
          <div className="aw-bar">
            <div className="aw-fill good" style={{ width: `${hrPct}%` }} />
          </div>
          <span className="aw-note">{hrNote}</span>
        </div>

        <div className="aw-card">
          <span className="aw-k">
            <Radio size={13} aria-hidden="true" /> {AWARD_NAMES.challenge}
          </span>
          <span className="aw-v">
            {aw.slotsConfirmed}
            <span className="aw-of"> / {CHALLENGE_AWARD}</span>
          </span>
          <div className="aw-bar">
            <div className="aw-fill good" style={{ width: `${challengePct}%` }} />
          </div>
          <span className="aw-note">{t('awards.challenge.note', { worked: aw.slotsWorked })}</span>
        </div>

        <div className="aw-card">
          <span className="aw-k">
            <CheckCircle2 size={13} aria-hidden="true" /> {t('awards.confirmed.label')}
          </span>
          <span className="aw-v">
            {confRate}
            <span className="aw-of">%</span>
          </span>
          <span className="aw-note">
            {t('awards.confirmed.note', { confirmed: aw.confirmedQsos, total: aw.qsos })}
          </span>
        </div>

        <div className="aw-card">
          <span className="aw-k">
            <Layers size={13} aria-hidden="true" /> {AWARD_NAMES.fiveBandDxcc}
          </span>
          <span className="aw-v">
            {aw.fiveBandConfirmed}
            <span className="aw-of"> / 100</span>
          </span>
          <div className="aw-bar">
            <div className="aw-fill good" style={{ width: `${Math.min(100, aw.fiveBandConfirmed)}%` }} />
          </div>
          <span className="aw-note">
            {t('awards.fiveBand.note', { worked: aw.fiveBandWorked })}
          </span>
        </div>

        <div className="aw-card">
          <span className="aw-k">
            <Globe2 size={13} aria-hidden="true" /> {AWARD_NAMES.waz}
          </span>
          <span className="aw-v">
            {aw.wazConfirmed}
            <span className="aw-of"> / {WAZ_ZONES}</span>
          </span>
          <div className="aw-bar">
            <div
              className="aw-fill good"
              style={{ width: `${Math.min(100, (aw.wazConfirmed / WAZ_ZONES) * 100)}%` }}
            />
          </div>
          <span className="aw-note">
            {aw.wazConfirmed >= WAZ_ZONES
              ? t('awards.waz.note.achieved', { worked: aw.wazWorked })
              : t('awards.waz.note.toGo', {
                  remaining: WAZ_ZONES - aw.wazConfirmed,
                  worked: aw.wazWorked,
                })}
          </span>
        </div>

        <div className={`aw-card${aw.was.confirmed >= WAS_STATES ? ' aw-card-elite' : ''}`}>
          <span className="aw-k">
            <Flag size={13} aria-hidden="true" /> {AWARD_NAMES.was}
          </span>
          <span className="aw-v">
            {aw.was.confirmed}
            <span className="aw-of"> / {WAS_STATES}</span>
          </span>
          <div className="aw-bar">
            <div
              className="aw-fill good"
              style={{ width: `${Math.min(100, (aw.was.confirmed / WAS_STATES) * 100)}%` }}
            />
          </div>
          <span className="aw-note">
            {aw.was.confirmed >= WAS_STATES
              ? t('awards.was.note.achieved', {
                  worked: aw.was.worked,
                  fiveBand: aw.was.fiveBandConfirmed,
                })
              : t('awards.was.note.toGo', {
                  remaining: WAS_STATES - aw.was.confirmed,
                  worked: aw.was.worked,
                  fiveBand: aw.was.fiveBandConfirmed,
                })}
          </span>
        </div>

        {/* The REAL VUCC standing (N9UM audit): 50 MHz-and-up, per band, each against
            its own ARRL threshold — never the all-band grid total, which is mostly HF
            FT8 grid exchange and corresponds to no ARRL award. Headline = the band
            closest to (or past) its threshold; the all-band count stays visible below
            as the tracker stat it is. */}
        {(() => {
          const best = [...(aw.vucc.awards ?? [])].sort(
            (a, b) => b.confirmed / b.threshold - a.confirmed / a.threshold,
          )[0]
          const achieved = (aw.vucc.awards ?? []).filter((x) => x.achieved)
          return (
            <div className={`aw-card${achieved.length > 0 ? ' aw-card-elite' : ''}`}>
              <span className="aw-k">
                <Grid3x3 size={13} aria-hidden="true" /> {AWARD_NAMES.vucc}
              </span>
              {best ? (
                <>
                  <span className="aw-v">
                    {best.confirmed}
                    <span className="aw-of">
                      {' '}
                      / {best.threshold} · {best.band}
                    </span>
                  </span>
                  <div className="aw-bar">
                    <div
                      className="aw-fill good"
                      style={{
                        width: `${Math.min(100, (best.confirmed / best.threshold) * 100)}%`,
                      }}
                    />
                  </div>
                  <span className="aw-note">
                    {achieved.length > 0
                      ? t('awards.vucc.note.achieved', {
                          bands: achieved.map((x) => x.band).join(' · '),
                          confirmed: aw.vucc.confirmed,
                        })
                      : t('awards.vucc.note.toGo', {
                          remaining: best.threshold - best.confirmed,
                          band: best.band,
                          confirmed: aw.vucc.confirmed,
                        })}
                  </span>
                </>
              ) : (
                <>
                  <span className="aw-v">
                    0<span className="aw-of"> / {VUCC_GRIDS}</span>
                  </span>
                  <span className="aw-note">
                    {t('awards.vucc.note.none', { confirmed: aw.vucc.confirmed })}
                  </span>
                </>
              )}
            </div>
          )
        })()}

        {/* The Fred Fish Memorial Award: the ARRL's 488 squares of the 48 contiguous states, on
            6 m only, confirmed as VUCC counts them. Absent from a station older than FFMA. */}
        {aw.ffma && (
          <div className={`aw-card${aw.ffma.confirmed >= aw.ffma.total ? ' aw-card-elite' : ''}`}>
            <span className="aw-k">
              <Grid3x3 size={13} aria-hidden="true" /> {AWARD_NAMES.ffma}
            </span>
            <span className="aw-v">
              {aw.ffma.confirmed}
              <span className="aw-of"> / {aw.ffma.total}</span>
            </span>
            <div className="aw-bar">
              <div
                className="aw-fill good"
                style={{ width: `${Math.min(100, (aw.ffma.confirmed / Math.max(1, aw.ffma.total)) * 100)}%` }}
              />
            </div>
            <span className="aw-note">
              {aw.ffma.confirmed >= aw.ffma.total
                ? t('awards.ffma.note.achieved', { total: aw.ffma.total })
                : t('awards.ffma.note.toGo', {
                    remaining: aw.ffma.total - aw.ffma.confirmed,
                    worked: aw.ffma.worked,
                  })}
            </span>
          </div>
        )}

        {/* Satellite VUCC — its own ARRL category, and the ONLY place a grid
            worked through a bird is ALLOWED to count. Since 2026-08-10 Nexus
            tags these ITSELF: a contact logged on the held bird's downlink gets
            PROP_MODE=SAT + the LoTW designator stamped as a pair
            (Engine::log_qso), so this card moves on Nexus-logged pass QSOs, not
            only imports. Residual: birds with no derivable designator (ISS)
            stay untagged. */}
        <div className={`aw-card${aw.vucc.satConfirmed >= VUCC_GRIDS ? ' aw-card-elite' : ''}`}>
          <span className="aw-k">
            <Satellite size={13} aria-hidden="true" /> {AWARD_NAMES.satVucc}
          </span>
          <span className="aw-v">
            {aw.vucc.satConfirmed}
            <span className="aw-of"> / {VUCC_GRIDS}</span>
          </span>
          <div className="aw-bar">
            <div
              className="aw-fill good"
              style={{ width: `${Math.min(100, (aw.vucc.satConfirmed / VUCC_GRIDS) * 100)}%` }}
            />
          </div>
          <span className="aw-note">
            {aw.vucc.satConfirmed >= VUCC_GRIDS
              ? t('awards.satVucc.note.achieved', {
                  worked: aw.vucc.satWorked,
                  satDxcc: aw.satDxccConfirmed ?? 0,
                })
              : t('awards.satVucc.note.toGo', {
                  remaining: VUCC_GRIDS - aw.vucc.satConfirmed,
                  worked: aw.vucc.satWorked,
                  satDxcc: aw.satDxccConfirmed ?? 0,
                })}
          </span>
          <span className="aw-note">{t('awards.satVucc.note.tagging')}</span>
        </div>

        {/* The ✓ gates on groups confirmed by LoTW or card: the IOTA programme credits QSL
            cards and, since May 2020, LoTW-matched contacts; eQSL never counts. The card-only
            count stays on the note. */}
        <div className={`aw-card${aw.iota.confirmed >= IOTA_ISLANDS ? ' aw-card-elite' : ''}`}>
          <span className="aw-k">
            <Globe2 size={13} aria-hidden="true" /> {AWARD_NAMES.iota}
          </span>
          <span className="aw-v">
            {aw.iota.confirmed}
            <span className="aw-of"> / {IOTA_ISLANDS}</span>
          </span>
          <div className="aw-bar">
            <div
              className="aw-fill good"
              style={{ width: `${Math.min(100, (aw.iota.confirmed / IOTA_ISLANDS) * 100)}%` }}
            />
          </div>
          <span className="aw-note">
            {aw.iota.confirmed >= IOTA_ISLANDS
              ? t('awards.iota.note.achieved', { cards: aw.iota.cardConfirmed ?? 0 })
              : t('awards.iota.note.worked', {
                  worked: aw.iota.worked,
                  cards: aw.iota.cardConfirmed ?? 0,
                })}
          </span>
        </div>

        {/* Worked All Provinces: the 13 Canadian provinces and territories, by the logged ADIF
            STATE on a Canadian contact. Absent from an older station. */}
        {aw.wap && (
          <div className={`aw-card${aw.wap.confirmed >= WAP_PROVINCES ? ' aw-card-elite' : ''}`}>
            <span className="aw-k">
              <Flag size={13} aria-hidden="true" /> {AWARD_NAMES.wap}
            </span>
            <span className="aw-v">
              {aw.wap.confirmed}
              <span className="aw-of"> / {WAP_PROVINCES}</span>
            </span>
            <div className="aw-bar">
              <div
                className="aw-fill good"
                style={{ width: `${Math.min(100, (aw.wap.confirmed / WAP_PROVINCES) * 100)}%` }}
              />
            </div>
            <span className="aw-note">
              {aw.wap.confirmed >= WAP_PROVINCES
                ? t('awards.wap.note.achieved', { worked: aw.wap.worked })
                : t('awards.wap.note.toGo', {
                    remaining: WAP_PROVINCES - aw.wap.confirmed,
                    worked: aw.wap.worked,
                  })}
            </span>
          </div>
        )}

        {/* CQ WPX: prefixes by CQ's rule. The award's own levels could not be read from CQ, so
            the card counts and claims no level (crates/propagation/src/wpx.rs). */}
        {aw.wpx && (
          <div className="aw-card">
            <span className="aw-k">
              <Award size={13} aria-hidden="true" /> {AWARD_NAMES.wpx}
            </span>
            <span className="aw-v">{aw.wpx.confirmed}</span>
            <span className="aw-note">{t('awards.wpx.note', { worked: aw.wpx.worked })}</span>
          </div>
        )}

        {/* DARC's WAE: 73 countries, and band points (at most five bands a country). */}
        {aw.wae && (
          <div className={`aw-card${aw.wae.class ? ' aw-card-elite' : ''}`}>
            <span className="aw-k">
              <Globe2 size={13} aria-hidden="true" /> {AWARD_NAMES.wae}
            </span>
            <span className="aw-v">
              {aw.wae.confirmed}
              <span className="aw-of"> / {aw.wae.total}</span>
            </span>
            <div className="aw-bar">
              <div
                className="aw-fill good"
                style={{ width: `${Math.min(100, (aw.wae.confirmed / Math.max(1, aw.wae.total)) * 100)}%` }}
              />
            </div>
            <span className="aw-note">
              {aw.wae.class
                ? t('awards.wae.note.class', { level: aw.wae.class, points: aw.wae.points })
                : t('awards.wae.note.toGo', { points: aw.wae.points, worked: aw.wae.worked })}
            </span>
          </div>
        )}

        {/* WWFF: references worked. WWFF credits only its own log search, so nothing here is
            "confirmed"; the levels are WWFF-H's. */}
        {aw.wwff && (
          <div className="aw-card">
            <span className="aw-k">
              <Globe2 size={13} aria-hidden="true" /> {AWARD_NAMES.wwff}
            </span>
            <span className="aw-v">{aw.wwff.worked}</span>
            <span className="aw-note">
              {aw.wwff.level != null
                ? t('awards.wwff.note.level', {
                    level: aw.wwff.level,
                    next: aw.wwff.nextLevel,
                    entities: aw.wwff.entities,
                  })
                : t('awards.wwff.note.toGo', { next: aw.wwff.nextLevel, entities: aw.wwff.entities })}
            </span>
          </div>
        )}
      </div>

      <div className="awards-body" ref={bodyRef} style={columnShareStyle(colShare, AWARDS_COLUMN_VARS)}>
        <div className="aw-left" ref={leftRef}>
          <div className="aw-panel">
            <h3>{t('awards.bands.head')}</h3>
            <div className="aw-bands">
              {aw.bands.map((b) => (
                <div className="aw-bandrow" key={b.band}>
                  <span className="aw-band">{b.band}</span>
                  <div
                    className="aw-bandbar"
                    title={t('awards.bar.title', { confirmed: b.confirmed, worked: b.worked })}
                  >
                    <div className="aw-worked" style={{ width: `${(b.worked / bandMax) * 100}%` }}>
                      <div
                        className="aw-confirmed"
                        style={{ width: `${b.worked ? (b.confirmed / b.worked) * 100 : 0}%` }}
                      />
                    </div>
                  </div>
                  <span className="aw-bandnum">
                    {b.confirmed}
                    <span className="aw-of">/{b.worked}</span>
                  </span>
                </div>
              ))}
            </div>
          </div>

          {aw.bandNeeds && <BandNeedsPanel needs={aw.bandNeeds} />}

          {aw.vucc.bands.length > 0 && (
            <div className="aw-panel">
              <h3>{t('awards.grids.head')}</h3>
              {/* VUCC bands only, by default. Grids are an award on 50 MHz and up and
                  nowhere else, so an HF grid count is a tracker statistic, not progress
                  toward anything — and on an FT8 station it is most of the list, which
                  buries the bands that do count. Reported by NT9E, 2026-08-27.

                  The VHF set is DERIVED from `vucc.awards`, never a second band list kept
                  here: Rust already filters that against VUCC_THRESHOLDS, so the two cannot
                  drift. `awards` only carries bands with grids worked, which is exactly what
                  this list wants anyway. The full tracker view stays one click away. */}
              <div className="aw-needctl">
                <button
                  type="button"
                  className={`aw-needsort${gridsVuccOnly ? ' active' : ''}`}
                  onClick={() => setGridsVuccOnly(true)}
                  title={t('awards.grids.filter.vucc.title')}
                >
                  {t('awards.grids.filter.vucc.label')}
                </button>
                <button
                  type="button"
                  className={`aw-needsort${gridsVuccOnly ? '' : ' active'}`}
                  onClick={() => setGridsVuccOnly(false)}
                  title={t('awards.grids.filter.all.title')}
                >
                  {t('awards.grids.filter.all.label')}
                </button>
              </div>
              {gridRows.length === 0 ? (
                <p className="aw-empty">{t('awards.grids.noVucc')}</p>
              ) : (
              <div className="aw-bands">
                {gridRows.map((b) => (
                  <div className="aw-bandrow" key={b.band}>
                    <span className="aw-band">{b.band}</span>
                    <div
                      className="aw-bandbar"
                      title={t('awards.bar.titleGrids', {
                        confirmed: b.confirmed,
                        worked: b.worked,
                      })}
                    >
                      <div className="aw-worked" style={{ width: `${(b.worked / gridBandMax) * 100}%` }}>
                        <div
                          className="aw-confirmed"
                          style={{ width: `${b.worked ? (b.confirmed / b.worked) * 100 : 0}%` }}
                        />
                      </div>
                    </div>
                    <span className="aw-bandnum">
                      {b.confirmed}
                      <span className="aw-of">/{b.worked}</span>
                    </span>
                  </div>
                ))}
              </div>
              )}
            </div>
          )}

          <div className="aw-panel">
            <h3>{t('awards.modes.head')}</h3>
            <div className="aw-bands aw-modes">
              {aw.modes.map((m) => (
                <div className="aw-bandrow" key={m.mode}>
                  <span className="aw-band">{m.mode}</span>
                  <div
                    className="aw-bandbar"
                    title={t('awards.bar.title', { confirmed: m.confirmed, worked: m.worked })}
                  >
                    <div className="aw-worked" style={{ width: `${(m.worked / modeMax) * 100}%` }}>
                      <div
                        className="aw-confirmed"
                        style={{ width: `${m.worked ? (m.confirmed / m.worked) * 100 : 0}%` }}
                      />
                    </div>
                  </div>
                  <span className="aw-bandnum">
                    {m.confirmed}
                    <span className="aw-of">/{m.worked}</span>
                  </span>
                </div>
              ))}
            </div>
          </div>
          {/* The whole-log DXCC chart, the station's own read. */}
          {!observation && <DxccChartPanel />}
        </div>

        <div className="aw-chases" ref={chasesRef}>
          <div className="aw-panel">
            <h3>
              <Target size={14} aria-hidden="true" />{' '}
              {t('awards.chase.entities.head', { count: aw.needed.length })}
            </h3>
            <NeedList items={aw.needed} empty={t('awards.chase.entities.empty')} />
          </div>
          <div className="aw-panel">
            <h3>
              <Radio size={14} aria-hidden="true" />{' '}
              {t('awards.chase.slots.head', { count: aw.slotNeeded.length })}
            </h3>
            <NeedList items={aw.slotNeeded} empty={t('awards.chase.slots.empty')} />
          </div>
          <div className="aw-panel">
            <h3>
              <Send size={14} aria-hidden="true" />{' '}
              {t('awards.chase.bandTargets.head', { count: aw.bandTargets.length })}
            </h3>
            <NeedList items={aw.bandTargets} empty={t('awards.chase.bandTargets.empty')} />
          </div>
          <div className="aw-panel">
            <h3>
              <Flag size={14} aria-hidden="true" />{' '}
              {t('awards.chase.was.head', { count: aw.was.needed.length })}
            </h3>
            {aw.was.needed.length === 0 ? (
              <p className="aw-empty">{t('awards.chase.was.empty')}</p>
            ) : (
              <span className="aw-needbands">
                {aw.was.needed.map((s) => (
                  <span className="aw-chip" key={s}>
                    {s}
                  </span>
                ))}
              </span>
            )}
          </div>
          {aw.ffma && <FfmaNeeded ffma={aw.ffma} />}
          {aw.wap && (
            <div className="aw-panel aw-wap">
              <h3>
                <Flag size={14} aria-hidden="true" />{' '}
                {t('awards.chase.wap.head', { count: aw.wap.needed.length })}
              </h3>
              {aw.wap.needed.length === 0 ? (
                <p className="aw-empty">{t('awards.chase.wap.empty')}</p>
              ) : (
                <span className="aw-needbands">
                  {aw.wap.needed.map((p) => (
                    <span className="aw-chip" key={p}>
                      {p}
                    </span>
                  ))}
                </span>
              )}
            </div>
          )}
          {aw.wae && <WaeNeeded wae={aw.wae} />}
          {/* The station's own reads: an observed summary carries none of them. */}
          {!observation && <WhatToQslPanel />}
          {!observation && <CustomAwardsPanel />}
          {!observation && <PotaActivatorPanel />}
          {(aw.dxccDisagreements ?? []).length > 0 && (
            <div className="aw-panel">
              <h3>
                <Globe2 size={14} aria-hidden="true" />{' '}
                {t('awards.dxccDiffer.head', { count: aw.dxccDisagreements?.length ?? 0 })}
              </h3>
              <DxccDifferList items={aw.dxccDisagreements ?? []} />
            </div>
          )}
        </div>
        {/* The progress column | chase lists divider (layout L7): out of flow, in the gap before
            the chase lists (styles.css `.awards-colseam`). The stacked sm/xs layout hides it. */}
        <PaneSeam
          above={leftRef}
          below={chasesRef}
          axis="x"
          columnsOn={bodyRef}
          varName="--awards-col"
          columnVars={AWARDS_COLUMN_VARS}
          floors={[VIEW_COLUMN_FLOOR, VIEW_COLUMN_FLOOR]}
          className="awards-colseam"
          onCommit={(a: number) => commitCols(a)}
          onReset={() => commitCols(null)}
          label={t('awards.columns.split.label')}
        />
      </div>

      {shownDiag && (shownDiag.diagnoses.length > 0 || shownDiag.pendingLag > 0 || shownDiag.waitingOnPartner > 0) && (
        <div className="aw-panel conf-panel">
          <h3>
            <CheckCircle2 size={14} aria-hidden="true" /> {t('awards.conf.head')}
          </h3>
          {(shownDiag.oneAway ?? []).length > 0 && (
            <div className="conf-oneaway">
              <span className="conf-oneaway-label">{t('awards.conf.oneAway.label')}</span>
              {shownDiag.oneAway.slice(0, 8).map((o) => (
                <span
                  key={o.entity}
                  className={`conf-oneaway-chip${o.newEntity ? ' conf-oneaway-new' : ''}`}
                  title={
                    o.newEntity
                      ? t('awards.conf.oneAway.newEntity.title', {
                          entity: o.entity,
                          bands: o.bands.join(', '),
                        })
                      : t('awards.conf.oneAway.slots.title', {
                          entity: o.entity,
                          bands: o.bands.join(', '),
                          count: o.bands.length,
                        })
                  }
                >
                  {o.newEntity && <span className="conf-oneaway-star">★</span>}
                  {o.entity} <span className="conf-oneaway-bands">{o.bands.join(' ')}</span>
                </span>
              ))}
              {shownDiag.oneAway.length > 8 && (
                <span className="conf-muted">
                  {t('awards.conf.oneAway.more', { count: shownDiag.oneAway.length - 8 })}
                </span>
              )}
            </div>
          )}
          {(() => {
            // Top action per flagged QSO → lets a bucket offer a one-click bulk upload
            // ONLY when every member is a LoTW (re)upload (the one service with an
            // in-app bulk path). The engine already splits buckets by source + re-auth,
            // but require every member so a QRZ/ClubLog or re-auth record can never be
            // shipped through the LoTW upload button.
            const actionByIndex = new Map(
              shownDiag.diagnoses.map((d) => [d.index, d.reasons[0]?.action]),
            )
            const bucketUploadable = (indices: number[]) =>
              indices.length > 0 && indices.every((i) => isLotwUpload(actionByIndex.get(i)))
            return (
              shownDiag.buckets.length > 0 && (
                <div className="conf-buckets">
                  {shownDiag.buckets.map((b, i) => {
                    const key = `bucket-${i}`
                    const ids = bucketIds(b)
                    return (
                      <div className="conf-bucket" key={i}>
                        <span className="conf-bucket-count">{b.count}</span>
                        <span className="conf-bucket-kind">{b.kind}</span>
                        {ids && bucketUploadable(b.qsoIndices) && (
                          <button
                            className="conf-btn conf-btn-bulk"
                            disabled={busyKey !== null}
                            onClick={() => upload(ids, key)}
                          >
                            <UploadCloud size={12} aria-hidden="true" />
                            {busyKey === key
                              ? t('awards.conf.uploading')
                              : t('awards.conf.bucket.upload', { count: b.count })}
                          </button>
                        )}
                      </div>
                    )
                  })}
                </div>
              )
            )
          })()}
          {shownDiag.diagnoses.length > 0 && (
            <ul className="conf-list">
              {shownDiag.diagnoses.slice(0, SHOWN_DIAGNOSES).map((d) => {
                const r = d.reasons[0]
                return (
                  <li className="conf-row" key={d.index}>
                    <span className={`conf-code conf-${r?.code ?? 'x'}`}>{(r?.code ?? '').toUpperCase()}</span>
                    <span className="conf-expl">{r?.explanation}</span>
                    {r?.confidence === 'likely' && (
                      <span className="conf-likely">{t('awards.conf.likely')}</span>
                    )}
                    <RowAction
                      d={d}
                      busyKey={busyKey}
                      onUpload={upload}
                      onPush={push}
                      onOpenSettings={observation ? undefined : onOpenSettings}
                      observed={!!observation}
                    />
                    {/* The contact itself, in the Logbook's editor: the Logbook row's ✎, the one edit
                        path. Only for a contact the report names by id, in the station's own window. */}
                    {!observation && onOpenLogbook && d.id && d.call && (
                      <button
                        type="button"
                        className="conf-btn"
                        onClick={() => onOpenLogbook(d.call!, d.id)}
                        title={t('logbook.row.edit', { call: d.call })}
                        aria-label={t('logbook.row.edit', { call: d.call })}
                      >
                        ✎
                      </button>
                    )}
                  </li>
                )
              })}
            </ul>
          )}
          {uploadMsg && <p className="conf-msg">{uploadMsg}</p>}
          {shownDiag.waitingOnPartner > 0 && (
            <p className="conf-muted">
              {t('awards.conf.waitingOnPartner', { count: shownDiag.waitingOnPartner })}
            </p>
          )}
          {shownDiag.pendingLag > 0 && (
            <p className="conf-muted">
              {t('awards.conf.pendingLag', { count: shownDiag.pendingLag })}
            </p>
          )}
        </div>
      )}

      {showGamification && (
      <div className="aw-panel aw-achievements">
        <h3>
          <Trophy size={14} aria-hidden="true" />{' '}
          {t('awards.achievements.head', {
            unlocked: aw.achievements.filter((a) => a.unlocked).length,
            total: aw.achievements.length,
          })}
        </h3>
        <div className="aw-badges">
          {aw.achievements.map((a) => (
            <div
              className={`aw-badge${a.unlocked ? ' on' : ''}${a.critical ? ' crit' : ''}`}
              key={a.id}
              title={a.detail}
            >
              <span className="aw-badge-mark" aria-hidden="true">
                {a.unlocked ? '★' : '○'}
              </span>
              <div className="aw-badge-body">
                <span className="aw-badge-title">{a.title}</span>
                {a.unlocked ? (
                  <span className="aw-badge-detail">{a.detail}</span>
                ) : (
                  <>
                    <span className="aw-badge-detail">
                      {Math.min(a.current, a.target).toLocaleString()} / {a.target.toLocaleString()}
                    </span>
                    <div className="aw-badge-bar">
                      <div
                        className="aw-badge-fill"
                        style={{ width: `${a.target > 0 ? Math.min(100, (a.current / a.target) * 100) : 0}%` }}
                      />
                    </div>
                  </>
                )}
              </div>
            </div>
          ))}
        </div>
      </div>
      )}
    </section>
  )
}
