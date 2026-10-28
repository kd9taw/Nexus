// ---------------------------------------------------------------------------
// ⭐ THE RADIO SETUP — "Set up radios…": a station with more than one radio walks through which radio
// does what, in the shape of the contest setup (a dialog with steps, one save at the end):
//   1. Your radios      names, on/off, model, port, and the CAT state the station reports.
//   2. Bands            which radio for which band (each radio's band list; no list = every band).
//   3. Shared bands     where two radios list a band: which one for each mode class (skipped if none).
//   4. Everything else  the default radio for a band no radio lists.
//   5. Switching        what happens when Nexus needs the other radio: switch and say so, or silently.
//   6. Audio, keying    per radio, what it uses, and a Test CAT for each one.
//   7. Check            "Where would this go?" and every rule Finish writes, in plain words. Finish.
//
// It is OFFERED, never forced: Settings ▸ Radio ▸ Radios has the button, and the first-run setup offers
// it once a second radio is added. Nothing is imposed on a station that never runs it.
//
// ⭐ CANCEL WRITES NOTHING. Until Finish every answer is a draft. The two things it asks the station
// before then are read-only: "Where would this go?" (`route_preview_draft`, the engine's own routing
// function over a copy of the settings with the draft laid on it, never a second copy of the rules)
// and Test CAT (the radio in use: `test_cat`; any other radio: `test_radio_cat`, which reads the radio
// monitor's next reading of it and never selects, switches or keys a radio).
//
// ⭐ FINISH WRITES WHAT CHANGED, THROUGH THE VERBS SETTINGS USES: rename_radio, set_radio_enabled,
// set_radio_bands, set_routing_rules, set_default_radio, set_radio_switch_policy. Never a whole
// settings save (the flat-mirror and stale-form hazards). An untouched setup writes nothing.
//
// ⛔ NO TRANSMIT BEHAVIOUR CHANGES HERE. A radio switch Nexus makes by itself is the engine's as
// before: it stops transmitting, and a contact in progress ends with its log line set aside (Restore
// brings it back). This page only says so. Audio devices and keying are shown, not edited: they are
// changed with the radio's Edit button, where the per-radio form writes them.
// ---------------------------------------------------------------------------
import { useEffect, useMemo, useState } from 'react'
import { Dialog } from './ui/Dialog'
import { t } from '../i18n'
import type { AppSnapshot, RouteMode, Settings } from '../types'
import {
  getSnapshot,
  renameRadio,
  routePreviewDraft,
  setDefaultRadio,
  setRadioBands,
  setRadioEnabled,
  setRadioSwitchPolicy,
  setRoutingRules,
  testCat,
  testRadioCat,
} from '../api'
import { describeRadioRouting, ROUTE_MODES, ROUTE_MODE_LABEL, type RouteLine } from '../settings/describeRouting'
import {
  answersFrom,
  finishPlan,
  planIsEmpty,
  routingDraft,
  sharedBands,
  withCells,
  type FinishPlan,
  type RadioSetupAnswers,
} from '../settings/radioSetup'

type Step = 'radios' | 'bands' | 'shared' | 'rest' | 'switching' | 'audio' | 'check'
/** A step's name on the step dots. Literal keys, so the catalog guards read every one. */
function stepTitle(s: Step): string {
  switch (s) {
    case 'radios':
      return t('radioSetup.step.radios')
    case 'bands':
      return t('radioSetup.step.bands')
    case 'shared':
      return t('radioSetup.step.shared')
    case 'rest':
      return t('radioSetup.step.rest')
    case 'switching':
      return t('radioSetup.step.switching')
    case 'audio':
      return t('radioSetup.step.audio')
    case 'check':
      return t('radioSetup.step.check')
  }
}

/** The Test CAT answer under a radio's card. */
type CatLine = { ok: boolean | null; text: string }

export function RadioSetupWizard({
  settings,
  bands,
  onClose,
  onFinished,
}: {
  /** The stored settings, read once: what the setup opens on and what Finish compares against. */
  settings: Settings
  /** The bands offered, in order (Settings' band list). */
  bands: readonly string[]
  onClose: () => void
  /** After a Finish that wrote something: what it wrote. */
  onFinished?: (plan: FinishPlan) => void
}) {
  const stored = settings.routingRules ?? []
  const [a, setA] = useState<RadioSetupAnswers>(() => answersFrom(settings, bands))
  const shared = sharedBands(a, bands)
  const steps: Step[] = ['radios', 'bands', ...(shared.length ? (['shared'] as Step[]) : []), 'rest', 'switching', 'audio', 'check']
  const [stepIx, setStepIx] = useState(0)
  const step = steps[Math.min(stepIx, steps.length - 1)]
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [snap, setSnap] = useState<AppSnapshot | null>(null)
  const [cat, setCat] = useState<Record<number, CatLine | 'busy'>>({})
  const [probe, setProbe] = useState<{ band: string; mode: RouteMode }>(() => ({
    band: shared[0] ?? (bands.includes('20m') ? '20m' : bands[0]),
    mode: 'digital',
  }))
  const [probeAnswer, setProbeAnswer] = useState<string | null>(null)
  const radios = settings.radios ?? []
  const active = settings.activeRadio ?? radios[0]?.id ?? 0
  const name = (id: number) => a.names[id]?.trim() || radios.find((r) => r.id === id)?.name || `#${id}`

  useEffect(() => {
    let live = true
    void getSnapshot()
      .then((s) => live && setSnap(s))
      .catch(() => {})
    return () => {
      live = false
    }
  }, [])

  // Every band two radios now list gets its cells, from the stored rules, when the band list changes.
  const sharedKey = shared.join(',')
  useEffect(() => {
    setA((prev) => withCells(prev, stored, bands))
  }, [sharedKey])

  const draft = useMemo(() => routingDraft(a, stored, bands), [a, stored, bands])
  const rules = draft.rules
  const runProbe = (band: string, mode: RouteMode) => {
    setProbe({ band, mode })
    setProbeAnswer(null)
    void routePreviewDraft(draft, band, mode)
      .then((r) => setProbeAnswer(r.name))
      .catch(() => setProbeAnswer(null))
  }
  useEffect(() => {
    if (step === 'check') runProbe(probe.band, probe.mode)
  }, [step])

  const toggleBand = (id: number, band: string) =>
    setA((prev) => {
      const now = prev.bands[id] ?? []
      const next = now.includes(band) ? now.filter((b) => b !== band) : bands.filter((b) => b === band || now.includes(b))
      return { ...prev, bands: { ...prev.bands, [id]: next } }
    })
  const setCell = (band: string, mode: RouteMode, radio: number | null) =>
    setA((prev) => ({
      ...prev,
      cellsEdited: true,
      cells: { ...prev.cells, [band]: { ...(prev.cells[band] ?? {}), [mode]: radio } },
    }))

  const testOne = (id: number) => {
    setCat((c) => ({ ...c, [id]: 'busy' }))
    const done = (line: CatLine) => setCat((c) => ({ ...c, [id]: line }))
    if (id === active) {
      void testCat()
        .then((r) => done({ ok: r.ok, text: r.detail }))
        .catch((e) => done({ ok: false, text: String(e) }))
      return
    }
    void testRadioCat(id)
      .then((r) => {
        const mhz = r.dialMhz != null ? r.dialMhz.toFixed(3) : null
        switch (r.state) {
          case 'answered':
            return done({ ok: true, text: mhz ? t('radioSetup.cat.answered', { mhz }) : t('radioSetup.cat.answeredNoDial') })
          case 'notAnswering':
            return done({ ok: false, text: t('radioSetup.cat.notAnswering') })
          case 'noFreshReport':
            return done({
              ok: null,
              text:
                r.ok === true && mhz
                  ? t('radioSetup.cat.stale.ok', { mhz })
                  : r.ok === false
                    ? t('radioSetup.cat.stale.bad')
                    : t('radioSetup.cat.stale.none'),
            })
          case 'disabled':
            return done({ ok: null, text: t('radioSetup.cat.disabled') })
          case 'noCat':
            return done({ ok: null, text: t('radioSetup.cat.noCat') })
          default:
            return done({ ok: null, text: t('radioSetup.cat.unknown') })
        }
      })
      .catch((e) => done({ ok: false, text: String(e) }))
  }

  const finish = async () => {
    const plan = finishPlan(settings, a, bands)
    if (planIsEmpty(plan)) {
      onClose()
      return
    }
    setBusy(true)
    setError(null)
    try {
      for (const r of plan.renames) await renameRadio(r.id, r.name)
      // On first, so a rule or default can name a radio switched back on; off after.
      for (const e of plan.enabled.filter((x) => x.enabled)) await setRadioEnabled(e.id, true)
      for (const b of plan.bands) await setRadioBands(b.id, b.bands)
      if (plan.rules) await setRoutingRules(plan.rules)
      if (plan.defaultRadio !== undefined) await setDefaultRadio(plan.defaultRadio)
      for (const e of plan.enabled.filter((x) => !x.enabled)) await setRadioEnabled(e.id, false)
      if (plan.policy) await setRadioSwitchPolicy(plan.policy)
      onFinished?.(plan)
      onClose()
    } catch (e) {
      setError(t('radioSetup.finish.failed', { error: e instanceof Error ? e.message : String(e) }))
    } finally {
      setBusy(false)
    }
  }

  const routeLine = (l: RouteLine): string => {
    switch (l.kind) {
      case 'rule':
        return t('radioSetup.route.rule', {
          n: l.n,
          bands: l.bands.length ? l.bands.join(', ') : t('settings.routing.rule.summary.anyBand'),
          mode: l.satellite
            ? t('settings.routing.rule.satellite')
            : l.mode
              ? ROUTE_MODE_LABEL[l.mode]
              : t('settings.routing.rule.summary.anyMode'),
        })
      case 'bands':
        return l.bands.join(', ')
      case 'catchAll':
        return t('settings.radios.routes.catchAll')
      case 'default':
        return t('settings.radios.routes.default')
      case 'disabled':
        return t('settings.radios.routes.disabled')
    }
  }
  const view = {
    radios: a.ids.map((id) => ({ id, name: name(id), bands: a.bands[id] ?? [], enabled: a.enabled[id] !== false })),
    routingRules: rules,
    defaultRadio: a.defaultRadio,
  }
  const live = (id: number) => snap?.radios?.find((r) => r.id === id)
  const catNow = (id: number): string => {
    const r = live(id)
    if (!r) return a.enabled[id] === false ? t('radioSetup.cat.disabled') : t('radioSetup.cat.notConnected')
    if (r.catOk === true) return t('radioSetup.cat.live', { mhz: r.dialMhz.toFixed(3) })
    if (r.catOk === false) return t('radioSetup.cat.notAnswering')
    return t('radioSetup.cat.notConnected')
  }

  return (
    <Dialog open onOpenChange={(o) => !o && !busy && onClose()} title={t('radioSetup.title')} className="radio-setup">
      <div className="wizard-dots" aria-label={t('radioSetup.steps.aria', { n: stepIx + 1, total: steps.length, title: stepTitle(step) })}>
        {steps.map((s, i) => (
          <button
            key={s}
            type="button"
            className={`wizard-dot${i === stepIx ? ' cur' : ''}${i < stepIx ? ' done' : ''}`}
            disabled={busy}
            onClick={() => setStepIx(i)}
          >
            <span className="wizard-dot-n">{i + 1}</span> {stepTitle(s)}
          </button>
        ))}
      </div>

      {step === 'radios' && (
        <>
          <h2 className="wizard-title">{t('radioSetup.radios.title')}</h2>
          <p className="wizard-sub">{t('radioSetup.radios.sub')}</p>
          <div className="radio-setup-cards">
            {radios.map((r) => (
              <div key={r.id} className="radio-setup-card">
                <label className="wizard-field">
                  <span>{t('radioSetup.radios.name')}</span>
                  <input
                    className="settings-input"
                    value={a.names[r.id] ?? ''}
                    onChange={(e) => setA({ ...a, names: { ...a.names, [r.id]: e.target.value } })}
                  />
                </label>
                <label className="radio-setup-tick">
                  <input
                    type="checkbox"
                    checked={a.enabled[r.id] !== false}
                    disabled={r.id === active}
                    onChange={(e) => setA({ ...a, enabled: { ...a.enabled, [r.id]: e.target.checked } })}
                  />
                  <span>{r.id === active ? t('radioSetup.radios.inUse') : t('radioSetup.radios.on')}</span>
                </label>
                <p className="settings-hint radio-setup-meta">
                  {t('radioSetup.radios.meta', {
                    model: r.rigModelName && r.rigModelName !== 'None / VOX' ? r.rigModelName : t('settings.radios.card.noModel'),
                    port: r.rigConn === 'serial' ? r.serialPort || t('settings.radios.card.noPort') : r.rigAddr || r.rigConn,
                    cat: catNow(r.id),
                  })}
                </p>
              </div>
            ))}
          </div>
        </>
      )}

      {step === 'bands' && (
        <>
          <h2 className="wizard-title">{t('radioSetup.bands.title')}</h2>
          <p className="wizard-sub">{t('radioSetup.bands.sub')}</p>
          <table className="radio-setup-grid">
            <thead>
              <tr>
                <th scope="col">{t('radioSetup.bands.band')}</th>
                {a.ids.map((id) => (
                  <th key={id} scope="col">
                    {name(id)}
                  </th>
                ))}
              </tr>
              <tr>
                <th scope="row">{t('radioSetup.bands.any')}</th>
                {a.ids.map((id) => (
                  <td key={id}>
                    <input
                      type="checkbox"
                      aria-label={t('radioSetup.bands.any.aria', { radio: name(id) })}
                      checked={(a.bands[id] ?? []).length === 0}
                      disabled={(a.bands[id] ?? []).length === 0 || a.enabled[id] === false}
                      onChange={() => setA({ ...a, bands: { ...a.bands, [id]: [] } })}
                    />
                  </td>
                ))}
              </tr>
            </thead>
            <tbody>
              {bands.map((b) => (
                <tr key={b} className={shared.includes(b) ? 'shared' : undefined}>
                  <th scope="row">{b}</th>
                  {a.ids.map((id) => (
                    <td key={id}>
                      <input
                        type="checkbox"
                        aria-label={t('radioSetup.bands.cell.aria', { radio: name(id), band: b })}
                        checked={(a.bands[id] ?? []).includes(b)}
                        disabled={a.enabled[id] === false}
                        onChange={() => toggleBand(id, b)}
                      />
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
          <p className="settings-hint">
            {shared.length ? t('radioSetup.bands.shared', { bands: shared.join(', ') }) : t('radioSetup.bands.noShared')}
          </p>
        </>
      )}

      {step === 'shared' && (
        <>
          <h2 className="wizard-title">{t('radioSetup.shared.title')}</h2>
          <p className="wizard-sub">{t('radioSetup.shared.sub')}</p>
          {shared.map((band) => (
            <fieldset key={band} className="radio-setup-shared">
              <legend>{band}</legend>
              {ROUTE_MODES.map(([mode, label]) => (
                <label key={mode} className="wizard-field radio-setup-mode">
                  <span>{label}</span>
                  <select
                    className="settings-input"
                    aria-label={t('radioSetup.shared.cell.aria', { band, mode: label })}
                    value={a.cells[band]?.[mode] ?? ''}
                    onChange={(e) => setCell(band, mode, e.target.value === '' ? null : Number(e.target.value))}
                  >
                    <option value="">{t('radioSetup.shared.noRule')}</option>
                    {a.ids.map((id) => (
                      <option key={id} value={id}>
                        {name(id)}
                      </option>
                    ))}
                  </select>
                </label>
              ))}
            </fieldset>
          ))}
        </>
      )}

      {step === 'rest' && (
        <>
          <h2 className="wizard-title">{t('radioSetup.rest.title')}</h2>
          <p className="wizard-sub">{t('radioSetup.rest.sub')}</p>
          <label className="wizard-field">
            <span>{t('settings.routing.default.label')}</span>
            <select
              className="settings-input"
              value={a.defaultRadio ?? ''}
              onChange={(e) => setA({ ...a, defaultRadio: e.target.value === '' ? null : Number(e.target.value) })}
            >
              <option value="">{t('settings.routing.default.stay')}</option>
              {a.ids.map((id) => (
                <option key={id} value={id}>
                  {name(id)}
                </option>
              ))}
            </select>
          </label>
        </>
      )}

      {step === 'switching' && (
        <>
          <h2 className="wizard-title">{t('radioSetup.switching.title')}</h2>
          <p className="wizard-sub">{t('radioSetup.switching.sub')}</p>
          <div className="radio-setup-policy" role="radiogroup" aria-label={t('settings.routing.policy.label')}>
            {(['tell', 'act'] as const).map((p) => (
              <label key={p} className="radio-setup-tick">
                <input type="radio" name="radio-setup-policy" checked={a.policy === p} onChange={() => setA({ ...a, policy: p })} />
                <span>{p === 'tell' ? t('settings.routing.policy.tell') : t('settings.routing.policy.act')}</span>
              </label>
            ))}
          </div>
          <ul className="radio-setup-facts">
            <li>{t('radioSetup.switching.stopsTx')}</li>
            <li>{t('radioSetup.switching.contact')}</li>
            <li>{t('radioSetup.switching.pin')}</li>
          </ul>
        </>
      )}

      {step === 'audio' && (
        <>
          <h2 className="wizard-title">{t('radioSetup.audio.title')}</h2>
          <p className="wizard-sub">{t('radioSetup.audio.sub')}</p>
          <div className="radio-setup-cards">
            {radios.map((r) => {
              const c = cat[r.id]
              return (
                <div key={r.id} className="radio-setup-card">
                  <strong>{name(r.id)}</strong>
                  <p className="settings-hint">
                    {t('radioSetup.audio.devices', {
                      rx: r.audioIn || t('settings.radios.card.audioDefault'),
                      tx: r.audioOut || t('settings.radios.card.audioDefault'),
                    })}
                  </p>
                  <p className="settings-hint">{t('radioSetup.audio.keying', { ptt: pttLabel(r.pttMethod) })}</p>
                  {/ #\d+$/.test(r.audioIn) && <p className="settings-hint radio-setup-warn">{t('radioSetup.audio.sameName')}</p>}
                  <div className="radio-setup-test">
                    <button
                      type="button"
                      className="settings-refresh"
                      disabled={c === 'busy' || a.enabled[r.id] === false}
                      title={r.id === active ? t('radioSetup.cat.test.inUse.title') : t('radioSetup.cat.test.other.title')}
                      onClick={() => testOne(r.id)}
                    >
                      {c === 'busy' ? t('radioSetup.cat.testing') : t('radioSetup.cat.test')}
                    </button>
                    {c && c !== 'busy' && (
                      <span className={`settings-hint${c.ok === true ? ' ok' : c.ok === false ? ' bad' : ''}`} role="status">
                        {c.text}
                      </span>
                    )}
                  </div>
                </div>
              )
            })}
          </div>
        </>
      )}

      {step === 'check' && (
        <>
          <h2 className="wizard-title">{t('radioSetup.check.title')}</h2>
          <p className="wizard-sub">{t('radioSetup.check.sub')}</p>
          <div className="radio-setup-probe">
            <span className="settings-label">{t('settings.routing.test.label')}</span>
            <div className="settings-input-row">
              <select className="settings-input" value={probe.band} aria-label={t('settings.routing.test.band.aria')} onChange={(e) => runProbe(e.target.value, probe.mode)}>
                {bands.map((b) => (
                  <option key={b} value={b}>
                    {b}
                  </option>
                ))}
              </select>
              <select
                className="settings-input"
                value={probe.mode}
                aria-label={t('settings.routing.test.mode.aria')}
                onChange={(e) => runProbe(probe.band, e.target.value as RouteMode)}
              >
                {ROUTE_MODES.map(([v, label]) => (
                  <option key={v} value={v}>
                    {label}
                  </option>
                ))}
              </select>
            </div>
            {probeAnswer && (
              <p className="settings-note radio-setup-answer" role="status">
                {t('radioSetup.check.answer', { band: probe.band, mode: ROUTE_MODE_LABEL[probe.mode], radio: probeAnswer })}
              </p>
            )}
          </div>
          <h3 className="wizard-modes-title">{t('radioSetup.check.radios')}</h3>
          <ul className="radio-setup-lines">
            {view.radios.map((r) => (
              <li key={r.id}>
                {t('settings.routing.plain.line', {
                  radio: r.name,
                  list: describeRadioRouting(view, r.id).map(routeLine).join('; '),
                })}
              </li>
            ))}
          </ul>
          <h3 className="wizard-modes-title">{t('radioSetup.check.rules')}</h3>
          {rules.length === 0 ? (
            <p className="settings-hint">{t('radioSetup.check.noRules')}</p>
          ) : (
            <ol className="radio-setup-rules">
              {rules.map((r, i) => (
                <li key={i}>
                  {t('settings.routing.rule.summary', {
                    bands: r.bands.length ? r.bands.join(', ') : t('settings.routing.rule.summary.anyBand'),
                    mode:
                      r.context === 'satellite'
                        ? t('settings.routing.rule.satellite')
                        : r.mode
                          ? ROUTE_MODE_LABEL[r.mode]
                          : t('settings.routing.rule.summary.anyMode'),
                    radio: name(r.radio),
                  })}
                </li>
              ))}
            </ol>
          )}
          <p className="settings-hint">
            {t('radioSetup.check.else', {
              radio: a.defaultRadio == null ? t('settings.routing.default.stay') : name(a.defaultRadio),
            })}
          </p>
        </>
      )}

      {error && (
        <p className="contest-setup-bad" role="alert">
          {error}
        </p>
      )}

      <div className="wizard-actions">
        <div className="wizard-actions-right">
          {stepIx > 0 && (
            <button type="button" className="wizard-skip" disabled={busy} onClick={() => setStepIx(stepIx - 1)}>
              {t('radioSetup.nav.back')}
            </button>
          )}
          <button type="button" className="wizard-skip" disabled={busy} onClick={onClose}>
            {t('radioSetup.nav.cancel')}
          </button>
          {stepIx < steps.length - 1 ? (
            <button type="button" className="wizard-go" onClick={() => setStepIx(stepIx + 1)}>
              {t('radioSetup.nav.next')}
            </button>
          ) : (
            <button type="button" className="wizard-go" disabled={busy} onClick={() => void finish()}>
              {busy ? t('radioSetup.finish.busy') : t('radioSetup.finish')}
            </button>
          )}
        </div>
      </div>
    </Dialog>
  )
}

function pttLabel(m: string): string {
  switch (m) {
    case 'cat':
      return t('settings.rigControl.ptt.cat')
    case 'rts':
      return t('settings.rigControl.ptt.rts')
    case 'dtr':
      return t('settings.rigControl.ptt.dtr')
    case 'vox':
      return t('settings.rigControl.ptt.vox')
    default:
      return m
  }
}

/** Whether a station has more than one radio: the setup is offered only then. */
export function offersRadioSetup(s: Pick<Settings, 'radios'> | null | undefined): boolean {
  return (s?.radios?.length ?? 0) > 1
}
