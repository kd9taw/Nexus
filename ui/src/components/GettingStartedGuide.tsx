import { useRef, useState } from 'react'
import { Dialog } from './ui/Dialog'

/**
 * Getting started — the four-step on-ramp, in-app.
 *
 * The four things that must happen, in order, before Nexus is useful:
 * callsign & grid, the radio, license class, and the ADIF log. Step 4 carries
 * the weighted treatment because it is the payoff: without a log the app starts
 * blind and worked-before, the Needed board and the awards engine all have
 * nothing to work with.
 *
 * Opened from Help ▸ Getting started, and offered as a walkthrough at the end
 * of the first-run wizard. It is DOCUMENTATION — nothing here writes a setting
 * or touches the radio, and the wizard panels in each step's right column are
 * static recreations of the real dialog, not live controls.
 *
 * Those panels quote `SetupWizard.tsx` verbatim and reuse its own `.wizard-*`
 * classes, so a restyle of the wizard carries into this guide by construction.
 * If the wizard's STRINGS change, change them here too — these are pictures of
 * real UI, not marketing copy. Note the numbering difference: this guide puts
 * license class at step 3 and the log at step 4 (the log is the payoff); the
 * app's own wizard has the log at step 3 and license class inside step 4
 * (Goals). The caption under each panel says which real step it is.
 *
 * The same four steps are published at hamradiotools.io/getting-started, one
 * source of copy for both.
 */

interface Props {
  onClose: () => void
}

const STEPS = [
  { num: '01', label: 'Callsign & grid', blurb: 'Who and where you are' },
  { num: '02', label: 'Your radio', blurb: 'Detect, pair audio, Test CAT' },
  { num: '03', label: 'License class', blurb: 'A real transmit lockout' },
  { num: '04', label: 'Your ADIF log', blurb: 'Where the magic happens' },
]

/** The real wizard's step titles, for the dot row inside the recreated panels. */
const WIZ = ['Your station', 'Your rig', 'Your log', 'Your goals']

const NEXT_LABEL = [
  'Next — set up the radio →',
  'Next — your license →',
  'Next — the log (the important one) →',
  "That's all four",
]

/** The dot row of a recreated wizard panel. `cur` is the REAL wizard step
 * (1-based) the panel is showing. */
function WizardDots({ cur }: { cur: number }) {
  return (
    <div className="wizard-dots">
      {WIZ.map((t, i) => (
        <span
          key={t}
          className={`wizard-dot${i === cur - 1 ? ' cur' : ''}${i < cur - 1 ? ' done' : ''}`}
        >
          <span className="wizard-dot-n">{i + 1}</span> {t}
        </span>
      ))}
    </div>
  )
}

/** A recreated wizard panel plus the caption naming where it lives in the app. */
function WizardShot({ caption, children }: { caption: string; children: React.ReactNode }) {
  return (
    <div>
      <p className="gsg-shot-label">In Nexus</p>
      <div className="gsg-shot">{children}</div>
      <p className="gsg-shot-cap">{caption}</p>
    </div>
  )
}

export function GettingStartedGuide({ onClose }: Props) {
  const [step, setStep] = useState(0)
  const rootRef = useRef<HTMLDivElement>(null)

  const go = (i: number) => {
    const next = Math.max(0, Math.min(STEPS.length - 1, i))
    setStep(next)
    // The dialog box is the scroll owner (`.ui-dialog { overflow-y: auto }`),
    // so a step change scrolls THAT, not the window — the window behind a modal
    // does not move. Smooth is fine here; the reduced-motion rules in styles.css
    // cover transitions, not programmatic scrolling, so ask for it explicitly.
    const box = rootRef.current?.closest('.ui-dialog')
    const reduce = window.matchMedia('(prefers-reduced-motion: reduce)').matches
    box?.scrollTo({ top: 0, behavior: reduce ? 'auto' : 'smooth' })
  }

  const progress = `Step ${step + 1} of ${STEPS.length}`

  return (
    <Dialog
      open
      onOpenChange={(o) => {
        if (!o) onClose()
      }}
      title="Getting started"
      hideTitle
      className="gsg-dialog"
    >
      <div className="gsg" ref={rootRef}>
        {/* Breadcrumb strip — this guide's whole chrome inside the app. */}
        <div className="gsg-crumb">
          <span className="gsg-crumb-mark" aria-hidden="true" />
          <span className="gsg-crumb-app">NEXUS</span>
          <span className="gsg-crumb-dot" aria-hidden="true">
            ●
          </span>
          <span>Help ▸ Getting started</span>
          <span className="gsg-crumb-note">
            Opened from the Help menu · also offered at the end of the setup wizard
          </span>
        </div>

        <div className="gsg-body">
          <nav className="gsg-rail" aria-label="Guide steps">
            <p className="gsg-rail-head">The order matters</p>
            {STEPS.map((s, i) => (
              <button
                key={s.num}
                type="button"
                className={`gsg-rail-btn${i === step ? ' cur' : ''}${i < step ? ' done' : ''}${
                  i === 3 ? ' hero' : ''
                }`}
                aria-current={i === step ? 'step' : undefined}
                onClick={() => go(i)}
              >
                <span className="gsg-rail-num">{s.num}</span>
                <span className="gsg-rail-text">
                  <span className="gsg-rail-label">{s.label}</span>
                  <span className="gsg-rail-blurb">{s.blurb}</span>
                </span>
              </button>
            ))}
            <div className="gsg-progress">
              <div
                className="gsg-progress-fill"
                style={{ width: `${((step + 1) / STEPS.length) * 100}%` }}
              />
            </div>
            <p className="gsg-progress-label">{progress}</p>
          </nav>

          <div className="gsg-content">
            {step === 0 && (
              <section aria-label="Step 1 of 4: Callsign and grid">
                <p className="gsg-eyebrow">Step 1 of 4 · Your station</p>
                <h2 className="gsg-h2">Who&rsquo;s on the air?</h2>
                <p className="gsg-lede">
                  Your callsign and your grid square. Everything location-shaped in Nexus — the
                  propagation map, satellite passes, distances, bearings, DXpedition windows, VHF
                  opening locality — is computed from these two fields.
                </p>

                <div className="gsg-split">
                  <div className="gsg-prose">
                    <p>
                      Open the first-run wizard (it appears on first launch) or{' '}
                      <strong>Settings ▸ Station</strong> at any time.
                    </p>
                    <ul>
                      <li>
                        <strong>Callsign</strong> — stored uppercase. Nexus will not start PSK
                        Reporter or the RBN/cluster feed until a valid one is set: 3–10 characters,
                        at least one letter and one digit.
                      </li>
                      <li>
                        <strong>Grid square</strong> — your Maidenhead locator. QRZ shows yours.
                      </li>
                      <li>
                        <strong>Name</strong> — optional, in Settings ▸ Station. Feeds the CW{' '}
                        <code>{'{NAME}'}</code> macro and logbook autofill.
                      </li>
                    </ul>
                    <div className="gsg-callout accent">
                      <p className="gsg-callout-label">Give all six characters</p>
                      <p className="gsg-callout-body">
                        Four characters pins you to the middle of a ~100-mile square, and every
                        distance and bearing you will ever read is measured from that point.{' '}
                        <code>EN52xa</code> beats <code>EN52</code>. A malformed locator is refused
                        outright — the wizard will not let you past it.
                      </p>
                    </div>
                  </div>

                  <WizardShot caption="Setup wizard ▸ step 1 · re-run any time from Settings ▸ Appearance ▸ Features">
                    <WizardDots cur={1} />
                    <p className="wizard-title">Who&rsquo;s on the air?</p>
                    <p className="wizard-sub">
                      Your grid square is the anchor for everything location-based — satellite
                      passes, propagation, the map, and DXpedition windows are all computed from it.
                    </p>
                    <div className="wizard-fields">
                      <span className="wizard-field">
                        <span>Callsign</span>
                        <span className="gsg-shot-input">KD9TAW</span>
                      </span>
                      <span className="wizard-field">
                        <span>Grid square</span>
                        <span className="gsg-shot-input">EN52xa</span>
                        <span className="wizard-field-hint">
                          Maidenhead locator (qrz.com shows yours). Give all 6 — 4 characters pins
                          you to the middle of a ~100-mile square.
                        </span>
                      </span>
                    </div>
                    <div className="gsg-shot-actions">
                      <span className="wizard-skip">I&rsquo;ll set it up myself</span>
                      <span className="wizard-go">Next →</span>
                    </div>
                  </WizardShot>
                </div>
              </section>
            )}

            {step === 1 && (
              <section aria-label="Step 2 of 4: Your radio">
                <p className="gsg-eyebrow">Step 2 of 4 · Your rig</p>
                <h2 className="gsg-h2">How does the radio connect?</h2>
                <p className="gsg-lede">
                  One button does the archaeology. Nexus reads the USB descriptors, matches the rig
                  model, pairs the audio CODEC, and looks for FlexRadios on your network — in a
                  single scan.
                </p>

                <div className="gsg-split">
                  <div className="gsg-prose">
                    <p>
                      In the wizard, or in <strong>Settings ▸ Radio ▸ Rig Control</strong>, click{' '}
                      <strong>Detect my radio</strong>.
                    </p>
                    <ul>
                      <li>
                        <strong>Pick the row that is your radio.</strong> It fills the port, the
                        Hamlib model and both audio devices at once. On a dual-UART Icom two rows
                        describe the same rig — take the one tagged <em>CI-V port — use this one</em>
                        .
                      </li>
                      <li>
                        <strong>Generic cable?</strong> A CH340 reporting only &ldquo;USB
                        Serial&rdquo; fills the port and audio but leaves the model blank — choose it
                        from the dropdown.
                      </li>
                      <li>
                        <strong>FlexRadio</strong> is found on the network and configured the
                        WSJT-X-proven way: CAT through the SmartSDR CAT app at{' '}
                        <code>127.0.0.1:5002</code>, audio through DAX.
                      </li>
                      <li>
                        <strong>Then press Test CAT.</strong> Nexus saves what you entered, starts{' '}
                        <code>rigctld</code> on port 4532, and reports the dial frequency it read
                        back — or the specific error.
                      </li>
                    </ul>
                    <div className="gsg-callout neutral">
                      <p className="gsg-callout-label">Nothing found?</p>
                      <p className="gsg-callout-body">
                        USB: plug it in and power it on. Flex: it has to be on this network. Either
                        way you can skip and set it up later — the wizard never blocks on hardware.
                        Set <strong>Tx&nbsp;Level</strong> (default 0.9) down until your rig&rsquo;s
                        ALC reads zero before you transmit.
                      </p>
                    </div>
                  </div>

                  <WizardShot caption="Setup wizard ▸ step 2 · Hamlib ships inside the installer">
                    <WizardDots cur={2} />
                    <p className="wizard-title">How does the radio connect?</p>
                    <p className="wizard-sub">
                      One detect finds everything — USB rigs and FlexRadios on the network.
                      Skippable; Settings ▸ Rig Control has all of this later.
                    </p>
                    <div className="wizard-detect">
                      <span className="wizard-btn">🔍 Detect my radio</span>
                      <span className="wizard-detect-row sel">
                        <b>Icom IC-7300</b> on COM7
                        <span className="wizard-field-hint"> · CP210x · CI-V port — use this one</span>
                      </span>
                      <span className="wizard-detect-row">
                        <b>Icom IC-7300</b> on COM8
                        <span className="wizard-field-hint"> · CP210x · second port, not CI-V</span>
                      </span>
                      <span className="wizard-field-hint">Selected: Icom IC-7300 on COM7</span>
                    </div>
                    <div className="wizard-rigconn">
                      <span className="wizard-mode sel">
                        <span className="wizard-mode-label">USB / Serial</span>
                        <span className="wizard-mode-blurb">Most rigs — one cable</span>
                      </span>
                      <span className="wizard-mode">
                        <span className="wizard-mode-label">Network</span>
                        <span className="wizard-mode-blurb">FlexRadio / remote rigctld</span>
                      </span>
                    </div>
                    <div className="wizard-fields">
                      <span className="wizard-field">
                        <span>Audio in</span>
                        <span className="gsg-shot-input">USB Audio CODEC</span>
                      </span>
                      <span className="wizard-field">
                        <span>Audio out</span>
                        <span className="gsg-shot-input">USB Audio CODEC</span>
                      </span>
                    </div>
                    <div className="wizard-detect">
                      <span className="wizard-btn">⚡ Test CAT</span>
                      <span className="wizard-field-hint">
                        ✓ Radio answered on COM7 — dial reads 14.074.000 MHz
                      </span>
                    </div>
                  </WizardShot>
                </div>
              </section>
            )}

            {step === 2 && (
              <section aria-label="Step 3 of 4: License class">
                <p className="gsg-eyebrow">Step 3 of 4 · Your license</p>
                <h2 className="gsg-h2">Declare your class, get a real lockout</h2>
                <p className="gsg-lede">
                  This one field becomes a software guard in <em>every</em> transmit path in the app.
                  Nexus parks the dial in your licensed segments and refuses to key up outside them,
                  with a toast that says why.
                </p>

                <div className="gsg-split">
                  <div className="gsg-prose">
                    <p>
                      On the wizard&rsquo;s last step, under <strong>What&rsquo;s your license?</strong>
                      . It is persisted the moment you click it.
                    </p>
                    <ul>
                      <li>
                        A <strong>Technician</strong> on 40&nbsp;m is held to the CW segment — phone
                        and FT8 TX outside it are blocked.
                      </li>
                      <li>
                        A <strong>General</strong> on 20&nbsp;m cannot transmit in the Extra-only
                        portion at the low end.
                      </li>
                      <li>
                        The channelized <strong>60&nbsp;m</strong> segments are included.
                      </li>
                      <li>
                        Outside the US? Pick <strong>Outside the US</strong>. The lockout models US
                        FCC Part 97 / ITU Region 2 only, so it is off rather than wrong.
                      </li>
                    </ul>
                    <div className="gsg-callout accent">
                      <p className="gsg-callout-label">It is a safety net</p>
                      <p className="gsg-callout-body">
                        You are responsible for knowing your privileges. The lockout catches the
                        slip; it does not replace the knowledge. A fresh install defaults to{' '}
                        <strong>Open</strong>, so nothing is silently restricted before you say so.
                      </p>
                    </div>
                  </div>

                  <WizardShot caption="Setup wizard ▸ step 4 · alongside goals and modes">
                    <p className="gsg-shot-h3">What&rsquo;s your license?</p>
                    <p className="wizard-license-sub">
                      Sets your transmit privileges — the app parks the dial in your licensed band
                      segments and won&rsquo;t let you transmit outside them. Pick &ldquo;Outside the
                      US&rdquo; for no limits.
                    </p>
                    <div className="wizard-modes">
                      <span className="wizard-mode">
                        <span className="wizard-mode-label">Technician</span>
                        <span className="wizard-mode-blurb">US — limited HF + full VHF/UHF</span>
                      </span>
                      <span className="wizard-mode sel">
                        <span className="wizard-mode-label">General</span>
                        <span className="wizard-mode-blurb">US — most HF privileges</span>
                      </span>
                      <span className="wizard-mode">
                        <span className="wizard-mode-label">Amateur Extra</span>
                        <span className="wizard-mode-blurb">US — full privileges</span>
                      </span>
                      <span className="wizard-mode">
                        <span className="wizard-mode-label">Outside the US</span>
                        <span className="wizard-mode-blurb">No transmit limits</span>
                      </span>
                    </div>
                    <div className="gsg-shot-toast">
                      <span className="gsg-shot-toast-dot" aria-hidden="true" />
                      <span className="gsg-shot-toast-msg">
                        TX blocked — 7.150 MHz is outside your General phone privileges on 40 m.
                      </span>
                    </div>
                  </WizardShot>
                </div>
              </section>
            )}

            {step === 3 && (
              <section aria-label="Step 4 of 4: Your ADIF log">
                <div className="gsg-hero-card">
                  <div className="gsg-hero-card-glow" aria-hidden="true" />
                  <div className="gsg-hero-card-inner">
                    <p className="gsg-eyebrow">Step 4 of 4 · The one that matters</p>
                    <h2 className="gsg-h2 big">Bring in your existing log</h2>
                    <p className="gsg-lede wide">
                      One log, every mode. Import one ADIF file and the app knows every station you
                      have already worked, every entity you still need, and exactly how far you are
                      from your next award — on digital, phone and CW alike, computed offline from
                      your own history.
                    </p>

                    <div className="gsg-compare">
                      <div className="gsg-compare-cell">
                        <p className="gsg-compare-label faint">Without a log</p>
                        <p className="gsg-compare-body">
                          Every callsign looks the same, whether it arrives as a decode, a cluster
                          spot or a voice on the band. Every station is new. The Needed board has
                          nothing to say.
                        </p>
                      </div>
                      <div className="gsg-compare-cell">
                        <p className="gsg-compare-label ok">With it — worked before</p>
                        <p className="gsg-compare-body">
                          B4 chips wherever a callsign appears — digital decodes, the Call Roster,
                          spots, and the recall card in the Phone and CW cockpits.
                        </p>
                      </div>
                      <div className="gsg-compare-cell">
                        <p className="gsg-compare-label ok">With it — the Needed board</p>
                        <p className="gsg-compare-body">
                          New DXCC, new state, new grid, new band slot — ranked across every band and
                          mode at once, with the evidence that says it is workable now.
                        </p>
                      </div>
                      <div className="gsg-compare-cell">
                        <p className="gsg-compare-label ok">With it — awards</p>
                        <p className="gsg-compare-body">
                          DXCC, Challenge, Honor Roll, WAS, WAZ, VUCC and IOTA — mode-agnostic and
                          per-mode alike — the moment the import finishes.
                        </p>
                      </div>
                    </div>
                  </div>
                </div>

                <div className="gsg-split wide">
                  <div className="gsg-prose">
                    <p className="gsg-prose-h3">Where your ADIF comes from</p>
                    <p>
                      Any standard ADIF export — <code>.adi</code> or <code>.adif</code>. If you have
                      been operating, you already have one.
                    </p>
                    <ul className="tight">
                      <li>
                        <strong>WSJT-X</strong> — <code>wsjtx_log.adi</code> in your log directory.
                      </li>
                      <li>
                        <strong>QRZ Logbook</strong> or <strong>LoTW</strong> — download your full
                        ADIF export.
                      </li>
                      <li>
                        <strong>N1MM+, Log4OM, HRD, ClubLog</strong> — export ADIF from the logbook.
                      </li>
                    </ul>
                    <p className="gsg-prose-h3">Then import it</p>
                    <p>
                      Wizard step 3, or <strong>Logbook ▸ Import ADIF</strong> at any time. Import as
                      many files as you like — duplicates are detected and skipped, so a second pass
                      costs you nothing.
                    </p>
                    <div className="gsg-callout ok">
                      <p className="gsg-callout-label">Nothing leaves your computer</p>
                      <p className="gsg-callout-body">
                        The import is local. Your log lives in <code>log.adi</code> on your own
                        machine, and the awards engine computes against it offline. Uploading to
                        LoTW, QRZ, ClubLog or eQSL is a separate, opt-in connector.
                      </p>
                    </div>
                  </div>

                  <WizardShot caption="Setup wizard ▸ step 3 · or Logbook ▸ Import ADIF, any time">
                    <WizardDots cur={3} />
                    <p className="wizard-title">Bring in your existing log</p>
                    <p className="wizard-sub">
                      Nexus works best when it knows your history. Importing your ADIF log is what
                      powers <strong>worked-before</strong> flags, the <strong>Needed</strong> board,
                      and your <strong>awards</strong> progress — without it, the app starts blind
                      and treats every station as new.
                    </p>
                    <div className="gsg-shot-log">
                      <span className="wizard-go">Import my ADIF log…</span>
                      <p className="wizard-log-result">
                        ✓ Imported <strong>4,812</strong> QSOs · 36 already present. Your
                        worked-before and Needed board are now seeded.
                      </p>
                      <p className="wizard-license-sub">
                        From WSJT-X, N1MM, Log4OM, HRD, QRZ, LoTW, ClubLog — any standard ADIF
                        (.adi/.adif) export. Nothing leaves your computer; duplicates are detected
                        and skipped.
                      </p>
                    </div>
                  </WizardShot>
                </div>

                <div className="gsg-closing">
                  <p className="gsg-eyebrow ok">You&rsquo;re set — now go work someone</p>
                  <div className="gsg-closing-grid">
                    <div className="gsg-closing-card">
                      <p className="gsg-closing-num">01</p>
                      <p className="gsg-closing-body">
                        Open <strong>Digital</strong>. The dial starts on 14.074 MHz and the decoder
                        runs every 15-second slot — there is no Monitor toggle to forget.
                      </p>
                    </div>
                    <div className="gsg-closing-card">
                      <p className="gsg-closing-num">02</p>
                      <p className="gsg-closing-body">
                        Within a period or two, decodes fill Band Activity — now annotated with
                        country, B4 and <strong>New / DXCC</strong> badges, because you imported the
                        log.
                      </p>
                    </div>
                    <div className="gsg-closing-card">
                      <p className="gsg-closing-num">03</p>
                      <p className="gsg-closing-body">
                        <strong>Double-click</strong> a station calling CQ. The sequencer runs the
                        whole exchange and logs it. The same log feeds the Phone and CW cockpits, so
                        a familiar call shows their name and your history there too.
                      </p>
                    </div>
                  </div>
                </div>
              </section>
            )}

            <div className="gsg-nav">
              <button
                type="button"
                className="gsg-btn back"
                disabled={step === 0}
                onClick={() => go(step - 1)}
              >
                ← Back
              </button>
              <span className="gsg-nav-label">{progress}</span>
              {step === STEPS.length - 1 ? (
                // The last step's forward action is the way out: there is nothing
                // after "that's all four" but the app itself.
                <button type="button" className="gsg-btn next" onClick={onClose}>
                  That&rsquo;s all four — close
                </button>
              ) : (
                <button type="button" className="gsg-btn next" onClick={() => go(step + 1)}>
                  {NEXT_LABEL[step]}
                </button>
              )}
            </div>

            <aside className="gsg-wsjtx">
              <p className="gsg-wsjtx-label">Coming from WSJT-X? The short path</p>
              <p className="gsg-wsjtx-body">
                Your muscle memory transfers — double-click semantics, <code>Esc</code> /{' '}
                <code>F4</code> / <code>F6</code> / <code>Alt+1–6</code>, Band Activity
                bottom-pinned, early decodes at 11.8&nbsp;s, Fake-It split, Hound auto-move. So do
                your settings: point step 2 at the same rig and audio devices WSJT-X uses, and hand
                step 4 your <code>wsjtx_log.adi</code>. JTAlert and GridTracker keep working — Nexus
                speaks the full WSJT-X UDP protocol and they see it as a WSJT-X.
              </p>
            </aside>
          </div>
        </div>
      </div>
    </Dialog>
  )
}
