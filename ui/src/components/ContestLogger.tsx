// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every word comes from the
// catalog; the modes below are wire values (ADIF mode names), never translated, and `A−` / `A+` are
// the text size control's glyphs, as on a Connect box's menu.
//
// THE CONTEST LOGGER WINDOW (`?panel=contestlog`) — a log line for a second person logging at a
// second monitor and keyboard while the operator works the radio in the main window (two keyboards
// and two mice on one computer, through MouseMux), with the whole contest screen under it.
//
// ⭐ THE ENTRY FIRST, AND LARGE (the operator's layout, 2026-10-11). For the person at this window
// the call and the exchange are the job, so they are the top of the window, in large text, with the
// dupe warning right under the boxes; the last contacts come next, newest first, at the same size;
// and the contest screen (band board, score, log) scrolls below. A− / A+ in the bar over the
// entry sizes the entry and the last contacts together, and this computer remembers it for this
// window. The sizes are styles.css's, scoped to the entry: the cockpits' log lines are untouched.
//
// ⭐ THE OPERATOR BOX IS ON THE ENTRY'S TITLE LINE, beside Sending and I moved (the operator,
// 2026-10-12: "I want to move the field to change operators up near the top"), in both windows, and
// the screen below draws none, so there is one. Ctrl+O from the log line lands in it. It writes the
// operator through the host's narrow save, never the whole-settings one, and its Enter is its own:
// it logs nothing.
//
// ⭐ AND THE SAME ENTRY TOPS THE MAIN WINDOW'S CONTEST SCREEN (the operator's pick, 2026-10-11: "The
// Logger window's large Call / RST / County boxes, dupe warning and last contacts at the top of the
// Contest screen"). It is one component, `ContestLargeEntry`, mounted by this window and by App: there
// it is the main window's contest strip, so it shares the contact as the cockpits' strips do, Esc there
// is the main window's stop, and it transmits nothing of its own either.
//
// ⭐ ONE CONTACT, TWO WINDOWS. The log line here and the operator's own contest strip share the
// contact in progress through the engine (`features/contestEntryShare`): what is typed in either
// shows in both as it is typed, every hint follows it in both, and Enter in either logs it once.
//
// ⭐ A PASS'S CONTACTS GO ON THE SATELLITES SCREEN (the operator's pick, 2026-10-13: "Say to use the
// Satellites screen", while a bird is held). This log line logs every contact the ordinary way, on the
// band the radio is on and with no bird (`contestLogManual`); only the Satellites screen's strip takes a
// contact's band and bird from the pass (`contestLogSatellite`). So while the engine holds a bird's
// transponder, read every 2 s as the Satellites screen and the cockpits' rotor strip read it
// (`getSatTransponder`), one line under the bar names the bird and says to log its pass contacts there.
// It blocks nothing. It is this window's alone: the main window's Contest screen tops itself with the
// same entry and was not part of the pick, so it draws no such line and asks nothing about a bird.
//
// ⛔ NOTHING IN THIS WINDOW TRANSMITS. No PTT, no F-key messages, no Tune, no Enter Sends Message:
// the log line here gets no `esm`; the screen's contest switch, Running / S&P and scoring are
// read-only (`ContestView`'s `logger`); the F-keys do nothing (the webview's own as well, so F5
// does not reload the logger mid-contact); Esc clears the entry (`LogEntry`'s `sharedEntry`); and
// A− / A+ only size the text.
// Every TX control stays on the operator's screen, whose hotkeys are bound in the main window's
// document only. The ONE exception is a stop: Esc here is also the station's halt (`DetachedPanel`,
// since the Illinois QSO Party, 2026-10-18, where this window was the operator's entry and Esc did
// not stop a CW message), so it stops and clears on the same press. This is a separate window with a document of its own, so a keystroke
// here never reaches them, and one there never reaches this.
//
// ⭐ KEEP IN FRONT (the operator, from a contest tester: "resize it to the point where I can drop it in
// an area and keep it to the front"). The window drags down to 420×320 with the entry whole in it, and a
// button on the bar pins it in front of Nexus and of other programs (`set_window_on_top`), remembered in
// the window's record with its box, so it reopens pinned. Only this window draws it: the main window's
// Contest screen tops itself with the same entry and has no pin.
//
// ⭐ THE ENTRY ALONE (the operator: the contest screen is cluttered). The same window has a second
// layout, `entry` (`features/contestLoggerLayout`): the bar and the entry, with no last contacts and no
// contest screen under them. The main window's Contest screen opens it so with "Pop out entry" on its
// entry's bar; "Logger window" opens the whole window; the toggle on this window's bar switches between
// the two. It is this window in both, so the entry is shared and logs once exactly as above, and the box
// and the pin are the window's. With no contest running there is no entry, so the window is whole.
//
// It logs as THIS computer's own position — the same engine, contest log and club sync, so the
// club wire carries nothing new — and under the station's operating mode, read live from the
// engine, so a contact logged here is credited as the operator's own would be. A Remote viewer
// never sees this window: the stream is the main window's picture.

import { useEffect, useId, useMemo, useRef, useState, type CSSProperties, type ReactNode, type RefObject } from 'react'
import { getSatTransponder, getWindowOnTop, setWindowOnTop, type FdRulesetDto, type WindowOnTop } from '../api'
import { isFieldDay } from '../fdEvent'
import { BOX_SCALE_MAX, BOX_SCALE_MIN, BOX_SCALE_STEP, boxScaleValue } from '../features/panelState'
import { CONTEST_RECENT_SPLIT_MAX, CONTEST_RECENT_SPLIT_MIN } from '../features/paneSeam'
import { surfaceGet, surfaceSet } from '../features/windowScope'
import { t } from '../i18n'
import { pollSingleFlight } from '../singleFlight'
import { withErrorToast } from '../toast'
import type { AppSnapshot, FieldDayQso, FieldDayStatus } from '../types'
import { useContestSwitch } from '../contestSwitch'
import { contactLabel } from '../features/contestRemoval'
import { isStreamInput } from '../remote-native/stream-input'
import { useOperatorBoxAsk } from '../features/contestKeys'
import { useLoggerLayout } from '../features/contestLoggerLayout'
import { contestLogMode, stationMode, useKeyboardMode } from '../features/contestMode'
import { closedText } from '../features/contestEdit'
import { ContestContactEdit, focusOpenContestEdit, type EditNote } from './ContestContactEdit'
import { annotate, ContestLogHead, ContestLogRow, ContestView, logIsNumbered, OperatorBox } from './ContestView'
import { NoCreditChip } from './FdAdvisories'
import { LogEntry } from './LogEntry'
import { PaneSeam } from './PaneSeam'

/** Where the entry keeps its text size. Per surface (`windowScope`): each window its own, on this
 *  computer — the main window's Contest screen under the bare key, this window under its own, which
 *  starts from the main window's until its first press here. */
const TEXT_SCALE_KEY = 'nexus.contestLogger.textScale'

/** How many of the newest contacts the entry lists under its boxes. */
export const LAST_CONTACTS = 10

/** The text size as stored, clamped on load into the range A− / A+ writes — a Connect box's, 80–160 %
 *  in steps of 10 — and 100 % for anything that is not a size (nothing stored, a hand edit). */
export function loggerTextScale(stored: string | null): number {
  return boxScaleValue(Number(stored)) ?? 1
}

/**
 * The logger window's pin: keeps this window in front of other windows, Nexus's and other programs',
 * and pressed again lets it stack as an ordinary window. Drawn only where the shell offers it for this
 * window (`get_window_on_top`), and it shows what the window really is: a refused change leaves it as
 * it was. A press takes no focus, so the caret stays in the box it was in.
 */
function KeepInFront() {
  const [state, setState] = useState<WindowOnTop | null>(null)
  useEffect(() => {
    let live = true
    getWindowOnTop()
      .then((s) => live && setState(s))
      .catch(() => {})
    return () => {
      live = false
    }
  }, [])
  if (!state?.supported) return null
  const toggle = () => {
    void withErrorToast(() => setWindowOnTop(!state.on), t('contestLogger.pin.failed')).then((s) => {
      if (s) setState(s)
    })
  }
  return (
    <button
      type="button"
      className={`contest-logger-toggle contest-logger-pin${state.on ? ' active' : ''}`}
      aria-pressed={state.on}
      title={t('contestLogger.pin.title')}
      onMouseDown={(e) => e.preventDefault()}
      onClick={toggle}
    >
      {t('contestLogger.pin.label')}
    </button>
  )
}

/** The newest contacts of the running contest, newest first, in the contest screen's own columns.
 *  Each row's Edit opens the contest screen's own edit (`ContestContactEdit`) in the row's place, with
 *  the screen's refusals: never through the Remote stream, and at a club the line says how a contact
 *  is fixed there instead, because club sync cannot carry an edit to the other laptops.
 *
 *  The edit is the contact's, by its call and time, not a place in the list: contacts logged while it
 *  is open push its row down, and past the ten it stays open after them, as typed. The line it leaves
 *  is held to the bottom edge of the list's box, so no scroll position hides it (the logbook-copy
 *  warning above all) and in a short window it takes its room from the list, never from the window;
 *  it goes at the next edit or contact logged. */
export function LastContacts({
  fieldDay,
  style,
  boxRef,
  onEditDone,
}: {
  fieldDay: FieldDayStatus
  style: CSSProperties
  boxRef: RefObject<HTMLElement>
  /** An edit is over (saved or left): the host gives the keyboard back to its entry. */
  onEditDone: () => void
}) {
  const titleId = useId()
  const fieldDayColumns = isFieldDay(fieldDay.event)
  const slotColumns = fieldDayColumns ? [] : (fieldDay.receives ?? [])
  const numbered = logIsNumbered(fieldDay.log)
  // Marked over the whole log, as the screen marks it (a new section is new in log order), then the
  // newest few, each keyed by its place in the log.
  const rows = useMemo(
    () =>
      annotate(fieldDay.log)
        .map((row, at) => ({ row, at }))
        .slice(-LAST_CONTACTS)
        .reverse(),
    [fieldDay.log],
  )
  const [editing, setEditing] = useState<{ call: string; whenUnix: number } | null>(null)
  const [note, setNote] = useState<EditNote | null>(null)
  // A county line is one contact: the edit takes all its rows from the whole log.
  const editRows = editing
    ? fieldDay.log.filter((q) => q.call === editing.call && q.whenUnix === editing.whenUnix)
    : []
  // A contact this window no longer finds while its edit is open — its call changed elsewhere, the
  // commonest edit, or it was removed: the edit closes and says which (`closedText`). This edit's
  // own save changing the call answers for itself. One edited elsewhere with its call kept closes
  // itself and says so (`ContestContactEdit`).
  const saving = useRef(false)
  useEffect(() => {
    if (!editing || editRows.length > 0) return
    setEditing(null)
    if (!saving.current) setNote({ text: closedText(fieldDay.log, editing.whenUnix), alert: true })
  }, [editing, editRows.length, fieldDay.log])
  // A contact logged retires the line; an edit, which leaves the count as it was, does not.
  const logLen = fieldDay.log.length
  const seenLen = useRef(logLen)
  useEffect(() => {
    if (logLen > seenLen.current) setNote(null)
    seenLen.current = logLen
  }, [logLen])
  const editRow = (q: FieldDayQso) => {
    if (isStreamInput()) {
      setNote({ text: t('contest.edit.remote'), alert: true })
      return
    }
    if (fieldDay.club) {
      setNote({ text: t('contest.edit.club'), alert: true })
      return
    }
    // Another edit open in this window (the strip's Ctrl+Y, the contest screen's): it keeps the
    // keyboard and its typing, and no second one opens.
    if (!editing && focusOpenContestEdit()) {
      setNote({ text: t('contest.edit.open'), alert: false })
      return
    }
    setNote(null)
    saving.current = false
    setEditing({ call: q.call, whenUnix: q.whenUnix ?? 0 })
  }
  const editDone = (done: EditNote | null) => {
    saving.current = false
    setEditing(null)
    setNote(done)
    onEditDone()
  }
  // The edit stands in the place of the first of its rows shown here, or after the ten once
  // newer contacts have pushed all of them out.
  const editAt = rows.findIndex(({ row }) => editRows.includes(row.qso))
  const edit =
    editRows.length > 0 ? (
      <ContestContactEdit
        key={`edit-${editRows[0].call}-${editRows[0].whenUnix}`}
        rows={editRows}
        receives={fieldDay.receives ?? []}
        role={fieldDay.role}
        bands={fieldDay.bands}
        onSaving={() => {
          saving.current = true
        }}
        onDone={editDone}
      />
    ) : null
  return (
    <>
      <section className="contest-logger-recent" aria-labelledby={titleId} style={style} ref={boxRef}>
        <div className="contest-logger-recent-body">
          <h2 id={titleId} className="contest-logger-recent-title">
            {t('contestLogger.recent.title')}
          </h2>
          <ContestLogHead
            fieldDayColumns={fieldDayColumns}
            isWfd={fieldDay.event === 'wfd'}
            slotColumns={slotColumns}
            role={fieldDay.role}
            numbered={numbered}
          />
          {rows.length === 0 && <p className="empty">{t('fieldDay.log.empty')}</p>}
          {rows.map(({ row, at }, i) => {
            if (editRows.includes(row.qso)) return i === editAt ? edit : null
            return (
              <ContestLogRow
                key={`${row.qso.call}-${at}`}
                row={row}
                fieldDayColumns={fieldDayColumns}
                slotColumns={slotColumns}
                numbered={numbered}
              >
                {row.qso.whenUnix ? (
                  <button
                    type="button"
                    className="fd-edit"
                    title={t('contest.edit.title')}
                    aria-label={t('contest.edit.aria', { contact: contactLabel([row.qso]) })}
                    onClick={() => editRow(row.qso)}
                  >
                    {t('contest.edit.label')}
                  </button>
                ) : null}
              </ContestLogRow>
            )
          }).concat(editAt === -1 ? [edit] : [])}
        </div>
        {note && (
          <p className="fd-removed-note contest-logger-recent-note" role={note.alert ? 'alert' : 'status'}>
            {note.text}
          </p>
        )}
      </section>
    </>
  )
}

/**
 * ⭐ THE LARGE LOG LINE — the bar (its name and A− / A+), the contest strip at a size of its own with
 * the dupe warning right under the boxes, and the last contacts. The logger window's top, and the top
 * of the main window's Contest screen: one component, so the two cannot drift.
 *
 * It logs under the station's operating section (`contestLogMode`), as the cockpit there logs, and
 * says so when that is a mode the contest does not permit: "FT8 · no credit" on its top row, right
 * after the mode chip, with the header chip's own sentence on hover, for the same mode
 * (`stationMode`). A chip and not a band over the boxes, the operator's pick (2026-10-12). Logging
 * stays allowed; the contact is the sponsor's to score.
 *
 * Given `satellite`, the bird whose transponder the engine holds, one plain line under the bar says
 * to log that pass's contacts on the Satellites screen. Only the logger window passes it.
 *
 * ⛔ NOTHING HERE TRANSMITS: the strip gets no `esm`, and A− / A+ only size the text. `sharedEntry`
 * is the logger window's (`LogEntry`): there Esc clears the entry. Absent, in the main window, Esc
 * stays the window's stop (App's), and the strip takes part in the shared contact as the cockpits'
 * strips do.
 */
export function ContestLargeEntry({
  snap,
  fieldDay,
  fdActive,
  fdRuleset,
  title,
  sharedEntry,
  operator,
  onSaveOperator,
  operatorAsk,
  onOperatorFocused,
  satellite,
  barEnd,
  entryOnly = false,
}: {
  snap: AppSnapshot
  fieldDay: FieldDayStatus
  fdActive: boolean
  fdRuleset: FdRulesetDto | null
  /** The bar's name for the entry: the window's own in the logger window, the screen's in the main. */
  title: string
  sharedEntry?: 'logger'
  /** The operator at the key as this window's settings hold it (`fdOperator`), or null until they
   *  are read. The box on the title line shows it. */
  operator: string | null
  /** The window's narrow operator write (`setFdOperator`), never the whole-settings save. */
  onSaveOperator: (call: string) => void | Promise<unknown>
  /** Ctrl+O: the asks the operator box has not answered (`features/contestKeys.ts`). */
  operatorAsk?: number
  onOperatorFocused?: () => void
  /** The bird whose transponder the engine holds, while one is held (the logger window's read of
   *  `getSatTransponder`); null while none is. Absent in the main window, which draws no such line. */
  satellite?: string | null
  /** What ends the bar after A− / A+: the logger window's pin. Absent in the main window. */
  barEnd?: ReactNode
  /** The entry alone: no last contacts under it (the logger window's `entry` layout). */
  entryOnly?: boolean
}) {
  const section = snap.radio.operatingMode
  // PSK and JTTY are one section: the mode on the air is the snapshot's `keyboardMode`.
  const keyboardMode = useKeyboardMode(section, snap.radio.keyboardMode)
  const m = contestLogMode(section, keyboardMode, snap.link.tier)
  // THE TEXT SIZE (A− / A+): a factor on the app's Text size for the entry and the last contacts,
  // remembered on this computer for this window and clamped as it is read back.
  const [textScale, setTextScale] = useState(() => loggerTextScale(surfaceGet(TEXT_SCALE_KEY)))
  const stepText = (dir: 1 | -1) => {
    const next = boxScaleValue(Math.round((textScale + dir * BOX_SCALE_STEP) * 100) / 100) ?? 1
    setTextScale(next)
    surfaceSet(TEXT_SCALE_KEY, String(next))
  }
  const scaled = { '--logger-text-scale': textScale } as CSSProperties
  const recentBox = useRef<HTMLElement>(null)
  // A last contact's edit is over: the caret goes back to Call, as after the strip's own Ctrl+Y edit.
  const entryBox = useRef<HTMLDivElement>(null)
  const focusCall = () =>
    requestAnimationFrame(() =>
      entryBox.current?.querySelector<HTMLInputElement>('input.le-fd-input-call')?.focus({ preventScroll: true }),
    )
  return (
    <>
      <section className="contest-logger-entry" aria-label={t('contestLogger.entry.aria')}>
        <div className="contest-logger-bar">
          <span className="contest-logger-title">{title}</span>
          <span className="contest-logger-size">
            {t('contestLogger.size.label', { pct: Math.round(textScale * 100) })}
          </span>
          {/* A press sizes the text and takes no focus, so the caret stays in the box it was in. */}
          <button
            type="button"
            className="contest-logger-step"
            aria-label={t('contestLogger.size.smaller')}
            title={t('contestLogger.size.smaller')}
            disabled={textScale <= BOX_SCALE_MIN}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => stepText(-1)}
          >
            A−
          </button>
          <button
            type="button"
            className="contest-logger-step"
            aria-label={t('contestLogger.size.larger')}
            title={t('contestLogger.size.larger')}
            disabled={textScale >= BOX_SCALE_MAX}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => stepText(1)}
          >
            A+
          </button>
          {barEnd}
        </div>
        {satellite != null && (
          <p className="contest-logger-sat" role="status">
            {t('contestLogger.satellite.note', { bird: satellite })}
          </p>
        )}
        <div className="contest-logger-scaled" style={scaled} ref={entryBox}>
          <LogEntry
            snap={snap}
            mode={m.mode}
            defaultRst={m.defaultRst}
            exchange="terrestrial"
            titled={false}
            fieldDay={fieldDay}
            fdMode={m.fdMode}
            fdSubmode={m.fdSubmode}
            sharedEntry={sharedEntry}
            noCreditChip={
              <NoCreditChip fdActive={fdActive} ruleset={fdRuleset} activeMode={stationMode(section, snap.link.tier, keyboardMode)} />
            }
            operatorBox={
              <OperatorBox
                onLogLine
                value={operator}
                onSave={onSaveOperator}
                operatorAsk={operatorAsk}
                onOperatorFocused={onOperatorFocused}
              />
            }
          />
        </div>
      </section>
      {!entryOnly && (
        <>
          <LastContacts fieldDay={fieldDay} style={scaled} boxRef={recentBox} onEditDone={focusCall} />
          {/* THE LAST CONTACTS' DIVIDER: it sizes the list, and the Contest screen under it takes what is
              left (the Illinois QSO Party, 2026-10-18: "splitters to resize sub-windows"). The list stays
              no taller than its rows, so the divider shortens it and gives the room back; it paints
              nothing until moved (`.contest-logger-recent[data-sized]`, styles.css), is stored per window
              and fitted into the window on every load and resize, and Backspace or a double-click puts
              the list's own height back. It stops nothing and sends nothing. */}
          <PaneSeam
            axis="y"
            varName="--contest-recent-h"
            strip={recentBox}
            storageKey="nexus.split.contest.recent"
            min={CONTEST_RECENT_SPLIT_MIN}
            max={CONTEST_RECENT_SPLIT_MAX}
            defaultPct={null}
            label={t('contestLogger.recent.splitter.label')}
            className="contest-seam"
          />
        </>
      )}
    </>
  )
}

export function ContestLogger({
  snap,
  fdActive,
  fdRuleset,
  operator,
  onSaveOperator,
}: {
  snap: AppSnapshot
  fdActive: boolean
  fdRuleset: FdRulesetDto | null
  /** This window's operator at the key and its narrow write (DetachedPanel's): the entry's box. */
  operator: string | null
  onSaveOperator: (call: string) => void | Promise<unknown>
}) {
  // THE F-KEYS DO NOTHING HERE. In the main window they send messages; in this one they are not
  // bound to anything, and the webview's own F-key actions (F5 reloads the page) are held too.
  // With Alt, Ctrl or Cmd held a key is the system's (Alt+F4 still closes the window), as on the
  // main window's Contest screen.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (/^F\d{1,2}$/.test(e.key) && !e.altKey && !e.ctrlKey && !e.metaKey) e.preventDefault()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])
  const fd = snap.fieldDay ?? null
  // CTRL+O FROM THE LOG LINE (`features/contestKeys.ts`): the operator box is this window's own, on
  // the entry's title line while a contest runs, else in its copy of the Contest screen below.
  const [operatorAsk, setOperatorAsk] = useState(0)
  useOperatorBoxAsk(() => setOperatorAsk((n) => n + 1))
  // A contest switched by a pick saved in the main window: this copy of the Contest screen says
  // so too.
  const contestSwitch = useContestSwitch(fd)
  // THE LAYOUT: the whole window, or the entry alone. Only while a contest runs is there an entry.
  const [layout, setLayout] = useLoggerLayout()
  const entryOnly = fd != null && layout === 'entry'
  // THE BIRD HELD, for the line under the entry's bar: the engine's own hold, read every 2 s as the
  // Satellites screen and the rotor strip read it (one call at a time: it takes the engine mutex). A
  // read that fails keeps the last answer, as theirs do.
  const [satellite, setSatellite] = useState<string | null>(null)
  useEffect(
    () =>
      pollSingleFlight('contest logger satellite', 2_000, (owns) =>
        getSatTransponder()
          .then((h) => owns() && setSatellite(h?.name ?? null))
          .catch(() => {}),
      ),
    [],
  )
  return (
    <>
      {fd && (
        <ContestLargeEntry
          snap={snap}
          fieldDay={fd}
          fdActive={fdActive}
          fdRuleset={fdRuleset}
          title={t('contestLogger.title')}
          sharedEntry="logger"
          operator={operator}
          onSaveOperator={onSaveOperator}
          operatorAsk={operatorAsk}
          onOperatorFocused={() => setOperatorAsk(0)}
          satellite={satellite}
          barEnd={
            <>
              <button
                type="button"
                className={`contest-logger-toggle${entryOnly ? ' active' : ''}`}
                aria-pressed={entryOnly}
                title={t('contestLogger.layout.title')}
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => setLayout(entryOnly ? 'full' : 'entry')}
              >
                {t('contestLogger.layout.label')}
              </button>
              <KeepInFront />
            </>
          }
          entryOnly={entryOnly}
        />
      )}
      {!entryOnly && <main className="layout single">
        <ContestView
          fieldDay={fd}
          fdActive={fdActive}
          fdRuleset={fdRuleset}
          tier={snap.link.tier}
          section={snap.radio.operatingMode}
          keyboardMode={snap.radio.keyboardMode}
          logger
          contestSwitch={contestSwitch}
          operatorAsk={operatorAsk}
          onOperatorFocused={() => setOperatorAsk(0)}
          operatorBoxAbove={fd != null}
          entryAbove={fd != null}
        />
      </main>}
    </>
  )
}
