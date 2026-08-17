import { useEffect, useRef, useState } from 'react'
import { Dialog } from './ui/Dialog'
import type { ProfileId } from '../features/profiles'
import type { FeatureId, View } from '../features/registry'
import type { AudioDevices, CatTestResult, DetectedRig, ImportStats, Settings } from '../types'
import {
  addRadio,
  detectRigs,
  discoverFlex,
  getAudioDevices,
  getRigModels,
  getSettings,
  importAdif,
  probeCatPorts,
  updateRadioProfile,
} from '../api'
import { baudForRig, radioPatch } from './SettingsPanel'
import { SetupHealth } from './SetupHealth'
import { isValidGrid } from '../grid'
import { findDaxDevices, isDaxPaired } from '../features/dax'
import { STARTER_PACKS, importPack } from '../features/packs'
import { memoriesStore } from '../features/memories'

/** What the wizard collects beyond the goal profiles: station identity + rig.
 * Only the fields the operator actually touched are set — App merges this into
 * settings with ONE apply_settings write (it's a heavyweight call). */
export interface WizardDraft {
  mycall?: string
  mygrid?: string
  rigConn?: string
  rigAddr?: string
  rigModel?: number
  rigModelName?: string
  serialPort?: string
  baud?: number
  pttMethod?: string
  audioIn?: string
  audioOut?: string
}

interface Props {
  /** Current settings — prefills so the Settings ▸ re-open path edits in place. */
  settings: Settings | null
  /** LIVE radio state for the Setup-health strip (the verify stage) — the same
   * snapshot slice Settings hands its copy of the strip. Optional: tests and any
   * host without a snapshot render the strip in its untested state. */
  radio?: {
    catOk?: boolean | null
    catDetail?: string
    rxLevel: number
    audioError?: string | null
    txEnabled: boolean
    tuning?: boolean
    txPower?: number | null
  }
  /** Apply goal profile(s) + modes + license + the station/rig draft, then navigate. */
  onApply: (
    ids: ProfileId[],
    landing: View,
    modes: FeatureId[],
    license: string,
    draft: WizardDraft,
  ) => void
  /** Save the draft so far and probe CAT against it (Settings' test, wizard-reachable). */
  onTestCat: (draft: WizardDraft) => Promise<CatTestResult>
  /** Key a bounded tune carrier to prove CAT→PTT→RF (the strip's Prove TX; confirm-gated). */
  onProveTx?: () => void
  /** Close without changing the current feature set (also ESC / backdrop). */
  onSkip: () => void
  /** Open the Getting started guide once the wizard finishes, if the operator
   * asked for the walkthrough on the last step. Omitted ⇒ no offer is shown. */
  onOpenGuide?: () => void
}

// License class → sets the transmit-privilege lockout + the licensed-segment band dropdown.
// "Outside the US" = Open (no transmit limits). Single-select; defaults to Open so the
// lockout is opt-in (a US op declares their class to turn it on).
const LICENSE: { id: string; label: string; blurb: string }[] = [
  { id: 'technician', label: 'Technician', blurb: 'US — limited HF + full VHF/UHF' },
  { id: 'general', label: 'General', blurb: 'US — most HF privileges' },
  { id: 'extra', label: 'Amateur Extra', blurb: 'US — full privileges' },
  { id: 'open', label: 'Outside the US', blurb: 'No transmit limits' },
]

/** Strict Maidenhead check — a persisted locator must be a real one, not just
 * something the lenient distance parser happens to swallow. */
const gridOk = isValidGrid

/**
 * First-run setup wizard — four short, individually skippable steps:
 * 1. STATION (callsign + grid — the locator every feature computes from),
 * 2. RIG & AUDIO (auto-detect / Serial-vs-Network with Find-my-Flex + DAX / audio),
 * 3. LOG (optional ADIF import — seeds worked-before / needs / awards from your history),
 * 4. FINISH (license class + optional starter packs + the walkthrough offer —
 *    every feature starts ON; the goal picker is gone, operator 2026-08-09).
 * Everything stays changeable later in Settings; ESC/backdrop = skip-all
 * (marks seen, keeps the current feature set). Prefilled from settings so the
 * Settings ▸ re-open path acts as an editor. Built on the Radix [`Dialog`].
 * See feature-modularity.md §4.6.
 */
export function SetupWizard({ settings, radio, onApply, onTestCat, onProveTx, onSkip, onOpenGuide }: Props) {
  const [step, setStep] = useState(0) // 0 station · 1 rig · 2 log · 3 goals

  // --- Step 3: optional ADIF log import (seeds worked-before / needs / awards) ---
  const [importStats, setImportStats] = useState<ImportStats | null>(null)
  const [importError, setImportError] = useState<string | null>(null)
  const [importing, setImporting] = useState(false)
  const logFileRef = useRef<HTMLInputElement>(null)
  const onImportAdif = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const f = e.target.files?.[0]
    e.target.value = '' // let the same file be re-picked
    if (!f) return
    setImporting(true)
    setImportError(null)
    setImportStats(null)
    try {
      // Feedback is INLINE (below), not a toast — a toast paints dimmed + undismissable behind
      // this modal wizard. A 0/0 result (non-ADIF file) is surfaced as a warning, not a success.
      setImportStats(await importAdif(await f.text()))
    } catch (err) {
      setImportError(`Import failed: ${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setImporting(false)
    }
  }

  // --- Step 1: station identity ---
  const [mycall, setMycall] = useState(() => settings?.mycall ?? '')
  const [mygrid, setMygrid] = useState(() => settings?.mygrid ?? '')

  // --- Step 2: rig & audio (loaded lazily when the step opens) ---
  const [rigConn, setRigConn] = useState(() => settings?.rigConn ?? 'serial')
  const [rigAddr, setRigAddr] = useState(() => settings?.rigAddr ?? '')
  const [rigModel, setRigModel] = useState<number | undefined>(undefined)
  const [rigModelName, setRigModelName] = useState<string | undefined>(undefined)
  const [serialPort, setSerialPort] = useState(() => settings?.serialPort ?? '')
  const [audioIn, setAudioIn] = useState(() => settings?.audioIn ?? '')
  const [audioOut, setAudioOut] = useState(() => settings?.audioOut ?? '')
  // Device NAMES (what gets stored) kept apart from their display labels — the Settings
  // panel does the same, so the dedupe/DAX code below keeps working on plain strings.
  const [audio, setAudio] = useState<{ input: string[]; output: string[] }>({ input: [], output: [] })
  // Per direction, not merged: one ALSA PCM can read differently in the two lists (its
  // label shortens only where its card is unambiguous within that list).
  const [audioLabels, setAudioLabels] = useState<{
    input: Record<string, string>
    output: Record<string, string>
  }>({ input: {}, output: {} })
  const applyAudio = (d: AudioDevices) => {
    setAudio({ input: d.input.map((x) => x.name), output: d.output.map((x) => x.name) })
    setAudioLabels({
      input: Object.fromEntries(d.input.map((x) => [x.name, x.label])),
      output: Object.fromEntries(d.output.map((x) => [x.name, x.label])),
    })
  }
  const [detected, setDetected] = useState<DetectedRig[] | null>(null)
  const [detectedFlex, setDetectedFlex] = useState<{ model: string; nickname: string; ip: string }[]>([])
  const [detecting, setDetecting] = useState(false)
  const [detectError, setDetectError] = useState<string | null>(null)
  const [flexNote, setFlexNote] = useState<string | null>(null)
  const [catResult, setCatResult] = useState<CatTestResult | null>(null)
  const [catTesting, setCatTesting] = useState(false)
  // Baud + PTT enter the draft (setup re-envisioning, 2026-08-09): the old wizard had
  // neither field, so a probed baud was thrown away and every wizard-configured rig
  // shipped on the silent-VOX default — Test CAT then reported "✗ VOX — no CAT" for a
  // gap the wizard itself created.
  const [baud, setBaud] = useState<number>(() => settings?.baud ?? 38400)
  const [pttMethod, setPttMethod] = useState<string>(() => settings?.pttMethod ?? 'vox')
  // The Hamlib model catalog for the confirm dropdown ("found the port but not the
  // exact model" — an FT-991A answers the FTDX10 seed, so the GUESS must be confirmable).
  const [models, setModels] = useState<[number, string][]>([])
  // The seeded-probe flag: the port+baud are proven, the model is a guess — the dropdown
  // below becomes REQUIRED reading until the operator picks (Settings refuses to persist
  // seeded models; the wizard is finally the control that accepts a real one).
  const [modelSeeded, setModelSeeded] = useState(false)
  // Auto-test (the port_prober sweep, seeded with common rigs at their bauds). Honest
  // elapsed counter, no fake progress and no fake Cancel — the probe cannot be
  // cancelled, and a dead bar would lie (operator decision 2026-08-09: counter only).
  const [probing, setProbing] = useState(false)
  const [probeElapsed, setProbeElapsed] = useState(0)
  const [probeNote, setProbeNote] = useState<string | null>(null)
  useEffect(() => {
    if (!probing) return
    setProbeElapsed(0)
    const id = window.setInterval(() => setProbeElapsed((s) => s + 1), 1000)
    return () => window.clearInterval(id)
  }, [probing])
  const runProbe = () => {
    setProbing(true)
    setProbeNote(null)
    probeCatPorts()
      .then((r) => {
        if (!r.found) {
          setProbeNote(r.detail)
          return
        }
        setSerialPort(r.portName)
        setBaud(r.baud)
        setRigConn('serial')
        // A real answer means CAT works — kill the silent-VOX default.
        setPttMethod('cat')
        if (r.modelSeeded) {
          // Port + baud proven; the model is a guess. Keep the guess as the dropdown's
          // preselection and say plainly that picking the exact model is the next step.
          setRigModel(r.model)
          setRigModelName(r.modelName)
          setModelSeeded(true)
        } else {
          setRigModel(r.model)
          setRigModelName(r.modelName)
          setModelSeeded(false)
        }
        setProbeNote(r.detail)
      })
      .catch((e) => setProbeNote(`Auto-test failed: ${e instanceof Error ? e.message : String(e)}`))
      .finally(() => setProbing(false))
  }
  /** Confirm the exact model: fixed-baud rigs get their one true rate (the Settings
   * rule, same function), and a confirmed model clears the seeded flag. */
  const pickModel = (num: number, name: string) => {
    setRigModel(num)
    setRigModelName(name)
    setModelSeeded(false)
    const fixed = baudForRig(num, baud)
    if (fixed != null) setBaud(fixed)
    if (pttMethod === 'vox') setPttMethod('cat')
  }
  // --- The SECOND radio: a card in this step, not a fifth step (multi-radio had zero
  // wizard presence — ~11 steps across three Settings sections, with a recorded field
  // failure in the Edit-vs-Active trap; updateRadioProfile is the by-id verb that
  // dodges it). Flow: add → probe with radio 1's port excluded (server-side) → the
  // probed CAT + this port's 1:1-assigned codec written to the NEW profile.
  const [second, setSecond] = useState<null | {
    id: number
    probing: boolean
    note: string | null
    portName?: string
    model?: number
    modelName?: string
    seeded?: boolean
  }>(null)
  const [secondElapsed, setSecondElapsed] = useState(0)
  const [swapOffer, setSwapOffer] = useState(false)
  useEffect(() => {
    if (!second?.probing) return
    setSecondElapsed(0)
    const id = window.setInterval(() => setSecondElapsed((t) => t + 1), 1000)
    return () => window.clearInterval(id)
  }, [second?.probing])
  const addSecondRadio = async () => {
    try {
      await addRadio()
      const s = await getSettings()
      const roster = s.radios ?? []
      if (roster.length === 0) throw new Error('no radio profiles after add')
      const newId = Math.max(...roster.map((p) => p.id))
      setSecond({ id: newId, probing: true, note: null })
      const r = await probeCatPorts(newId)
      if (!r.found) {
        setSecond({ id: newId, probing: false, note: r.detail })
        return
      }
      // This port's roster row carries its 1:1-assigned codec (never radio 1's — the
      // assignment pass hands generic codecs out one per rig).
      const row = detected?.find((d) => d.portName === r.portName)
      const p = roster.find((x) => x.id === newId)
      await updateRadioProfile(
        newId,
        radioPatch({
          ...p,
          serialPort: r.portName,
          baud: r.baud,
          rigModel: r.model,
          rigModelName: r.modelName,
          rigConn: 'serial',
          pttMethod: 'cat',
          ...(row?.suggestedAudio ? { audioIn: row.suggestedAudio } : {}),
          ...(row?.suggestedAudioOut || row?.suggestedAudio
            ? { audioOut: row?.suggestedAudioOut ?? row?.suggestedAudio ?? '' }
            : {}),
        }),
      )
      setSecond({
        id: newId,
        probing: false,
        note: r.detail,
        portName: r.portName,
        model: r.model,
        modelName: r.modelName,
        seeded: r.modelSeeded,
      })
      // Two rigs whose codecs both came from the generic pool are told apart only by
      // position — the 1:1 pass makes the worst case a SWAP, and this is the one
      // binary question that fixes it. Product-named codecs are unambiguous: no offer.
      const generic = (name: string | null | undefined) =>
        !!name && !/\b(FT|IC|TS|TX)-?\d/i.test(name)
      setSwapOffer(generic(row?.suggestedAudio) && generic(audioIn))
    } catch (e) {
      setSecond({
        id: -1,
        probing: false,
        note: `Couldn't add the radio: ${e instanceof Error ? e.message : String(e)}`,
      })
    }
  }
  /** Confirm the second radio's exact model (a seeded probe guessed it). */
  const pickSecondModel = async (num: number, name: string) => {
    if (!second || second.id < 0) return
    const s = await getSettings()
    const p = (s.radios ?? []).find((x) => x.id === second.id)
    if (!p) return
    const fixed = baudForRig(num, p.baud)
    await updateRadioProfile(
      second.id,
      radioPatch({ ...p, rigModel: num, rigModelName: name, ...(fixed != null ? { baud: fixed } : {}) }),
    )
    setSecond({ ...second, model: num, modelName: name, seeded: false })
  }
  /** The swap answer: exchange the two radios' audio devices in their profiles. */
  const swapAudio = async () => {
    if (!second || second.id < 0) return
    const s = await getSettings()
    const roster = s.radios ?? []
    const b = roster.find((x) => x.id === second.id)
    // Radio 1 = the radio this wizard pass configured (the active/flat one).
    const a = roster.find((x) => x.id !== second.id && x.serialPort === serialPort) ?? roster[0]
    if (!a || !b) return
    await updateRadioProfile(a.id, radioPatch({ ...a, audioIn: b.audioIn, audioOut: b.audioOut }))
    await updateRadioProfile(b.id, radioPatch({ ...b, audioIn: a.audioIn, audioOut: a.audioOut }))
    // The wizard's own draft mirrors radio 1's new devices so the final apply agrees.
    setAudioIn(b.audioIn)
    setAudioOut(b.audioOut)
    setSwapOffer(false)
  }

  const scannedOnEntry = useRef(false)
  useEffect(() => {
    if (step !== 1) return
    getAudioDevices()
      .then(applyAudio)
      .catch(() => {})
    getRigModels()
      .then(setModels)
      .catch(() => {})
    // The pipeline runs ON ENTRY (scan first — cheap USB/LAN enumeration): the operator
    // watches the wizard find things instead of pressing buttons they must know exist
    // ("nothing is working" was the literal output of the empty screen). Once per open.
    if (!scannedOnEntry.current) {
      scannedOnEntry.current = true
      runDetect()
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [step])

  // --- Step 3: license (goals/modes questions removed — everything starts on) ---
  const [license, setLicense] = useState('open')

  // --- Step 3: optional starter-channel packs. First run ONLY: snapshot whether the
  // bank was empty when the wizard opened, and show the offer only then — so a returning
  // operator who re-opens the wizard (it doubles as a Settings editor) never has packs
  // they deleted silently re-added. Pre-check the two most broadly useful. ---
  const [bankWasEmpty] = useState(() => memoriesStore.get().memories.length === 0)
  const [packs, setPacks] = useState<Set<string>>(
    () => new Set(bankWasEmpty ? ['na-calling', 'na-digital'] : []),
  )
  const togglePack = (id: string) =>
    setPacks((s) => {
      const n = new Set(s)
      if (n.has(id)) n.delete(id)
      else n.add(id)
      return n
    })
  /** Seed the chosen starter packs into the shared Memories bank (idempotent — importPack
   * dedupes). Called on wizard COMPLETION only, never on "I'll set it up myself". */
  const seedPacks = () => {
    if (packs.size === 0) return
    memoriesStore.update((bank) => {
      let b = bank
      for (const p of STARTER_PACKS) if (packs.has(p.id)) b = importPack(b, p).bank
      return b
    })
  }

  /** Only fields the operator actually changed vs the prefill go in the draft —
   * an untouched wizard must not rewrite settings it never asked about. */
  const draft = (): WizardDraft => {
    const d: WizardDraft = {}
    const call = mycall.trim().toUpperCase()
    if (call !== (settings?.mycall ?? '')) d.mycall = call
    const grid = mygrid.trim()
    if (grid !== (settings?.mygrid ?? '') && (grid === '' || gridOk(grid))) d.mygrid = grid
    if (rigConn !== (settings?.rigConn ?? 'serial')) d.rigConn = rigConn
    if (rigAddr.trim() !== (settings?.rigAddr ?? '')) d.rigAddr = rigAddr.trim()
    if (rigModel != null) {
      d.rigModel = rigModel
      d.rigModelName = rigModelName ?? ''
    }
    if (serialPort !== (settings?.serialPort ?? '')) d.serialPort = serialPort
    if (baud !== (settings?.baud ?? 38400)) d.baud = baud
    if (pttMethod !== (settings?.pttMethod ?? 'vox')) d.pttMethod = pttMethod
    if (audioIn !== (settings?.audioIn ?? '')) d.audioIn = audioIn
    if (audioOut !== (settings?.audioOut ?? '')) d.audioOut = audioOut
    return d
  }

  // --- Step 3: the optional walkthrough. Offered on the last step only, and
  // honoured on either completion path — "instead of dismissing", the wizard
  // hands the operator the Getting started guide for what they just set up.
  const [wantGuide, setWantGuide] = useState(false)

  /** The one exit that applies: seed packs, write the settings, then open the
   * guide if it was asked for. Both finish buttons go through here so they can
   * never drift apart. */
  const finish = (profileIds: ProfileId[], view: View, modeIds: FeatureId[]) => {
    seedPacks()
    onApply(profileIds, view, modeIds, license, draft())
    if (wantGuide) onOpenGuide?.()
  }

  const gridState = mygrid.trim() === '' ? 'empty' : gridOk(mygrid) ? 'ok' : 'bad'
  const dax = findDaxDevices(audio.input, audio.output)
  // Same rule as Settings, including WHY the wording is careful: this compares against the
  // list we OFFER, which is not the set cpal will try to OPEN, so "not detected" would assert
  // something we cannot know. See the long note in SettingsPanel.
  const audioLabel = (name: string, kind: 'input' | 'output') =>
    audio[kind].includes(name)
      ? (audioLabels[kind][name] ?? name)
      : `${name} — saved, not in the list`

  const runDetect = () => {
    setDetecting(true)
    setDetectError(null)
    // One scan, every radio kind: USB enumeration + Flex LAN discovery run together; either probe
    // may fail without killing the other's results. Failures are SURFACED (not swallowed to an
    // empty list that reads as "no radios found") so a newcomer isn't left staring at nothing.
    Promise.allSettled([detectRigs(), discoverFlex()]).then(([rigsR, flexR]) => {
      const errs: string[] = []
      if (rigsR.status === 'fulfilled') setDetected(rigsR.value)
      else {
        setDetected([])
        errs.push(`USB scan failed: ${String(rigsR.reason)}`)
      }
      if (flexR.status === 'fulfilled') setDetectedFlex(flexR.value)
      else {
        setDetectedFlex([])
        errs.push(`Flex network scan failed: ${String(flexR.reason)}`)
      }
      setDetectError(errs.length ? errs.join(' · ') : null)
      setDetecting(false)
    })
  }
  const applyDetected = (r: DetectedRig) => {
    if (r.suggestedModel != null) {
      setRigModel(r.suggestedModel)
      setRigModelName(r.suggestedModelName ?? '')
      setModelSeeded(false)
      // A named rig has one true rate where the table knows it (the Settings rule).
      const fixed = baudForRig(r.suggestedModel, baud)
      if (fixed != null) setBaud(fixed)
    }
    setSerialPort(r.portName)
    setRigConn('serial')
    // PTT: a recognised interface cable names its keying line; a model-named rig
    // gets CAT keying (killing the silent-VOX default that made the wizard's own
    // Test CAT report "✗ VOX — no CAT"). An unidentified bridge chip is left
    // alone — the Auto-test chain below settles it when it finds the rig.
    if (r.interfacePttMethod) setPttMethod(r.interfacePttMethod)
    else if (r.suggestedModel != null && pttMethod === 'vox') setPttMethod('cat')
    // RX from the capture list, TX from the OUTPUT list — the rig's CODEC enumerates under
    // different input/output names on Windows, so reusing the input name for audioOut sent TX
    // audio to the PC speakers. Fall back to the input name only if no output paired.
    if (r.suggestedAudio) setAudioIn(r.suggestedAudio)
    if (r.suggestedAudioOut || r.suggestedAudio) {
      setAudioOut(r.suggestedAudioOut ?? r.suggestedAudio ?? '')
    }
    // The pipeline's second stage: an unidentified rig (bridge chip, no model) chains
    // straight into Auto-test — the sweep is seeded with the common rigs at their bauds
    // (an FTDX10 and an FT-991 are its first two seeds), so the wizard identifies the
    // radio instead of leaving a blank model that silently disarms Test CAT.
    if (r.suggestedModel == null && r.interfaceName == null && !probing) {
      runProbe()
    }
  }
  const applyDetectedFlex = (f: { model: string; nickname: string; ip: string }) => {
    // The WSJT-X-proven path: CAT rides the SmartSDR CAT app on THIS PC
    // (model 2036 @ SmartSDR CAT's default slice-A TCP port 5002), never the
    // radio's own :4992 — Hamlib's direct native backend is alpha and failed
    // on a real 6400M. Discovery proves the radio is reachable.
    setRigConn('network')
    setRigAddr('127.0.0.1:5002')
    setRigModel(2036)
    setRigModelName('FlexRadio FLEX-6xxx (SmartSDR CAT)')
    setFlexNote(
      `${f.model}${f.nickname ? ` "${f.nickname}"` : ''} at ${f.ip} — CAT set via SmartSDR CAT (slice A, port 5002; a second slice uses 60001). Test CAT below.`,
    )
  }
  const runCatTest = () => {
    setCatTesting(true)
    setCatResult(null)
    onTestCat(draft())
      .then(setCatResult)
      .catch((e) =>
        setCatResult({ ok: false, detail: e instanceof Error ? e.message : String(e) }),
      )
      .finally(() => setCatTesting(false))
  }

  const stepTitles = ['Your station', 'Your rig', 'Your log', 'Finish']

  return (
    <Dialog
      open
      // ESC / backdrop / close → skip (keeps the current set, marks seen).
      onOpenChange={(o) => {
        if (!o) onSkip()
      }}
      title="Set up Nexus"
      hideTitle
    >
      <div className="wizard-dots" aria-label={`Step ${step + 1} of ${stepTitles.length}: ${stepTitles[step]}`}>
        {stepTitles.map((t, i) => (
          <button
            key={t}
            type="button"
            className={`wizard-dot${i === step ? ' cur' : ''}${i < step ? ' done' : ''}`}
            // Same gate as Next: a malformed grid can't be walked past via the dots.
            disabled={step === 0 && gridState === 'bad' && i !== 0}
            onClick={() => setStep(i)}
            title={t}
          >
            <span className="wizard-dot-n">{i + 1}</span> {t}
          </button>
        ))}
      </div>

      {step === 0 && (
        <>
          <h2 className="wizard-title">Who&rsquo;s on the air?</h2>
          <p className="wizard-sub">
            Your grid square is the anchor for everything location-based — satellite passes,
            propagation, the map, and DXpedition windows are all computed from it.
          </p>
          <div className="wizard-fields">
            <label className="wizard-field">
              <span>Callsign</span>
              <input
                type="text"
                value={mycall}
                placeholder="N0CALL"
                autoComplete="off"
                spellCheck={false}
                onChange={(e) => setMycall(e.target.value.toUpperCase())}
              />
            </label>
            <label className="wizard-field">
              <span>Grid square</span>
              <input
                type="text"
                value={mygrid}
                placeholder="FN31"
                autoComplete="off"
                spellCheck={false}
                className={gridState === 'bad' ? 'bad' : ''}
                // Announce the error to a screen reader (the red border + a
                // silently-disabled Next are invisible otherwise): aria-invalid
                // + the hint as an alert region tied by describedby.
                aria-invalid={gridState === 'bad'}
                aria-describedby="wizard-grid-hint"
                onChange={(e) => setMygrid(e.target.value)}
              />
              <span
                id="wizard-grid-hint"
                className={`wizard-field-hint${gridState === 'bad' ? ' bad' : ''}`}
                role={gridState === 'bad' ? 'alert' : undefined}
              >
                {gridState === 'bad'
                  ? 'Not a Maidenhead locator — 4 or 6 characters, like FN31 or FN31pr.'
                  : 'Maidenhead locator (qrz.com shows yours). Give all 6 — 4 characters pins you to the middle of a ~100-mile square, and every distance and bearing is measured from there.'}
              </span>
            </label>
          </div>
        </>
      )}

      {step === 1 && (
        <>
          <h2 className="wizard-title">How does the radio connect?</h2>
          <p className="wizard-sub">
            One detect finds everything — USB rigs and FlexRadios on the network.
            Skippable; Settings ▸ Radio ▸ Rig &amp; CAT has all of this later (including Test CAT).
          </p>
          <div className="wizard-detect">
            <button type="button" className="wizard-btn" disabled={detecting} onClick={runDetect}>
              {detecting ? 'Detecting…' : '🔍 Detect my radio'}
            </button>
            {detectError && (
              <span className="wizard-field-hint" role="alert" style={{ color: 'var(--danger, #e5484d)' }}>
                Detection hit an error: {detectError}. You can still pick your rig by hand below, or
                skip and set it up later.
              </span>
            )}
            {!detectError && detected != null && detected.length === 0 && detectedFlex.length === 0 && (
              <span className="wizard-field-hint">
                Nothing found — USB: plug in + power on; Flex: must be on this network.
                Or skip and set it up later.
              </span>
            )}
            {detectedFlex.map((f) => (
              <button
                key={f.ip}
                type="button"
                className={`wizard-detect-row${rigConn === 'network' && rigModel === 2036 ? ' sel' : ''}`}
                onClick={() => applyDetectedFlex(f)}
              >
                <b>
                  {f.model}
                  {f.nickname ? ` “${f.nickname}”` : ''}
                </b>{' '}
                on the network ({f.ip})
                <span className="wizard-field-hint"> · via SmartSDR CAT</span>
              </button>
            ))}
            {(detected ?? []).map((r) => (
              <button
                key={r.portName}
                type="button"
                className={`wizard-detect-row${serialPort === r.portName ? ' sel' : ''}`}
                onClick={() => applyDetected(r)}
              >
                <b>{r.suggestedModelName ?? r.product ?? 'Unknown radio'}</b> on {r.portName}
                <span className="wizard-field-hint">
                  {' '}
                  · {r.chip}
                  {/* Dual-UART Icoms: two rows say the same rig — tag the CI-V side. */}
                  {r.civSide === true
                    ? ' · CI-V port — use this one'
                    : r.civSide === false
                      ? ' · second port, not CI-V'
                      : ''}
                </span>
              </button>
            ))}
            {flexNote && <span className="wizard-field-hint">{flexNote}</span>}
            {rigConn === 'serial' && serialPort && (
              <span className="wizard-field-hint">
                Selected: {rigModelName ?? 'radio'} on {serialPort}
                {baud ? ` @ ${baud} baud` : ''}
              </span>
            )}
            {/* Auto-test: the port_prober sweep (seeded with the common rigs at their
                bauds) — Settings-only until now, and the one thing that could have
                answered "nothing is working" for a bridge-chip rig with no model. */}
            {rigConn === 'serial' && (
              <button
                type="button"
                className="wizard-btn"
                disabled={probing}
                onClick={runProbe}
                title="Probes every USB port until a radio answers — read-only, never transmits"
              >
                {probing
                  ? `Testing ports — ${probeElapsed}s (can take up to a minute)…`
                  : '🔎 Auto-test my ports'}
              </button>
            )}
            {probeNote && (
              <span className="wizard-field-hint" role="status">
                {probeNote}
              </span>
            )}
          </div>

          {/* The model CONFIRM — the wizard finally gets the dropdown Settings always had.
              A seeded probe hit means the PORT is proven and the model is a guess (an
              FT-991A answers the FTDX10 seed), so the question is asked instead of the
              guess being silently persisted. Picking a model applies its fixed baud. */}
          {rigConn === 'serial' && (rigModel != null || serialPort) && (
            <div className="wizard-fields">
              <label className="wizard-field">
                <span>Which radio is this?</span>
                <select
                  className={modelSeeded ? 'bad' : ''}
                  aria-invalid={modelSeeded || undefined}
                  value={rigModel ?? 0}
                  onChange={(e) => {
                    const num = Number(e.target.value)
                    const name = models.find(([m]) => m === num)?.[1] ?? ''
                    pickModel(num, name)
                  }}
                >
                  <option value={0} disabled>
                    Pick your rig model…
                  </option>
                  {/* Keep an out-of-catalog preselection visible rather than blanking it. */}
                  {rigModel != null && !models.some(([m]) => m === rigModel) && (
                    <option value={rigModel}>{rigModelName ?? `Model ${rigModel}`}</option>
                  )}
                  {models.map(([num, name]) => (
                    <option key={num} value={num}>
                      {name}
                    </option>
                  ))}
                </select>
                <span
                  className={`wizard-field-hint${modelSeeded ? ' bad' : ''}`}
                  role={modelSeeded ? 'alert' : undefined}
                >
                  {modelSeeded
                    ? `The port answered, but the exact model is a guess (${rigModelName ?? 'unknown'} responded). Pick your radio so its real command set is used.`
                    : 'Confirm the exact model — fixed-rate rigs get their baud set automatically.'}
                </span>
              </label>
            </div>
          )}
          <div className="wizard-rigconn">
            <button
              type="button"
              className={`wizard-mode${rigConn === 'serial' ? ' sel' : ''}`}
              aria-pressed={rigConn === 'serial'}
              onClick={() => setRigConn('serial')}
            >
              <span className="wizard-mode-label">USB / Serial</span>
              <span className="wizard-mode-blurb">Most rigs — one cable</span>
            </button>
            <button
              type="button"
              className={`wizard-mode${rigConn === 'network' ? ' sel' : ''}`}
              aria-pressed={rigConn === 'network'}
              onClick={() => setRigConn('network')}
            >
              <span className="wizard-mode-label">Network</span>
              <span className="wizard-mode-blurb">FlexRadio / remote rigctld</span>
            </button>
          </div>

          {rigConn === 'network' && (
            <div className="wizard-detect">
              <label className="wizard-field">
                <span>Address</span>
                <input
                  type="text"
                  value={rigAddr}
                  placeholder="127.0.0.1:5002"
                  autoComplete="off"
                  spellCheck={false}
                  onChange={(e) => setRigAddr(e.target.value)}
                />
              </label>
              <span className="wizard-field-hint">
                A found Flex configures the WSJT-X-proven path: CAT through the SmartSDR
                CAT app on this PC — its default TCP port 5002 drives slice A (per-slice
                ports: B=60001, C=60002) — and audio through DAX. Other network rigs:
                pick their model later in Settings ▸ Radio ▸ Rig &amp; CAT.
              </span>
              {dax && !isDaxPaired(audioIn, audioOut) && (
                <button
                  type="button"
                  className="wizard-btn"
                  onClick={() => {
                    setAudioIn(dax.input)
                    setAudioOut(dax.output)
                  }}
                  title="SmartSDR's DAX virtual audio devices were detected — pairs them as Nexus's audio in/out"
                >
                  ⚡ Pair DAX audio ({dax.input})
                </button>
              )}
            </div>
          )}

          <div className="wizard-fields">
            <label className="wizard-field">
              <span>Audio in</span>
              <select value={audioIn} onChange={(e) => setAudioIn(e.target.value)}>
                <option value="">System default</option>
                {/* Saved-but-unplugged device stays selectable (the Settings rule) —
                    a blank select that can only LOSE the saved routing is a trap. */}
                {[...new Set([...(audioIn ? [audioIn] : []), ...audio.input])].map((d) => (
                  <option key={d} value={d}>
                    {audioLabel(d, 'input')}
                  </option>
                ))}
              </select>
            </label>
            <label className="wizard-field">
              <span>Audio out</span>
              <select value={audioOut} onChange={(e) => setAudioOut(e.target.value)}>
                <option value="">System default</option>
                {[...new Set([...(audioOut ? [audioOut] : []), ...audio.output])].map((d) => (
                  <option key={d} value={d}>
                    {audioLabel(d, 'output')}
                  </option>
                ))}
              </select>
            </label>
          </div>

          <div className="wizard-detect">
            <button
              type="button"
              className="wizard-btn"
              disabled={catTesting}
              onClick={runCatTest}
              title="Saves what you've entered so far, then asks the radio for its frequency"
            >
              {catTesting ? 'Testing…' : '⚡ Test CAT'}
            </button>
            {catResult && (
              <span className={`wizard-field-hint${catResult.ok ? '' : ' bad'}`}>
                {catResult.ok ? '✓ ' : '✗ '}
                {catResult.detail}
              </span>
            )}
          </div>

          {/* The VERIFY stage — the same Setup-health strip Settings renders (extracted
              shared component), so the wizard ends on evidence, not faith: Rig / RX dB /
              TX, with Prove TX keying a bounded carrier behind its confirm. */}
          <SetupHealth radio={radio} catResult={catResult} onProveTx={onProveTx} />

          {/* The SECOND radio — a card, not a fifth step. Offered once radio 1 has a
              port; the probe automatically skips radio 1's port (it's held). */}
          {rigConn === 'serial' && serialPort && (
            <div className="wizard-detect">
              {!second && (
                <button
                  type="button"
                  className="wizard-btn"
                  onClick={() => void addSecondRadio()}
                  title="Adds a radio profile and probes the remaining USB ports for it"
                >
                  ＋ I have a second radio
                </button>
              )}
              {second?.probing && (
                <span className="wizard-field-hint" role="status">
                  Probing the other ports — {secondElapsed}s (radio 1&rsquo;s port is skipped;
                  this can take up to a minute)…
                </span>
              )}
              {second && !second.probing && (
                <span className="wizard-field-hint" role="status">
                  {second.portName
                    ? `Second radio: ${second.modelName ?? 'radio'} on ${second.portName} — saved to its own profile.`
                    : second.note}
                </span>
              )}
              {second && !second.probing && second.seeded && (
                <label className="wizard-field">
                  <span>Which radio is the second one?</span>
                  <select
                    className="bad"
                    aria-invalid
                    value={second.model ?? 0}
                    onChange={(e) => {
                      const num = Number(e.target.value)
                      const name = models.find(([m]) => m === num)?.[1] ?? ''
                      void pickSecondModel(num, name)
                    }}
                  >
                    <option value={0} disabled>
                      Pick the model…
                    </option>
                    {second.model != null && !models.some(([m]) => m === second.model) && (
                      <option value={second.model}>{second.modelName ?? `Model ${second.model}`}</option>
                    )}
                    {models.map(([num, name]) => (
                      <option key={num} value={num}>
                        {name}
                      </option>
                    ))}
                  </select>
                  <span className="wizard-field-hint bad" role="alert">
                    The port answered but the exact model is a guess — pick the radio so its
                    real command set is used.
                  </span>
                </label>
              )}
              {second && !second.probing && second.portName && swapOffer && (
                <span className="wizard-field-hint">
                  Both radios use identical USB sound cards, shared out one each. If you later
                  see the <em>wrong</em> rig&rsquo;s meters move when audio plays,&nbsp;
                  <button type="button" className="settings-linkbtn" onClick={() => void swapAudio()}>
                    swap them
                  </button>
                  .
                </span>
              )}
              {second && !second.probing && second.portName && (
                <span className="wizard-field-hint">
                  To run both radios at the same time, open Nexus twice — each window drives
                  one radio (the launcher asks which).
                </span>
              )}
            </div>
          )}
        </>
      )}

      {step === 2 && (
        <>
          <h2 className="wizard-title">Bring in your existing log</h2>
          <p className="wizard-sub">
            Nexus works best when it knows your history. Importing your ADIF log is what powers{' '}
            <strong>worked-before</strong> flags, the <strong>Needed</strong> board (new DXCC / states
            / grids), and your <strong>awards</strong> progress — without it, the app starts blind and
            treats every station as new. This is optional and you can import anytime from the Logbook,
            but it's the single biggest thing that makes the app useful on day one.
          </p>
          <div className="wizard-log-import">
            <input
              ref={logFileRef}
              type="file"
              accept=".adi,.adif,text/plain"
              style={{ display: 'none' }}
              onChange={onImportAdif}
            />
            <button
              type="button"
              className="wizard-go"
              disabled={importing}
              onClick={() => logFileRef.current?.click()}
            >
              {importing ? 'Importing…' : importStats ? 'Import another ADIF file' : 'Import my ADIF log…'}
            </button>
            {importError && (
              <p className="wizard-log-error" role="alert">
                ⚠ {importError}
              </p>
            )}
            {importStats &&
              (importStats.added > 0 ? (
                <p className="wizard-log-result" role="status">
                  ✓ Imported <strong>{importStats.added}</strong>{' '}
                  QSO{importStats.added === 1 ? '' : 's'}
                  {importStats.skipped ? ` · ${importStats.skipped} already present` : ''}. Your
                  worked-before and Needed board are now seeded.
                </p>
              ) : importStats.skipped > 0 ? (
                <p className="wizard-log-result" role="status">
                  ✓ All {importStats.skipped} QSOs were already in your log — you're seeded.
                </p>
              ) : (
                <p className="wizard-log-error" role="status">
                  ⚠ No QSOs found in that file — is it a standard ADIF (.adi/.adif) export?
                </p>
              ))}
            <p className="wizard-license-sub">
              From WSJT-X, N1MM, Log4OM, HRD, QRZ, LoTW, ClubLog — any standard ADIF (.adi/.adif)
              export. Nothing leaves your computer; duplicates are detected and skipped.
            </p>
          </div>
        </>
      )}

      {step === 3 && (
        <>
          <h2 className="wizard-title">You get everything</h2>
          <p className="wizard-sub">
            Every mode and every section starts ON — FT8/FT4, Phone, CW, RTTY, SSTV, APRS,
            satellites, the maps, the lot. Nexus is one program instead of six; there&rsquo;s
            nothing to unlock. If you ever want a leaner app, trim sections in Settings.
          </p>
          {/* The goal-profile picker that used to live here is gone (operator, 2026-08-09:
              "I want it to start with it all") — asking a newcomer to scope the app down
              before they have seen it was the opposite of easy-and-powerful-defaults. The
              profiles remain in Settings ▸ Appearance for anyone who wants a lean set. */}

          <h3 className="wizard-modes-title">What’s your license?</h3>
          <p className="wizard-license-sub">
            Sets your transmit privileges — the app parks the dial in your licensed band segments
            and won’t let you transmit outside them. Pick “Outside the US” for no limits.
          </p>
          <div className="wizard-modes">
            {LICENSE.map((l) => (
              <button
                key={l.id}
                type="button"
                className={`wizard-mode${license === l.id ? ' sel' : ''}`}
                aria-pressed={license === l.id}
                onClick={() => setLicense(l.id)}
              >
                <span className="wizard-mode-label">{l.label}</span>
                <span className="wizard-mode-blurb">{l.blurb}</span>
              </button>
            ))}
          </div>

          {bankWasEmpty && (
            <>
              <h3 className="wizard-modes-title">Start with some channels?</h3>
              <p className="wizard-license-sub">
                Optional — a ready-made set of common frequencies and nets, added to your Memories.
                Change or remove any of them later in the Memories section.
              </p>
              <div className="wizard-modes">
                {STARTER_PACKS.map((p) => (
                  <button
                    key={p.id}
                    type="button"
                    className={`wizard-mode${packs.has(p.id) ? ' sel' : ''}`}
                    aria-pressed={packs.has(p.id)}
                    onClick={() => togglePack(p.id)}
                  >
                    <span className="wizard-mode-label">{p.name}</span>
                    <span className="wizard-mode-blurb">
                      {p.memories.length} channels · {p.region}
                    </span>
                  </button>
                ))}
              </div>
            </>
          )}

          {onOpenGuide && (
            <>
              <h3 className="wizard-modes-title">Want a walkthrough of what you just set up?</h3>
              <div className="wizard-modes">
                <button
                  type="button"
                  className={`wizard-mode${wantGuide ? ' sel' : ''}`}
                  aria-pressed={wantGuide}
                  onClick={() => setWantGuide(!wantGuide)}
                >
                  <span className="wizard-mode-label">Show me Getting started</span>
                  <span className="wizard-mode-blurb">
                    The four things, in order — opens when this closes
                  </span>
                </button>
              </div>
            </>
          )}
        </>
      )}

      <div className="wizard-actions">
        <span />
        <div className="wizard-actions-right">
          {step > 0 && (
            <button type="button" className="wizard-skip" onClick={() => setStep(step - 1)}>
              ← Back
            </button>
          )}
          <button type="button" className="wizard-skip" onClick={onSkip}>
            I’ll set it up myself
          </button>
          {step < 3 ? (
            <button
              type="button"
              className="wizard-go"
              // A malformed grid must not ride into settings; empty is fine (skip).
              disabled={step === 0 && gridState === 'bad'}
              onClick={() => setStep(step + 1)}
            >
              Next →
            </button>
          ) : (
            <button
              type="button"
              className="wizard-go"
              onClick={() => finish(['everything'], 'operate', [])}
            >
              Finish — everything on
            </button>
          )}
        </div>
      </div>
    </Dialog>
  )
}
