// The Logbook's field filters and its saved filters: a "Filters" button beside the search box, and
// the panel it opens under the search bar.
//
// It edits a filter and hands it back (`onChange`); the Logbook puts it in the query, and the
// engine answers it in SQL (`features/logFilter`). Nothing here filters a row.
//
// The saved filters are read once from the settings and written through their one writer,
// `setLogbookFilters`, the moment one is saved, renamed or deleted — the same narrow write the
// column choice has, so no settings form can put an older list back.
import { useEffect, useId, useState } from 'react'
import { getSettings, setLogbookFilters } from '../api'
import { t } from '../i18n'
import {
  activeFilterCount,
  AWARD_NEEDS,
  cleanFilter,
  cleanSavedFilters,
  CONTINENTS,
  dayStartUnix,
  MAX_FILTER_NAME,
  QSL_SERVICES,
  unixDay,
  withFilterRenamed,
  withoutFilter,
  withSavedFilter,
  type AwardNeed,
  type PathFilter,
  type LogFilter,
  type QslFilter,
  type QslService,
  type SavedLogFilter,
} from '../features/logFilter'

/** The bands the filter offers, in the order the band table lists them (`band.ts`). */
const BANDS = [
  '160m', '80m', '60m', '40m', '30m', '20m', '17m', '15m', '12m', '10m', '6m', '4m', '2m',
  '1.25m', '70cm', '33cm', '23cm', '13cm', '9cm', '6cm', '3cm', '1.25cm',
] as const

/** Modes to suggest; any other may be typed. */
const MODES = ['CW', 'SSB', 'AM', 'FM', 'FT8', 'FT4', 'RTTY', 'PSK31', 'JT65', 'Q65', 'MSK144', 'JS8', 'SSTV', 'FST4', 'WSPR'] as const

const SERVICE_LABEL: Record<QslService, string> = {
  lotw: 'LoTW',
  eqsl: 'eQSL',
  card: '',
  qrz: 'QRZ',
  clublog: 'Club Log',
}

const NEED_LABEL: Record<AwardNeed, string> = { dxcc: 'DXCC', was: 'WAS', waz: 'WAZ', vucc: 'VUCC', ffma: 'FFMA' }

interface Props {
  filter: LogFilter
  onChange: (next: LogFilter) => void
}

/** A field set to `v`, or removed when `v` is empty. */
function withField<K extends keyof LogFilter>(f: LogFilter, k: K, v: LogFilter[K] | '' | undefined): LogFilter {
  const next = { ...f }
  if (v === '' || v === undefined) delete next[k]
  else next[k] = v as LogFilter[K]
  return next
}

function withQsl(f: LogFilter, s: QslService, v: string): LogFilter {
  const qsl: QslFilter = { ...(f.qsl ?? {}) }
  if (v) (qsl as Record<string, string>)[s] = v
  else delete qsl[s]
  const next = { ...f }
  if (Object.keys(qsl).length) next.qsl = qsl
  else delete next.qsl
  return next
}

const zone = (v: string): number | undefined => {
  const n = Number(v)
  return v.trim() && Number.isInteger(n) ? n : undefined
}

export function LogFilterBar({ filter, onChange }: Props) {
  const [open, setOpen] = useState(false)
  const count = activeFilterCount(filter)
  const panelId = useId()
  return (
    <>
      <button
        type="button"
        className={`log-filter-chip${count ? ' active' : ''}`}
        aria-expanded={open}
        aria-controls={panelId}
        onClick={() => setOpen((v) => !v)}
        title={t('logbook.filters.title')}
      >
        {count ? t('logbook.filters.labelCount', { count }) : t('logbook.filters.label')}
      </button>
      {open && <LogFilterPanel id={panelId} filter={filter} onChange={onChange} />}
    </>
  )
}

function LogFilterPanel({ id, filter, onChange }: Props & { id: string }) {
  const [saved, setSaved] = useState<SavedLogFilter[]>([])
  const [chosen, setChosen] = useState('')
  const [name, setName] = useState('')
  useEffect(() => {
    let live = true
    getSettings()
      .then((s) => live && setSaved(cleanSavedFilters(s.logbookFilters)))
      .catch(() => {}) // no bridge: nothing saved to offer
    return () => {
      live = false
    }
  }, [])
  const keep = (next: SavedLogFilter[]) => {
    setSaved(next)
    void setLogbookFilters(next).catch(() => {})
  }
  const text = (k: 'call' | 'entity' | 'grid' | 'state' | 'county' | 'iota' | 'reference' | 'myRef' | 'contest' | 'text', label: string) => (
    <label className="log-filter-field">
      <span>{label}</span>
      <input
        className="settings-input"
        value={filter[k] ?? ''}
        onChange={(e) => onChange(withField(filter, k, e.target.value))}
        autoComplete="off"
        spellCheck={false}
      />
    </label>
  )
  const modesId = `${id}-modes`
  return (
    <div id={id} className="log-filter-panel" role="group" aria-label={t('logbook.filters.panel')}>
      <div className="log-filter-saved">
        <label className="log-filter-field">
          <span>{t('logbook.filters.saved')}</span>
          <select
            className="settings-input"
            value={chosen}
            onChange={(e) => {
              const pick = saved.find((s) => s.name === e.target.value)
              setChosen(e.target.value)
              if (pick) {
                setName(pick.name)
                onChange(cleanFilter(pick.filter))
              }
            }}
          >
            <option value="">{t('logbook.filters.savedNone')}</option>
            {saved.map((s) => (
              <option key={s.name} value={s.name}>
                {s.name}
              </option>
            ))}
          </select>
        </label>
        <label className="log-filter-field">
          <span>{t('logbook.filters.name')}</span>
          <input
            className="settings-input"
            value={name}
            maxLength={MAX_FILTER_NAME}
            onChange={(e) => setName(e.target.value)}
            autoComplete="off"
          />
        </label>
        <button
          type="button"
          className="log-filter-chip"
          disabled={!name.trim() || activeFilterCount(filter) === 0}
          onClick={() => {
            keep(withSavedFilter(saved, name, filter))
            setChosen(name.trim())
          }}
        >
          {t('logbook.filters.save')}
        </button>
        <button
          type="button"
          className="log-filter-chip"
          disabled={!chosen || !name.trim() || name.trim() === chosen}
          onClick={() => {
            const next = withFilterRenamed(saved, chosen, name)
            keep(next)
            if (next.some((s) => s.name === name.trim())) setChosen(name.trim())
          }}
        >
          {t('logbook.filters.rename')}
        </button>
        <button
          type="button"
          className="log-filter-chip"
          disabled={!chosen}
          onClick={() => {
            keep(withoutFilter(saved, chosen))
            setChosen('')
          }}
        >
          {t('logbook.filters.delete')}
        </button>
        <button type="button" className="log-filter-chip" disabled={activeFilterCount(filter) === 0} onClick={() => onChange({})}>
          {t('logbook.filters.clear')}
        </button>
      </div>
      <div className="log-filter-grid">
        <label className="log-filter-field">
          <span>{t('logbook.filters.call')}</span>
          <input
            className="settings-input"
            value={filter.call ?? ''}
            onChange={(e) => onChange(withField(filter, 'call', e.target.value))}
            placeholder={t('logbook.filters.callHint')}
            autoComplete="off"
            spellCheck={false}
          />
        </label>
        <label className="log-filter-field">
          <span>{t('logbook.column.band')}</span>
          <select className="settings-input" value={filter.band ?? ''} onChange={(e) => onChange(withField(filter, 'band', e.target.value))}>
            <option value="">{t('logbook.filters.any')}</option>
            {BANDS.map((b) => (
              <option key={b} value={b}>
                {b}
              </option>
            ))}
          </select>
        </label>
        <label className="log-filter-field">
          <span>{t('logbook.column.mode')}</span>
          <input
            className="settings-input"
            list={modesId}
            value={filter.mode ?? ''}
            onChange={(e) => onChange(withField(filter, 'mode', e.target.value))}
            autoComplete="off"
            spellCheck={false}
          />
          <datalist id={modesId}>
            {MODES.map((m) => (
              <option key={m} value={m} />
            ))}
          </datalist>
        </label>
        <label className="log-filter-field">
          <span>{t('logbook.filters.from')}</span>
          <input
            type="date"
            className="settings-input"
            value={unixDay(filter.fromUnix)}
            onChange={(e) => onChange(withField(filter, 'fromUnix', dayStartUnix(e.target.value)))}
          />
        </label>
        <label className="log-filter-field">
          <span>{t('logbook.filters.to')}</span>
          <input
            type="date"
            className="settings-input"
            // The filter ends where the next day starts, so the day picked is included.
            value={filter.toUnix !== undefined ? unixDay(filter.toUnix - 86_400) : ''}
            onChange={(e) => {
              const start = dayStartUnix(e.target.value)
              onChange(withField(filter, 'toUnix', start === undefined ? undefined : start + 86_400))
            }}
          />
        </label>
        {text('entity', t('logbook.filters.entity'))}
        <label className="log-filter-field">
          <span>{t('logbook.column.cont')}</span>
          <select className="settings-input" value={filter.continent ?? ''} onChange={(e) => onChange(withField(filter, 'continent', e.target.value))}>
            <option value="">{t('logbook.filters.any')}</option>
            {CONTINENTS.map((c) => (
              <option key={c} value={c}>
                {c}
              </option>
            ))}
          </select>
        </label>
        <label className="log-filter-field">
          <span>{t('logbook.column.cqz')}</span>
          <input
            type="number"
            min={1}
            max={40}
            className="settings-input"
            value={filter.cqZone ?? ''}
            onChange={(e) => onChange(withField(filter, 'cqZone', zone(e.target.value)))}
          />
        </label>
        <label className="log-filter-field">
          <span>{t('logbook.column.ituz')}</span>
          <input
            type="number"
            min={1}
            max={90}
            className="settings-input"
            value={filter.ituZone ?? ''}
            onChange={(e) => onChange(withField(filter, 'ituZone', zone(e.target.value)))}
          />
        </label>
        {text('grid', t('logbook.column.grid'))}
        {text('state', t('logbook.column.state'))}
        {text('county', t('logbook.column.county'))}
        {text('iota', t('logbook.column.iota'))}
        {text('reference', t('logbook.filters.reference'))}
        {text('myRef', t('logbook.filters.myRef'))}
        {text('contest', t('logbook.column.contest'))}
        <label className="log-filter-field">
          <span>{t('logbook.filters.need')}</span>
          <select
            className="settings-input"
            value={filter.need ?? ''}
            onChange={(e) => onChange(withField(filter, 'need', (e.target.value || undefined) as AwardNeed | undefined))}
            title={t('logbook.filters.needTitle')}
          >
            <option value="">{t('logbook.filters.any')}</option>
            {AWARD_NEEDS.map((a) => (
              <option key={a} value={a}>
                {NEED_LABEL[a]}
              </option>
            ))}
          </select>
        </label>
        <label className="log-filter-field">
          <span>{t('logbook.filters.path')}</span>
          <select
            className="settings-input"
            value={filter.path ?? ''}
            onChange={(e) => onChange(withField(filter, 'path', (e.target.value || undefined) as PathFilter | undefined))}
            title={t('logbook.filters.pathTitle')}
          >
            <option value="">{t('logbook.filters.any')}</option>
            <option value="long">{t('logbook.filters.pathLong')}</option>
            <option value="polar">{t('logbook.filters.pathPolar')}</option>
          </select>
        </label>
        {text('text', t('logbook.filters.text'))}
        {QSL_SERVICES.map((s) => (
          <label key={s} className="log-filter-field">
            <span>{s === 'card' ? t('logbook.filters.card') : SERVICE_LABEL[s]}</span>
            <select className="settings-input" value={filter.qsl?.[s] ?? ''} onChange={(e) => onChange(withQsl(filter, s, e.target.value))}>
              <option value="">{t('logbook.filters.any')}</option>
              {s !== 'clublog' && <option value="confirmed">{t('logbook.filters.confirmed')}</option>}
              <option value="sent">{t('logbook.filters.sent')}</option>
              <option value="notSent">{t('logbook.filters.notSent')}</option>
            </select>
          </label>
        ))}
      </div>
    </div>
  )
}
