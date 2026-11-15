// ---------------------------------------------------------------------------
// ⭐ BUILD YOUR OWN CONTEST — the builder steps the contest setup (ContestSetupWizard.tsx) shows
// inside its own dialog: Basics (name, Cabrillo name, modes, when it runs), Exchange (up to five
// fields from the fixed menu), Rules (the dupe rule and points per contact) and Check (the
// station's own preview), then Save.
//
// ⭐ THE STATION DECIDES EVERY RULE. The preview (`preview_custom_contest`) compiles the draft with
// the real compiler and renders what this station would send, a sample Cabrillo line and the
// refusal, verbatim; this file checks only that a required box is filled before Next, and says
// which. Nothing is installed or written until Save.
//
// ⭐ FT8/FT4 SAY WHAT THIS BUILD DOES. The line reads the preview's `ftMessages` — in this build
// always standard messages, the exchange not sent — never what a matching exchange might one day do.
// An RST + serial exchange ASKS whether to use WSJT-X's RTTY Roundup messages (`ft_roundup`),
// defaulting to No: it is the most common club exchange, and most clubs do not mean the Roundup.
//
// ⚠️ A TYPED VALUE (age, name, text) IS NEVER IN A DEFINITION: the operator types it when the
// contest is set up, into `Settings.contestEntered`, so an exported file carries none.
//
// Layout: inside the portaled `.ui-dialog` (its one scroll owner), on the wizard's own classes.
// ---------------------------------------------------------------------------
import { useEffect, useRef, useState } from 'react'
import { t } from '../i18n'
import {
  deleteCustomContest,
  exportCustomContest,
  previewCustomContest,
  saveCustomContest,
  saveTextAs,
  type CustomContest,
  type CustomContestDef,
  type CustomContestField,
  type CustomContestFieldId,
  type CustomContestMode,
  type CustomContestPreview,
  type CustomContestWindow,
} from '../api'
import {
  CUSTOM_FIELDS,
  CUSTOM_MODES,
  FT_ACTIVITY_NAMES,
  MAX_CAPTION_CHARS,
  MAX_EXCHANGE_FIELDS,
  TOKEN_RE,
  blankCustomDef,
  customContests,
  customFieldName,
  customIdForName,
  reloadCustomContests,
} from '../features/customContests'
import { upperTyped } from '../typedInput'

/** The builder's steps, by the word each dot shows. */
const STEPS: (() => string)[] = [
  () => t('customContest.steps.basics'),
  () => t('customContest.steps.exchange'),
  () => t('customContest.steps.rules'),
  () => t('customContest.steps.check'),
]
const NTH_WEEKEND: (() => string)[] = [
  () => t('customContest.window.nth1'),
  () => t('customContest.window.nth2'),
  () => t('customContest.window.nth3'),
  () => t('customContest.window.nth4'),
]
const DUPE_WORDS: Record<CustomContestDef['dupe'], () => string> = {
  once: () => t('customContest.rules.dupe.once'),
  band: () => t('customContest.rules.dupe.band'),
  band_mode: () => t('customContest.rules.dupe.bandMode'),
}
const POINTS_WORDS: Record<'CW' | 'PH' | 'DIG', () => string> = {
  CW: () => t('customContest.rules.points.cw'),
  PH: () => t('customContest.rules.points.ph'),
  DIG: () => t('customContest.rules.points.dig'),
}
/** Month names are date formatting, tokens like the setup's own `utcStamp`. */
const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']
/** The longest a contest may run (the shared validator's bound). */
const MAX_HOURS = 72
const ONCE_RE = /^(\d{4}-\d{2}-\d{2})T(\d{2}:\d{2})Z$/

const errorText = (e: unknown) => (typeof e === 'string' ? e : e instanceof Error ? e.message : String(e))
const hasFt = (modes: readonly CustomContestMode[]) => modes.includes('FT8') || modes.includes('FT4')
const shapeIs = (exchange: readonly CustomContestField[], ...fields: CustomContestFieldId[]) =>
  exchange.length === fields.length && exchange.every((f, i) => f.field === fields[i])

/** The definition as it is saved: the FT answers only where the exchange asks them, a blank caption
 *  or short name dropped, and the id this contest is saved under. */
export function cleanCustomDef(def: CustomContestDef, id: string): CustomContestDef {
  const out: CustomContestDef = {
    ...def,
    id,
    name: def.name.trim(),
    short: def.short?.trim() ?? '',
    exchange: def.exchange.map((f) => (f.field === 'text' && f.label?.trim() ? { field: f.field, label: f.label.trim() } : { field: f.field })),
  }
  if (!(hasFt(def.modes) && shapeIs(def.exchange, 'rst', 'serial') && def.ft_roundup)) delete out.ft_roundup
  if (!(hasFt(def.modes) && shapeIs(def.exchange, 'grid4') && def.ft_cq)) delete out.ft_cq
  return out
}

/** ⭐ Why Next is refused on a step: the first required box left empty, in words, or null. */
export function builderNeed(step: number, def: CustomContestDef): string | null {
  if (step === 0) {
    if (!def.name.trim()) return t('customContest.need.name')
    if (!TOKEN_RE.test(def.cabrillo_contest)) return t('customContest.need.token')
    if (def.modes.length === 0) return t('customContest.need.modes')
    const w = def.window
    if (w.kind === 'once' && !ONCE_RE.test(w.start_utc)) return t('customContest.need.date')
    if (w.kind === 'weekend' && !(w.start_hour_utc >= 0 && w.start_hour_utc <= 47)) return t('customContest.need.startHour')
    if (!(w.duration_hours >= 1 && w.duration_hours <= MAX_HOURS)) return t('customContest.need.duration', { max: MAX_HOURS })
  }
  if (step === 1 && def.exchange.length === 0) return t('customContest.need.exchange')
  return null
}

/** ⭐ The FT8/FT4 line: what this build sends on FT8 and FT4 for this exchange (the preview's answer). */
export function ftLine(p: CustomContestPreview): string {
  if (p.ftMessages === 'standard') {
    if (p.ftActivity) return t('customContest.ft.notYet', { activity: FT_ACTIVITY_NAMES[p.ftActivity] })
    if (p.ftShape) return t('customContest.ft.standard')
    return t('customContest.ft.none')
  }
  return t('customContest.ft.standard')
}

/** A number box's value: a whole number, or 0 for a cleared box. */
const whole = (v: string) => Math.max(0, Math.floor(Number(v) || 0))

export function CustomContestBuilder({
  editing,
  onSaved,
  onCancel,
}: {
  /** The installed contest to edit; absent to build a new one. */
  editing?: CustomContest
  /** After the station saved it, with the id it is saved under. */
  onSaved: (id: string) => void
  /** Back to the pick, nothing saved. */
  onCancel: () => void
}) {
  const [def, setDef] = useState<CustomContestDef>(() => (editing ? structuredClone(editing.def) : blankCustomDef()))
  const [asNew, setAsNew] = useState(false)
  const [step, setStep] = useState(0)
  const [preview, setPreview] = useState<CustomContestPreview | null>(null)
  const [previewing, setPreviewing] = useState(true)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // Edit keeps the id the journals name it by; a new contest (or "Save as a new contest") takes one
  // from its name that no installed contest has.
  const kept = asNew ? undefined : editing
  const id = kept ? kept.def.id : customIdForName(def.name || 'contest', (c) => customContests().some((x) => x.def.id === c))
  const locked = !!kept?.exchangeLocked
  const saved = cleanCustomDef(def, id)
  const savedKey = JSON.stringify(saved)

  // The station's preview of the draft: read-only, the newest asked wins.
  const asked = useRef(0)
  useEffect(() => {
    const n = ++asked.current
    setPreviewing(true)
    previewCustomContest(saved)
      .then((p) => n === asked.current && setPreview(p ?? null))
      .catch(() => n === asked.current && setPreview(null))
      .finally(() => n === asked.current && setPreviewing(false))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [savedKey])

  const patch = (p: Partial<CustomContestDef>) => setDef((d) => ({ ...d, ...p }))
  const setWindow = (w: CustomContestWindow) => patch({ window: w })
  const setField = (i: number, f: CustomContestField) => patch({ exchange: def.exchange.map((x, j) => (j === i ? f : x)) })
  const move = (i: number, by: number) => {
    const next = [...def.exchange]
    const [row] = next.splice(i, 1)
    next.splice(i + by, 0, row)
    patch({ exchange: next })
  }
  const addField = () => {
    const used = new Set(def.exchange.map((f) => (f.field.startsWith('grid') ? 'grid' : f.field)))
    const free = CUSTOM_FIELDS.find((f) => !used.has(f.startsWith('grid') ? 'grid' : f)) ?? 'text'
    patch({ exchange: [...def.exchange, { field: free }] })
  }

  const need = builderNeed(step, def)
  const good = !!preview && !previewing && preview.refusal === ''
  const ft = hasFt(def.modes)
  const roundupAsk = ft && shapeIs(def.exchange, 'rst', 'serial')
  const cqAsk = ft && shapeIs(def.exchange, 'grid4')

  const save = async () => {
    setSaving(true)
    setError(null)
    try {
      await saveCustomContest(saved)
      await reloadCustomContests()
      onSaved(id)
    } catch (e) {
      setError(t('customContest.saveFailed', { error: errorText(e) }))
    } finally {
      setSaving(false)
    }
  }

  const ftBlock = ft && (
    <div className="cc-ft">
      {roundupAsk && (
        <div className="contest-setup-group" role="radiogroup" aria-label={t('customContest.ft.roundup.ask')}>
          <h3 className="wizard-modes-title">{t('customContest.ft.roundup.ask')}</h3>
          {([false, true] as const).map((yes) => (
            <label key={String(yes)} className="contest-setup-tick">
              <input type="radio" name="cc-roundup" checked={!!def.ft_roundup === yes} onChange={() => patch({ ft_roundup: yes })} />
              <span>{yes ? t('customContest.ft.roundup.yes') : t('customContest.ft.roundup.no')}</span>
            </label>
          ))}
        </div>
      )}
      {cqAsk && (
        <label className="wizard-field">
          <span>{t('customContest.ft.cq.label')}</span>
          <select
            value={def.ft_cq ?? ''}
            onChange={(e) => patch({ ft_cq: (e.target.value || undefined) as CustomContestDef['ft_cq'] })}
          >
            <option value="">{t('customContest.ft.cq.none')}</option>
            <option value="TEST">{t('customContest.ft.cq.test')}</option>
            <option value="WW">{t('customContest.ft.cq.ww')}</option>
          </select>
        </label>
      )}
      {good && preview && (
        <p className="wizard-field-hint" data-ft-line>
          {ftLine(preview)}
        </p>
      )}
    </div>
  )

  const w = def.window
  const once = w.kind === 'once' ? ONCE_RE.exec(w.start_utc) : null

  return (
    <>
      <div className="wizard-dots" aria-label={t('contestSetup.steps.aria', { n: step + 1, total: STEPS.length, title: STEPS[step]() })}>
        {STEPS.map((word, i) => (
          <button
            key={i}
            type="button"
            className={`wizard-dot${i === step ? ' cur' : ''}${i < step ? ' done' : ''}`}
            disabled={saving || (i > step && STEPS.slice(0, i).some((_, j) => builderNeed(j, def) !== null))}
            onClick={() => setStep(i)}
          >
            <span className="wizard-dot-n">{i + 1}</span> {word()}
          </button>
        ))}
      </div>

      {step === 0 && (
        <>
          <h2 className="wizard-title">{editing ? t('customContest.basics.titleEdit', { contest: editing.def.name }) : t('customContest.basics.title')}</h2>
          <p className="wizard-sub">{t('customContest.basics.sub')}</p>
          {locked && (
            <div className="contest-setup-warn" role="status">
              <p>{t('customContest.locked')}</p>
              <button type="button" className="settings-linkbtn" onClick={() => setAsNew(true)}>
                {t('customContest.saveAsNew')}
              </button>
            </div>
          )}
          {asNew && <p className="wizard-field-hint">{t('customContest.asNew')}</p>}
          <div className="wizard-fields">
            <label className="wizard-field">
              <span>{t('customContest.basics.name')}</span>
              <input type="text" value={def.name} maxLength={60} onChange={(e) => patch({ name: e.target.value })} autoComplete="off" />
            </label>
            <label className="wizard-field">
              <span>{t('customContest.basics.short')}</span>
              <input type="text" value={def.short ?? ''} maxLength={20} onChange={(e) => patch({ short: e.target.value })} autoComplete="off" />
            </label>
            <label className="wizard-field">
              <span>{t('customContest.basics.token')}</span>
              <input
                type="text"
                className="mono"
                value={def.cabrillo_contest}
                maxLength={32}
                disabled={locked}
                onChange={(e) => patch({ cabrillo_contest: upperTyped(e.target) })}
                autoComplete="off"
                spellCheck={false}
              />
            </label>
          </div>
          <p className="wizard-field-hint">{t('customContest.basics.tokenHint')}</p>
          <div className="contest-setup-group" role="group" aria-label={t('customContest.basics.modes')}>
            <h3 className="wizard-modes-title">{t('customContest.basics.modes')}</h3>
            <div className="cc-modes">
              {CUSTOM_MODES.map((m) => (
                <label key={m} className="contest-setup-tick">
                  <input
                    type="checkbox"
                    checked={def.modes.includes(m)}
                    onChange={() => patch({ modes: def.modes.includes(m) ? def.modes.filter((x) => x !== m) : CUSTOM_MODES.filter((x) => x === m || def.modes.includes(x)) })}
                  />
                  <span>{m}</span>
                </label>
              ))}
            </div>
            <p className="wizard-field-hint">{t('customContest.basics.modesHint')}</p>
          </div>
          <div className="contest-setup-group" role="radiogroup" aria-label={t('customContest.window.title')}>
            <h3 className="wizard-modes-title">{t('customContest.window.title')}</h3>
            <label className="contest-setup-tick">
              <input
                type="radio"
                name="cc-window"
                checked={w.kind === 'weekend'}
                onChange={() => setWindow({ kind: 'weekend', month: 1, weekend: 'nth_full', n: 1, start_hour_utc: 0, duration_hours: w.duration_hours })}
              />
              <span>{t('customContest.window.weekend')}</span>
            </label>
            <label className="contest-setup-tick">
              <input type="radio" name="cc-window" checked={w.kind === 'once'} onChange={() => setWindow({ kind: 'once', start_utc: '', duration_hours: w.duration_hours })} />
              <span>{t('customContest.window.once')}</span>
            </label>
          </div>
          <div className="wizard-fields">
            {w.kind === 'weekend' ? (
              <>
                <label className="wizard-field">
                  <span>{t('customContest.window.month')}</span>
                  <select value={w.month} onChange={(e) => setWindow({ ...w, month: Number(e.target.value) })}>
                    {MONTHS.map((m, i) => (
                      <option key={m} value={i + 1}>
                        {m}
                      </option>
                    ))}
                  </select>
                </label>
                <label className="wizard-field">
                  <span>{t('customContest.window.which')}</span>
                  <select
                    value={w.weekend === 'last_full' ? 'last' : String(w.n ?? 1)}
                    onChange={(e) =>
                      setWindow(
                        e.target.value === 'last'
                          ? { ...w, weekend: 'last_full', n: undefined }
                          : { ...w, weekend: 'nth_full', n: Number(e.target.value) },
                      )
                    }
                  >
                    {NTH_WEEKEND.map((word, i) => (
                      <option key={i} value={i + 1}>
                        {word()}
                      </option>
                    ))}
                    <option value="last">{t('customContest.window.last')}</option>
                  </select>
                </label>
                <label className="wizard-field">
                  <span>{t('customContest.window.startHour')}</span>
                  <input
                    type="number"
                    min={0}
                    max={47}
                    value={w.start_hour_utc}
                    onChange={(e) => setWindow({ ...w, start_hour_utc: whole(e.target.value) })}
                  />
                </label>
              </>
            ) : (
              <>
                <label className="wizard-field">
                  <span>{t('customContest.window.date')}</span>
                  <input
                    type="date"
                    value={once?.[1] ?? w.start_utc.slice(0, 10)}
                    onChange={(e) => setWindow({ ...w, start_utc: `${e.target.value}T${once?.[2] ?? '00:00'}Z` })}
                  />
                </label>
                <label className="wizard-field">
                  <span>{t('customContest.window.time')}</span>
                  <input
                    type="time"
                    value={once?.[2] ?? ''}
                    onChange={(e) => setWindow({ ...w, start_utc: `${once?.[1] ?? ''}T${e.target.value}Z` })}
                  />
                </label>
              </>
            )}
            <label className="wizard-field">
              <span>{t('customContest.window.hours')}</span>
              <input
                type="number"
                min={1}
                max={MAX_HOURS}
                value={w.duration_hours || ''}
                onChange={(e) => setWindow({ ...w, duration_hours: whole(e.target.value) })}
              />
            </label>
          </div>
          {w.kind === 'weekend' && <p className="wizard-field-hint">{t('customContest.window.startHint')}</p>}
        </>
      )}

      {step === 1 && (
        <>
          <h2 className="wizard-title">{t('customContest.exchange.title')}</h2>
          <p className="wizard-sub">{t('customContest.exchange.sub', { max: MAX_EXCHANGE_FIELDS })}</p>
          {locked && <p className="contest-setup-warn">{t('customContest.locked')}</p>}
          <ol className="cc-rows">
            {def.exchange.map((f, i) => (
              <li key={i} className="cc-row" data-cc-row>
                <select
                  aria-label={t('customContest.exchange.fieldN', { n: i + 1 })}
                  value={f.field}
                  disabled={locked}
                  onChange={(e) => setField(i, { field: e.target.value as CustomContestFieldId })}
                >
                  {CUSTOM_FIELDS.map((id) => (
                    <option key={id} value={id}>
                      {customFieldName(id)}
                    </option>
                  ))}
                </select>
                {f.field === 'text' && (
                  <input
                    type="text"
                    aria-label={t('customContest.exchange.caption')}
                    placeholder={t('customContest.exchange.caption')}
                    value={f.label ?? ''}
                    maxLength={MAX_CAPTION_CHARS}
                    disabled={locked}
                    onChange={(e) => setField(i, { ...f, label: e.target.value })}
                    autoComplete="off"
                  />
                )}
                <span className="cc-row-tools">
                  <button type="button" className="settings-linkbtn" disabled={locked || i === 0} onClick={() => move(i, -1)}>
                    {t('customContest.exchange.up')}
                  </button>
                  <button type="button" className="settings-linkbtn" disabled={locked || i === def.exchange.length - 1} onClick={() => move(i, 1)}>
                    {t('customContest.exchange.down')}
                  </button>
                  <button
                    type="button"
                    className="settings-linkbtn"
                    disabled={locked}
                    onClick={() => patch({ exchange: def.exchange.filter((_, j) => j !== i) })}
                  >
                    {t('customContest.exchange.remove')}
                  </button>
                </span>
              </li>
            ))}
          </ol>
          <button type="button" className="wizard-skip" disabled={locked || def.exchange.length >= MAX_EXCHANGE_FIELDS} onClick={addField}>
            {t('customContest.exchange.add')}
          </button>
          {def.exchange.length >= MAX_EXCHANGE_FIELDS && <p className="wizard-field-hint">{t('customContest.exchange.full', { max: MAX_EXCHANGE_FIELDS })}</p>}
          {def.exchange.some((f) => f.field === 'age' || f.field === 'name' || f.field === 'text') && (
            <p className="wizard-field-hint">{t('customContest.exchange.typed')}</p>
          )}
          {ftBlock}
          {!previewing && preview && preview.refusal && (
            <p className="contest-setup-warn" role="status">
              {preview.refusal}
            </p>
          )}
        </>
      )}

      {step === 2 && (
        <>
          <h2 className="wizard-title">{t('customContest.rules.title')}</h2>
          <div className="contest-setup-group" role="radiogroup" aria-label={t('customContest.rules.dupe')}>
            <h3 className="wizard-modes-title">{t('customContest.rules.dupe')}</h3>
            {(['once', 'band', 'band_mode'] as const).map((d) => (
              <label key={d} className="contest-setup-tick">
                <input type="radio" name="cc-dupe" checked={def.dupe === d} onChange={() => patch({ dupe: d })} />
                <span>{DUPE_WORDS[d]()}</span>
              </label>
            ))}
          </div>
          <h3 className="wizard-modes-title">{t('customContest.rules.points')}</h3>
          <div className="wizard-fields">
            {(['CW', 'PH', 'DIG'] as const).map((k) => (
              <label key={k} className="wizard-field">
                <span>{POINTS_WORDS[k]()}</span>
                <input
                  type="number"
                  min={0}
                  max={100}
                  value={def.points?.[k] ?? 1}
                  onChange={(e) => patch({ points: { ...def.points, [k]: Math.min(100, whole(e.target.value)) } })}
                />
              </label>
            ))}
          </div>
          <p className="wizard-field-hint">{t('customContest.rules.pointsHint')}</p>
        </>
      )}

      {step === 3 && (
        <>
          <h2 className="wizard-title">{t('customContest.check.title', { contest: def.name.trim() })}</h2>
          {previewing ? (
            <p className="wizard-field-hint">{t('contestSetup.sends.checking')}</p>
          ) : !preview ? (
            <p className="contest-setup-bad" role="alert">
              {t('customContest.check.failed')}
            </p>
          ) : preview.refusal ? (
            <div className="contest-setup-bad" role="alert" data-refusal>
              <p>{t('customContest.check.refused')}</p>
              <p className="mono">{preview.refusal}</p>
            </div>
          ) : (
            <>
              {preview.youSend && <p className="contest-setup-exchange mono">{t('contestSetup.sends.exchange', { exchange: preview.youSend })}</p>}
              {preview.startProblem && (
                <p className="contest-setup-warn" role="status">
                  {preview.startProblem}
                </p>
              )}
              {preview.sampleQsoLine && (
                <>
                  <p className="wizard-field-hint">{t('customContest.check.sample')}</p>
                  <p className="cc-sample mono">{preview.sampleQsoLine}</p>
                </>
              )}
              <p className="wizard-field-hint">{t('customContest.check.scoring')}</p>
            </>
          )}
          {ftBlock}
        </>
      )}

      {need && (
        <p className="contest-setup-bad" role="alert" data-need>
          {need}
        </p>
      )}
      {error && (
        <p className="contest-setup-bad" role="alert">
          {error}
        </p>
      )}

      <div className="wizard-actions">
        <div className="wizard-actions-right">
          <button type="button" className="wizard-skip" disabled={saving} onClick={() => (step > 0 ? setStep(step - 1) : onCancel())}>
            {t('contestSetup.nav.back')}
          </button>
          <button type="button" className="wizard-skip" disabled={saving} onClick={onCancel}>
            {t('contestSetup.nav.cancel')}
          </button>
          {step < STEPS.length - 1 ? (
            <button type="button" className="wizard-go" disabled={need !== null} onClick={() => setStep(step + 1)}>
              {t('contestSetup.nav.next')}
            </button>
          ) : (
            <button type="button" className="wizard-go" disabled={saving || !good} onClick={() => void save()}>
              {saving ? t('customContest.saving') : t('customContest.save')}
            </button>
          )}
        </div>
      </div>
    </>
  )
}

/** ⭐ Edit, Export… and Delete for one of the operator's contests — the pick list's and Settings'.
 *  Delete asks first, in place; the station refuses it while the contest is the picked one and says
 *  so. Export saves the definition where the operator picks (the OS save dialog). */
export function CustomContestActions({
  contest,
  onEdit,
  onDeleted,
  onNote,
}: {
  contest: CustomContest
  onEdit: () => void
  onDeleted?: (id: string) => void
  /** A line to show the operator: what was saved where, or why something was refused. */
  onNote: (note: { text: string; bad?: boolean }) => void
}) {
  const [asking, setAsking] = useState(false)
  const name = contest.def.name
  const doExport = async () => {
    try {
      const text = await exportCustomContest(contest.def.id)
      const path = await saveTextAs(`${contest.def.id.replace(/^custom_/, '')}.nexus-contest.json`, text)
      if (path) onNote({ text: t('customContest.exported', { contest: name, path }) })
    } catch (e) {
      onNote({ text: t('customContest.exportFailed', { contest: name, error: errorText(e) }), bad: true })
    }
  }
  const doDelete = async () => {
    setAsking(false)
    try {
      await deleteCustomContest(contest.def.id)
      await reloadCustomContests()
      onNote({ text: t('customContest.deleted', { contest: name }) })
      onDeleted?.(contest.def.id)
    } catch (e) {
      onNote({ text: t('customContest.deleteFailed', { contest: name, error: errorText(e) }), bad: true })
    }
  }
  return (
    <span className="cc-actions">
      {asking ? (
        <>
          <span>{t('customContest.delete.ask', { contest: name })}</span>
          <button type="button" className="settings-linkbtn" onClick={() => void doDelete()}>
            {t('customContest.delete.yes')}
          </button>
          <button type="button" className="settings-linkbtn" onClick={() => setAsking(false)}>
            {t('customContest.delete.no')}
          </button>
        </>
      ) : (
        <>
          <button type="button" className="settings-linkbtn" onClick={onEdit}>
            {t('customContest.action.edit')}
          </button>
          <button type="button" className="settings-linkbtn" onClick={() => void doExport()}>
            {t('customContest.action.export')}
          </button>
          <button type="button" className="settings-linkbtn" onClick={() => setAsking(true)}>
            {t('customContest.action.delete')}
          </button>
        </>
      )}
    </span>
  )
}
