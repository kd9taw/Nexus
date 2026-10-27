// ---------------------------------------------------------------------------
// ⭐ THE CONTEST-OVER CHECKLIST — what Contest mode going off OFFERS, and never does unasked:
// merge the contest's contacts into the logbook (the Contest screen's own merge, with its count),
// export the Cabrillo log (the Contest screen's own export), and put back what the contest changed
// — the station callsign a club set, the operator at the key, the position, the entry category,
// the entry class, the power category and Unassisted — each shown as it is now and as it was
// before the contest.
//
// It also offers the Summary export, names the Cabrillo lines the sponsor requires that the log leaves
// out (with a box for each one Settings fills at export time, saved just before the export), and SHOWS
// where the sponsor takes the log, to copy: Nexus never opens that address or sends a log anywhere.
//
// Every item is a tick the operator makes; none is ticked to begin with. "Not now" (and Esc)
// leaves everything as it is, Contest mode included. "Turn off" does what is ticked and then turns
// the mode off.
//
// At a club it also says how many of this laptop's contacts the club's log does not have yet (the
// club chip's count): they still go to the host once the link is back, Contest mode on or off,
// which only holds while Nexus stays open with club sync on.
//
// ⚠️ IT OPENS BEFORE THE MODE GOES OFF, NOT AFTER: the merge and the export read the session the
// mode holds, and the engine refuses both once it has gone. So both hosts — the Contest screen's
// switch and a Settings save that turns it off — hold their save until this answers.
//
// ⚠️ THE ORDER IS LOAD-BEARING: merge, the missing lines typed in, export, Summary, Unassisted, then
// ONE save that turns the mode off with the other put-backs in it.
//   • The export comes before any put-back: a Cabrillo log is the contest's, written under the
//     contest's call and categories.
//   • Unassisted goes back through its own switch (which journals the change) while the mode is
//     still on, and everything else rides the save that turns it off: the engine lets its note of
//     "before" go when the mode goes off with all of it put back (`engine/contest_before.rs`), and
//     a change after that would start a new note holding the contest's value as "before".
// A step that fails stops the rest, says why, and leaves the mode on.
//
// "Before" is the engine's note (`settings.contestBefore`), taken the first time one of these
// changed for this contest, or when the mode went on. A callsign it has no value for is never
// offered: a station with a blank callsign cannot go on the air.
// ---------------------------------------------------------------------------
import { useEffect, useState } from 'react'
import { Dialog } from './ui/Dialog'
import { t } from '../i18n'
import type { FdMergeReport, Settings } from '../types'
import type { FdRulesetDto } from '../api'
import { exportLog, fdMergeToGeneral, saveTextToDownloads, setUnassistedMode } from '../api'
import { CONTESTS, contestFileStem, contestName } from '../fdEvent'
import { cabrilloLineWords, READ_AS_THE_SESSION_STARTS } from '../features/contestStart'
import { isStreamInput } from '../remote-native/stream-input'
import { pushToast } from '../toast'
import { patchSettings } from '../settings/patch'

/** The settings the checklist can put back, in the order it lists them. */
const PUT_BACK = [
  'mycall',
  'fdOperator',
  'fdPositionName',
  'contestCategoryOperator',
  'contestEntryClass',
  'contestCategoryPower',
  'unassistedMode',
] as const
export type PutBackKey = (typeof PUT_BACK)[number]

/** Each one's name, as Settings words it. A literal key per setting rather than a table read through
 *  `t(table[key])`, so the catalog guard (`i18n/placeholders.test.ts`) can read every call site. */
function settingLabel(key: PutBackKey): string {
  switch (key) {
    case 'mycall':
      return t('settings.fdWho.call.label')
    case 'fdOperator':
      return t('settings.fdWho.operator.label')
    case 'fdPositionName':
      return t('settings.fdWho.position.label')
    case 'contestCategoryOperator':
      return t('settings.contestPick.category.label')
    case 'contestEntryClass':
      return t('settings.contestPick.entryClass.label')
    case 'contestCategoryPower':
      return t('settings.contestPick.power.label')
    case 'unassistedMode':
      return t('settings.contestCategory.unassisted.label')
  }
}

/** An empty entry category IS this declaration (Settings' own rule), and a Cabrillo token. */
const SINGLE_OP = 'SINGLE-OP'
/** Between "now" and "before". A symbol, not prose. */
const ARROW = '→'
/** Between the parts of the merge report, as the Contest screen joins them. */
const SEP = ' · '

export interface PutBackRow {
  key: PutBackKey
  now: string | boolean
  before: string | boolean
}

export interface ContestOverOffer {
  /** The contacts the session holds: the merge's count, read as the Contest screen's button reads it. */
  contacts: number
  /** The contest's rules-file id, which names the Cabrillo file. */
  event: string | undefined
  /** Only the settings the contest changed: what each is now, and what it was before. */
  putBack: PutBackRow[]
  /** This laptop's contacts the club's log does not have yet: the club chip's count
   *  (`FdClubStatus.queued`), 0 without club sync. */
  clubQueued: number
  /** The Cabrillo lines the sponsor requires that the running log leaves out
   *  (`FieldDayStatus.cabrilloMissing`). */
  cabrilloMissing?: string[]
  /** The sponsor's own values for the lines that take a list (the preview's). */
  choices?: Pick<FdRulesetDto, 'entryClasses' | 'bandCategories' | 'transmitterCategories'>
}

/** The Cabrillo lines a box here can fill, each with the setting it writes. Read at export time, so
 *  a value saved just before the export reaches the file. */
const FIXABLE: Record<string, { key: keyof Settings; list?: keyof NonNullable<ContestOverOffer['choices']> }> = {
  EMAIL: { key: 'contestEmail' },
  OPERATORS: { key: 'contestOperators' },
  'ENTRY-CLASS': { key: 'contestEntryClass', list: 'entryClasses' },
  'CATEGORY-BAND': { key: 'contestCategoryBand', list: 'bandCategories' },
  'CATEGORY-TRANSMITTER': { key: 'contestCategoryTransmitter', list: 'transmitterCategories' },
}

/** Where the sponsor takes the log, if the menu knows: a page or an address, shown to copy. */
export function contestSubmitAddress(event: string | undefined): string | null {
  return CONTESTS.find((c) => c.id === (event || 'arrlfd'))?.submit ?? null
}

/** What to offer, from the settings the station holds now and the engine's note in them, and the
 *  club chip's count of contacts the host does not have yet. */
export function contestOverOffer(
  now: Settings,
  contacts: number,
  event: string | undefined,
  clubQueued = 0,
  cabrillo: { missing?: string[]; choices?: ContestOverOffer['choices'] | null } = {},
): ContestOverOffer {
  const note = now.contestBefore
  const putBack: PutBackRow[] = []
  if (note) {
    for (const key of PUT_BACK) {
      const before = key === 'unassistedMode' ? note.unassistedMode === true : (note[key] ?? '')
      const is = key === 'unassistedMode' ? now.unassistedMode === true : (now[key] ?? '')
      if (before === is) continue
      if (key === 'mycall' && before === '') continue
      putBack.push({ key, now: is, before })
    }
  }
  const choices = {
    entryClasses: cabrillo.choices?.entryClasses,
    bandCategories: cabrillo.choices?.bandCategories,
    transmitterCategories: cabrillo.choices?.transmitterCategories,
  }
  return { contacts, event, putBack, clubQueued, cabrilloMissing: cabrillo.missing ?? [], choices }
}

/** Is there anything to offer, or to say? With nothing, the mode simply goes off, as it always did. */
export const hasOffer = (o: ContestOverOffer): boolean =>
  o.contacts > 0 || o.putBack.length > 0 || o.clubQueued > 0

/** A value as the checklist shows it: a blank one in the words its setting means by blank. */
function shown(key: PutBackKey, v: string | boolean): string {
  if (key === 'unassistedMode') return v ? t('contestOver.value.on') : t('contestOver.value.off')
  if (v !== '') return String(v)
  if (key === 'fdOperator') return t('contestOver.value.yourCall')
  if (key === 'contestCategoryOperator') return SINGLE_OP
  if (key === 'fdPositionName') return t('contestOver.value.none')
  return t('contestOver.value.notDeclared')
}

/** The merge's report, in the Contest screen's words. */
function mergeReportText(r: FdMergeReport): string {
  const parts = [t('fieldDay.merge.added', { count: r.added }), t('fieldDay.merge.already', { count: r.already })]
  if (r.refused) parts.push(t('fieldDay.merge.refused', { count: r.refused }))
  if (r.queued) parts.push(t('fieldDay.merge.queued'))
  return parts.join(SEP)
}

type Tick = 'merge' | 'export' | 'summary' | PutBackKey

export function ContestOverDialog({
  offer,
  onNotNow,
  onTurnOff,
  onStats,
  summaryText,
}: {
  /** What to offer; `null` is closed. */
  offer: ContestOverOffer | null
  /** The Summary export's text, as the Contest screen's Summary button writes it, read when the
   *  export runs. Absent, the checklist offers no Summary. */
  summaryText?: () => string | Promise<string>
  /** Nothing is done: the dialog closes and Contest mode stays on. */
  onNotNow: () => void
  /** Turn Contest mode off, in ONE save, with these settings put back. Rejects with the reason. */
  onTurnOff: (putBack: Partial<Settings>) => Promise<void>
  /** Show the contest's Stats tab instead, Contest mode still on. Not offered when absent. */
  onStats?: () => void
}) {
  const [ticked, setTicked] = useState<Set<Tick>>(new Set())
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [fixes, setFixes] = useState<Partial<Record<keyof Settings, string>>>({})
  const [copied, setCopied] = useState(false)
  // Every opening starts with nothing ticked: a tick from an earlier offer is not this one's.
  useEffect(() => {
    setTicked(new Set())
    setError(null)
    setFixes({})
    setCopied(false)
  }, [offer])
  if (!offer) return null
  const submit = contestSubmitAddress(offer.event)

  const tick = (k: Tick) =>
    setTicked((prev) => {
      const next = new Set(prev)
      if (next.has(k)) next.delete(k)
      else next.add(k)
      return next
    })

  const turnOff = async () => {
    // Only at the station: a press through the Remote stream writes no file and merges no contact,
    // as the Contest screen's own buttons refuse. Read before anything awaits, inside the dispatch.
    if (isStreamInput() && (ticked.has('merge') || ticked.has('export') || ticked.has('summary'))) {
      setError(ticked.has('merge') ? t('fieldDay.merge.remote') : t('fieldDay.export.remote'))
      return
    }
    setBusy(true)
    setError(null)
    try {
      if (ticked.has('merge')) {
        pushToast(mergeReportText(await fdMergeToGeneral()), 'success')
      }
      const stamp = new Date().toISOString().slice(0, 10)
      if (ticked.has('export')) {
        // The lines typed in here first, saved over the live settings, so the export writes them.
        const typed = Object.fromEntries(
          Object.entries(fixes)
            .map(([k, v]) => [k, (v ?? '').trim()])
            .filter(([, v]) => v !== ''),
        ) as Partial<Settings>
        if (Object.keys(typed).length > 0) await patchSettings(() => typed)
        // The Contest screen's own export: the same command and the same file name.
        const text = await exportLog('cabrillo')
        const path = await saveTextToDownloads(`${contestFileStem(offer.event)}-log-${stamp}.cbr`, text)
        pushToast(t('fieldDay.export.done', { path }), 'success')
      }
      if (ticked.has('summary') && summaryText) {
        // The Contest screen's own Summary: the same text and the same file name.
        const path = await saveTextToDownloads(`${contestFileStem(offer.event)}-summary-${stamp}.txt`, await summaryText())
        pushToast(t('fieldDay.export.done', { path }), 'success')
      }
      const back: Partial<Settings> = {}
      for (const row of offer.putBack) {
        if (!ticked.has(row.key)) continue
        if (row.key === 'unassistedMode') await setUnassistedMode(row.before === true)
        else (back as Record<string, unknown>)[row.key] = row.before
      }
      await onTurnOff(back)
    } catch (e) {
      setError(typeof e === 'string' ? e : e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog
      open
      // Esc and a click outside are "Not now" — but never while a step is running.
      onOpenChange={(open) => {
        if (!open && !busy) onNotNow()
      }}
      title={t('contestOver.title')}
      description={t('contestOver.desc')}
    >
      <div className="contest-over">
        {offer.clubQueued > 0 && (
          <p className="contest-over-club">{t('contestOver.club.queued', { count: offer.clubQueued })}</p>
        )}
        {offer.contacts > 0 && (
          <>
            <label className="contest-over-item">
              <input type="checkbox" checked={ticked.has('merge')} onChange={() => tick('merge')} disabled={busy} />
              <span>{t('fieldDay.merge.label', { count: offer.contacts })}</span>
            </label>
            <label className="contest-over-item">
              <input type="checkbox" checked={ticked.has('export')} onChange={() => tick('export')} disabled={busy} />
              <span>{t('contestOver.export')}</span>
            </label>
            {(offer.cabrilloMissing?.length ?? 0) > 0 && (
              <fieldset className="contest-over-missing">
                <legend>{t('contestOver.missing.legend', { contest: contestName(offer.event || 'arrlfd') })}</legend>
                <p className="contest-over-note">{t('contestOver.missing.note')}</p>
                {(offer.cabrilloMissing ?? []).map((line) => {
                  const fix = FIXABLE[line]
                  const list = fix?.list ? offer.choices?.[fix.list] : undefined
                  return (
                    <div className="contest-over-line" key={line} data-line={line}>
                      <span className="contest-over-setting">{cabrilloLineWords(line)}</span>
                      {fix && list?.length ? (
                        <select
                          aria-label={cabrilloLineWords(line)}
                          value={fixes[fix.key] ?? ''}
                          disabled={busy}
                          onChange={(e) => setFixes((f) => ({ ...f, [fix.key]: e.target.value }))}
                        >
                          <option value="">{t('contestSetup.entry.unset')}</option>
                          {list.map((v) => (
                            <option key={v} value={v}>
                              {v}
                            </option>
                          ))}
                        </select>
                      ) : fix && !fix.list ? (
                        <input
                          type={line === 'EMAIL' ? 'email' : 'text'}
                          aria-label={cabrilloLineWords(line)}
                          value={fixes[fix.key] ?? ''}
                          disabled={busy}
                          onChange={(e) => setFixes((f) => ({ ...f, [fix.key]: e.target.value }))}
                          autoComplete="off"
                        />
                      ) : READ_AS_THE_SESSION_STARTS.includes(line) ? (
                        <span className="contest-over-note">{t('contestOver.missing.readAtStart')}</span>
                      ) : null}
                    </div>
                  )
                })}
              </fieldset>
            )}
            {summaryText && (
              <label className="contest-over-item">
                <input type="checkbox" checked={ticked.has('summary')} onChange={() => tick('summary')} disabled={busy} />
                <span>{t('contestOver.summary')}</span>
              </label>
            )}
            {submit && (
              <div className="contest-over-submit" data-submit>
                <span>{t('contestOver.submit.label', { contest: contestName(offer.event || 'arrlfd') })}</span>{' '}
                <code className="contest-over-address">{submit}</code>{' '}
                <button
                  type="button"
                  className="settings-linkbtn"
                  onClick={() => {
                    void navigator.clipboard
                      ?.writeText(submit)
                      .then(() => setCopied(true))
                      .catch(() => {})
                  }}
                >
                  {copied ? t('contestOver.submit.copied') : t('contestOver.submit.copy')}
                </button>{' '}
                <span className="contest-over-note">{t('contestOver.submit.note')}</span>
              </div>
            )}
          </>
        )}
        {offer.putBack.length > 0 && (
          <fieldset className="contest-over-putback">
            <legend>{t('contestOver.putBack.legend')}</legend>
            <p className="contest-over-note">{t('contestOver.putBack.note')}</p>
            <div className="contest-over-head" aria-hidden="true">
              <span />
              <span />
              <span>{t('contestOver.putBack.now')}</span>
              <span />
              <span>{t('contestOver.putBack.before')}</span>
            </div>
            {offer.putBack.map((row) => (
              <label className="contest-over-row" key={row.key} data-setting={row.key}>
                <input
                  type="checkbox"
                  checked={ticked.has(row.key)}
                  onChange={() => tick(row.key)}
                  disabled={busy}
                  aria-label={t('contestOver.putBack.aria', {
                    setting: settingLabel(row.key),
                    now: shown(row.key, row.now),
                    before: shown(row.key, row.before),
                  })}
                />
                <span className="contest-over-setting">{settingLabel(row.key)}</span>
                <span className="contest-over-now" title={shown(row.key, row.now)}>
                  {shown(row.key, row.now)}
                </span>
                <span className="contest-over-arrow" aria-hidden="true">
                  {ARROW}
                </span>
                <span className="contest-over-before" title={shown(row.key, row.before)}>
                  {shown(row.key, row.before)}
                </span>
              </label>
            ))}
          </fieldset>
        )}
        {onStats && offer.contacts > 0 && (
          <button type="button" className="settings-refresh contest-over-stats" onClick={onStats} disabled={busy} title={t('contestOver.stats.title')}>
            {t('contestOver.stats')}
          </button>
        )}
        {error && (
          <p className="contest-over-error" role="alert">
            {error}
          </p>
        )}
        {/* "Not now" first, so it takes the focus: the answer that changes nothing is the default. */}
        <div className="confirm-actions">
          <button type="button" className="settings-refresh" onClick={onNotNow} disabled={busy} autoFocus>
            {t('contestOver.notNow')}
          </button>
          <button type="button" className="settings-save" onClick={() => void turnOff()} disabled={busy}>
            {busy ? t('contestOver.busy') : t('contestOver.turnOff')}
          </button>
        </div>
      </div>
    </Dialog>
  )
}
