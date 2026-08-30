// THE FIELD DAY OPERATING COCKPIT — the shell (phase 3, landing 3).
//
// One screen for one job: log the contact in front of you, and never lose the callsign you
// are typing. Everything here is arranged around that.
//
// ── WHAT THIS SHELL IS ────────────────────────────────────────────────────────────────
// A Batch-1 cockpit shell with the four sanctioned child kinds and no more: the shared
// CockpitHeader (dial, band, power, Tune, STOP TX + this cockpit's own chips), ONE
// `.cockpit-panes` region of two columns, and the pinned `.cockpit-txdock`. No scope strip —
// legal, and the mode pane carries the feed instead.
//
// ── ZERO REMOVABLE PANES, AND THAT IS THE POINT ───────────────────────────────────────
// There is no ⊞ Panels menu, no `usePanelLayout`, no `panelHost`, and no vocabulary export —
// APRS's precedent, for a much sharper reason. A Field Day cockpit is worked at 2 AM by
// whoever is awake; a pane mis-hidden then is a pane nobody finds again before sunrise, and a
// keyer hidden mid-over is a stuck transmitter. With no ids at all, mis-hiding is
// UNREPRESENTABLE rather than guarded. The cost is real and paid deliberately: the
// vocabulary-driven guards (`panelState.test.ts`, the `stop-line.test.tsx` sweeps) never
// engage here, so `FdCockpit.structure.test.tsx` is hand-written and asserts the same things
// by construction — STOP TX present by accessible name, the dock outside the region, the
// router never stealing focus.
//
// ── THE STOP LINE ─────────────────────────────────────────────────────────────────────
// THE OPERATOR MUST NEVER BE UNABLE TO STOP A TRANSMISSION. This cockpit's census:
//   · Stop TX — CockpitHeader's, wired to `abort()` (stopCw when the class is CW, then
//     haltTx always). Header chrome, outside the pane region, no id anywhere.
//   · Esc — the window keydown below, checked BEFORE the typing guard, so it stops the rig
//     mid-callsign. Keyboard-only, so census-only by construction (no sweep can see it).
//   · Tune — the header's, and it stops only the carrier it started.
//   · PTT (PH class) — the dock's row, which drops the mic key it holds. Lifted verbatim from
//     Phone (see FdPttRow.tsx). Its keyup is what releases a mic key held by the window Space
//     bar, and it releases WHEREVER the caret went, which Phone's does not — the router below
//     can move focus mid-hold, and an unkey a guard can swallow is a stuck transmitter.
//   · Leaving this screen is a stop: the PH strip's unmount force-unkeys (rig AND this
//     cockpit's `keyed`, or a class that flips back paints ON-AIR over an idle rig), and the
//     unmount below stops the CW keyer, so the Dashboard button in this header cannot walk
//     away from a macro that is still sending.
//   ⚠️ Space is NOT on this census, and the reason is worth stating rather than rediscovering:
//     it keys only while NO field is focused, and in this cockpit the caret is in the Call
//     field almost all of the time (LogEntry lands it there on mount and after every logged
//     contact). Space is a convenience for the moments focus is elsewhere; it is never what
//     the guarantee rests on, and the FD call input drops spaces so a reflexive press
//     mid-callsign neither keys nor corrupts the call.
// Nothing on that list can be hidden, because nothing in this cockpit can be hidden.
//
// ── FOCUS IS THE PRODUCT ──────────────────────────────────────────────────────────────
// A callsign heard once and typed into a field that is no longer focused is a lost contact.
// Three mechanisms, in order of how often they save one: LogEntry focuses Call on mount and
// again after every logged contact (landing 2); the BARE-KEY ROUTER below catches the first
// printable character typed while nothing is focused and puts the caret in Call; and the
// boards are inert — no cell click, no section click, nothing in the pane region takes focus
// away from the entry row.
//
// ⚠️ THIS FILE IS ON THE **MIGRATED** LIST (i18n/hardcoded-strings.test.ts). What is NOT in
// the catalog and never will be: the band, the mode-class codes (CW/PH/DIG — the codes an
// entry is submitted with), the exchange, callsigns, every count and the event's clock time.
// ⚠️ THE SCREEN IS NOT FULLY TRANSLATED, and this file being MIGRATED does not say it is: the
// transmit surface is deferred as one batch, so the PTT row (FdPttRow.tsx), the voice-keyer
// pane (VoiceKeyer.tsx) and the header's own Stop TX / Tune / TX latch (CockpitHeader.tsx) all
// still read English in every locale. That is the transmit-path batch, and it moves together
// or the two PTT rows come to read as different controls; see FdPttRow.tsx's header.
import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from 'react'
import type { AppSnapshot, FieldDayStatus, ModeRequest, Tier } from '../types'
import type { FdRulesetDto } from '../api'
import {
  atuTune,
  cwDecode,
  haltTx,
  sendCw,
  setCwWpm,
  setFrequency,
  setPtt,
  setRfPower,
  setTune,
  setTxEnabled,
  stopCw,
} from '../api'
import { bandLabelForMhz, sidebandForQsy } from '../band'
import { pushToast, withErrorToast } from '../toast'
import { fdCountdownLabel, fdEventFromWindow, type FdKind } from '../fdEvent'
import { useRegionCols } from '../useRegionCols'
import { usePinnedScroll } from '../usePinnedScroll'
import { IS_MAC, FN_KEY_HINT } from '../platform'
import { CockpitHeader } from './CockpitHeader'
import { BandPicker } from './BandPicker'
import { CockpitPaneFrame } from './panes/CockpitPaneFrame'
import { LogEntry } from './LogEntry'
import { OperateDecodes } from './OperateDecodes'
import { VoiceKeyer } from './VoiceKeyer'
import { FdAdvisories } from './FdAdvisories'
import { FdPttRow } from './FdPttRow'
import { FdBandModeGrid, FD_MODE_CLASSES, type FdModeClass } from './FdBandModeGrid'
import { FdRateChip, FdScoreChip } from './FdChips'
import { FdSectionsPanel, fdWorkedSectionSet } from './FdSectionsPanel'
import { DEFAULT_FD_MACROS } from './CwCockpit'
import { t } from '../i18n'

/**
 * THE POSITION'S MODE CLASS, read from the radio.
 *
 * ⚠️ THIS DECIDES THE DUPE KEY AND THE LOGGED CLASS. Get it wrong and the cockpit tells the
 * operator a station is new when the log will refuse them (or the reverse), and the contact
 * that does land is submitted under the wrong class. That is why it is a pure function with a
 * unit test over every mode string the radio layer can emit, and why the header chip that
 * shows it is OVERRIDABLE — a rig parked in a DATA sub-mode while the operator works SSB is
 * the case no derivation can get right on its own.
 *
 * ⚠️ THE DESIGN SAID `snap.radio.mode`. THERE IS NO SUCH FIELD. The two that exist are
 * `rigMode` — the rig's own mode read back over CAT (`Engine::observe_rig_mode`, Hamlib
 * names; `None` until a CAT read succeeds) — and `operatingMode`, the engine's live SECTION
 * ('digital' | 'phone' | 'cw' | 'rtty' | 'keyboard', `settings::OperatingMode`). The rig's
 * answer is what is actually on the air, so it wins; the section is the fallback that is
 * always present, which matters because a Field Day station with no CAT is an ordinary
 * Field Day station.
 *
 * THE ORDER OF THE TESTS BELOW IS LOAD-BEARING, and the anchoring is half of why. Every data
 * sub-mode name contains a sideband name (PKTUSB, DATA-U, USB-D, LSB-D, PKTLSB, PKTFM, PKTAM);
 * the voice test is `^`-anchored so the PKT and DATA prefixes miss it anyway, but the SUFFIX
 * forms — USB-D, LSB-D, FM-D — start with a sideband and would be called voice by a rule that
 * ran first. Data, then CW, then voice, then the design's "else DIG"; the derivation test
 * drives the swap and watches those three go red.
 */
export function fdModeClassFromRig(
  rigMode: string | null | undefined,
  operatingMode?: string | null,
): FdModeClass {
  const m = (rigMode ?? '').trim().toUpperCase()
  if (m !== '') {
    // Data sub-modes and the true digital modes. PKT/DATA/-D are the sub-mode spellings
    // Hamlib, OmniRig and the CI-V broker all produce; RTTY/FSK/PSK are modes in their own
    // right. All of them are DIG for Field Day scoring.
    if (/PKT|DATA|RTTY|FSK|PSK|DIG|C4FM|DSTAR|DPMR|NXDN|P25|DCR|-D$/.test(m)) return 'DIG'
    // CW and CW-reverse (CW, CWR, CW-R, CWL). No digital mode name starts with CW.
    if (m.startsWith('CW')) return 'CW'
    // Voice: both sidebands, AM and its synchronous variants, FM and its narrow/wide names.
    if (/^(USB|LSB|SSB|DSB|AM|AMN|AMS|SAM|SAL|SAH|ECSS|FM|FMN|WFM)/.test(m)) return 'PH'
    return 'DIG'
  }
  // No CAT read yet: the engine's own operating section, which is always present.
  switch ((operatingMode ?? '').trim().toLowerCase()) {
    case 'cw':
      return 'CW'
    case 'phone':
      return 'PH'
    default:
      return 'DIG'
  }
}

/** Which licensed band plan the header's picker should offer for a mode class. */
const BAND_PLAN_MODE: Record<FdModeClass, string> = { CW: 'cw', PH: 'phone', DIG: 'digital' }

/** UTC clock time for the event-end chip. A measurement, formatted here — never a catalog
 *  entry, and never localised into a form the operator's log sheet does not use. */
function utcHhMm(unix: number): string {
  const d = new Date(unix * 1000)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}`
}

/**
 * ⚠️ MIRRORS `clubChipText` in FieldDayView.tsx — same three states, same catalog keys, same
 * honesty rule (the queue is IN the label, so "connected but behind" can never masquerade as
 * synced). Mirrored rather than exported across because FieldDayView is a hot shared file
 * this programme touches with exactly one word; the mirror is five lines and is pinned in
 * FdCockpit.structure.test.tsx against that function's source.
 */
function clubChipText(syncState: string, queued: number): string {
  if (syncState === 'synced') return t('fieldDay.club.state.synced')
  if (syncState === 'behind') return t('fieldDay.club.state.behind', { queued })
  return t('fieldDay.club.state.offline', { queued })
}

// ── chip styling (inline off shared tokens — the FdChips / SectionsBoard idiom) ────────

const CHIP: CSSProperties = {
  display: 'inline-flex',
  alignItems: 'baseline',
  gap: 5,
  padding: '3px 10px',
  borderRadius: 'var(--radius-sm)',
  fontSize: 12,
  fontWeight: 700,
  whiteSpace: 'nowrap',
  color: 'var(--text-dim)',
  background: 'var(--bg-elev)',
  border: '1px solid var(--border-soft)',
}

interface Props {
  snap: AppSnapshot
  onSnap?: (s: AppSnapshot) => void
  /** The Field Day status block (`snap.fieldDay`), passed as FieldDayView takes it. */
  fieldDay: FieldDayStatus | null
  /** Run / S&P — the same two requests the dashboard's role toggle sends. */
  onSetMode: (mode: ModeRequest) => void
  /** The active event's ruleset facts (App fetches `get_fd_ruleset` once) — the warn-only
   *  advisories read these. */
  fdRuleset?: FdRulesetDto | null
  /** The active digital tier (`snap.link.tier`) — the banned-mode advisory checks it, and
   *  the DIG monitor stamps its period separators with it. */
  tier?: Tier
  /** Leave for the Field Day dashboard (setup, exports, bonuses, the club board). Omitted ⇒
   *  no toggle button; App owns the persisted choice. */
  onOpenDashboard?: () => void
  /** Open the FT Operate cockpit — the DIG note's one action. Omitted ⇒ no button. */
  onOpenOperate?: () => void
}

/**
 * The Field Day operating cockpit.
 *
 * Run cost per contact, steady state: the callsign and Enter. The exchange refills itself
 * from the previous contact (LogEntry), the band and mode class come from the radio, and the
 * boards repaint from the same 300 ms snapshot everything else on this screen reads. There is
 * no IPC on a keystroke and no new backend behind any of it.
 */
export function FdCockpit({
  snap,
  onSnap,
  fieldDay,
  onSetMode,
  fdRuleset = null,
  tier,
  onOpenDashboard,
  onOpenOperate,
}: Props) {
  // The class is DERIVED and OVERRIDABLE. `null` = follow the radio; a value pins it. Session
  // state on purpose: an override answers "the rig is lying right now", which is not a fact
  // worth carrying into next weekend.
  const [override, setOverride] = useState<FdModeClass | null>(null)
  const derivedClass = fdModeClassFromRig(snap.radio.rigMode, snap.radio.operatingMode)
  const modeClass = override ?? derivedClass

  // The in-progress exchange, mirrored out of LogEntry (landing 2). The boards paint from it
  // per keystroke; nothing else reads it.
  const [draft, setDraft] = useState({ call: '', cls: '', section: '' })

  // Double-clicking a decode PREFILLS the call — it never transmits. Same `pendingWork`
  // channel the CW and Phone cockpits use for a spot click.
  const [pendingWork, setPendingWork] = useState<{ call: string; ts: number } | null>(null)

  // PTT state, LIFTED VERBATIM from PhoneCockpit (the row's markup is FdPttRow.tsx). It lives
  // here because the voice-keyer pane must see `keyed` — playing a canned message over a held
  // mic key fights the live over — and because the unmount force-unkey belongs to the shell.
  const [keyed, setKeyed] = useState(false)
  const [lock, setLock] = useState(false) // hands-free PTT (toggle instead of hold)
  const [power, setPower] = useState(100) // % — only pushed to the rig once touched

  // The CW copy feed (mode pane, CW class only).
  const [decoded, setDecoded] = useState<{ text: string; sent: string[] }>({ text: '', sent: [] })

  const { ref: panesRef } = useRegionCols<HTMLDivElement>(2)
  const decodePin = usePinnedScroll<HTMLDivElement>()
  const sentPin = usePinnedScroll<HTMLDivElement>()
  /** The dock, so the router can find the Call field without LogEntry exposing a ref. */
  const dockRef = useRef<HTMLDivElement>(null)

  // Live snapshot ref so the keyboard/PTT handlers (bound once, or on `lock` alone) read the
  // CURRENT privilege state rather than whatever existed when they were bound.
  const snapRef = useRef(snap)
  snapRef.current = snap
  const modeClassRef = useRef(modeClass)
  modeClassRef.current = modeClass
  /** Is THIS cockpit holding the mic key right now? Read by the Space keyup and by the
   *  force-unkey, both of which must act on what is true at the moment they run rather than on
   *  what was true when they were bound. See the Space effect for why an unkey may not be
   *  conditional on anything the operator can change mid-hold. */
  const keyedRef = useRef(keyed)
  keyedRef.current = keyed

  // ── THE STOP ──────────────────────────────────────────────────────────────────────────
  // Stop the CW keyer when this position is keying CW, and drop any carrier / stray PTT
  // always. Wired to the header's Stop TX AND to Esc, so the two are literally one action.
  const abort = useCallback(() => {
    if (modeClassRef.current === 'CW') void stopCw()
    void haltTx()
  }, [])

  // LEAVING THIS SCREEN IS A STOP, for every class and not just PH. The PH strip's own effect
  // force-unkeys when it goes (below), but a CW macro is a QUEUE the engine keeps sending:
  // F1 arms `send_cw`, the operator clicks the Dashboard button in this very header, and the
  // cockpit that owned the only Stop TX beside it is gone while the keyer runs on. Mount-
  // scoped (empty deps) so it fires on the way OUT of the cockpit and not on a class flip —
  // the class read at that moment is the one that was actually keying.
  useEffect(() => {
    return () => {
      if (modeClassRef.current === 'CW') void stopCw()
    }
  }, [])

  // ── PTT (verbatim from PhoneCockpit) ──────────────────────────────────────────────────
  const key = (on: boolean) => {
    // Don't key (or show ON-AIR) outside license privileges — the engine blocks it anyway.
    if (on && !snapRef.current.radio.txAllowed) {
      pushToast(t('phone.tx.locked'), 'info', 3500)
      return
    }
    if (on && !snapRef.current.radio.txEnabled) {
      pushToast(t('phone.tx.turnedBackOn'), 'info', 4000)
      void setTxEnabled(true)
        .then((s) => onSnap?.(s))
        .catch(() => {})
      return
    }
    setKeyed(on)
    void setPtt(on)
  }
  const onPttDown = () => {
    if (lock) {
      key(!keyed) // hands-free: toggle
    } else {
      key(true)
    }
  }
  const onPttUp = () => {
    if (!lock) key(false)
  }

  // Spacebar = push-to-talk (hold), unless typing in a field. Phone's listener, with ONE
  // deliberate divergence, named below and pinned by the lift guard in the structure test.
  // Bound only while the position is on PHONE: a CW or digital position has no PTT row, and a
  // Space bar that keys a rig nobody is talking into is a stuck transmitter with no button
  // beside it to release.
  //
  // ⚠️ THE RELEASE IS UNCONDITIONAL, AND PHONE'S IS NOT. Phone guards BOTH halves on the
  // target ("not while typing in a field"), which is sound there because nothing in that
  // cockpit ever moves focus by itself. Here the bare-key router below puts the caret in the
  // Call field on a keystroke — so an operator can key with Space, type the callsign he is
  // hearing, and release into a guard that swallows the unkey, leaving the rig keyed with the
  // button still reading "release to stop", which he did. So: the DOWN half keeps Phone's
  // guard (a space typed into a field must never start a transmission), and the UP half asks
  // only whether WE are keyed. An unkey a guard can swallow is a stuck transmitter.
  //
  // Where the caret is decides whether Space can KEY at all, and in this cockpit the caret is
  // in the Call field almost all of the time (LogEntry lands it there on mount and after every
  // contact) — see the census note in this file's header. That is why the FD call input drops
  // spaces outright (LogEntry): the reflexive press mid-callsign neither keys nor corrupts.
  const phone = modeClass === 'PH'
  useEffect(() => {
    if (!phone) return
    const isField = (el: EventTarget | null) =>
      el instanceof HTMLElement && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA')
    const down = (e: KeyboardEvent) => {
      if (e.code === 'Space' && !e.repeat && !isField(e.target) && !lock) {
        e.preventDefault()
        key(true)
      }
    }
    const up = (e: KeyboardEvent) => {
      if (e.code === 'Space' && keyedRef.current && !lock) {
        e.preventDefault()
        key(false)
      }
    }
    window.addEventListener('keydown', down)
    window.addEventListener('keyup', up)
    return () => {
      window.removeEventListener('keydown', down)
      window.removeEventListener('keyup', up)
      // Safety: never leave the rig keyed on unmount — a nav away, or a mode-class flip that
      // takes the PH strip off the dock. `setKeyed(false)` goes WITH it: the rig and this
      // cockpit's idea of the rig must never part company, or a class that flips back (a
      // single CAT poll reading CW does it, with no operator action at all) redraws a red
      // "ON AIR — release to stop" over an unkeyed rig — and in Lock mode the press that
      // reads as a release would key the transmitter.
      setKeyed(false)
      void setPtt(false)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [lock, phone])

  // ── CW SEND ───────────────────────────────────────────────────────────────────────────
  const sendMacro = (line: string) => {
    if (!line.trim()) return
    if (!snapRef.current.radio.txAllowed) {
      pushToast(t('cw.send.txLocked'), 'info', 3500)
      return
    }
    void withErrorToast(() => sendCw(line), t('cw.send.failed'))
  }

  // The CW copy feed. Polled only while the position is on CW — a Phone tent has no use for
  // a decoder thread, and an FD laptop is usually the oldest machine in the club.
  const cw = modeClass === 'CW'
  useEffect(() => {
    if (!cw) return
    let alive = true
    const tick = () => {
      cwDecode(0.5)
        .then((d) => {
          if (alive) setDecoded({ text: d.text, sent: d.sent })
        })
        .catch(() => {})
    }
    tick()
    const id = setInterval(tick, 700)
    return () => {
      alive = false
      clearInterval(id)
    }
  }, [cw])

  // ── FOCUS ─────────────────────────────────────────────────────────────────────────────
  /**
   * Put the caret in the Call field.
   *
   * ⚠️ FOUND BY CLASS, and that is a real coupling: `LogEntry` owns the input and exposes no
   * ref, and giving it one would be a second seam in a file this programme has already opened
   * once. `.le-fd-input-call` is that input's own class in the FD variant; the structure test
   * asserts the router actually lands focus in an input inside `.log-entry-fd`, so a rename
   * there fails HERE rather than silently costing the router.
   * `preventScroll`: this readies the field, it must never scroll the cockpit.
   */
  const focusCall = useCallback(() => {
    const el = dockRef.current?.querySelector<HTMLInputElement>('.le-fd-input-call')
    el?.focus({ preventScroll: true })
  }, [])

  // ── THE KEYBOARD ──────────────────────────────────────────────────────────────────────
  // One window listener, and the ORDER inside it is the contract:
  //
  //   1. Esc — BEFORE the typing guard. Mid-callsign is exactly when an operator reaches for
  //      it, and a stop that only works when no field is focused is not a stop.
  //   2. The mode's own keys — F1–F8 macros and PgUp/PgDn speed while the class is CW. Bound
  //      regardless of focus, on the CW cockpit's precedent: F-keys do not type, and the whole
  //      point of a run is to send the exchange without leaving the Call field.
  //   3. THE BARE-KEY ROUTER — a printable character with no modifier, and only while NOTHING
  //      is focused. It moves the caret to Call so the character lands there. It never
  //      preventDefaults: the browser delivers the keypress to whatever is focused when the
  //      default action runs, which is now the Call field. (jsdom performs no default text
  //      insertion at all, so the test asserts the FOCUS, not the character.)
  //
  // Space is deliberately outside the router (it is the PH class's PTT, handled above), and
  // every modifier combination passes straight through — Ctrl/⌘+1–9 is App's memory recall.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        abort()
        return
      }
      if (e.ctrlKey || e.metaKey || e.altKey) return
      if (modeClassRef.current === 'CW') {
        const macro = DEFAULT_FD_MACROS.find((m) => m.key === e.key)
        if (macro) {
          e.preventDefault()
          sendMacro(macro.text)
          return
        }
        if (e.key === 'PageUp' || e.key === 'PageDown') {
          e.preventDefault()
          const step = (e.shiftKey ? 4 : 2) * (e.key === 'PageUp' ? 1 : -1)
          const next = Math.min(50, Math.max(5, (snapRef.current.radio.cwWpm ?? 20) + step))
          void setCwWpm(next, true)
            .then((s) => onSnap?.(s))
            .catch(() => {})
          return
        }
      }
      const el = document.activeElement
      // SELECT is in this list for a reason that is not typing: a native select answers a
      // printable key with type-ahead ("4" picks 40m in the band picker, "C" picks CW in the
      // mode-class override), so a router that fired there would eat the selection AND yank
      // the caret out of a control the operator is mid-way through using.
      const typing =
        el instanceof HTMLElement &&
        (el.tagName === 'INPUT' ||
          el.tagName === 'TEXTAREA' ||
          el.tagName === 'SELECT' ||
          el.isContentEditable)
      if (typing) return
      if (/^[a-zA-Z0-9/]$/.test(e.key)) focusCall()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [abort, focusCall])

  // A decode double-click prefills the Call field and puts the caret in it. It never
  // transmits and never touches the FT sequencer — the cockpit's whole relationship with the
  // digital path is read-only (the FT hard gate).
  const prefillCall = (call: string) => {
    setPendingWork({ call, ts: Date.now() })
    focusCall()
  }

  // Commit a typed dial from the shared header readout — the CW/Phone path verbatim: keep the
  // current sideband so an in-band entry never flips the mode, and let an off-plan frequency
  // through (listening off the ham bands is first-class, operator 2026-08-13).
  const commitDial = (mhz: number) => {
    void setFrequency(
      mhz,
      bandLabelForMhz(mhz),
      sidebandForQsy(mhz, snap.radio.dialMhz, snap.radio.sideband),
    )
      .then((s) => s && onSnap?.(s))
      .catch(() => {})
  }

  // ── HEADER CHIPS ──────────────────────────────────────────────────────────────────────
  const running = fieldDay?.running ?? false
  const club = fieldDay?.club ?? null
  const fdExchange = fieldDay ? `${fieldDay.myClass} ${fieldDay.mySection}`.trim() : ''
  const eventKind: FdKind = fieldDay?.event === 'wfd' ? 'wfd' : 'arrlfd'
  const fdEvent = useMemo(
    () => fdEventFromWindow(eventKind, fieldDay?.eventStartUnix, fieldDay?.eventEndUnix),
    [eventKind, fieldDay?.eventStartUnix, fieldDay?.eventEndUnix],
  )
  // Running: the wall time the window closes (a Field Day operator plans the last hour around
  // it). Not running: how long until it opens — the dashboard's own countdown wording.
  const eventChip = useMemo(() => {
    if (!fdEvent) return null
    const now = new Date()
    const nowUnix = Math.floor(now.getTime() / 1000)
    if (nowUnix >= fdEvent.startUnix && nowUnix < fdEvent.endUnix) {
      return t('fieldDay.cockpit.event.ends', { time: utcHhMm(fdEvent.endUnix) })
    }
    return fdCountdownLabel(now, fdEvent)
  }, [fdEvent])

  // Growth-keyed for the reason the grid is (FdBandModeGrid) and the panel below now is:
  // `fieldDay` is a fresh object every 300 ms poll, so keying on it walks the whole worked list
  // twice a second for a number that only changes when the log grows.
  const workedLen = fieldDay?.workedSections?.length ?? 0
  const fdLogLen = fieldDay?.log?.length ?? 0
  const workedCount = useMemo(
    () => fdWorkedSectionSet(fieldDay).size,
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [workedLen, fdLogLen],
  )

  // ── THE MODE PANE (lead column) ───────────────────────────────────────────────────────
  // One pane, three contents, and it NEVER changes its DOM parent: the column is keyed
  // "lead", so a class flip swaps the pane's contents without moving the boards beside it.
  const modePane =
    modeClass === 'DIG' ? (
      <CockpitPaneFrame title={t('fieldDay.cockpit.pane.decodes')} paneId="fdDecodes">
        <div className="fd-dig-monitor" title={t('fieldDay.cockpit.dig.prefill.title')}>
          <OperateDecodes
            decodes={snap.recentDecodes}
            slot={snap.radio.slot}
            rxOffsetHz={snap.radio.rxOffsetHz}
            band={snap.radio.band}
            tier={tier ?? 'FT8'}
            harqRescues={snap.harqRescues}
            myGrid={snap.mygrid}
            // READ-ONLY MONITOR. WSJT-X's double-click works a station; here it does the one
            // thing this cockpit is allowed to do with the digital path — put the callsign in
            // the log. Driving the FT sequencer from a second cockpit is the FT hard gate.
            onCall={(call) => prefillCall(call)}
            title={t('fieldDay.cockpit.pane.decodes')}
          />
        </div>
      </CockpitPaneFrame>
    ) : modeClass === 'PH' ? (
      <CockpitPaneFrame title={t('phone.pane.voiceKeyer.title')} paneId="fdKeyer">
        <VoiceKeyer
          txEnabled={snap.radio.txEnabled}
          keyed={keyed}
          transmitting={snap.radio.transmitting}
          fdExchange={fdExchange}
        />
      </CockpitPaneFrame>
    ) : (
      // ⚠️ TWO FRAMES, NOT ONE PANE WITH TWO BLOCKS — the CW cockpit's shape, and it is
      // structural rather than cosmetic. `.pane-body > .cw-decode` is `height: 100%`
      // (styles.css), so two of them in ONE body ask for 200% of it and the second lands
      // entirely below the pane's fold at every window size: the "what I just sent" echo —
      // the only confirmation that a macro expanded to the right callsign — would never be
      // on screen. Copy is the fill pane; the sent echo is a bounded echo, so `fit="content"`,
      // exactly as CwCockpit sizes the same two blocks.
      <>
        <CockpitPaneFrame title={t('cw.pane.decode.title')} paneId="fdCw" weight={3}>
          <div className="cw-decode panel">
            <div
              className="cw-decode-text"
              ref={decodePin.ref}
              onScroll={decodePin.onScroll}
              role="log"
              aria-label={t('cw.decode.log.aria')}
            >
              {decoded.text || <span className="cw-decode-idle">{t('cw.decode.listening')}</span>}
            </div>
          </div>
        </CockpitPaneFrame>
        <CockpitPaneFrame title={t('cw.pane.sent.title')} paneId="fdCwSent" fit="content">
          <div className="cw-decode cw-sent-panel panel" title={t('cw.sent.title')}>
            <div className="cw-decode-text" ref={sentPin.ref} onScroll={sentPin.onScroll}>
              {decoded.sent.map((line, i) => (
                <div key={i} className="cw-sent-line">
                  {line}
                </div>
              ))}
            </div>
          </div>
        </CockpitPaneFrame>
      </>
    )

  // ── THE MODE STRIP (dock) ─────────────────────────────────────────────────────────────
  const modeStrip =
    modeClass === 'CW' ? (
      <div className="cw-macros" role="group" aria-label={t('cw.macros.aria')}>
        {/* The buttons ADVERTISE their F-keys, and default Mac keyboards eat bare F-keys as
            media keys — the tooltip carries the cure there. A caption that is a WORD arrives
            as a catalog key; one that is on-air shorthand (CQ FD, TU, AGN, ?) is written as
            it stands, because that is what goes out on the air. */}
        {DEFAULT_FD_MACROS.map((m) => (
          <button
            key={m.key}
            type="button"
            className="cw-macro"
            onClick={() => sendMacro(m.text)}
            title={`${m.text}${IS_MAC ? `\n${FN_KEY_HINT}` : ''}`}
          >
            <span className="cw-macro-key">{m.key}</span>
            <span className="cw-macro-label">{m.labelKey ? t(m.labelKey) : m.label || m.key}</span>
          </button>
        ))}
      </div>
    ) : modeClass === 'PH' ? (
      <FdPttRow
        txAllowed={snap.radio.txAllowed}
        txEnabled={snap.radio.txEnabled}
        keyed={keyed}
        lock={lock}
        onLockChange={setLock}
        onPttDown={onPttDown}
        onPttUp={onPttUp}
        onToggleKey={() => key(!keyed)}
        fdExchange={fdExchange}
      />
    ) : (
      <div className="fd-dig-note">
        <span>{t('fieldDay.cockpit.dig.note')}</span>
        {onOpenOperate && (
          <button type="button" className="export-btn" onClick={onOpenOperate}>
            {t('fieldDay.cockpit.dig.open')}
          </button>
        )}
      </div>
    )

  return (
    <main className="layout single fd-cockpit">
      <CockpitHeader
        snap={snap}
        onSnap={onSnap}
        modeIndicator={
          // THE FIRST AND LARGEST CHIP, and it is a control: the derivation is right almost
          // always and catastrophic when it is not (see fdModeClassFromRig). The <select>
          // beside the reading is the recourse — "Auto (PH)" follows the radio, a code pins it.
          <span className="fd-modeclass" title={t('fieldDay.cockpit.modeClass.title')}>
            <span className="fd-modeclass-val">{modeClass}</span>
            <select
              className="fd-modeclass-sel"
              value={override ?? ''}
              aria-label={t('fieldDay.cockpit.modeClass.aria')}
              onChange={(e) => setOverride((e.target.value || null) as FdModeClass | null)}
            >
              <option value="">{t('fieldDay.cockpit.modeClass.auto', { mode: derivedClass })}</option>
              {FD_MODE_CLASSES.map((m) => (
                <option key={m} value={m}>
                  {m}
                </option>
              ))}
            </select>
          </span>
        }
        bandControl={<BandPicker snap={snap} mode={BAND_PLAN_MODE[modeClass]} onSnap={onSnap} />}
        onCommitDial={commitDial}
        wheelTune
        digitTune
        // No RF power slider on CW (the header collapses the region) — CW has none, exactly
        // as in the CW cockpit.
        power={
          modeClass === 'CW'
            ? undefined
            : {
                value: power,
                unit: '%',
                onChange: (v) => {
                  setPower(v)
                  void setRfPower(v / 100)
                },
              }
        }
        onTune={(on) => void setTune(on).then((s) => onSnap?.(s))}
        onAtuTune={() =>
          void atuTune()
            .then((s) => onSnap?.(s))
            .catch((e) => pushToast(String(e), 'error'))
        }
        onStopTx={abort}
      >
        <div className="fd-role-toggle" role="group" aria-label={t('fieldDay.role.aria')}>
          <button
            type="button"
            className={`fd-role-btn${running ? ' active' : ''}`}
            aria-pressed={running}
            onClick={() => onSetMode('fieldday-run')}
          >
            {t('fieldDay.role.running')}
          </button>
          <button
            type="button"
            className={`fd-role-btn${!running ? ' active' : ''}`}
            aria-pressed={!running}
            onClick={() => onSetMode('fieldday-sp')}
          >
            {t('fieldDay.role.sp')}
          </button>
        </div>
        <FdRateChip log={fieldDay?.log ?? []} />
        <FdScoreChip fieldDay={fieldDay} />
        {eventChip && <span style={CHIP}>{eventChip}</span>}
        {club && (
          <span style={CHIP} title={t('fieldDay.club.state.title')}>
            {clubChipText(club.syncState, club.queued)}
          </span>
        )}
        <FdAdvisories fdActive ruleset={fdRuleset} activeMode={tier} />
        {onOpenDashboard && (
          <button
            type="button"
            className="export-btn"
            onClick={onOpenDashboard}
            title={t('fieldDay.cockpit.dashboard.title')}
          >
            {t('fieldDay.cockpit.dashboard')}
          </button>
        )}
      </CockpitHeader>

      {/* THE PANE REGION — two columns, both ALWAYS RENDERED, both keyed.
          Always rendered is what actually holds the panes' identity across a tier flip today:
          two static slots of one fragment reconcile by position, and a pane that landed under
          a different column div would be UNMOUNTED by React — which is how a half-typed QSO
          was wiped mid-run in the 2026-07-31 fix round. The KEYS are belt to that brace: the
          moment a tier ternary swaps whole branches here (CW and Phone both have one), keys
          are the only thing left holding identity, and adding them then is a fix nobody
          remembers to make. `maxCols={2}` so the region never opens a third track it has
          nothing to put in.
          Nothing in here takes focus: the grid cells are inert (click-to-QSY was refused for
          v1) and the sections board is a checklist, not a picker. */}
      <div className="cockpit-panes" ref={panesRef}>
        <div className="cockpit-col" key="lead">
          {modePane}
        </div>
        <div className="cockpit-col" key="boards">
          <CockpitPaneFrame title={t('fieldDay.cockpit.pane.grid')} paneId="fdGrid" fit="content">
            <FdBandModeGrid
              log={fieldDay?.log ?? []}
              clubDupes={club?.dupes}
              band={snap.radio.band}
              modeClass={modeClass}
              draftCall={draft.call}
            />
          </CockpitPaneFrame>
          <CockpitPaneFrame
            title={t('fieldDay.cockpit.pane.sections', { count: workedCount })}
            paneId="fdSections"
          >
            <FdSectionsPanel fieldDay={fieldDay} draftSection={draft.section} />
          </CockpitPaneFrame>
        </div>
      </div>

      {/* THE TX DOCK — the entry row and the mode's transmit strip, pinned OUTSIDE the pane
          region. There is no pane layout here to move them and no ⊞ id to hide them; Stop TX
          and Tune are up in the header for the same reason.

          ⚠️ THE DOCK IS BOTTOM-ANCHORED, so anything that makes it TALLER moves the fields UP —
          and the FD strip's verdicts (own dupe, club warning, incomplete exchange) appear and
          vanish per keystroke, right underneath the fields being typed into. The fix is not to
          move them: `.fd-cockpit .log-entry-fd` reserves one verdict line's height in
          styles.css, so the line is already there and simply fills. Same reasoning as the TX
          meters sitting at the TOP of the Phone and CW docks. */}
      <div className="cockpit-txdock" ref={dockRef}>
        <div className="fd-entry" role="group" aria-label={t('fieldDay.cockpit.entry.aria')}>
          <LogEntry
            snap={snap}
            mode={modeClass === 'CW' ? 'CW' : 'SSB'}
            defaultRst={modeClass === 'CW' ? '599' : '59'}
            exchange="terrestrial"
            titled={false}
            fieldDay={fieldDay}
            // The scoring class, straight through — the strip's own dupe verdict, the boards
            // here and the contact the engine commits all key off the SAME value, so they
            // cannot disagree about which cell a station is in.
            fdMode={modeClass}
            onFdDraftChange={setDraft}
            // The one host that asks for it: this strip IS this screen, so the caret starts in
            // Call. Phone and CW must not (a focused field disarms their Space PTT) — see the
            // prop's own note in LogEntry.
            autoFocusCall
            pendingWork={pendingWork}
            onConsumeWork={() => setPendingWork(null)}
          />
        </div>
        {modeStrip}
      </div>
    </main>
  )
}
