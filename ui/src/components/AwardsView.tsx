// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog; a hardcoded one fails CI. What does NOT come from it: the
// AWARD NAMES in AWARD_NAMES below (DXCC, Honor Roll, WAZ, VUCC, IOTA… are the names of ARRL/CQ
// programmes, not words), the DXCC entity names, band names, mode names, CQ zones and US state
// codes the tables print (data), and the achievement titles/details and diagnosis explanations,
// which the backend writes. See the invariant-token rule in `i18n/index.ts`.

import { useEffect, useRef, useState } from 'react'
import { Trophy, CheckCircle2, Radio, Target, Layers, Send, Globe2, Award, Flag, UploadCloud, Grid3x3, Satellite } from 'lucide-react'
import type { AwardSummary, EntityNeed, DiagnosticsReport, DiagAction, QsoDiagnosis, UploadReport, LoggedQso } from '../types'
import {
  getAwards,
  getConfirmationDiagnostics,
  uploadLotwReport,
  getLog,
  qrzPushQso,
  clublogPushQso,
  eqslPushQso,
} from '../api'
import { t } from '../i18n'
import { StateBlock } from './StateBlock'

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
  satVucc: 'Sat VUCC',
  iota: 'IOTA',
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

type NeedSort = 'entity' | 'bands'

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
 * service with an in-app bulk/by-index upload path. A `reUpload` for QRZ/ClubLog
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

/** The per-QSO push targets the diagnostics can drive (each has a single-QSO
 * push command; LoTW goes through the TQSL by-index upload path instead). */
type PushService = 'QRZ' | 'ClubLog' | 'eQSL'

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
  canPush,
  onOpenSettings,
}: {
  d: QsoDiagnosis
  busyKey: string | null
  onUpload: (indices: number[], key: string) => void
  onPush: (index: number, service: PushService, key: string) => void
  /** False while the log hasn't loaded — pushes need the QSO record. */
  canPush: boolean
  /** Open Settings at a section id. Absent ⇒ the re-login row stays a guidance chip. */
  onOpenSettings?: (target: string) => void
}) {
  const a = d.reasons[0]?.action
  if (!a) return null
  const key = `row-${d.index}`
  // Only LoTW has an in-app one-click (re)upload (via TQSL) — show the live button.
  if (isLotwUpload(a)) {
    return (
      <button
        className="conf-btn"
        disabled={busyKey !== null}
        onClick={() => onUpload([d.index], key)}
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
    if (!canPush) return <span className="conf-act">{label}</span>
    return (
      <button
        className="conf-btn conf-btn-push"
        disabled={busyKey !== null}
        title={t('awards.conf.push.title', { service: svc })}
        onClick={() => onPush(d.index, svc, key)}
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

export function AwardsView({
  showGamification = true,
  onOpenSettings,
}: {
  showGamification?: boolean
  /** Open Settings at a section id (see settings/registry.ts). */
  onOpenSettings?: (target: string) => void
}) {
  const [aw, setAw] = useState<AwardSummary | null>(null)
  const [diag, setDiag] = useState<DiagnosticsReport | null>(null)
  // The log itself, so a diagnosis row (indexed oldest-first, same order as
  // get_log) can hand its QsoRecord to the per-QSO QRZ/ClubLog/eQSL push.
  const [log, setLog] = useState<LoggedQso[] | null>(null)
  const [err, setErr] = useState(false)
  // Grid list: VUCC bands only by default — see the grids panel for why. Declared up here
  // with the other hooks rather than beside its own derivations, because AwardsView early-
  // returns for the loading, error and empty-log states and a hook below those is not
  // called on every render path.
  const [gridsVuccOnly, setGridsVuccOnly] = useState(true)
  const [busyKey, setBusyKey] = useState<string | null>(null)
  const [uploadMsg, setUploadMsg] = useState<string | null>(null)
  // Guards post-await setState in upload() — TQSL signing can take seconds, during
  // which the operator may switch tabs and unmount this view.
  const mounted = useRef(true)
  useEffect(() => {
    mounted.current = true
    let live = true
    getAwards()
      .then((a) => live && setAw(a))
      .catch(() => live && setErr(true))
    getConfirmationDiagnostics()
      .then((d) => live && setDiag(d))
      .catch(() => {}) // diagnostics are a best-effort add-on; never block the dashboard
    getLog()
      .then((l) => live && setLog(l))
      .catch(() => {}) // without it the push buttons degrade to guidance chips
    return () => {
      live = false
      mounted.current = false
    }
  }, [])

  /** Sign + upload the given QSOs via TQSL, then re-diagnose so the panel reflects
   * the new state (uploaded rows drop to Pending/waiting; bounced ones show R9). */
  async function upload(indices: number[], key: string) {
    setBusyKey(key)
    setUploadMsg(null)
    try {
      const r = await uploadLotwReport(indices)
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
   * cases), then re-diagnose so the row reflects the new upload state. */
  async function push(index: number, service: PushService, key: string) {
    const q = log?.[index]
    if (!q) {
      setUploadMsg(t('awards.push.noQso'))
      return
    }
    setBusyKey(key)
    setUploadMsg(null)
    try {
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

  if (err) {
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

        {/* The ✓ gates on CARD-confirmed groups: the IOTA programme credits QSL cards
            and Club Log matching — never LoTW — so a LoTW-only confirmation tracks
            here but cannot earn the award (N9UM audit). */}
        <div
          className={`aw-card${(aw.iota.cardConfirmed ?? 0) >= IOTA_ISLANDS ? ' aw-card-elite' : ''}`}
        >
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
            {(aw.iota.cardConfirmed ?? 0) >= IOTA_ISLANDS
              ? t('awards.iota.note.achieved', { cards: aw.iota.cardConfirmed ?? 0 })
              : t('awards.iota.note.worked', {
                  worked: aw.iota.worked,
                  cards: aw.iota.cardConfirmed ?? 0,
                })}
          </span>
        </div>
      </div>

      <div className="awards-body">
        <div className="aw-left">
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
        </div>

        <div className="aw-chases">
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
        </div>
      </div>

      {diag && (diag.diagnoses.length > 0 || diag.pendingLag > 0 || diag.waitingOnPartner > 0) && (
        <div className="aw-panel conf-panel">
          <h3>
            <CheckCircle2 size={14} aria-hidden="true" /> {t('awards.conf.head')}
          </h3>
          {(diag.oneAway ?? []).length > 0 && (
            <div className="conf-oneaway">
              <span className="conf-oneaway-label">{t('awards.conf.oneAway.label')}</span>
              {diag.oneAway.slice(0, 8).map((o) => (
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
              {diag.oneAway.length > 8 && (
                <span className="conf-muted">
                  {t('awards.conf.oneAway.more', { count: diag.oneAway.length - 8 })}
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
              diag.diagnoses.map((d) => [d.index, d.reasons[0]?.action]),
            )
            const bucketUploadable = (indices: number[]) =>
              indices.length > 0 && indices.every((i) => isLotwUpload(actionByIndex.get(i)))
            return (
              diag.buckets.length > 0 && (
                <div className="conf-buckets">
                  {diag.buckets.map((b, i) => {
                    const key = `bucket-${i}`
                    return (
                      <div className="conf-bucket" key={i}>
                        <span className="conf-bucket-count">{b.count}</span>
                        <span className="conf-bucket-kind">{b.kind}</span>
                        {bucketUploadable(b.qsoIndices) && (
                          <button
                            className="conf-btn conf-btn-bulk"
                            disabled={busyKey !== null}
                            onClick={() => upload(b.qsoIndices, key)}
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
          {diag.diagnoses.length > 0 && (
            <ul className="conf-list">
              {diag.diagnoses.slice(0, 50).map((d) => {
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
                      canPush={log !== null}
                      onOpenSettings={onOpenSettings}
                    />
                  </li>
                )
              })}
            </ul>
          )}
          {uploadMsg && <p className="conf-msg">{uploadMsg}</p>}
          {diag.waitingOnPartner > 0 && (
            <p className="conf-muted">
              {t('awards.conf.waitingOnPartner', { count: diag.waitingOnPartner })}
            </p>
          )}
          {diag.pendingLag > 0 && (
            <p className="conf-muted">
              {t('awards.conf.pendingLag', { count: diag.pendingLag })}
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
