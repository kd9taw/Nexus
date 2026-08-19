import { useEffect, useState } from 'react'
import type { BandChannel, LinkState, RadioStatus, RadioSummary, Tier } from '../types'
import { isRxOnly } from '../types'
import { Menu } from './ui/Menu'
import { FrequencyControl } from './FrequencyControl'
import { StatusLane } from './StatusLane'
import { LiveLevelMeter } from './LiveMeters'
import { RadioSwitcher } from './RadioSwitcher'
import { appVersion } from '../api'

/** The tier pills in the top bar. FT8/FT4 transmit; the six WSJT-X modes below
 *  them are DECODE-ONLY (modes::tx_mode refuses them in the engine) and carry a
 *  dashed edge so that is visible before the operator tries to call CQ rather
 *  than after. Order is deliberate: the two Tempo tiers, then the two production
 *  FT modes, then the receive-only set roughly by how often they get used.
 *
 *  Where a title points into Settings it names the SECTION, never the tab: a pill is a tier
 *  selector, so these five cannot become deep links, and a tab name in a sentence nobody can
 *  click goes stale the next time the rail is rearranged (this said "Settings ▸ Modes"). */
const TIER_PILLS: {
  tier: Tier
  small?: string
  name: string
  title: string
  rxOnly?: boolean
}[] = [
  { tier: 'TempoFast', small: 'Tempo', name: 'Fast', title: 'Fast conversational tier' },
  {
    tier: 'TempoDeep',
    small: 'Tempo',
    name: 'Deep',
    title: 'Robust non-coherent tier — fading-resilient (15 s)',
  },
  { tier: 'FT4', name: 'FT4', title: 'Standard WSJT-X FT4 (7.5 s)' },
  { tier: 'FT8', name: 'FT8', title: 'Standard WSJT-X FT8 (15 s)' },
  {
    tier: 'FT2',
    name: 'FT2',
    title:
      'FT2 — Decodium’s fast slotted mode (3.75 s), FT4 with a halved symbol time. Transmit and receive. No settings: the period is fixed',
  },
  {
    tier: 'WSPR',
    name: 'WSPR',
    small: 'BCN',
    title:
      'WSPR propagation beacons — 2 min intervals. Transmits on a schedule; set the transmit % and power in Settings ▸ Beacons (WSPR & FST4W)',
  },
  {
    tier: 'Q65',
    name: 'Q65',
    title: 'Q65 — EME / VHF+ scatter. Transmit and receive. Period + submode in Settings ▸ Q65',
  },
  {
    tier: 'MSK144',
    name: 'MSK144',
    title:
      'MSK144 — meteor scatter. Transmits for nearly the whole period (that is how the mode works). Period in Settings ▸ MSK144',
  },
  {
    tier: 'JT65',
    name: 'JT65',
    small: 'RX',
    rxOnly: true,
    title:
      'JT65 — classic EME, 60 s. Receive only in this build (transmit is disabled pending a fix). Submode in Settings ▸ JT65',
  },
  {
    tier: 'FST4',
    name: 'FST4',
    title: 'FST4 — slow weak-signal QSO mode (LF/MF). Transmit and receive',
  },
  {
    tier: 'FST4W',
    name: 'FST4W',
    small: 'BCN',
    title:
      'FST4W — LF/MF beacons. Transmits on a schedule; set the transmit % and power in Settings ▸ Beacons (WSPR & FST4W). Hashed calls show as <...>',
  },
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
    <div className="utc-clock" title="UTC time">
      <span className="utc-time">{hhmmss}</span>
      <span className="utc-label">UTC</span>
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
  const NO_TX_WHY = 'This mode is receive-only in Nexus — it decodes but does not transmit'
  const chipCluster = (
    <div className="topbar-group topbar-chips">
        {/* Help lives in the one group that renders in every section — the
            cockpits hide the TX cluster, the readout and the digital chrome,
            but never this one, so the guide is one click away everywhere. */}
        {/* WHO IS AT THE KEY (#25). Shown ONLY when an operator is set — that is the
            multi-op case, and for the single-op station that is nearly everyone this costs
            no width at all. It has to be somewhere always visible rather than in Settings,
            because a wrong operator is silent: nothing misbehaves, and it is discovered at
            submission when the log is already wrong. Same group as Help, the one group no
            cockpit hides. */}
        {operator && operator.trim() !== '' && (
          <Menu
            trigger={
              <button
                type="button"
                className="theme-chip op-chip"
                title={`Operating as ${operator} — click to change who is at the key`}
              >
                OP {operator}
              </button>
            }
            items={[
              // The roster is the operators this log has already seen, so the second and
              // every later seat swap is a click. The first one is still typed, in Settings.
              ...(operatorRoster ?? [])
                .filter((o) => o.toUpperCase() !== operator.toUpperCase())
                .map((o) => ({
                  label: `Switch to ${o}`,
                  onSelect: () => onSetOperator?.(o),
                })),
              {
                label: 'Single operator (clear)',
                onSelect: () => onSetOperator?.(''),
              },
            ]}
          />
        )}
        <Menu
          trigger={
            <button type="button" className="theme-chip" title="Help">
              Help
            </button>
          }
          items={[{ label: 'Getting started', onSelect: onOpenGuide }]}
        />
        {/* Light/Dark moved to Settings ▸ Appearance (operator, 2026-08-10) — the bar
            keeps only the outdoor quick toggle: Field must stay one tap away, because
            the operator who needs it is the one who cannot read Settings to find it. */}
        {onFieldChange && (
          <button
            type="button"
            title={
              field
                ? 'Field mode is on: larger type, maximum contrast. Click to turn off.'
                : 'Field mode for operating outdoors: larger type, maximum contrast'
            }
            aria-pressed={field === true}
            className={`theme-chip field-chip${field ? ' active' : ''}`}
            onClick={() => onFieldChange(!field)}
          >
            Field
          </button>
        )}
      </div>
  )

  return (
    <header className={`topbar${hideFrequencyControl ? ' topbar--no-readout' : ''}`}>
      <div className="topbar-group brand">
        <span className="logo-wrap">
          <span className="logo">Nexus</span>
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
              title={`Your rig is on ${rigModeMismatch}, but Nexus has ${radio.sideband}. Turn the rig's mode knob (or pick the band in an operating cockpit) to match.`}
            >
              rig: {rigModeMismatch}
            </span>
          )}
        </div>
      )}

      <div className="topbar-group txrx">
        <span className={`txrx-indicator ${radio.transmitting ? 'tx' : 'rx'}`}>
          {radio.transmitting ? 'TX' : 'RX'}
        </span>

        {/* Live-polled meter (100 ms, lock-free backend) — the meter's own title carries the
            live dB readout, so the wrapper title stays static. */}
        <div className="rx-level" title="RX audio level (aim ~30 dB, like WSJT-X)">
          <span className="rx-level-label">RX</span>
          <LiveLevelMeter label="RX audio level" variant="compact" />
        </div>

        {radio.qsoRecording && (
          <button
            type="button"
            className="topbar-rec"
            onClick={() => onStopRecording?.()}
            title="Recording this QSO to a WAV — click to stop"
          >
            ● REC
          </button>
        )}

        {!hideTxControls && !hideDigitalChrome && (
        <div className="op-controls" role="group" aria-label="Transmit controls">
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
            title="Hold Tx Freq: keep your TX offset where you put it when you double-click a station to work them. Off, your TX moves onto theirs (WSJT-X's behaviour). A plain waterfall click never moves TX either way."
          >
            Hold Tx
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
          <div className="slot-clock" title="Time to next slot">
            <span className="slot-count">{countdown}s</span>
            <span className="slot-label">next slot</span>
          </div>
        )}
        <UtcClock />
        {!hideDigitalChrome && (
          <>
            {radio.clockOffsetMs != null ? (
              <span
                className={`timesync ${clockClass(radio.clockOffsetMs)}`}
                title={`PC clock is ${clockLabel(radio.clockOffsetMs)} vs UTC (NTP). TempoFast/TempoDeep need it within ~0.5 s — sync via NTP / time.is (off-grid: GPS).`}
              >
                <span className="dot" />
                clock {clockLabel(radio.clockOffsetMs)}
              </span>
            ) : (
              <span
                className={`timesync ${radio.timeSyncOk ? 'ok' : 'bad'}`}
                title={
                  radio.timeSyncOk
                    ? 'Time sync OK (from decode timing)'
                    : 'Decodes land far off the slot boundary — sync your PC clock (NTP / time.is; off-grid: GPS).'
                }
              >
                <span className="dot" />
                {radio.timeSyncOk ? 'Sync' : 'No Sync'}
              </span>
            )}
            <span
              className={`dt-readout${Math.abs(link.dtSec) > 0.5 ? ' bad' : ''}`}
              title="Decode time offset (how far heard signals land from the slot boundary)"
            >
              {dtLabel(link.dtSec)}
            </span>
          </>
        )}
      </div>

      {!hideDigitalChrome && (
      <>
      <div className="topbar-group tier-toggle" role="group" aria-label="Link tier">
        {TIER_PILLS.map((p) => (
          <button
            key={p.tier}
            type="button"
            className={`tier-btn${tier === p.tier ? ' active' : ''}${
              p.rxOnly ? ' rx-only' : ''
            }`}
            aria-pressed={tier === p.tier}
            onClick={() => onTierChange(p.tier)}
            title={p.title}
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

      <div className="topbar-group tier-toggle tx-period" role="group" aria-label="Transmit cycle">
        <button
          type="button"
          className={`tier-btn${radio.txCycleAuto ? ' active' : ''}`}
          aria-pressed={radio.txCycleAuto ?? false}
          onClick={() => onSetTxCycleAuto(true)}
          title="Auto-cycle (FT8-style): when you answer a station, transmit on the opposite T/R cycle to theirs"
        >
          Auto{' '}
          <small>{radio.txCycleAuto ? (radio.txEven ? '1st' : '2nd') : 'cycle'}</small>
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
          title="Lock transmit to the even (1st) T/R slots — the station you work must be Tx 2nd"
        >
          Tx 1st <small>even</small>
        </button>
        <button
          type="button"
          className={`tier-btn${!radio.txCycleAuto && !radio.txEven ? ' active' : ''}${radio.txCycleAuto && !radio.txEven ? ' derived' : ''}`}
          aria-pressed={!radio.txCycleAuto && !radio.txEven}
          onClick={() => onSetTxEven(false)}
          title="Lock transmit to the odd (2nd) T/R slots — the station you work must be Tx 1st"
        >
          Tx 2nd <small>odd</small>
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
