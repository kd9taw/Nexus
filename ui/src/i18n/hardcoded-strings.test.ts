// THE GUARD — a migrated surface cannot drift back to hardcoded English.
//
// Migrating 250 files is a long job, and every long job in this tree has the same failure: the
// migrated part decays while the rest is being done, because nothing stops the next feature
// from adding a plain `title="…"` next to a translated one. So this exists before the second
// batch, not after the last.
//
// IT COMPUTES. The detector PARSES each file with the TypeScript compiler (already a
// devDependency — no new one) and asks the syntax tree which strings reach an operator: JSX
// text, the handful of attributes that are read aloud or shown on hover, string literals used
// as JSX children, and the first argument of the calls that put prose on screen. It never
// greps for a phrase, and it never asserts a phrase is absent — the presence-matching CSS
// tests that let dead fixes ship twice are the reason that rule exists in this project.
//
// IT IS PROVEN TO FIRE ON EVERY RUN, not once by hand: `the detector fires` below parses a
// fixture written to break every rule and asserts it is caught, kind by kind. A guard that has
// only ever been green is a guard nobody has tested. (It was also driven red by hand against
// the real files while it was written — a `title="Stop"` added to SettingsStation.tsx produced
// `components/SettingsStation.tsx:114 attr:title "Stop"`.)
//
// ---------------------------------------------------------------------------------------
// ⚠️ SCOPE — WHAT THIS DOES NOT COVER, stated because a partial guard read as a total one is
// worse than none.
// ---------------------------------------------------------------------------------------
//
//   • It checks exactly the files in MIGRATED. Everything else — including the other 8,900
//     lines of SettingsPanel.tsx, which appears in PARTIAL for its keys alone — is
//     deliberately unchecked and still hardcoded English.
//   • The list only ever GROWS. Removing a file from it is how a surface silently un-migrates,
//     so removal needs the same scrutiny as the migration did.
//   • It cannot see prose that reaches the operator from Rust (~440 `format!` sites), from a
//     data registry (`features/registry.ts` labels, `settings/registry.ts` labels/keywords),
//     or from a string built with `+`. Those are phase-2 and phase-3 decisions.
//   • It cannot tell a WRONG translation from a right one, and it cannot tell that a key is
//     used in the right place. It only proves the English is not baked into the JSX.
//   • A prose string hidden behind a named constant (`const MSG = 'Saved'; <b>{MSG}</b>`)
//     passes. That is the deliberate escape hatch invariant TOKENS use (see STATION_EXAMPLES),
//     and it costs an author a conscious act.

import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import ts from 'typescript'
import { EN, type MessageKey } from './index'

/**
 * The migrated surfaces. ADD A FILE HERE THE MOMENT YOU MIGRATE IT — and not before, or CI
 * goes red on strings nobody has moved yet.
 *
 * The pilot is "Settings ▸ Station": the fieldset itself, the search box that is the way into
 * it, and the first-run banner that points at it — plus `RevealNudge.tsx`, which is here for
 * one reason: the Station surface has no emphasised prose, and the rich-text path needs to be
 * proven on a real shipped sentence rather than on a fixture.
 */
const MIGRATED = [
  'components/SettingsStation.tsx',
  'components/SettingsSearch.tsx',
  'components/OnboardingBanner.tsx',
  'components/RevealNudge.tsx',
  // Batch 1 (2026-08-18) — the getting-started guide and the app's own notices. Pure prose,
  // no radio state: the guide is documentation, the update banner and crash fallback are the
  // app talking about itself. Chosen as the first batch because GettingStartedGuide.tsx is
  // the densest markup surface in the tree, which is what proves the `<T>` marker path at
  // volume rather than on one shipped sentence.
  'components/GettingStartedGuide.tsx',
  'components/AssistanceNote.tsx',
  'components/UpdateBanner.tsx',
  'features/updateCheck.ts',
  'components/ErrorBoundary.tsx',
  'main.tsx',
  // Batch 2 (2026-08-18) — the logbook and QSO entry. The densest UNITS surface in the tree:
  // every callsign, RST, band, mode, frequency, park reference and ADIF field name in these
  // five files is invariant and stays in the code (LOG_EXAMPLES in Logbook.tsx and
  // LogEntry.tsx, PARK_PROGRAMS, and the Q-code/service labels beside them), while the prose
  // around them moves. It also carried nine of the tree's hand-rolled plurals, which is what
  // proves the `{{count}}` path on real shipped counts rather than on a fixture.
  'components/Logbook.tsx',
  'components/LogEntry.tsx',
  'components/LogConfirm.tsx',
  'components/StationCard.tsx',
  'components/StationList.tsx',
  // Batch 3 (2026-08-18) — awards, journey, stats and the needed board. Two things this
  // batch proves that the first two did not: the AWARD NAMES are invariant tokens exactly as
  // callsigns are (DXCC, WAZ, VUCC, IOTA name programmes an operator applies to — a
  // translated one names nothing), and a REGISTRY can be migrated without touching its
  // consumers. `needVisuals.ts` and `statusMeta.ts` are label tables a dozen surfaces index
  // directly; their words resolve through getters, so the record shape is unchanged and the
  // lookup happens when the string is read rather than when the module loads.
  'components/AwardsView.tsx',
  'components/AwardsJourney.tsx',
  'components/JourneyView.tsx',
  'components/NeededPanel.tsx',
  'components/StatsView.tsx',
  'features/needVisuals.ts',
  'statusMeta.ts',
  'useJourneyUnlocks.ts',
  // Batch 4 (2026-08-18) — the maps, the globes and the propagation panes. The WIDEST
  // batch (33 files) and the densest MEASUREMENT surface in the tree: grids, bearings,
  // distances, SFI/Kp/A/Bz indices, MUF in MHz, R/S/G scale letters and every band and
  // mode name on these screens is an invariant token and stays in the code (MUF_LABEL,
  // INDEX, PATH_SP/PATH_LP, ENGINE_P533, QRZ_LABEL and the RSG letters). It also draws the
  // line this phase does NOT cross: the prose these surfaces receive from the BACKEND —
  // workability words, band reasons, insight sentences, window headlines — is interpolated
  // as a value, never translated, and moves in phase 3.
  'components/MapView.tsx',
  'components/MapLegend.tsx',
  'components/Globe3D.tsx',
  'components/QsoGlobe.tsx',
  'propViz.ts',
  'openingAlert.ts',
  'components/DxpeditionsView.tsx',
  'features/dxpedChase.ts',
  'features/dxpedAlarm.ts',
  'features/chaseFeed.ts',
  'features/shareCard.ts',
  'components/prop/ActivityMatrix.tsx',
  'components/prop/BandAdvisor.tsx',
  'components/prop/BandConditionStrip.tsx',
  'components/prop/BeaconMonitor.tsx',
  'components/prop/BestBandTable.tsx',
  'components/prop/ChaseFeedPane.tsx',
  'components/prop/ChasePane.tsx',
  'components/prop/DxpedCalendar.tsx',
  'components/prop/DxpedDigest.tsx',
  'components/prop/DxpedMonth.tsx',
  'components/prop/dxpedLink.ts',
  'components/prop/GetoutCompass.tsx',
  'components/prop/GreylineWindow.tsx',
  'components/prop/InsightFeed.tsx',
  'components/prop/LikelihoodHeatmap.tsx',
  'components/prop/MapInsightRail.tsx',
  'components/prop/MeasuredMuf.tsx',
  'components/prop/OpeningStrip.tsx',
  'components/prop/OpeningsLogPane.tsx',
  'components/prop/ScalesAnnunciator.tsx',
  'components/prop/SpaceWxGauges.tsx',
  'components/prop/WorkNowCard.tsx',
  // Batch 5 (2026-08-18) — spots, the watch list, the display filters and the Settings
  // sections that configure them. The firehose is nearly ALL tokens: every callsign,
  // spotter, DXCC entity, US state, band, mode, submode, frequency and cluster comment on
  // these screens is data and stays in the code, as do the POTA/SOTA programme names, the
  // P/S/✈/B badge glyphs, `de`, `DXCC`, and the prefix/grid EXAMPLES the watch and hide
  // filters offer (WATCH_EXAMPLES, HIDE_EXAMPLES). It also proves the registry-by-getter
  // path a second time: `SpotLegend.tsx`'s two badge tables resolve their words when read,
  // so `BandStrip.tsx` — which this batch does not own — reads them unchanged.
  'components/SpotsPanel.tsx',
  'components/SpotDialog.tsx',
  'components/SpotLegend.tsx',
  'components/BandMap.tsx',
  'components/PounceBanner.tsx',
  'components/WatchlistPanel.tsx',
  'components/HideCallsPicker.tsx',
  'components/CountryExclude.tsx',
  'components/RoamPanel.tsx',
  'alerts.ts',
  // Batch 6 (2026-08-18) — POTA/SOTA, Field Day and the Settings ▸ Contesting sections that
  // set them up. The whole review here is the units rule, and it lands on the EXCHANGE: park
  // and summit references (K-1234, W7A/MN-001), ARRL/RAC section codes and their division
  // names, Field Day class and category strings (3A, 2O) with the H/I/M/O letters, the FD
  // mode codes (DIG/CW/PH), every score and multiplier, and the FD_BONUSES names — all of
  // them values an entry is submitted with, so all of them stay in the code. So do the
  // programme and event names themselves (POTA, SOTA, ARRL Field Day, Winter Field Day,
  // WFD): they name a thing an operator enters, exactly as an award name does, and a
  // translated one names nothing. What moved is the prose around them. Deliberately NOT
  // migrated and flagged in the file: the Summary and Dupe-sheet EXPORTS, which build a
  // fixed-width document rather than interface prose.
  'components/PotaSotaView.tsx',
  'components/FieldDayView.tsx',
  'components/ContestCalendarPane.tsx',
  'fdEvent.ts',
  // Batch 7 (2026-08-18) — the Satellites section, the Connect Passes pane and the nine
  // composers behind them. One 3,900-line planning surface plus nine small modules, and the
  // units rule lands on the SKY and the DIAL: bird names, NORAD ids, TLE epochs, every
  // uplink/downlink frequency and offset, the SatNOGS transponder descriptions with their
  // per-leg mode names, azimuths, elevations, ranges, altitudes and the compass letters all
  // stay in the code, as do the mode names the radio binding prints (MODE_FM/MODE_SSB), the
  // NORAD label and the passband plot's centre tick. Two things this batch proves the earlier
  // ones did not: an INSTRUMENT can have text that is not prose — the sky dome's `az 143°`
  // plates are sized from the string by viewBox arithmetic, so they are tick labels and are
  // deliberately not migrated — and a module-level LABEL TABLE with a wire value beside every
  // label (`satVfo.ts`, read by two components) migrates through getters exactly as
  // `needVisuals.ts` did, so Settings ▸ Radio reads it unchanged. It also carried the tree's
  // heaviest run of mid-sentence conditionals: the Doppler row's six states, the badge's
  // four, the readiness rail's uplink offers and the TX-sideband note are each ONE entry with
  // the variable clause interpolated.
  'components/SatellitesView.tsx',
  'features/satHealth.ts',
  'features/satLane.ts',
  'features/satVfo.ts',
  'features/satAlarm.ts',
  'features/issAutoArm.ts',
  'features/tleMessages.ts',
  'features/elementBands.ts',
  'features/satPassAlert.ts',
  'components/prop/SatPassesPane.tsx',
  // Batch 8 (2026-08-18) — memories, recall and radio programming. The densest UNITS surface
  // of the low-risk half: every string on these screens sits beside a dial frequency, a step,
  // a CTCSS tone, an offset or a band name, and all of those stay in the code — as do the
  // rig-model names in the Program section's "Max name" list (FT-60, Baofeng, Yaesu, Anytone
  // are tokens exactly as a callsign is), the mode and CTCSS datalists, and the `value` of
  // every <select>, whose LABEL moved while the stored token did not. Three things this batch
  // settles that the earlier seven did not. A string can be operator-visible AND legally
  // fixed: the two repeater directories' attribution lines are written verbatim into the
  // exported CSV as well as shown in the footer, so they are constants, not entries. A
  // FILE NAME must not be built from a translated word — the export slug is invariant now,
  // because the old ASCII squeeze would have reduced a non-Latin view name to
  // `nexus-memories-.csv`. And a name spliced into "Search …" was replaced by one whole
  // placeholder per view: lower-casing a translated noun is wrong wherever nouns capitalise.
  // The five hand-rolled plurals in MemoriesView became `{{count}}` entries; the two-count
  // reports (imported/skipped, added/refreshed, saved/already-there) are one entry per count.
  'components/MemoriesView.tsx',
  'components/RadioProgView.tsx',
  'components/MemoryStrip.tsx',
  'components/RecallPanel.tsx',
  'components/BandPicker.tsx',
  'components/BandStrip.tsx',
  'components/FrequencyControl.tsx',
  'components/RadioPicker.tsx',
  'components/RadioSwitcher.tsx',
  'rigFormChecks.ts',
  // Batch 13 (2026-08-19) — Settings ▸ Radio's audio half (Audio, Headphone monitor,
  // Satellite Doppler, Orbital elements, Rotator) and the SETUP WIZARD, migrated together
  // because the wizard's steps ARE those settings: two surfaces asking the same question in
  // two wordings is how the pair drifted before, and one catalog now holds both. The units
  // rule lands on the HARDWARE here — sound-card device names, sample rates, the dB and ×
  // gain readings, every baud rate, rotator model names and Hamlib model numbers, azimuth and
  // elevation degrees, Keplerian element sources and epochs, and the callsign/grid/address
  // EXAMPLES (WIZARD_EXAMPLES, ROTATOR_EXAMPLES) all stay in the code. Two things this batch
  // settles: a CONFIGURATION control on the transmit path is not a transmit control — Tx
  // Power's drive slider moved exactly as PTT Method did in batch 12, while `SetupHealth`'s
  // Prove TX, which really does key a carrier, did not (see PARTIAL below); and the wizard's
  // 14 mid-sentence conditionals are each ONE entry with the variable clause interpolated
  // whole, including the "swap them" sentence, whose BUTTON is supplied by the call site.
  'components/SetupWizard.tsx',
  // Batch 14 (2026-08-19) — the Connect board with its pane grid, the Tempo conversation,
  // and APRS. The densest CONCATENATION surface in the tree: `paneFormat.ts`'s Basic lines
  // and APRS's two health chips were sentences assembled from conditional fragments, and
  // every one of them is ONE catalog sentence now with the data interpolated — the
  // selection line's optional heading and band, the space-weather line's flare and blackout
  // (four whole sentences, not a stem plus two tails), the wrong-frequency verdict's three
  // wordings, the failed-checksum message with its headroom advice, and the APRS-IS feed's
  // four states with their iGate clause. Two things this batch settles. A REGISTRY OF PANES
  // migrates exactly as a registry of badges did (batch 3): `panes.tsx` resolves each pane's
  // name through a getter, so PaneFrame and the picker read it unchanged. And a frequency
  // inside a sentence is a bug the catalog guard already refuses: the 6 m Es calling
  // frequency and the North American APRS channel are constants in their components now,
  // interpolated into the message rather than written in it. The units rule lands on the
  // PACKET here — callsign-SSIDs, symbol codes, digipeater paths (WIDE1-1), positions,
  // grids, courses, speeds, altitudes, dBFS levels and every dial reading stay in the code,
  // as do the CQ line Tempo puts on the air, the operator's own macros, the Winter Field Day
  // chip, and the two intent chips named for a programme and a band (POTA/SOTA, 6m/VHF).
  'components/ConnectView.tsx',
  'components/connect/PaneFrame.tsx',
  'components/connect/panes.tsx',
  'components/connect/paneFormat.ts',
  'components/Conversation.tsx',
  'components/Composer.tsx',
  'components/MessageBubble.tsx',
  'components/FreetextMeter.tsx',
  'components/AprsStationCard.tsx',
  // Batch 15 (2026-08-19) — the shell: its chrome, the navigation rail, the ⊞ panel menu,
  // and the two registries whose words reach Settings and the wizard. The first batch whose
  // surface is what sits BETWEEN the cockpits, so it is where the vocabulary split is
  // widest: a section named for a MODE keeps its name in the code (CW, Phone, RTTY, PSK,
  // SSTV, APRS, FT, Tempo, TempoFast/TempoDeep), as do the ones named for an EVENT or a
  // PROGRAMME (Field Day, POTA / SOTA), the Q-code in the mode badge, the three feed names
  // the Now-Bar pills carry (Cluster, Phone, PSKR), and the link report's own field names
  // and unit symbols (RV, dT, MHz, Hz — LINK_TOKENS). Everything around them moved.
  //
  // Three things this batch settles that the fourteen before it did not. A REGISTRY OF
  // FEATURES migrates exactly as a registry of badges did (batch 3): `registry.ts` and
  // `profiles.ts` resolve each label, one-liner and blurb through getters, so SettingsPanel,
  // the wizard and RevealNudge read them unchanged — the single consumer line that did move
  // is the Features tab's category HEADING, which the batch-9 comment beside it said would
  // move when this registry did. A file whose SOURCE a test parses moves its guard with it:
  // `App.logtoast.test.ts` looked for the literal 'Logged QSO' inside the handler and now
  // follows the key, asserting exactly what it asserted before. And the Now-Bar's connector
  // pills were the batch's concatenation: six states each assembled from a stem, an optional
  // age clause and an optional host suffix, and each is ONE catalog sentence now — including
  // the two forms of "retrying", because "retrying" and "retrying, last event 4m ago" are
  // two statements and not a stem plus a tail.
  //
  // Nothing on a transmit path moved. App.tsx owns the halt_tx / TX-enable wiring and the
  // Esc bindings, PanelsMenu renders the ⊞ vocabulary the stop line is defined against, and
  // CockpitPaneFrame's header states the rule — but none of the three renders a stop
  // CONTROL: what moved here is menu entries, pane titles and the failure toasts those
  // handlers raise, no one of which any stop-line sweep can see. The Tempo header's drive
  // slider moved on the batch-13 ruling (a configuration control on the transmit path is not
  // a transmit control) and its ▲ TX indicator, a state token, did not move at all.
  'App.tsx',
  'DetachedPanel.tsx',
  'components/ModeNav.tsx',
  'components/NowBar.tsx',
  'components/TempoHeader.tsx',
  'components/PanelsMenu.tsx',
  'components/ThemeSwitcher.tsx',
  'components/PalettePicker.tsx',
  'components/Toasts.tsx',
  'components/Splitter.tsx',
  'components/SplitterSeam.tsx',
  'components/LinkPill.tsx',
  'components/panes/CockpitPaneFrame.tsx',
  'features/profiles.ts',
  'features/registry.ts',
  // Batch 18 (2026-08-19) — the Operate cockpit: its header, the waterfall strip, the two
  // decode panes, the Call Roster and the WSJT-X Tx1–Tx6 message machine. THE FIRST BATCH OF
  // THE TRANSMIT HALF, and the line it draws is the point: a cockpit is not a transmit
  // control, and neither is a pane that starts a transmission — the decode panes and the
  // roster start a QSO on a double-click and their strings moved normally, exactly as the ⊞
  // menu entry for a sender did in batch 15. What did NOT move is in `OperateQsoStrip.tsx`,
  // which is on PARTIAL below for four controls and nothing else.
  //
  // The units rule lands on the DIAL AND THE DECODE here: every callsign, grid, entity,
  // state, band, mode, tier, SNR/dB report, DT in seconds, audio offset in Hz, dial and split
  // frequency, bearing, distance, slot count and UTC stamp on these screens is data and stays
  // in the code — as do the vocabularies each file gathers as named constants (DECODE_TOKENS,
  // ROSTER_TOKENS, and OperateCockpit's HOUND/SPLIT/Native/Companion/Rx/Tx/Hz). Two things
  // this batch settles that the seventeen before it did not. A backend WORD an operator reads
  // beside a button of ours is a token, not prose: `radio.sourceLabel` is interpolated into
  // the signal-source tooltip, so `Native` and `Companion` on the buttons stay as written or
  // the button and the readout beside it would disagree. And an ARIA row label assembled from
  // optional appositives — the decode row's country and heading, the roster row's grid, need,
  // worked and working-now — is ONE entry per row with each clause interpolated whole,
  // carrying its own separator, rather than six variant sentences or a glued string.
  'components/OperateCockpit.tsx',
  'components/OperateDecodes.tsx',
  'components/OperateRoster.tsx',
  'components/TxPanel.tsx',
  // Batch 20 (2026-08-19) — the CW cockpit. ONE file of the transmit half that graduates
  // straight to MIGRATED rather than to PARTIAL, and that is the batch's finding: CW's
  // stop-line census is Stop TX (→ stopCw + haltTx), Tune and Esc, and NOT ONE OF THEM HAS
  // A STRING IN THIS FILE — the first two are drawn by `CockpitHeader` (deferred there,
  // with the RTTY/SSTV latch) and Esc is a window keydown with no element at all. So the
  // file has nothing to defer: the F-key macros and the send bar are SENDERS (the batch-18
  // ruling), and the keyer back-end, speed and pitch controls are CONFIGURATION on the
  // transmit path, which moves exactly as PTT Method and the drive slider did (batch 13).
  // `stop-line.test.tsx` was re-run for the CW case, which finds both controls by
  // accessible name in the header this file does not own.
  //
  // The units rule lands on the KEY AND THE DIAL: every WPM figure, sidetone pitch, filter
  // width, scope span and reference level, dial reading, callsign, RST and bearing stays in
  // the code, as does the vocabulary the file gathers as constants (CW, the WPM/dB/dBm
  // units, the rig's own DSP/NR/AGC/BW group names, CAT and WinKeyer — a protocol and a
  // product — and the SPLIT ▲/REC plates), the macro TEXTS with their {MYCALL}/{RST}/{NAME}/
  // {EXCH} tokens, the ± zoom presets and every <select> value.
  //
  // Two things this batch settles that the nineteen before it did not. A LABEL TABLE THAT
  // ANOTHER TEST PARSES OUT OF THE SOURCE moves its guard with it: `docs-match-code.test.ts`
  // reads DEFAULT_MACROS/DEFAULT_FD_MACROS out of this file and compares the captions with
  // the tables docs/manual/CW.md publishes, so the captions that are WORDS now carry a
  // `labelKey` and that guard resolves it through this catalog — asserting exactly what it
  // asserted before (proved by drifting one entry and watching both doc rows go red). And a
  // MEASUREMENT PLATE beside a translated one is computed, not written in the catalog: the
  // scope sub-plate's fed span in MHz is assembled in the component and the audio view's
  // word is the only entry, the same split the AI decoder's 400–1200 Hz window and the BW
  // nudge's ±50 Hz take inside their sentences.
  'components/CwCockpit.tsx',
  // Batch 22 (2026-08-19) — THE SHARED COCKPIT FURNITURE, and the last batch of the phase. The
  // instruments every cockpit hangs on the same frame: the dial readout, the tuning strip, the
  // rig scope and the FT wide graph, the RX and TX meters, the MSK144 Fast Graph, and the
  // rotator's strip and pane. None of these is a transmit control — they read, they draw, and
  // the two ■ buttons among them stop a MAST and a satellite track, never an over — so all
  // eight files graduate straight to MIGRATED. The two files that DO draw transmit controls,
  // `CockpitHeader.tsx` and `TopBar.tsx`, are on PARTIAL below and are the reason this batch
  // was ordered last.
  //
  // The units rule lands on the INSTRUMENT: the formatted dial and every digit's decade, the
  // tuning steps and RIT/XIT offsets, the scope's three window widths and its S-units, the
  // waterfall's spans and its `dBr` legend, every meter reading in SWR/percent/watts/dB, and
  // every azimuth in degrees are measurements and stay in the code — as do the vocabularies
  // each file now gathers as named constants (`dB`/`dBr`, `FLEX RF`/`CI-V RF`, the `DIAL`,
  // `SAT` and `ROTOR` plates, the four TX meter names, `RIT`/`XIT` and the VFO letters).
  //
  // Three things this batch settles that the twenty-one before it did not. TEXT DRAWN ON A
  // CANVAS is not automatically a tick label: the waterfall's frequency axis and its scrollback
  // time tape are measurements and stayed, while the ⏸ PAUSED chip beside them is a state
  // message and moved — the test is whether the string says something or measures something.
  // A CONTROL THAT STOPS SOMETHING THAT IS NOT A TRANSMISSION is an ordinary control: the Rotor
  // pane's ■ STOP and the strip's two ■ buttons halt the mast, the satellite track and a
  // transponder hold, no sweep looks for them, and their words moved with everything around
  // them. And a DEFAULT PROP VALUE that is prose resolves as a parameter default (`label =
  // t(…)`), which is evaluated on every render — a module constant would freeze the first
  // locale loaded, which is the same trap `RF_SPANS` documented in batch 20.
  'components/FrequencyReadout.tsx',
  'components/TuningStrip.tsx',
  'components/PhoneScope.tsx',
  'components/Waterfall.tsx',
  'components/FastGraph.tsx',
  'components/LevelMeter.tsx',
  'components/LiveMeters.tsx',
  'components/TxMeters.tsx',
  'components/RotorStrip.tsx',
  'components/prop/RotorPane.tsx',
]

/**
 * Files where ONE SECTION is migrated and the rest is not.
 *
 * They are scanned for KEYS (so the catalog checks below see the entries they use) but NOT
 * for hardcoded strings, because the un-migrated remainder of the file is still English by
 * design. `SettingsPanel.tsx` is 9,000 lines: its Spots & Alerts and Contesting sections were
 * migrated with the panels they configure, batch 9 (2026-08-19) took the SHELL — the panel
 * chrome, the tab rail, Save, the toasts/confirms its handlers raise — plus the whole
 * Appearance tab (Workspace + Features + Accessibility), batch 10 (2026-08-19) took the
 * Logging & Connectors sections down to the Confirmations fieldset: Connections, Worked-before
 * (B4) & dupes, Integrations & Feeds with its Antenna gain disclosure, DXKeeper, N3FJP, N1MM+,
 * the LoTW users list and the callsign→state database, and batch 11 (2026-08-19) took
 * Confirmations itself — LoTW, eQSL, QRZ, HamQTH, ClubLog, HRDLog, RepeaterBook and
 * Cloudlog/Wavelog. Batch 12 (2026-08-19) took the first three sections of the Radio tab —
 * the dual-radio roster with its band coverage and band+mode routing table, Profiles, and Rig
 * & CAT down to Test CAT, Advanced included — and batch 13 the rest of that tab's audio half:
 * Audio, Headphone monitor, Satellite Doppler, Orbital elements and Rotator. Batch 16
 * (2026-08-19) took the Digital tab: the FT8/FT4 section's Logging Behavior, Decoder and
 * Station Housekeeping sub-groups, and the six weak-signal mode sections after it (JT65,
 * MSK144, Beacons — WSPR & FST4W, FST4, Q65, Quick-reply macros). Batch 17 (2026-08-19) took
 * the rest of the per-mode surface: the whole Phone tab, the whole CW tab (keyer, keying
 * ports, CW ID and the F-key macro editor with its role table and its two profile prompts),
 * and the Digital tab's RTTY, PSK, SSTV, APRS and Working Frequencies sections. Batch 22
 * (2026-08-19) took Transmit limits & sharing — the band-edge tones, the per-mode power caps,
 * the setup backup and the CAT-broker sharing block with its foreign-PTT permission, all of
 * them transmit POLICY and station plumbing rather than a control that keys or stops an over.
 * Putting the file on MIGRATED would report the tabs still to come; leaving it off entirely
 * would make every key those sections use look like an orphan.
 *
 * ⚠️ AND THE DIGITAL TAB IS THE OTHER REASON THIS FILE CANNOT GRADUATE YET, on the same
 * ruling that keeps `SetupHealth.tsx` below: the FT8/FT4 section's first two sub-groups,
 * "Transmit & Sequencing" and "Auto-CQ & Caller Selection", are the FT-mode TX / timing /
 * QSO-management surface. Their labels, hints and accessible names stay written in the panel
 * until the transmit-path batch moves them with the stop-line sweeps re-run — WSJT-X parity
 * there is a compatibility contract, and it cannot be verified in CI.
 *
 * `SetupHealth.tsx` is here for a DIFFERENT and much narrower reason, and it is the only kind
 * that may be added: every string in it is migrated except one CONTROL — Prove TX, which keys
 * a bounded tune carrier. Transmit-path controls and their accessible names move in their own
 * batch, with the stop-line sweeps re-run, so the button's label, its tooltip and the consent
 * prompt in front of it stay as written and the file cannot be clean yet. It graduates to
 * MIGRATED the moment that batch lands; nothing else is deferred in it.
 *
 * `AprsCockpit.tsx` (batch 14, 2026-08-19) is here for exactly that reason and no other:
 * every string in it is migrated except the TX On/Off ARM LATCH — its label and its two
 * tooltips. APRS renders no stop control at all (the latch only holds the queue, so it is on
 * no cockpit's stop-line census), but the latch is still a transmit-path control, and those
 * move with the sweeps. It graduates the moment that batch lands.
 *
 * `OperateQsoStrip.tsx` (batch 18, 2026-08-19) is here on the same narrow ruling, for FOUR
 * controls: STOP TX and TUNE — Operate's stop-line census, and Stop TX is the only control in
 * this cockpit that cuts an over in flight; ATU, which keys the rig's own tuning carrier
 * exactly as SetupHealth's Prove TX does; and the TX On/Off TOOLTIP, the wording that states
 * the abort semantics ("an FT over already in flight finishes"). Their labels and tooltips
 * stay written in the strip until the transmit-path batch moves them with the stop-line
 * sweeps re-run. Everything else in that file is migrated — the sequencer roles, the state
 * cap, the rig-divergence and narrow-filter chips, the now-sending readout, the free-text
 * form, the transmit-cycle and Skip Tx1 controls and the next-slot countdown — and the
 * mode-capability prose those controls SHARE ("this mode is receive-only", the beacon note)
 * moved with them: it describes the MODE, not the control it is hung on. Operate's third
 * census holder, Esc, is a window keydown with no string at all.
 *
 * `RttyCockpit.tsx`, `PskCockpit.tsx` and `SstvView.tsx` (batch 19, 2026-08-19) are here on
 * the same narrow ruling, and this batch is where it is stated in full: THE BATCH MOVES NO
 * CONTROL THAT STOPS A TRANSMISSION, whether or not a sweep looks for it. These three share
 * one stop-line shape, which is why they were migrated together — one reviewer, one mental
 * model. What stayed written, per file:
 *
 *   · RTTY — the dock's Esc/Stop macro (both spans and its tooltip; `stop-line.test.tsx`
 *     finds it by accessible name, /^esc\s*stop$/i), the auto-sequencer's Abort (census,
 *     conditionally rendered so no sweep sees it), the `stream` pane's "Auto on" toggle
 *     (its off-click is seq.abort() + Engine::rtty_stop() — a REAL stop, pane-resident,
 *     which is exactly why it is on no sweep's list), and the continuous-TX latch.
 *   · PSK — the same Esc/Stop macro and the same continuous-TX latch. No sequencer.
 *   · SSTV — the transmit dock's Stop (census; found by accessible name, /^stop$/i), the
 *     bar's own aria-label, which names the controls it holds, and the sentence Stop
 *     announces when it fires.
 *   · RTTY and PSK both — the TX-on-air pill's tooltip, which is the wording that states
 *     what Stop TX does to an over in flight (the batch-18 ruling on Operate's TX On/Off
 *     tooltip, verbatim).
 *
 * The TX-ENABLE LATCH is on all three cockpits' censuses and is drawn by `CockpitHeader`,
 * so it is deferred there rather than in any of these files. What DID move is everything
 * around them, senders included — the F-key macros, both compose bars and their Send, the
 * auto-sequencer's CQ/Answer, SSTV's Send and its whole composer — on the batch-18 ruling
 * that a control which STARTS a transmission is not what "transmit control" means here. So
 * did the refusal TOASTS those deferred controls raise: a toast is not a control and no
 * sweep can see one. All three graduate the moment the transmit-path batch lands.
 *
 * `PhoneCockpit.tsx` and `VoiceKeyer.tsx` (batch 21, 2026-08-19) are here on that same narrow
 * ruling — THE BATCH MOVES NO CONTROL THAT STOPS A TRANSMISSION, whether or not a sweep looks
 * for it — and Phone is the densest transmit surface in the tree, so what stayed written is
 * worth naming exactly:
 *
 *   · Phone — THE PTT ROW, whole. The button's four labels ARE the accessible name
 *     `stop-line.test.tsx` matches (/push to talk|on air — release to stop|tx locked|tx off —
 *     click to enable/i) and its three-armed tooltip is that control's description, naming
 *     which switch is down and the mic the operator talks on. The LOCK toggle beside it stays
 *     with them because it decides whether the window's Space keyup is a PTT release at all —
 *     the census's fourth holder — and the Field Day exchange chip stays because it shares the
 *     row. Stop TX and Tune, Phone's other two census holders, are drawn by `CockpitHeader`
 *     and are deferred there with the RTTY/SSTV latch.
 *   · The voice keyer — its ■ Stop, which really does end an over (stopVoice →
 *     Engine::stop_voice flushes the output ring and unkeys) and is pane-resident, so it is
 *     deliberately on no sweep's list; and the ■ Stop & save beside it, which ends the
 *     RECORDER rather than a transmission but shares that vocabulary, so the pair is reworded
 *     in one batch instead of drifting apart.
 *
 * What DID move includes everything those controls are surrounded by, the refusal TOASTS the
 * PTT handler raises included (the batch-19 ruling: a toast is not a control and no sweep can
 * see one), and the whole voice keyer besides its two buttons — a pane that STARTS a
 * transmission is not a transmit control (batch 18). Both files graduate the moment the
 * transmit-path batch lands. Deliberately NOT migrated and flagged in PhoneCockpit: the two ⊞
 * menu notes for the voice keyer (VOICE_KEYER_STOPS_ON_HIDE / VOICE_KEYER_UNDO_ENDS), because
 * the other four notes on that same menu live in `features/panelHost.ts`, `waterfall.ts` and
 * `TxMeters.tsx`, which this batch does not own — moving two of five would leave one menu
 * speaking two languages. They are named constants, so the guard cannot see them either way.
 *
 * `CockpitHeader.tsx` and `TopBar.tsx` (batch 22, 2026-08-19) are here on that same ruling, and
 * they are WHERE THE STOP LINE ACTUALLY RENDERS — which is why that batch was ordered last:
 *
 *   · The cockpit header draws FOUR transmit controls for SIX cockpits at once (Phone, CW,
 *     RTTY, PSK, SSTV and Operate), so one wrong word there is a six-cockpit regression that
 *     the name backstop — exact-word on vocabulary ids — cannot see. STOP TX (the `halt_tx` on
 *     every one of those censuses), TUNE (Phone, CW and Operate), the TX-ENABLE LATCH (a stop
 *     control in RTTY and SSTV, and the reason its branch keys on the slot flag rather than the
 *     arbiter) and ATU, which keys the rig's own tuning carrier exactly as SetupHealth's Prove
 *     TX does. Three of the four are matched by accessible name in `stop-line.test.tsx`
 *     (/^stop tx$/i, /^tune$|^tuning…$/i, /^▼ tx on$|^■ tx off$/i). The TX/RX pill's three
 *     plates stay with them: the pill is the PASSIVE rendering of that same latch, and the two
 *     must never read as different controls.
 *   · The top bar keeps its TX CLUSTER: TX On/Off, whose two tooltip arms state the abort
 *     semantics ("an FT over already in flight finishes" — the batch-18 ruling), TUNE, STOP TX,
 *     and the TX WATCHDOG chip, which is what the watchdog says once it has halted an over.
 *
 * Everything around them moved — the wheel-tune tooltips and the band-edge toast, the power
 * slider (batch 13: a configuration control on the transmit path is not a transmit control),
 * the CAT pill, the tier pills, the operator/Help/Field chips, the RX meter, the clock and
 * sync readouts, Hold Tx and the whole transmit-cycle group — and so did the mode-capability
 * prose the deferred controls SHARE ("this mode is receive-only"), which describes the MODE
 * and not the control it is hung on. Both files graduate the moment the transmit-path batch
 * lands.
 *
 * ⚠️ THIS LIST IS A CONCESSION, NOT A HOME. A file belongs here only while a migration is
 * partial; when the last section moves it graduates to MIGRATED, and nothing else may be
 * added to it to dodge a failing check.
 */
const PARTIAL = [
  'components/SettingsPanel.tsx',
  'components/CockpitHeader.tsx',
  'components/TopBar.tsx',
  'components/SetupHealth.tsx',
  'components/AprsCockpit.tsx',
  'components/OperateQsoStrip.tsx',
  'components/RttyCockpit.tsx',
  'components/PskCockpit.tsx',
  'components/SstvView.tsx',
  'components/PhoneCockpit.tsx',
  'components/VoiceKeyer.tsx',
]

/** Attributes whose value a human reads — on hover, or through a screen reader. */
const VISIBLE_ATTRS = new Set([
  'title',
  'placeholder',
  'alt',
  'label',
  'aria-label',
  'aria-description',
  'aria-placeholder',
  'aria-roledescription',
  'aria-valuetext',
])

/** Calls whose first argument becomes visible prose. */
const PROSE_CALLS = new Set(['pushToast', 'withErrorToast', 'confirm', 'alert', 'confirmDialog'])

interface Finding {
  file: string
  line: number
  kind: string
  text: string
}

/** Two or more letters in a row — the difference between prose and `✕`, `×`, `—`, `{' '}`. */
const isProse = (s: string) => /\p{L}{2,}/u.test(s)

/** The literal text an expression contributes, or null when it is computed at runtime. */
function literalText(sf: ts.SourceFile, n: ts.Node | undefined): string | null {
  if (!n) return null
  if (ts.isStringLiteral(n) || ts.isNoSubstitutionTemplateLiteral(n)) return n.text
  if (ts.isTemplateExpression(n)) {
    return [n.head.text, ...n.templateSpans.map((s) => s.literal.text)].join(' ')
  }
  if (ts.isJsxExpression(n)) return literalText(sf, n.expression)
  if (ts.isParenthesizedExpression(n)) return literalText(sf, n.expression)
  if (ts.isConditionalExpression(n)) {
    // `title={ok ? 'Receiving audio' : 'No RX audio'}` — both arms are prose.
    const parts = [literalText(sf, n.whenTrue), literalText(sf, n.whenFalse)].filter(Boolean)
    return parts.length ? parts.join(' ') : null
  }
  return null
}

/** Every user-visible string literal in one source file. */
export function findHardcoded(file: string, src: string): Finding[] {
  const sf = ts.createSourceFile(file, src, ts.ScriptTarget.ES2020, true, ts.ScriptKind.TSX)
  const out: Finding[] = []
  const line = (n: ts.Node) => sf.getLineAndCharacterOfPosition(n.getStart(sf)).line + 1
  const add = (n: ts.Node, kind: string, text: string) =>
    out.push({ file, line: line(n), kind, text: text.trim().replace(/\s+/g, ' ').slice(0, 60) })

  const visit = (n: ts.Node): void => {
    if (ts.isJsxText(n)) {
      if (isProse(n.text)) add(n, 'jsx-text', n.text)
    } else if (ts.isJsxAttribute(n)) {
      const name = n.name.getText(sf)
      if (VISIBLE_ATTRS.has(name)) {
        const text = literalText(sf, n.initializer)
        if (text && isProse(text)) add(n, `attr:${name}`, text)
      }
    } else if (
      ts.isJsxExpression(n) &&
      n.parent &&
      (ts.isJsxElement(n.parent) || ts.isJsxFragment(n.parent))
    ) {
      // A literal used as a JSX child: `{'Save'}`, `{`Work ${call}`}`.
      const text = literalText(sf, n.expression)
      if (text && isProse(text)) add(n, 'jsx-child', text)
    } else if (ts.isCallExpression(n)) {
      const callee = ts.isPropertyAccessExpression(n.expression)
        ? n.expression.name.text
        : ts.isIdentifier(n.expression)
          ? n.expression.text
          : ''
      if (PROSE_CALLS.has(callee)) {
        const text = literalText(sf, n.arguments[0])
        if (text && isProse(text)) add(n, `call:${callee}`, text)
      }
    }
    n.forEachChild(visit)
  }
  visit(sf)
  return out
}

/** Every catalog key a file names — `t('…')` and `<T k="…">`. */
export function findKeys(file: string, src: string): string[] {
  const sf = ts.createSourceFile(file, src, ts.ScriptTarget.ES2020, true, ts.ScriptKind.TSX)
  const out: string[] = []
  const visit = (n: ts.Node): void => {
    if (ts.isCallExpression(n) && ts.isIdentifier(n.expression) && n.expression.text === 't') {
      const a = n.arguments[0]
      if (a && ts.isStringLiteral(a)) out.push(a.text)
    } else if (ts.isJsxAttribute(n) && n.name.getText(sf) === 'k') {
      const v = n.initializer
      if (v && ts.isStringLiteral(v)) out.push(v.text)
    } else if (ts.isPropertyAssignment(n)) {
      // Key tables: `{ labelKey: 'settings.station.callsign.label' }`.
      const name = n.name.getText(sf)
      if (/Key$/.test(name) && ts.isStringLiteral(n.initializer)) out.push(n.initializer.text)
      if (name === 'prose' && ts.isStringLiteral(n.initializer)) out.push(n.initializer.text)
    }
    n.forEachChild(visit)
  }
  visit(sf)
  return out
}

const read = (rel: string) => readFileSync(fileURLToPath(new URL(`../${rel}`, import.meta.url)), 'utf8')

// ── the guard fires (positive control) ────────────────────────────────────────────────

const VIOLATIONS = `
import { pushToast } from '../toast'
export function Bad({ ok, call }: { ok: boolean; call: string }) {
  return (
    <div title="Stop transmitting" aria-label="Dismiss alert">
      Hardcoded prose here
      {'Also hardcoded'}
      <input placeholder={\`QSY to \${call}\`} />
      <button title={ok ? 'Receiving audio' : 'No RX audio'} onClick={() => pushToast('Could not save')} />
      <span aria-hidden>✕</span>
      {' '}
    </div>
  )
}
`

const CLEAN = `
import { t } from '../i18n'
import { T } from '../i18n/T'
export function Good({ call }: { call: string }) {
  return (
    <div title={t('common.dismiss')} aria-label={t('settings.search.label')}>
      <T k="reveal.prompt" tags={{ b: <strong /> }} vals={{ achievement: call, feature: call }} />
      <input placeholder={t('settings.search.placeholder')} type="search" role="combobox" />
      <span aria-hidden>✕</span>{' '}
      <em>{t('reveal.enable')}</em>
    </div>
  )
}
`

describe('the detector fires', () => {
  const found = findHardcoded('fixture.tsx', VIOLATIONS)
  const kinds = found.map((f) => f.kind).sort()

  it('catches every kind of hardcoded operator-visible string', () => {
    expect(kinds).toEqual([
      'attr:aria-label',
      'attr:placeholder',
      'attr:title',
      'attr:title',
      'call:pushToast',
      'jsx-child',
      'jsx-text',
    ])
  })

  it('reports where, so the failure is actionable', () => {
    const text = found.find((f) => f.kind === 'jsx-text')
    expect(text?.text).toBe('Hardcoded prose here')
    expect(text?.line).toBeGreaterThan(0)
  })

  it('does NOT fire on glyphs, spacers or non-visible attributes — the other direction', () => {
    expect(findHardcoded('fixture.tsx', CLEAN)).toEqual([])
  })
})

// ── the migrated surfaces are clean (the guard proper) ────────────────────────────────

describe('migrated surfaces carry no hardcoded operator-visible strings', () => {
  it('has a non-empty scope — a guard over nothing proves nothing', () => {
    expect(MIGRATED.length).toBeGreaterThan(0)
    for (const rel of [...MIGRATED, ...PARTIAL])
      expect(read(rel).length, `${rel} is readable`).toBeGreaterThan(0)
  })

  it('never checks a PARTIAL file for hardcoded strings — that is the whole concession', () => {
    for (const rel of PARTIAL) expect(MIGRATED).not.toContain(rel)
  })

  for (const rel of MIGRATED) {
    it(`${rel}`, () => {
      const found = findHardcoded(rel, read(rel))
      expect(
        found.map((f) => `${f.file}:${f.line} ${f.kind} "${f.text}"`),
        'this file is on the migrated list — move the string into ui/src/i18n/en.ts and ' +
          'call t() / <T>, or, if it is a technical token (callsign, grid, frequency, mode, ' +
          'ADIF field), name it as a constant and say so',
      ).toEqual([])
    })
  }
})

// ── keys and catalog agree ────────────────────────────────────────────────────────────

describe('every key resolves, and every entry is used', () => {
  const used = new Set([...MIGRATED, ...PARTIAL].flatMap((rel) => findKeys(rel, read(rel))))

  it('finds keys at all — the extractor is not silently returning nothing', () => {
    expect(used.size).toBeGreaterThan(10)
  })

  it('names no key the English catalog lacks', () => {
    const missing = [...used].filter((k) => !(k in EN))
    expect(missing, 'a t()/<T> key with no catalog entry renders the key itself').toEqual([])
  })

  it('leaves no orphan entry behind', () => {
    const orphans = (Object.keys(EN) as MessageKey[]).filter((k) => !used.has(k))
    expect(
      orphans,
      'catalog entries nothing references — every one of these would be handed to a ' +
        'translator to translate for nobody',
    ).toEqual([])
  })
})
