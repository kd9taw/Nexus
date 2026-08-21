import { useRef, useState } from 'react'
import { t } from '../i18n'
import type { MessageKey } from '../i18n'
import { T } from '../i18n/T'
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
 *
 * ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every
 * sentence lives in `i18n/en.ts` under `gettingStarted.*`; the emphasised ones
 * are `<T>` with markers, because a guide sentence with the control name bolded
 * in the middle of it cannot be split into three keys and still be translated.
 * What stays HERE is GUIDE_EXAMPLES — the invariant values the recreated wizard
 * panels display.
 */

interface Props {
  onClose: () => void
}

/** The wordmark in the breadcrumb strip. A brand, not prose — invariant in every language. */
const APP_WORDMARK = 'NEXUS'

/**
 * The invariant technical values this guide shows: a callsign, two grid squares, hardware
 * identifiers reported by the OS, and three dial frequencies. They are the same characters in
 * every language — `14.074 MHz` is not `14,074 MHz`, and a decimal comma reaching a dial reading
 * is an operating fault. Gathered here (rather than inlined) so the guard can prove they never
 * became catalog entries, exactly as STATION_EXAMPLES does for Settings ▸ Station.
 */
const GUIDE_EXAMPLES = {
  callsign: 'KD9TAW',
  grid: 'EN52xa',
  gridShort: 'EN52',
  rig: 'Icom IC-7300',
  portCiv: 'COM7',
  portSecond: 'COM8',
  chip: 'CP210x',
  audioDevice: 'USB Audio CODEC',
  /** What Test CAT reports back — a dial reading, formatted by the radio layer. */
  dialRead: '14.074.000 MHz',
  /** The FT8 calling frequency the Digital cockpit opens on. */
  ft8Dial: '14.074 MHz',
  /** A 40 m phone frequency a General may not use — the lockout's worked example. */
  blockedDial: '7.150 MHz',
  /** An example import result. Already grouped; never re-formatted by a locale. */
  importedQsos: '4,812',
  importedDupes: '36',
} as const

interface Step {
  num: string
  labelKey: MessageKey
  blurbKey: MessageKey
  /** The forward button's wording on this step. Step 4's is unused — it closes instead. */
  nextKey: MessageKey
}

const STEPS: Step[] = [
  {
    num: '01',
    labelKey: 'gettingStarted.rail.station.label',
    blurbKey: 'gettingStarted.rail.station.blurb',
    nextKey: 'gettingStarted.next.station',
  },
  {
    num: '02',
    labelKey: 'gettingStarted.rail.radio.label',
    blurbKey: 'gettingStarted.rail.radio.blurb',
    nextKey: 'gettingStarted.next.radio',
  },
  {
    num: '03',
    labelKey: 'gettingStarted.rail.license.label',
    blurbKey: 'gettingStarted.rail.license.blurb',
    nextKey: 'gettingStarted.next.license',
  },
  {
    num: '04',
    labelKey: 'gettingStarted.rail.log.label',
    blurbKey: 'gettingStarted.rail.log.blurb',
    nextKey: 'gettingStarted.next.log',
  },
]

/** The real wizard's step titles, for the dot row inside the recreated panels. */
const WIZ: { titleKey: MessageKey }[] = [
  { titleKey: 'gettingStarted.wizard.station' },
  { titleKey: 'gettingStarted.wizard.rig' },
  { titleKey: 'gettingStarted.wizard.log' },
  { titleKey: 'gettingStarted.wizard.finish' },
]

/** The dot row of a recreated wizard panel. `cur` is the REAL wizard step
 * (1-based) the panel is showing. */
function WizardDots({ cur }: { cur: number }) {
  return (
    <div className="wizard-dots">
      {WIZ.map((w, i) => (
        <span
          key={w.titleKey}
          className={`wizard-dot${i === cur - 1 ? ' cur' : ''}${i < cur - 1 ? ' done' : ''}`}
        >
          <span className="wizard-dot-n">{i + 1}</span> {t(w.titleKey)}
        </span>
      ))}
    </div>
  )
}

/** A recreated wizard panel plus the caption naming where it lives in the app. */
function WizardShot({ caption, children }: { caption: string; children: React.ReactNode }) {
  return (
    <div>
      <p className="gsg-shot-label">{t('gettingStarted.shot.label')}</p>
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

  const progress = t('gettingStarted.progress', { step: step + 1, total: STEPS.length })
  const isMac = navigator.userAgent.includes('Mac')

  return (
    <Dialog
      open
      onOpenChange={(o) => {
        if (!o) onClose()
      }}
      title={t('gettingStarted.title')}
      hideTitle
      className="gsg-dialog"
    >
      <div className="gsg" ref={rootRef}>
        {/* Breadcrumb strip — this guide's whole chrome inside the app. */}
        <div className="gsg-crumb">
          <span className="gsg-crumb-mark" aria-hidden="true" />
          {/* The product name, not prose — invariant in every language. */}
          <span className="gsg-crumb-app">{APP_WORDMARK}</span>
          <span className="gsg-crumb-dot" aria-hidden="true">
            ●
          </span>
          <span>{t('gettingStarted.crumb.path')}</span>
          <span className="gsg-crumb-note">{t('gettingStarted.crumb.note')}</span>
        </div>

        <div className="gsg-body">
          <nav className="gsg-rail" aria-label={t('gettingStarted.rail.label')}>
            <p className="gsg-rail-head">{t('gettingStarted.rail.head')}</p>
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
                  <span className="gsg-rail-label">{t(s.labelKey)}</span>
                  <span className="gsg-rail-blurb">{t(s.blurbKey)}</span>
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
              <section aria-label={t('gettingStarted.station.aria')}>
                <p className="gsg-eyebrow">{t('gettingStarted.station.eyebrow')}</p>
                <h2 className="gsg-h2">{t('gettingStarted.station.heading')}</h2>
                <p className="gsg-lede">{t('gettingStarted.station.lede')}</p>

                <div className="gsg-split">
                  <div className="gsg-prose">
                    <p>
                      <T k="gettingStarted.station.where" tags={{ b: <strong /> }} />
                    </p>
                    <ul>
                      <li>
                        <T k="gettingStarted.station.callsign" tags={{ b: <strong /> }} />
                      </li>
                      <li>
                        <T k="gettingStarted.station.grid" tags={{ b: <strong /> }} />
                      </li>
                      <li>
                        {/* `{NAME}` is a CW macro token inside the catalog string; it survives
                            because interpolation is `{{double}}`. */}
                        <T
                          k="gettingStarted.station.name"
                          tags={{ b: <strong />, code: <code /> }}
                        />
                      </li>
                    </ul>
                    <div className="gsg-callout accent">
                      <p className="gsg-callout-label">
                        {t('gettingStarted.station.callout.label')}
                      </p>
                      <p className="gsg-callout-body">
                        <T
                          k="gettingStarted.station.callout.body"
                          tags={{ code: <code /> }}
                          vals={{ full: GUIDE_EXAMPLES.grid, short: GUIDE_EXAMPLES.gridShort }}
                        />
                      </p>
                    </div>
                  </div>

                  <WizardShot caption={t('gettingStarted.station.shot.caption')}>
                    <WizardDots cur={1} />
                    <p className="wizard-title">{t('gettingStarted.station.shot.title')}</p>
                    <p className="wizard-sub">{t('gettingStarted.station.shot.sub')}</p>
                    <div className="wizard-fields">
                      <span className="wizard-field">
                        <span>{t('gettingStarted.station.shot.callsignLabel')}</span>
                        <span className="gsg-shot-input">{GUIDE_EXAMPLES.callsign}</span>
                      </span>
                      <span className="wizard-field">
                        <span>{t('gettingStarted.station.shot.gridLabel')}</span>
                        <span className="gsg-shot-input">{GUIDE_EXAMPLES.grid}</span>
                        <span className="wizard-field-hint">
                          {t('gettingStarted.station.shot.gridHint')}
                        </span>
                      </span>
                    </div>
                    <div className="gsg-shot-actions">
                      <span className="wizard-skip">
                        {t('gettingStarted.station.shot.skip')}
                      </span>
                      <span className="wizard-go">{t('gettingStarted.station.shot.next')}</span>
                    </div>
                  </WizardShot>
                </div>
              </section>
            )}

            {step === 1 && (
              <section aria-label={t('gettingStarted.radio.aria')}>
                <p className="gsg-eyebrow">{t('gettingStarted.radio.eyebrow')}</p>
                <h2 className="gsg-h2">{t('gettingStarted.radio.heading')}</h2>
                <p className="gsg-lede">{t('gettingStarted.radio.lede')}</p>

                <div className="gsg-split">
                  <div className="gsg-prose">
                    <p>
                      <T k="gettingStarted.radio.where" tags={{ b: <strong /> }} />
                    </p>
                    <ul>
                      <li>
                        <T
                          k="gettingStarted.radio.pickRow"
                          tags={{ b: <strong />, em: <em /> }}
                        />
                      </li>
                      <li>
                        <T k="gettingStarted.radio.genericCable" tags={{ b: <strong /> }} />
                      </li>
                      <li>
                        <T
                          k="gettingStarted.radio.flex"
                          tags={{ b: <strong />, code: <code /> }}
                        />
                      </li>
                      <li>
                        <T
                          k="gettingStarted.radio.testCat"
                          tags={{ b: <strong />, code: <code />, em: <em /> }}
                        />
                      </li>
                    </ul>
                    <div className="gsg-callout neutral">
                      <p className="gsg-callout-label">{t('gettingStarted.radio.callout.label')}</p>
                      <p className="gsg-callout-body">
                        <T k="gettingStarted.radio.callout.body" tags={{ b: <strong /> }} />
                      </p>
                    </div>
                  </div>

                  <WizardShot
                    // "Ships inside the installer" is true on Windows only (the .deb declares it;
                    // a Mac gets it from Homebrew) — a Mac field report followed this caption to a
                    // dead end, so the claim is platform-aware now. Two whole captions, not a stem
                    // plus two tails: a fragment cannot be re-ordered by a translator.
                    caption={
                      isMac
                        ? t('gettingStarted.radio.shot.captionMac')
                        : t('gettingStarted.radio.shot.caption')
                    }
                  >
                    <WizardDots cur={2} />
                    <p className="wizard-title">{t('gettingStarted.radio.shot.title')}</p>
                    <p className="wizard-sub">{t('gettingStarted.radio.shot.sub')}</p>
                    <div className="wizard-detect">
                      <span className="wizard-btn">{t('gettingStarted.radio.shot.detect')}</span>
                      <span className="wizard-detect-row sel">
                        <T
                          k="gettingStarted.radio.shot.row"
                          tags={{ b: <b /> }}
                          vals={{ rig: GUIDE_EXAMPLES.rig, port: GUIDE_EXAMPLES.portCiv }}
                        />
                        <span className="wizard-field-hint">
                          {' '}
                          {t('gettingStarted.radio.shot.rowCiv', { chip: GUIDE_EXAMPLES.chip })}
                        </span>
                      </span>
                      <span className="wizard-detect-row">
                        <T
                          k="gettingStarted.radio.shot.row"
                          tags={{ b: <b /> }}
                          vals={{ rig: GUIDE_EXAMPLES.rig, port: GUIDE_EXAMPLES.portSecond }}
                        />
                        <span className="wizard-field-hint">
                          {' '}
                          {t('gettingStarted.radio.shot.rowSecond', { chip: GUIDE_EXAMPLES.chip })}
                        </span>
                      </span>
                      <span className="wizard-field-hint">
                        {t('gettingStarted.radio.shot.selected', {
                          rig: GUIDE_EXAMPLES.rig,
                          port: GUIDE_EXAMPLES.portCiv,
                        })}
                      </span>
                    </div>
                    <div className="wizard-rigconn">
                      <span className="wizard-mode sel">
                        <span className="wizard-mode-label">
                          {t('gettingStarted.radio.shot.usbLabel')}
                        </span>
                        <span className="wizard-mode-blurb">
                          {t('gettingStarted.radio.shot.usbBlurb')}
                        </span>
                      </span>
                      <span className="wizard-mode">
                        <span className="wizard-mode-label">
                          {t('gettingStarted.radio.shot.netLabel')}
                        </span>
                        <span className="wizard-mode-blurb">
                          {t('gettingStarted.radio.shot.netBlurb')}
                        </span>
                      </span>
                    </div>
                    <div className="wizard-fields">
                      <span className="wizard-field">
                        <span>{t('gettingStarted.radio.shot.audioIn')}</span>
                        <span className="gsg-shot-input">{GUIDE_EXAMPLES.audioDevice}</span>
                      </span>
                      <span className="wizard-field">
                        <span>{t('gettingStarted.radio.shot.audioOut')}</span>
                        <span className="gsg-shot-input">{GUIDE_EXAMPLES.audioDevice}</span>
                      </span>
                    </div>
                    <div className="wizard-detect">
                      <span className="wizard-btn">{t('gettingStarted.radio.shot.testCatBtn')}</span>
                      <span className="wizard-field-hint">
                        {t('gettingStarted.radio.shot.testCatResult', {
                          port: GUIDE_EXAMPLES.portCiv,
                          dial: GUIDE_EXAMPLES.dialRead,
                        })}
                      </span>
                    </div>
                  </WizardShot>
                </div>
              </section>
            )}

            {step === 2 && (
              <section aria-label={t('gettingStarted.license.aria')}>
                <p className="gsg-eyebrow">{t('gettingStarted.license.eyebrow')}</p>
                <h2 className="gsg-h2">{t('gettingStarted.license.heading')}</h2>
                <p className="gsg-lede">
                  <T k="gettingStarted.license.lede" tags={{ em: <em /> }} />
                </p>

                <div className="gsg-split">
                  <div className="gsg-prose">
                    <p>
                      <T k="gettingStarted.license.where" tags={{ b: <strong /> }} />
                    </p>
                    <ul>
                      <li>
                        <T k="gettingStarted.license.technician" tags={{ b: <strong /> }} />
                      </li>
                      <li>
                        <T k="gettingStarted.license.general" tags={{ b: <strong /> }} />
                      </li>
                      <li>
                        <T k="gettingStarted.license.sixty" tags={{ b: <strong /> }} />
                      </li>
                      <li>
                        <T k="gettingStarted.license.outsideUs" tags={{ b: <strong /> }} />
                      </li>
                    </ul>
                    <div className="gsg-callout accent">
                      <p className="gsg-callout-label">
                        {t('gettingStarted.license.callout.label')}
                      </p>
                      <p className="gsg-callout-body">
                        <T k="gettingStarted.license.callout.body" tags={{ b: <strong /> }} />
                      </p>
                    </div>
                  </div>

                  <WizardShot caption={t('gettingStarted.license.shot.caption')}>
                    <p className="gsg-shot-h3">{t('gettingStarted.license.shot.title')}</p>
                    <p className="wizard-license-sub">{t('gettingStarted.license.shot.sub')}</p>
                    <div className="wizard-modes">
                      <span className="wizard-mode">
                        <span className="wizard-mode-label">
                          {t('gettingStarted.license.shot.technician')}
                        </span>
                        <span className="wizard-mode-blurb">
                          {t('gettingStarted.license.shot.technicianBlurb')}
                        </span>
                      </span>
                      <span className="wizard-mode sel">
                        <span className="wizard-mode-label">
                          {t('gettingStarted.license.shot.general')}
                        </span>
                        <span className="wizard-mode-blurb">
                          {t('gettingStarted.license.shot.generalBlurb')}
                        </span>
                      </span>
                      <span className="wizard-mode">
                        <span className="wizard-mode-label">
                          {t('gettingStarted.license.shot.extra')}
                        </span>
                        <span className="wizard-mode-blurb">
                          {t('gettingStarted.license.shot.extraBlurb')}
                        </span>
                      </span>
                      <span className="wizard-mode">
                        <span className="wizard-mode-label">
                          {t('gettingStarted.license.shot.outside')}
                        </span>
                        <span className="wizard-mode-blurb">
                          {t('gettingStarted.license.shot.outsideBlurb')}
                        </span>
                      </span>
                    </div>
                    <div className="gsg-shot-toast">
                      <span className="gsg-shot-toast-dot" aria-hidden="true" />
                      <span className="gsg-shot-toast-msg">
                        {t('gettingStarted.license.shot.toast', {
                          freq: GUIDE_EXAMPLES.blockedDial,
                        })}
                      </span>
                    </div>
                  </WizardShot>
                </div>
              </section>
            )}

            {step === 3 && (
              <section aria-label={t('gettingStarted.log.aria')}>
                <div className="gsg-hero-card">
                  <div className="gsg-hero-card-glow" aria-hidden="true" />
                  <div className="gsg-hero-card-inner">
                    <p className="gsg-eyebrow">{t('gettingStarted.log.eyebrow')}</p>
                    <h2 className="gsg-h2 big">{t('gettingStarted.log.heading')}</h2>
                    <p className="gsg-lede wide">{t('gettingStarted.log.lede')}</p>

                    <div className="gsg-compare">
                      <div className="gsg-compare-cell">
                        <p className="gsg-compare-label faint">
                          {t('gettingStarted.log.without.label')}
                        </p>
                        <p className="gsg-compare-body">{t('gettingStarted.log.without.body')}</p>
                      </div>
                      <div className="gsg-compare-cell">
                        <p className="gsg-compare-label ok">{t('gettingStarted.log.b4.label')}</p>
                        <p className="gsg-compare-body">{t('gettingStarted.log.b4.body')}</p>
                      </div>
                      <div className="gsg-compare-cell">
                        <p className="gsg-compare-label ok">
                          {t('gettingStarted.log.needed.label')}
                        </p>
                        <p className="gsg-compare-body">{t('gettingStarted.log.needed.body')}</p>
                      </div>
                      <div className="gsg-compare-cell">
                        <p className="gsg-compare-label ok">
                          {t('gettingStarted.log.awards.label')}
                        </p>
                        <p className="gsg-compare-body">{t('gettingStarted.log.awards.body')}</p>
                      </div>
                    </div>
                  </div>
                </div>

                <div className="gsg-split wide">
                  <div className="gsg-prose">
                    <p className="gsg-prose-h3">{t('gettingStarted.log.sources.head')}</p>
                    <p>
                      <T k="gettingStarted.log.sources.intro" tags={{ code: <code /> }} />
                    </p>
                    <ul className="tight">
                      <li>
                        <T
                          k="gettingStarted.log.sources.wsjtx"
                          tags={{ b: <strong />, code: <code /> }}
                        />
                      </li>
                      <li>
                        <T k="gettingStarted.log.sources.qrz" tags={{ b: <strong /> }} />
                      </li>
                      <li>
                        <T k="gettingStarted.log.sources.others" tags={{ b: <strong /> }} />
                      </li>
                    </ul>
                    <p className="gsg-prose-h3">{t('gettingStarted.log.import.head')}</p>
                    <p>
                      <T k="gettingStarted.log.import.body" tags={{ b: <strong /> }} />
                    </p>
                    <div className="gsg-callout ok">
                      <p className="gsg-callout-label">{t('gettingStarted.log.callout.label')}</p>
                      <p className="gsg-callout-body">
                        <T k="gettingStarted.log.callout.body" tags={{ code: <code /> }} />
                      </p>
                    </div>
                  </div>

                  <WizardShot caption={t('gettingStarted.log.shot.caption')}>
                    <WizardDots cur={3} />
                    <p className="wizard-title">{t('gettingStarted.log.shot.title')}</p>
                    <p className="wizard-sub">
                      <T k="gettingStarted.log.shot.sub" tags={{ b: <strong /> }} />
                    </p>
                    <div className="gsg-shot-log">
                      <span className="wizard-go">{t('gettingStarted.log.shot.import')}</span>
                      <p className="wizard-log-result">
                        <T
                          k="gettingStarted.log.shot.result"
                          tags={{ b: <strong /> }}
                          vals={{
                            count: GUIDE_EXAMPLES.importedQsos,
                            present: GUIDE_EXAMPLES.importedDupes,
                          }}
                        />
                      </p>
                      <p className="wizard-license-sub">
                        {t('gettingStarted.log.shot.formats')}
                      </p>
                    </div>
                  </WizardShot>
                </div>

                <div className="gsg-closing">
                  <p className="gsg-eyebrow ok">{t('gettingStarted.log.closing.eyebrow')}</p>
                  <div className="gsg-closing-grid">
                    <div className="gsg-closing-card">
                      <p className="gsg-closing-num">01</p>
                      <p className="gsg-closing-body">
                        <T
                          k="gettingStarted.log.closing.digital"
                          tags={{ b: <strong /> }}
                          vals={{ freq: GUIDE_EXAMPLES.ft8Dial }}
                        />
                      </p>
                    </div>
                    <div className="gsg-closing-card">
                      <p className="gsg-closing-num">02</p>
                      <p className="gsg-closing-body">
                        <T k="gettingStarted.log.closing.decodes" tags={{ b: <strong /> }} />
                      </p>
                    </div>
                    <div className="gsg-closing-card">
                      <p className="gsg-closing-num">03</p>
                      <p className="gsg-closing-body">
                        <T k="gettingStarted.log.closing.doubleClick" tags={{ b: <strong /> }} />
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
                {t('gettingStarted.back')}
              </button>
              <span className="gsg-nav-label">{progress}</span>
              {step === STEPS.length - 1 ? (
                // The last step's forward action is the way out: there is nothing
                // after "that's all four" but the app itself.
                <button type="button" className="gsg-btn next" onClick={onClose}>
                  {t('gettingStarted.close')}
                </button>
              ) : (
                <button type="button" className="gsg-btn next" onClick={() => go(step + 1)}>
                  {t(STEPS[step].nextKey)}
                </button>
              )}
            </div>

            <aside className="gsg-wsjtx">
              <p className="gsg-wsjtx-label">{t('gettingStarted.wsjtx.label')}</p>
              <p className="gsg-wsjtx-body">
                <T k="gettingStarted.wsjtx.body" tags={{ code: <code /> }} />
                {/* Default Mac keyboards eat bare F-keys as media keys — same OS constraint
                    WSJT-X's own mac docs call out; say it where the F-keys are advertised.
                    A whole sentence of its own, so a translator may place it freely. */}
                {isMac && (
                  <>
                    {' '}
                    {t('gettingStarted.wsjtx.mac')}
                  </>
                )}
              </p>
            </aside>
          </div>
        </div>
      </div>
    </Dialog>
  )
}
