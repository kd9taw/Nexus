// The Awards view's station-side panels: the whole-log DXCC chart, What to QSL, and the custom
// awards the operator defines. Each reads its own command, so none of them renders on an
// observed (Remote) summary.
//
// ⚠️ ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts): every operator-visible string comes
// from the catalog. Not from it: the award names (programme names, invariant tokens), entity
// names, band and mode names, calls, prefixes, grids and references (data).

import { useEffect, useMemo, useRef, useState } from 'react'
import { Grid3x3, Send, Award, Trees } from 'lucide-react'
import type {
  CustomAward,
  CustomAwardsView,
  CustomConfirm,
  CustomKind,
  DxccChart,
  QslFor,
  QslWant,
} from '../types'
import type { ActivatorStats, PotaActivator } from '../api'
import {
  exportCustomAwards,
  getCustomAwards,
  getDxccChart,
  getPotaActivator,
  getWhatToQsl,
  readCustomAwardsFile,
  saveTextToDownloads,
  setCustomAwards,
} from '../api'
import { t } from '../i18n'
import { utcDate } from '../features/utcLog'
import { askAgainWhileSaving } from '../features/notAnswered'
import { pushToast, withErrorToast } from '../toast'

/** The programmes' own names — invariant tokens, never translated. */
const QSL_FOR_NAMES: Record<QslFor, string> = {
  dxccEntity: 'DXCC',
  dxccBand: 'Challenge',
  waz: 'WAZ',
  was: 'WAS',
  wap: 'WAP',
  wae: 'WAE',
  vucc: 'VUCC',
  wpx: 'WPX',
}

/** A chart cell's bits for each mode filter (worked, confirmed). */
export type ChartMode = 'all' | 'cw' | 'phone' | 'digital'
const CHART_BITS: Record<ChartMode, [number, number]> = {
  all: [1 | 4 | 16, 2 | 8 | 32],
  cw: [1, 2],
  phone: [4, 8],
  digital: [16, 32],
}
const CHART_MODES: ChartMode[] = ['all', 'cw', 'phone', 'digital']

/** A chart cell's standing under a mode filter. */
export function chartCell(cell: number, mode: ChartMode): 'confirmed' | 'worked' | 'needed' {
  const [w, c] = CHART_BITS[mode]
  if (cell & c) return 'confirmed'
  if (cell & w) return 'worked'
  return 'needed'
}

/** Read a command once on mount, asked again while a contact just logged is still being saved. */
function useStationRead<T>(read: () => Promise<T>): [T | null, boolean, () => void] {
  const [value, setValue] = useState<T | null>(null)
  const [failed, setFailed] = useState(false)
  const [tick, setTick] = useState(0)
  useEffect(() => {
    let live = true
    askAgainWhileSaving(read, () => live)
      .then((v) => live && setValue(v))
      .catch(() => live && setFailed(true))
    return () => {
      live = false
    }
    // `read` is a module function; the tick re-reads after a change made here.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tick])
  return [value, failed, () => setTick((n) => n + 1)]
}

/** The whole-log DXCC chart: every current entity × every DXCC band, shaded worked / confirmed /
 *  needed under a mode filter, with the entities filtered by name. */
export function DxccChartPanel() {
  const [chart, failed] = useStationRead<DxccChart>(getDxccChart)
  const [mode, setMode] = useState<ChartMode>('all')
  const [q, setQ] = useState('')
  const [onlyNeeded, setOnlyNeeded] = useState(false)
  const rows = useMemo(() => {
    if (!chart) return []
    const needle = q.trim().toLowerCase()
    return chart.rows.filter(
      (r) =>
        (!needle || r.entity.toLowerCase().includes(needle)) &&
        (!onlyNeeded || r.cells.some((c) => chartCell(c, mode) !== 'confirmed')),
    )
  }, [chart, q, onlyNeeded, mode])
  const label: Record<ChartMode, string> = {
    all: t('awards.chart.mode.all'),
    cw: t('awards.chart.mode.cw'),
    phone: t('awards.chart.mode.phone'),
    digital: t('awards.chart.mode.digital'),
  }
  const stateLabel = {
    confirmed: t('awards.chart.cell.confirmed'),
    worked: t('awards.chart.cell.worked'),
    needed: t('awards.chart.cell.needed'),
  }
  return (
    <div className="aw-panel aw-chart-panel">
      <h3>
        <Grid3x3 size={14} aria-hidden="true" /> {t('awards.chart.head')}
      </h3>
      {failed ? (
        <p className="aw-empty">{t('awards.chart.error')}</p>
      ) : !chart ? (
        <p className="aw-empty">{t('awards.chart.loading')}</p>
      ) : (
        <>
          <div className="aw-needctl" role="group" aria-label={t('awards.chart.mode.label')}>
            {CHART_MODES.map((m) => (
              <button
                type="button"
                key={m}
                className={`aw-needsort${mode === m ? ' active' : ''}`}
                aria-pressed={mode === m}
                onClick={() => setMode(m)}
              >
                {label[m]}
              </button>
            ))}
            <label className="aw-chart-only">
              <input type="checkbox" checked={onlyNeeded} onChange={(e) => setOnlyNeeded(e.target.checked)} />{' '}
              {t('awards.chart.onlyNeeded')}
            </label>
            <input
              className="aw-needfilter"
              type="search"
              value={q}
              placeholder={t('awards.needList.filter.placeholder')}
              aria-label={t('awards.needList.filter.label')}
              onChange={(e) => setQ(e.target.value)}
            />
          </div>
          <p className="aw-empty">
            <span className="aw-cc" data-s="confirmed" /> {stateLabel.confirmed}{' '}
            <span className="aw-cc" data-s="worked" /> {stateLabel.worked}{' '}
            <span className="aw-cc" data-s="needed" /> {stateLabel.needed}
          </p>
          <div className="aw-chart-scroll">
            <table className="aw-chart">
              <thead>
                <tr>
                  <th scope="col">{t('awards.chart.col.entity')}</th>
                  {chart.bands.map((b) => (
                    <th scope="col" key={b}>
                      {b}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {rows.map((r) => (
                  <tr key={r.entity}>
                    <th scope="row" data-s={chartCell(r.any, mode)}>
                      {r.entity}
                    </th>
                    {r.cells.map((c, i) => {
                      const s = chartCell(c, mode)
                      return (
                        <td key={chart.bands[i]} className="aw-cc" data-s={s} title={t('awards.chart.cell.title', { entity: r.entity, band: chart.bands[i], state: stateLabel[s] })} />
                      )
                    })}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </>
      )}
    </div>
  )
}

/** How many What-to-QSL rows show before the operator asks for the rest. */
const QSL_ROWS_SHOWN = 100

/** What to QSL: the unconfirmed contacts whose confirmation fills a slot an award still needs,
 *  most valuable first, with the QSL route where the log holds one. Read-only. */
export function WhatToQslPanel() {
  const [wants, failed] = useStationRead<QslWant[]>(getWhatToQsl)
  const [all, setAll] = useState(false)
  const shown = wants ? (all ? wants : wants.slice(0, QSL_ROWS_SHOWN)) : []
  return (
    <div className="aw-panel aw-qsl-panel">
      <h3>
        <Send size={14} aria-hidden="true" /> {t('awards.qsl.head', { count: wants?.length ?? 0 })}
      </h3>
      {failed ? (
        <p className="aw-empty">{t('awards.qsl.error')}</p>
      ) : !wants ? (
        <p className="aw-empty">{t('awards.chart.loading')}</p>
      ) : wants.length === 0 ? (
        <p className="aw-empty">{t('awards.qsl.empty')}</p>
      ) : (
        <>
          <p className="aw-empty">{t('awards.qsl.note')}</p>
          <ul className="aw-needed aw-qsl">
            {shown.map((w) => (
              <li key={`${w.call}-${w.whenUnix}-${w.band}-${w.mode}`}>
                <span className="aw-entity">
                  {w.call} · {utcDate(w.whenUnix)} · {w.band} {w.mode}
                  <span className="aw-qsl-via">
                    {' · '}
                    {w.qslVia ? t('awards.qsl.via', { route: w.qslVia }) : t('awards.qsl.noRoute')}
                  </span>
                </span>
                <span className="aw-needbands">
                  {w.slots.map((s) => (
                    <span className="aw-chip" data-award={s.award} key={`${s.award}-${s.item}`}>
                      {QSL_FOR_NAMES[s.award]} {s.item}
                    </span>
                  ))}
                </span>
              </li>
            ))}
          </ul>
          {!all && wants.length > QSL_ROWS_SHOWN && (
            <button type="button" className="aw-needsort" onClick={() => setAll(true)}>
              {t('awards.qsl.showAll', { count: wants.length })}
            </button>
          )}
        </>
      )}
    </div>
  )
}

/** POTA's achievement names — the programme's own, invariant tokens, never translated. */
const POTA_NAMES = { repeat: 'Repeat Offender', kilo: 'Kilo', rover: 'Rover' }
/** POTA's Rover tiers: parks validly activated in one UTC day, by name (docs.pota.app). */
const ROVER_TIERS: Array<[number, string]> = [
  [5, 'Warthog'],
  [10, 'Rhino'],
  [15, 'Cheetah'],
  [20, 'Ostrich'],
  [25, 'Leopard'],
  [30, 'Lion'],
]
/** Repeat Offender: a tier per this many valid activations of one park; Kilo: contacts from one. */
const REPEAT_STEP = 20
const KILO_QSOS = 1000
const POTA_PARKS_SHOWN = 10

/** One callsign's achievements, each counted as POTA defines it (`tempo_core::pota_awards`). */
function ActivatorBlock({ s, many }: { s: ActivatorStats; many: boolean }) {
  const [all, setAll] = useState(false)
  // The park nearest its next Repeat Offender tier is the one with the most activations; Kilo's
  // is the one with the most contacts.
  const repeat = s.parks[0]
  const kilo = s.parks.reduce<ActivatorStats['parks'][number] | undefined>(
    (best, p) => (!best || p.qsos > best.qsos ? p : best),
    undefined,
  )
  const kilos = s.parks.filter((p) => p.qsos >= KILO_QSOS).length
  const shown = all ? s.parks : s.parks.slice(0, POTA_PARKS_SHOWN)
  return (
    <div className="aw-pota-call">
      {many && (
        <h4>{s.callsign ? t('awards.pota.call', { call: s.callsign }) : t('awards.pota.noCall')}</h4>
      )}
      <p className="aw-empty">
        {t('awards.pota.summary', { activations: s.activations, parks: s.parksActivated, qsos: s.qsos })}
        {s.attempts > 0 && <> · {t('awards.pota.attempts', { count: s.attempts })}</>}
      </p>
      <ul className="aw-needed aw-pota-ladders">
        <li>
          <span className="aw-entity">{t('awards.pota.parksLabel')}</span>
          <span className="aw-needbands">
            {s.nextParkTier !== null
              ? t('awards.pota.parksNext', { count: s.parksActivated, next: s.nextParkTier })
              : t('awards.pota.parksDone', { count: s.parksActivated })}
          </span>
        </li>
        <li>
          <span className="aw-entity">{POTA_NAMES.repeat}</span>
          <span className="aw-needbands">
            {repeat && repeat.activations > 0
              ? t('awards.pota.repeat', {
                  park: repeat.reference,
                  count: repeat.activations,
                  next: (Math.floor(repeat.activations / REPEAT_STEP) + 1) * REPEAT_STEP,
                })
              : t('awards.pota.repeatNone')}
          </span>
        </li>
        <li>
          <span className="aw-entity">{POTA_NAMES.kilo}</span>
          <span className="aw-needbands">
            {kilo && t('awards.pota.kilo', { park: kilo.reference, count: kilo.qsos, target: KILO_QSOS })}
            {kilos > 0 && <> · {t('awards.pota.kiloDone', { count: kilos })}</>}
          </span>
        </li>
        <li>
          <span className="aw-entity">{POTA_NAMES.rover}</span>
          <span className="aw-needbands">
            {s.roverBest > 0 && s.roverBestDate
              ? t('awards.pota.rover', { count: s.roverBest, date: s.roverBestDate })
              : t('awards.pota.roverNone')}
            {ROVER_TIERS.map(([n, name], k) =>
              (s.roverDays[k] ?? 0) > 0 ? (
                <span className="aw-chip" key={name} title={t('awards.pota.roverTier', { count: n })}>
                  {name} × {s.roverDays[k]}
                </span>
              ) : null,
            )}
          </span>
        </li>
      </ul>
      <table className="aw-bn-table aw-pota-parks">
        <thead>
          <tr>
            <th>{t('awards.pota.col.park')}</th>
            <th>{t('awards.pota.col.activations')}</th>
            <th>{t('awards.pota.col.attempts')}</th>
            <th>{t('awards.pota.col.qsos')}</th>
            <th>{t('awards.pota.col.last')}</th>
          </tr>
        </thead>
        <tbody>
          {shown.map((p) => (
            <tr key={p.reference}>
              <td>
                {p.reference}
                {p.name && <span className="aw-qsl-via"> · {p.name}</span>}
              </td>
              <td>{p.activations}</td>
              <td>{p.attempts}</td>
              <td>{p.qsos}</td>
              <td>{p.lastDate}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {!all && s.parks.length > POTA_PARKS_SHOWN && (
        <button type="button" className="aw-needsort" onClick={() => setAll(true)}>
          {t('awards.pota.showAll', { count: s.parks.length })}
        </button>
      )}
    </div>
  )
}

/** POTA's activator achievements, counted from the log: valid activations, parks activated,
 *  Repeat Offender, Kilo and Rover, per callsign the activations were worked under. */
export function PotaActivatorPanel() {
  const [pota, failed] = useStationRead<PotaActivator>(getPotaActivator)
  const calls = pota?.calls ?? []
  return (
    <div className="aw-panel aw-pota-panel">
      <h3>
        <Trees size={14} aria-hidden="true" /> {t('awards.pota.head')}
      </h3>
      {failed ? (
        <p className="aw-empty">{t('awards.pota.error')}</p>
      ) : !pota ? (
        <p className="aw-empty">{t('awards.chart.loading')}</p>
      ) : calls.length === 0 ? (
        <p className="aw-empty">{t('awards.pota.empty')}</p>
      ) : (
        <>
          <p className="aw-empty">{t('awards.pota.note')}</p>
          {calls.map((s) => (
            <ActivatorBlock key={s.callsign ?? ''} s={s} many={calls.length > 1} />
          ))}
        </>
      )}
    </div>
  )
}

const KINDS: CustomKind[] = ['call', 'prefix', 'grid', 'reference', 'field']

/** Example values in the editor's boxes: an ADIF field name, band labels and modes — data. */
const FIELD_EXAMPLE = 'CNTY'
const BANDS_EXAMPLE = '20m, 40m'
const MODES_EXAMPLE = 'CW, FT8'


/** A list typed by the operator: commas, spaces or new lines between items. */
function items(text: string): string[] {
  return text
    .split(/[\s,;]+/)
    .map((s) => s.trim())
    .filter(Boolean)
}

/** A field award's items are values, which may hold commas and spaces ("CT,Hartford"): one a
 *  line. */
function lines(text: string): string[] {
  return text
    .split('\n')
    .map((s) => s.trim())
    .filter(Boolean)
}

const BLANK: CustomAward = {
  id: '',
  name: '',
  kind: 'call',
  field: '',
  targets: [],
  bands: [],
  modes: [],
  confirmation: 'lotwCard',
}

/** The custom awards: each one's standing, and the editor that defines, changes, removes,
 *  imports and exports them. */
export function CustomAwardsPanel() {
  const [view, failed, reread] = useStationRead<CustomAwardsView>(getCustomAwards)
  const [draft, setDraft] = useState<CustomAward | null>(null)
  const [targets, setTargets] = useState('')
  const fileRef = useRef<HTMLInputElement>(null)
  const awards = view?.awards ?? []

  const keep = async (next: CustomAward[]) => {
    const kept = await withErrorToast(() => setCustomAwards(next), t('awards.custom.saveFailed'))
    if (kept) {
      reread()
      return true
    }
    return false
  }
  const edit = (a: CustomAward) => {
    setDraft(a)
    setTargets(a.targets.join(a.kind === 'field' ? '\n' : ', '))
  }
  const save = async () => {
    if (!draft) return
    const one = { ...draft, targets: draft.kind === 'field' ? lines(targets) : items(targets) }
    const next = draft.id ? awards.map((a) => (a.id === draft.id ? one : a)) : [...awards, one]
    if (await keep(next)) setDraft(null)
  }
  const onImport = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    e.target.value = ''
    if (!file) return
    const read = await withErrorToast(async () => readCustomAwardsFile(await file.text()), t('awards.custom.importFailed'))
    if (!read) return
    // Imported awards join the station's own; ids are made fresh where they collide.
    if (await keep([...awards, ...read.map((a) => ({ ...a, id: '' }))])) {
      pushToast(t('awards.custom.imported', { count: read.length }), 'success')
    }
  }
  const onExport = () =>
    withErrorToast(async () => {
      const stamp = new Date().toISOString().slice(0, 10)
      // The FILE NAME is invariant: a translated word in it breaks a non-Latin locale's name.
      const path = await saveTextToDownloads(`nexus-custom-awards-${stamp}.json`, await exportCustomAwards())
      pushToast(t('awards.custom.exported', { path }), 'success')
    }, t('awards.custom.exportFailed'))

  const kindLabel: Record<CustomKind, string> = {
    call: t('awards.custom.kind.call'),
    prefix: t('awards.custom.kind.prefix'),
    grid: t('awards.custom.kind.grid'),
    reference: t('awards.custom.kind.reference'),
    field: t('awards.custom.kind.field'),
  }
  return (
    <div className="aw-panel aw-custom-panel">
      <h3>
        <Award size={14} aria-hidden="true" /> {t('awards.custom.head', { count: awards.length })}
      </h3>
      {failed && <p className="aw-empty">{t('awards.custom.error')}</p>}
      {view && awards.length === 0 && !draft && <p className="aw-empty">{t('awards.custom.empty')}</p>}
      {view && (
        <ul className="aw-needed">
          {view.progress.map((p) => (
            <li key={p.id} data-award={p.id}>
              <span className="aw-entity">
                {p.name} · {t('awards.custom.standing', { confirmed: p.confirmed, worked: p.worked, total: p.total })}
              </span>
              <span className="aw-needbands">
                <button
                  type="button"
                  className="aw-needsort"
                  onClick={() => {
                    const a = awards.find((x) => x.id === p.id)
                    if (a) edit(a)
                  }}
                >
                  {t('awards.custom.edit')}
                </button>
                <button
                  type="button"
                  className="aw-needsort"
                  onClick={() => keep(awards.filter((x) => x.id !== p.id))}
                >
                  {t('awards.custom.remove')}
                </button>
              </span>
              {p.unconfirmed.length > 0 && (
                <p className="aw-bn-list" data-kind="unconfirmed">
                  <strong>{t('awards.custom.unconfirmed', { count: p.unconfirmed.length })}</strong>{' '}
                  {p.unconfirmed.join(' · ')}
                </p>
              )}
              {p.unworked.length > 0 && (
                <details>
                  <summary>{t('awards.custom.unworked', { count: p.unworked.length })}</summary>
                  <p className="aw-bn-list">{p.unworked.join(' · ')}</p>
                </details>
              )}
            </li>
          ))}
        </ul>
      )}
      {draft ? (
        <div className="aw-custom-form">
          <label>
            {t('awards.custom.name')}
            <input value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} />
          </label>
          <label>
            {t('awards.custom.kind')}
            <select value={draft.kind} onChange={(e) => setDraft({ ...draft, kind: e.target.value as CustomKind })}>
              {KINDS.map((k) => (
                <option key={k} value={k}>
                  {kindLabel[k]}
                </option>
              ))}
            </select>
          </label>
          {draft.kind === 'field' && (
            <label>
              {t('awards.custom.field')}
              <input
                value={draft.field}
                placeholder={FIELD_EXAMPLE}
                onChange={(e) => setDraft({ ...draft, field: e.target.value })}
              />
            </label>
          )}
          <label>
            {draft.kind === 'field' ? t('awards.custom.targets.lines') : t('awards.custom.targets')}
            <textarea rows={4} value={targets} onChange={(e) => setTargets(e.target.value)} />
          </label>
          <label>
            {t('awards.custom.bands')}
            <input
              value={draft.bands.join(', ')}
              placeholder={BANDS_EXAMPLE}
              onChange={(e) => setDraft({ ...draft, bands: items(e.target.value) })}
            />
          </label>
          <label>
            {t('awards.custom.modes')}
            <input
              value={draft.modes.join(', ')}
              placeholder={MODES_EXAMPLE}
              onChange={(e) => setDraft({ ...draft, modes: items(e.target.value) })}
            />
          </label>
          <label>
            {t('awards.custom.confirm')}
            <select
              value={draft.confirmation}
              onChange={(e) => setDraft({ ...draft, confirmation: e.target.value as CustomConfirm })}
            >
              <option value="lotwCard">{t('awards.custom.confirm.lotwCard')}</option>
              <option value="any">{t('awards.custom.confirm.any')}</option>
            </select>
          </label>
          <div className="aw-needctl">
            <button type="button" className="aw-needsort active" onClick={save}>
              {t('awards.custom.save')}
            </button>
            <button type="button" className="aw-needsort" onClick={() => setDraft(null)}>
              {t('awards.custom.cancel')}
            </button>
          </div>
        </div>
      ) : (
        <div className="aw-needctl">
          <button type="button" className="aw-needsort" onClick={() => edit(BLANK)} disabled={!view}>
            {t('awards.custom.add')}
          </button>
          <input ref={fileRef} type="file" accept=".json" style={{ display: 'none' }} onChange={onImport} />
          <button type="button" className="aw-needsort" onClick={() => fileRef.current?.click()} disabled={!view}>
            {t('awards.custom.import')}
          </button>
          <button type="button" className="aw-needsort" onClick={onExport} disabled={awards.length === 0}>
            {t('awards.custom.export')}
          </button>
        </div>
      )}
    </div>
  )
}
