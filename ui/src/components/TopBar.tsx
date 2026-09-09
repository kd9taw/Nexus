// ⚠️ THIS FILE IS **PARTIAL** ON THE i18n LIST (i18n/hardcoded-strings.test.ts), and what is
// deferred is the TX CLUSTER: TX On/Off (its two tooltip arms state the abort semantics — "an
// FT over already in flight finishes" — and its labels name the latch), TUNE and STOP TX, which
// key and cut a carrier, and the TX WATCHDOG chip, which is what the watchdog says when it has
// halted a transmission. Transmit-path controls and their accessible names move in their own
// batch, with the stop-line sweeps re-run. Everything else here is migrated under `topbar.*`.
//
// The units rule lands on the BAR: every tier and mode name, the DT and clock readouts, the
// slot countdown, the callsign and grid, and the four plates below are tokens and measurements
// and stay in the code.
import { useEffect, useState } from 'react'
import type { BandChannel, LinkState, RadioStatus, RadioSummary, Tier } from '../types'
import { isRxOnly } from '../types'
import { Menu } from './ui/Menu'
import { FrequencyControl } from './FrequencyControl'
import { StatusLane } from './StatusLane'
import { LiveLevelMeter } from './LiveMeters'
import { RadioSwitcher } from './RadioSwitcher'
import { appVersion } from '../api'
import { t } from '../i18n'
import type { MessageKey } from '../i18n'
import { T } from '../i18n/T'

/** The bar's own plates: the product name, the two directions of the TX/RX indicator, the
 *  operator prefix, the recording badge and the UTC label. Tokens, named so the catalog guard
 *  reads them as the deliberate constants they are. */
const NEXUS = 'Nexus'
const TX = 'TX'
const RX = 'RX'
const OP = 'OP'
const REC = 'REC'
const UTC = 'UTC'

/** The tier pills in the top bar. FT8/FT4 transmit; the six WSJT-X modes below
 *  them are DECODE-ONLY (modes::tx_mode refuses them in the engine) and carry a
 *  dashed edge so that is visible before the operator tries to call CQ rather
 *  than after. Order is deliberate: the two Tempo tiers, then the two production
 *  FT modes, then the receive-only set roughly by how often they get used.
 *
 *  Where a title points into Settings it names the SECTION, never the tab: a pill is a tier
 *  selector, so these five cannot become deep links, and a tab name in a sentence nobody can
 *  click goes stale the next time the rail is rearranged (this said "Settings ▸ Modes").
 *
 *  The NAMES are the modes' and tiers' own — tokens, and they stay written here. The titles are
 *  prose and resolve through the catalog when the pill renders (a resolved constant would
 *  freeze the first locale loaded). */
const TIER_PILLS: {
  tier: Tier
  small?: string
  name: string
  titleKey: MessageKey
  rxOnly?: boolean
}[] = [
  { tier: 'TempoFast', small: 'Tempo', name: 'Fast', titleKey: 'topbar.tier.tempoFast.title' },
  { tier: 'TempoDeep', small: 'Tempo', name: 'Deep', titleKey: 'topbar.tier.tempoDeep.title' },
  { tier: 'FT4', name: 'FT4', titleKey: 'topbar.tier.ft4.title' },
  { tier: 'FT8', name: 'FT8', titleKey: 'topbar.tier.ft8.title' },
  { tier: 'FT2', name: 'FT2', titleKey: 'topbar.tier.ft2.title' },
  { tier: 'WSPR', name: 'WSPR', small: 'BCN', titleKey: 'topbar.tier.wspr.title' },
  { tier: 'Q65', name: 'Q65', titleKey: 'topbar.tier.q65.title' },
  { tier: 'MSK144', name: 'MSK144', titleKey: 'topbar.tier.msk144.title' },
  { tier: 'JT65', name: 'JT65', small: 'RX', rxOnly: true, titleKey: 'topbar.tier.jt65.title' },
  { tier: 'FST4', name: 'FST4', titleKey: 'topbar.tier.fst4.title' },
  { tier: 'FST4W', name: 'FST4W', small: 'BCN', titleKey: 'topbar.tier.fst4w.title' },
]

interface Props {
  /** Hide the TX-control cluster (the FT cockpit shows its own consolidated
   * copy beside CQ/S&P — operator request; other sections keep it here). */
  hideTxControls?: boolean
  /** Hide the top frequency/band control. The Phone + CW cockpits carry their OWN
   * mode-appropriate band picker; the top one is fed the DIGITAL (FT8) band plan, so
   * showing it there is a confusing second, wrong-dial band dropdown. */
  hideFrequencyControl?: boolean
  /** Hide FT8/digital-only chrome (tier selector, TX-cycle, waterfall layout, slot
   * countdown, time-sync, DT readout, and the FT8 TX cluster). Set on the Phone/CW
   * cockpits so they focus on phone/CW operating, not the digital-mode furniture. */
  hideDigitalChrome?: boolean
  mycall: string
  mygrid: string
  radio: RadioStatus
  /** Multi-radio switcher summaries (dual-radio). Empty/1-element ⇒ no switcher shown. */
  radios?: RadioSummary[]
  /** Peg-lock state for the switcher. */
  radioPegged?: boolean
  onSetActiveRadio?: (id: number) => void
  onSetPegLock?: (on: boolean) => void
  link: LinkState
  bandPlan: BandChannel[]
  onSetFrequency: (dialMhz: number, band: string, mode: string) => void
  onSetTxEnabled: (enabled: boolean) => void
  onSetTune: (on: boolean) => void
  onHaltTx: () => void
  onSetTxEven: (even: boolean) => void
  onSetTxCycleAuto: (auto: boolean) => void
  onSetHoldTxFreq: (on: boolean) => void
  /** Stop the in-progress QSO recording (audio bridge). The REC badge only shows while
   * `radio.qsoRecording` is true, giving a persistent, mode-independent stop. */
  onStopRecording?: () => void
  tier: Tier
  onTierChange: (t: Tier) => void
  /** Open the Getting started guide (Help ▸ Getting started). */
  onOpenGuide: () => void
  /** Field mode (outdoor/POTA) — see useFieldMode. Optional: absent hides the chip. */
  field?: boolean
  onFieldChange?: (on: boolean) => void
  /** Callsign of whoever is at the key, when that is NOT the station call (#25 multi-op).
   *  Empty/absent is the single-op case and renders nothing at all. */
  operator?: string
  /** Operators already seen in this log — the picker's roster, so swapping seats is a click
   *  and not a re-typing of a callsign. */
  operatorRoster?: string[]
  /** Switch the operator at the key. Absent ⇒ the indicator is read-only. */
  onSetOperator?: (call: string) => void
  /** The Field Day master switch (`settings.fdActive`). It does one thing here: it keeps the
   *  operator chip on screen BEFORE anyone has been set, because that is the station where
   *  seats get swapped. Off ⇒ the chip behaves exactly as it always has. */
  fdActive?: boolean
}

// The robust tier is TempoDeep — a non-coherent, fading-resilient 15 s mode that
// holds up where TempoFast (and FT8) collapse under multipath/Doppler. FT8 itself is
// a separate Phase-2 addition (its decode pipeline isn't wired yet).

function dtLabel(dtSec: number): string {
  const v = Math.round(dtSec * 10) / 10
  return `DT ${v > 0 ? '+' : ''}${v.toFixed(1)}s`
}

/** Collapse a CAT mode string to its sideband FAMILY, so the top bar only flags a real
 * disagreement. The data variants ride the same sideband (PKTUSB is USB with the rear jack
 * live — an Icom shows it as USB-D), and FMN/WFM are still FM, so none of those are a
 * mismatch worth shouting about outside an operating cockpit. */
function modeFamily(mode: string): string {
  const m = mode.trim().toUpperCase()
  // PKTFM / FM-D belongs to the FM family for exactly the reason PKTUSB belongs to USB: same
  // emission, rear jack live. Nexus commands it while an SSTV image is on the air on an FM
  // channel, and a Hamlib backend reports an FTDX10 in DATA-FM as PKTFM — so without this the
  // top bar would flag a mismatch against the FM it believes, for the whole picture.
  if (/^(PKT|DATA[- ]?)?W?FM/.test(m) || m === 'FM-D') return 'FM'
  if (/^(PKT|DATA[- ]?)?USB/.test(m) || m === 'USB-D') return 'USB'
  if (/^(PKT|DATA[- ]?)?LSB/.test(m) || m === 'LSB-D') return 'LSB'
  if (m === 'CWR') return 'CW'
  if (m === 'RTTYR') return 'RTTY'
  return m
}

/** The rig's real mode when it disagrees with what Nexus believes, else null.
 * `rigMode` is Hamlib's `m`, which some backends answer from cache — good enough for a
 * display hint, never good enough to verify a set (see reference-section-follow).
 *
 * Exported because the Operate cockpit needs the SAME verdict and could not have it: App hides
 * the whole frequency-readout group there (`hideFrequencyControl`), and this pill lives inside
 * it, so a rig moved at the radio — the everyday SmartSDR case — was invisible in the one cockpit
 * that transmits unattended (2026-08-17 Flex audit, wave-1 #54). One rule, one place. */
export function modeMismatch(
  rigMode: string | null | undefined,
  believed: string,
  rigConfirmed: boolean | undefined,
): string | null {
  const rig = (rigMode ?? '').trim()
  if (!rigConfirmed || rig === '') return null
  return modeFamily(rig) === modeFamily(believed) ? null : rig.toUpperCase()
}

/** Color class for the NTP clock offset: ok ≤0.3 s, warn ≤1 s, else bad. */
function clockClass(ms: number): string {
  const a = Math.abs(ms)
  return a <= 300 ? 'ok' : a <= 1000 ? 'warn' : 'bad'
}

/** Format the clock offset as a signed seconds value, e.g. "+0.32s". */
function clockLabel(ms: number): string {
  const s = ms / 1000
  return `${s > 0 ? '+' : ''}${s.toFixed(2)}s`
}

/** Live UTC clock (HH:MM:SS), ticking once a second. */
function UtcClock() {
  const [now, setNow] = useState(() => new Date())
  useEffect(() => {
    const id = window.setInterval(() => setNow(new Date()), 1000)
    return () => window.clearInterval(id)
  }, [])
  const p = (n: number) => String(n).padStart(2, '0')
  const hhmmss = `${p(now.getUTCHours())}:${p(now.getUTCMinutes())}:${p(now.getUTCSeconds())}`
  return (
    <div className="utc-clock" title={t('topbar.utc.title')}>
      <span className="utc-time">{hhmmss}</span>
      <span className="utc-label">{UTC}</span>
    </div>
  )
}

export function TopBar({
  mycall,
  mygrid,
  radio,
  radios,
  radioPegged,
  onSetActiveRadio,
  onSetPegLock,
  link,
  bandPlan,
  onSetFrequency,
  onSetTxEnabled,
  onSetTune,
  onHaltTx,
  onSetTxEven,
  onSetTxCycleAuto,
  onSetHoldTxFreq,
  onStopRecording,
  tier,
  onTierChange,
  onOpenGuide,
  field,
  onFieldChange,
  operator,
  operatorRoster,
  onSetOperator,
  fdActive,
  hideTxControls,
  hideFrequencyControl,
  hideDigitalChrome,
}: Props) {
  const countdown = (radio.nextSlotMs / 1000).toFixed(1)
  const [version, setVersion] = useState('')
  useEffect(() => {
    appVersion().then(setVersion).catch(() => {})
  }, [])
  // The readout above prints `radio.sideband` — what Nexus BELIEVES the rig is on. The rig can
  // legitimately be somewhere else (it powered up in FM, or the operator turned the mode knob):
  // launch is a read-only act, so Nexus never commands the rig into agreement. Surface the
  // disagreement instead of printing the belief as if it were fact. Display only.
  const rigModeMismatch = modeMismatch(radio.rigMode, radio.sideband, radio.rigConfirmed)
  // The engine refuses to arm TX on a receive-only tier; don't offer the control.
  // Stop TX stays live — disarming is always allowed, and it is the operator's way
  // out if they switched tiers mid-over.
  const noTx = isRxOnly(tier)
  // WHY a mode cannot transmit — prose about the MODE, shown on every control it disables, so
  // it moves with the bar rather than with the deferred TX controls it happens to sit on (the
  // batch-18 ruling; the Operate strip states the same fact under `operate.strip.rxOnly.why`).
  const NO_TX_WHY = t('topbar.rxOnly.why')
  const chipCluster = (
    <div className="topbar-group topbar-chips">
        {/* Help lives in the one group that renders in every section — the
            cockpits hide the TX cluster, the readout and the digital chrome,
            but never this one, so the guide is one click away everywhere. */}
        {/* WHO IS AT THE KEY (#25). Shown when an operator is set — that is the multi-op
            case, and for the single-op station that is nearly everyone this costs no width
            at all. It has to be somewhere always visible rather than in Settings, because a
            wrong operator is silent: nothing misbehaves, and it is discovered at submission
            when the log is already wrong. Same group as Help, the one group no cockpit hides.

            ...AND WHEN FIELD DAY IS ON, EVEN BEFORE ONE IS (2026-08-30). "Only when set" made
            the app-wide seat-swap button invisible at the one station that swaps seats: it
            appeared only after somebody had already set an operator, and the only place to do
            that first set is the Field Day dashboard — so the operator sitting down in Phone,
            CW or Operate had to go find the screen this chip exists to save them from.
            (Operator: "when people are swapping out, there needs to be a button to swap out
            the operator easily across any mode".) The gate widens by exactly ONE case, not to
            "always", because the width it costs is paid on every screen. */}
        {(fdActive || (operator && operator.trim() !== '')) && (
          <Menu
            trigger={
              <button
                type="button"
                className="theme-chip op-chip"
                title={
                  operator && operator.trim() !== ''
                    ? t('topbar.operator.title', { call: operator })
                    : t('topbar.operator.set.title')
                }
              >
                {operator && operator.trim() !== '' ? `${OP} ${operator}` : t('topbar.operator.set')}
              </button>
            }
            items={[
              // The roster is the operators this log has already seen, so the second and
              // every later seat swap is a click.
              ...(operatorRoster ?? [])
                .filter((o) => o.toUpperCase() !== (operator ?? '').toUpperCase())
                .map((o) => ({
                  label: t('topbar.operator.switch', { call: o }),
                  onSelect: () => onSetOperator?.(o),
                })),
              // Clearing is only offered when there is something to clear; with nobody set it
              // would be a row that does nothing.
              ...(operator && operator.trim() !== ''
                ? [{ label: t('topbar.operator.single'), onSelect: () => onSetOperator?.('') }]
                : []),
              // ...which leaves ONE case with an empty menu, and it is the first event: the
              // roster is built from operators already in the log, so a station that has never
              // logged one opens this chip onto nothing. Say where the first name is typed
              // instead. Disabled on purpose — it is a signpost, not a second control.
              ...((operatorRoster ?? []).length === 0 && !(operator && operator.trim() !== '')
                ? [{ label: t('topbar.operator.firstSet'), onSelect: () => {}, disabled: true }]
                : []),
            ]}
          />
        )}
        <Menu
          trigger={
            <button type="button" className="theme-chip" title={t('topbar.help.label')}>
              {t('topbar.help.label')}
            </button>
          }
          items={[{ label: t('gettingStarted.title'), onSelect: onOpenGuide }]}
        />
        {/* Light/Dark moved to Settings ▸ Appearance (operator, 2026-08-10) — the bar
            keeps only the outdoor quick toggle: Field must stay one tap away, because
            the operator who needs it is the one who cannot read Settings to find it. */}
        {onFieldChange && (
          <button
            type="button"
            title={field ? t('topbar.field.on.title') : t('topbar.field.off.title')}
            aria-pressed={field === true}
            className={`theme-chip field-chip${field ? ' active' : ''}`}
            onClick={() => onFieldChange(!field)}
          >
            {t('topbar.field.label')}
          </button>
        )}
      </div>
  )

  return (
    <header className={`topbar${hideFrequencyControl ? ' topbar--no-readout' : ''}`}>
      <div className="topbar-group brand">
        <span className="logo-wrap">
          <span className="logo">{NEXUS}</span>
          {version && <span className="app-version">v{version}</span>}
        </span>
        <span className="mycall">
          {mycall}
          <span className="mygrid">{mygrid}</span>
        </span>
        {radios && radios.length > 1 && (
          <RadioSwitcher
            radios={radios}
            pegged={radioPegged ?? false}
            onSwitch={(id) => onSetActiveRadio?.(id)}
            onTogglePeg={(on) => onSetPegLock?.(on)}
          />
        )}
      </div>

      {!hideFrequencyControl && (
        <div className="topbar-group radio-readout">
          <FrequencyControl
            channels={bandPlan}
            dialMhz={radio.dialMhz}
            band={radio.band}
            mode={radio.sideband}
            variant="compact"
            showModeToggle={false}
            onSet={onSetFrequency}
          />
          {rigModeMismatch && (
            <span
              className="topbar-rig-mode"
              title={t('topbar.rigMode.title', {
                rig: rigModeMismatch,
                believed: radio.sideband,
              })}
            >
              {t('topbar.rigMode.chip', { mode: rigModeMismatch })}
            </span>
          )}
        </div>
      )}

      <div className="topbar-group txrx">
        <span className={`txrx-indicator ${radio.transmitting ? 'tx' : 'rx'}`}>
          {radio.transmitting ? TX : RX}
        </span>

        {/* Live-polled meter (100 ms, lock-free backend) — the meter's own title carries the
            live dB readout, so the wrapper title stays static. */}
        <div className="rx-level" title={t('topbar.rxLevel.title')}>
          <span className="rx-level-label">{RX}</span>
          <LiveLevelMeter label={t('topbar.rxLevel.label')} variant="compact" />
        </div>

        {radio.qsoRecording && (
          <button
            type="button"
            className="topbar-rec"
            onClick={() => onStopRecording?.()}
            title={t('topbar.recording.title')}
          >
            ● {REC}
          </button>
        )}

        {!hideTxControls && !hideDigitalChrome && (
        <div className="op-controls" role="group" aria-label={t('topbar.txControls.aria')}>
          {/* ⚠️ DEFERRED, the three controls below and the watchdog chip after them: TX On/Off's
              tooltip states the abort semantics, Tune keys a carrier and Stop TX cuts an over in
              flight. They move in the transmit-path batch — see this file's header. */}
          <button
            type="button"
            className={`op-btn monitor${radio.txEnabled ? ' on' : ''}`}
            aria-pressed={radio.txEnabled}
            onClick={() => onSetTxEnabled(!radio.txEnabled)}
            disabled={noTx}
            title={
              noTx
                ? NO_TX_WHY
                : radio.txEnabled
                ? 'Transmit ENABLED — your queued message will go out. Click to disable: an FT over already in flight finishes, then TX stays off (Stop TX is the immediate halt). Receive keeps decoding either way.'
                : 'Transmit DISABLED — receive keeps decoding. Click to enable transmit (WSJT-X "Enable Tx").'
            }
          >
            {radio.txEnabled ? 'TX On' : 'TX Off'}
          </button>
          <button
            type="button"
            className={`op-btn tune${radio.tuning ? ' keyed' : ''}`}
            aria-pressed={radio.tuning}
            onClick={() => onSetTune(!radio.tuning)}
            disabled={noTx}
            title={noTx ? NO_TX_WHY : 'Key a tune carrier'}
          >
            Tune
          </button>
          <button
            type="button"
            className="op-btn stop"
            onClick={onHaltTx}
            title="Stop transmitting immediately — cuts even an over already in flight"
          >
            Stop TX
          </button>
          <button
            type="button"
            className={`op-btn hold${radio.holdTxFreq ? ' on' : ''}`}
            aria-pressed={radio.holdTxFreq}
            onClick={() => onSetHoldTxFreq(!radio.holdTxFreq)}
            title={t('topbar.holdTx.title')}
          >
            {t('topbar.holdTx.label')}
          </button>
        </div>
        )}

        {radio.txWatchdog && (
          <span className="watchdog-chip" role="alert" title="Transmit was auto-halted by the TX watchdog. Click TX On to re-enable.">
            ⚠ TX watchdog — auto-halted
          </span>
        )}

        <StatusLane />
        {!hideDigitalChrome && (
          <div className="slot-clock" title={t('topbar.slotClock.title')}>
            <span className="slot-count">{countdown}s</span>
            <span className="slot-label">{t('topbar.slotClock.label')}</span>
          </div>
        )}
        <UtcClock />
        {!hideDigitalChrome && (
          <>
            {radio.clockOffsetMs != null ? (
              <span
                className={`timesync ${clockClass(radio.clockOffsetMs)}`}
                title={t('topbar.clock.title', { offset: clockLabel(radio.clockOffsetMs) })}
              >
                <span className="dot" />
                {t('topbar.clock.label', { offset: clockLabel(radio.clockOffsetMs) })}
              </span>
            ) : (
              <span
                className={`timesync ${radio.timeSyncOk ? 'ok' : 'bad'}`}
                title={
                  radio.timeSyncOk ? t('topbar.sync.ok.title') : t('topbar.sync.bad.title')
                }
              >
                <span className="dot" />
                {radio.timeSyncOk ? t('topbar.sync.ok.label') : t('topbar.sync.bad.label')}
              </span>
            )}
            <span
              className={`dt-readout${Math.abs(link.dtSec) > 0.5 ? ' bad' : ''}`}
              title={t('topbar.dt.title')}
            >
              {dtLabel(link.dtSec)}
            </span>
          </>
        )}
      </div>

      {!hideDigitalChrome && (
      <>
      <div className="topbar-group tier-toggle" role="group" aria-label={t('topbar.tier.aria')}>
        {TIER_PILLS.map((p) => (
          <button
            key={p.tier}
            type="button"
            className={`tier-btn${tier === p.tier ? ' active' : ''}${
              p.rxOnly ? ' rx-only' : ''
            }`}
            aria-pressed={tier === p.tier}
            onClick={() => onTierChange(p.tier)}
            title={t(p.titleKey)}
          >
            {p.small ? <small>{p.small}</small> : null}
            {p.name}
          </button>
        ))}
      </div>

      {/* Option 1 (operator, 2026-08-10): the Help/OP/Field cluster sits between the mode
          pills and the Tx-cycle group, everything left-packed with the slack on the right
          — the release-format flow. */}
      {chipCluster}

      <div
        className="topbar-group tier-toggle tx-period"
        role="group"
        aria-label={t('topbar.txCycle.aria')}
      >
        {/* THREE WHOLE labels, never a stem plus a period token: the <small> is supplied by
            this call site as a marker, so the catalog carries one label per state. */}
        <button
          type="button"
          className={`tier-btn${radio.txCycleAuto ? ' active' : ''}`}
          aria-pressed={radio.txCycleAuto ?? false}
          onClick={() => onSetTxCycleAuto(true)}
          title={t('topbar.txCycle.auto.title')}
        >
          {radio.txCycleAuto ? (
            radio.txEven ? (
              <T k="topbar.txCycle.auto.first" tags={{ s: <small /> }} />
            ) : (
              <T k="topbar.txCycle.auto.second" tags={{ s: <small /> }} />
            )
          ) : (
            <T k="topbar.txCycle.auto.idle" tags={{ s: <small /> }} />
          )}
        </button>
        {/* `derived`: the cycle auto-pick landed on this side (POTA field report — the
            flip on answering a station showed only in the Auto button's small text, so a
            correct flip read as a no-op and got reported as one). Distinct from `active`
            on purpose: active is the operator's LOCK, derived is the sequencer's current
            answer, and dressing one as the other would misreport who chose it. */}
        <button
          type="button"
          className={`tier-btn${!radio.txCycleAuto && radio.txEven ? ' active' : ''}${radio.txCycleAuto && radio.txEven ? ' derived' : ''}`}
          aria-pressed={!radio.txCycleAuto && radio.txEven}
          onClick={() => onSetTxEven(true)}
          title={t('topbar.txCycle.first.title')}
        >
          <T k="topbar.txCycle.first.label" tags={{ s: <small /> }} />
        </button>
        <button
          type="button"
          className={`tier-btn${!radio.txCycleAuto && !radio.txEven ? ' active' : ''}${radio.txCycleAuto && !radio.txEven ? ' derived' : ''}`}
          aria-pressed={!radio.txCycleAuto && !radio.txEven}
          onClick={() => onSetTxEven(false)}
          title={t('topbar.txCycle.second.title')}
        >
          <T k="topbar.txCycle.second.label" tags={{ s: <small /> }} />
        </button>
      </div>
      </>
      )}

      {/* Views without the tier row (CW/Phone/RTTY/SSTV/APRS/Sats) keep the cluster at
          the bar's end — the release position. */}
      {hideDigitalChrome && chipCluster}

    </header>
  )
}
