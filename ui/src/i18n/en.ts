// THE ENGLISH CATALOG — the source of truth for every migrated user-visible string.
//
// English is not "one of the languages": it is the SOURCE. Every other locale is a partial
// overlay on this file, and anything a locale is missing resolves back to the entry here (see
// `rawMessage` in ./index.ts). That makes this file the only catalog that must be complete —
// and it is complete by TYPE, not by discipline: `MessageKey` is `keyof typeof EN`, so a call
// site naming a key this file does not have is a compile error, never a runtime hole.
//
// ---------------------------------------------------------------------------------------
// KEY NAMING — read before adding one. Keys are a public contract with the translations.
// ---------------------------------------------------------------------------------------
//
//   <area>.<surface>.<element>[.<variant>]     lowerCamel segments, ASCII, dot-separated
//
//   settings.station.callsign.label            a field label
//   settings.station.callsign.hint             the hint under it
//   settings.search.empty                      a whole-sentence message
//   reveal.enable                              a button
//
// ⚠️ A KEY NAMES A MEANING, NOT A FILE, A COMPONENT OR A WORDING. That is what makes it stable
// under refactor, which is the property that matters: the component this string lives in will
// be renamed, split, or moved into a pane (this codebase has re-organised Settings four times),
// and every rename that reached the keys would orphan every translation of them. So:
//
//   • Never encode a component name (`SettingsPanel.*`), a file path, or a tab (`tab3.*`).
//     `settings.station.*` survives the section moving to another tab — its `<area>` is the
//     operator-facing surface, which is also how `settings/registry.ts` names sections.
//   • Never encode the English text (`settings.station.yourStationCallsignRequired`). Reword
//     the English in place; the key does not move and no translation is orphaned.
//   • Rewording a string in a way that changes its MEANING is a NEW key. Translations of the
//     old meaning are wrong for the new one, and silently keeping them is worse than falling
//     back to English. Add the new key, delete the old, let the other catalogs go stale.
//   • Never build a sentence by concatenating keys. Word order is not universal; a sentence
//     assembled from three keys cannot be translated. One sentence, one key — use
//     interpolation for the variable parts and markup markers for the emphasised ones.
//
// ---------------------------------------------------------------------------------------
// INTERPOLATION — `{{name}}`, double braces, and the reason is in this very file.
// ---------------------------------------------------------------------------------------
//
// Nexus prose is full of `{NAME}`, `{MYSTATE}`, `{CALL}` — the CW/macro tokens the expander
// matches LITERALLY, printed inside operator-facing hints (see `station.opName.hint` below).
// A single-brace interpolation syntax would eat them. Double braces cannot collide: no macro
// token is doubled. `{{count}}` additionally selects the plural form.
//
// ---------------------------------------------------------------------------------------
// PLURALS — an entry may be `{ one: '…', other: '…' }` instead of a string.
// ---------------------------------------------------------------------------------------
//
// Never hand-roll `n === 1 ? '' : 's'` in a migrated component (the tree has ~20 of those
// today): English has two forms, Polish four, Arabic six, and the ternary is unrepresentable
// in most of them. Pass `{{count}}` and let `Intl.PluralRules` pick.
//
// ---------------------------------------------------------------------------------------
// MARKUP — markers, not HTML. `<b>bold bit</b>` inside a message.
// ---------------------------------------------------------------------------------------
//
// Many hints here emphasise a word mid-sentence, and the naive fixes are both wrong: splitting
// the sentence into three keys is untranslatable (word order), and injecting HTML is an
// injection surface. Instead the message carries NAMED MARKERS and the call site supplies the
// element for each name:
//
//   catalog:   'reveal.prompt': '<b>{{achievement}}</b> — turn on <b>{{feature}}</b>?'
//   call site: <T k="reveal.prompt" tags={{ b: <strong /> }} vals={{ … }} />
//
// The renderer only substitutes marker names the call site passed; anything else stays literal
// text. A catalog cannot introduce an element, an attribute or an event handler, so a machine
// translation is not a script-injection vector. Markers are parsed BEFORE interpolation, so a
// value that happens to contain `<b>` is text, never markup.
//
// ---------------------------------------------------------------------------------------
// ⚠️ WHAT MUST NEVER ENTER THIS FILE — the invariant-token rule. Full statement in ./index.ts.
// ---------------------------------------------------------------------------------------
//
// This is a radio application. Frequencies, signal reports, grid squares, callsigns, band and
// mode names, Q-codes, ADIF field names, macro tokens and the `value` of a <select> are
// TECHNICAL TOKENS, not prose: 14.074 MHz is 14.074 MHz in every locale, and a decimal comma
// in a frequency is an operating hazard, not a cosmetic one. They never become catalog
// entries and they are never produced by a locale-aware formatter. Example values that are
// tokens (`KD9TAW`, `EN52xa`, `WI`) live as named constants in the component; example values
// that are HUMAN PROSE (a first name, "leave blank if that is you") are entries here.
//
// ---------------------------------------------------------------------------------------
// SCALE — how this file grows. Read before adding the 200th entry.
// ---------------------------------------------------------------------------------------
//
// One flat object per locale, one file, while a locale fits in one screenful of scrolling.
// When an `<area>` passes ~200 entries, split it to `./en/<area>.ts` and spread it back in
// here — the key strings do not change, so nothing else moves. Do NOT split by component:
// components move, areas do not. Locales are STATIC imports, never fetched: a ham application
// works with no network, and a catalog that arrives over HTTP is a catalog that is missing in
// a field-day tent.

import type { Message } from './types'

export const EN = {
  // ── Settings ▸ Station ──────────────────────────────────────────────────────────────
  // The pilot surface. Placeholders that are technical tokens (`KD9TAW`, `EN52xa`, `WI`) are
  // deliberately ABSENT — they live in `components/SettingsStation.tsx` as invariants.
  'settings.station.legend': 'Operator & Radio',

  'settings.station.callsign.label': 'Callsign',
  'settings.station.callsign.hint': 'Your station callsign (required).',

  'settings.station.grid.label': 'Grid',
  'settings.station.grid.hint':
    'Maidenhead locator. All 6 characters — 4 measures every distance and bearing from the middle of a ~100-mile square.',

  'settings.station.opName.label': 'Operator name',
  // Prose, not a token: a locale should offer a first name its operators recognise.
  'settings.station.opName.placeholder': 'Seth',
  // ⚠️ `{NAME}` is a CW macro token matched literally by the expander — it must survive
  // translation unchanged. It is safe here only because interpolation is `{{double}}`.
  'settings.station.opName.hint': 'Used by the CW {NAME} macro and logging.',

  'settings.station.fdOperator.label': 'Operator at the key',
  'settings.station.fdOperator.placeholder': 'leave blank if that is you',
  // ⚠️ `OPERATOR` is the ADIF field name — a wire identifier. Keep it verbatim.
  'settings.station.fdOperator.hint':
    'Only for multi-operator: the callsign of whoever is operating, when that is not the station call. Stamped on every contact you log (ADIF OPERATOR) so a shared activation can be split per operator — POTA and Field Day both want each operator to submit their own. Blank means single-op and nothing is stamped. Change it when you swap seats.',

  'settings.station.opState.label': 'State',
  // ⚠️ `{MYSTATE}` — CW macro token, as above.
  'settings.station.opState.hint':
    'Your US state/province — the CW {MYSTATE} macro (ragchew QTH).',

  'settings.station.licenseClass.label': 'License Class',
  // The <option> VALUES ('technician' … 'open') are persisted tokens and stay in the code.
  // These are only what the operator reads. Keep the US class names recognisable.
  'settings.station.licenseClass.technician': 'Technician (US)',
  'settings.station.licenseClass.general': 'General (US)',
  'settings.station.licenseClass.extra': 'Amateur Extra (US)',
  'settings.station.licenseClass.open': 'Open — no transmit limits',
  'settings.station.licenseClass.hint':
    'Sets your transmit privileges + the licensed-segment band dropdown. Open = no limits (outside the US).',

  'settings.station.frequency.label': 'Band & Frequency',
  'settings.station.frequency.hint':
    'Pick a band-plan channel, or type a dial frequency in MHz.',

  // ── Settings ▸ search box ───────────────────────────────────────────────────────────
  'settings.search.placeholder': 'Find a setting…',
  'settings.search.label': 'Find a setting',
  // ⚠️ `{{query}}` is what the operator typed — inserted verbatim, never formatted. The three
  // example words must be words that appear in THIS locale's `settings/registry.ts` keywords,
  // or the advice sends the operator to a search that finds nothing.
  'settings.search.empty':
    'Nothing matches “{{query}}”. Try the words on the control — “sound card”, “COM port”, “WPM”.',
  // `{{term}}` is a registry keyword — data, not prose. Never translated at this call site.
  'settings.search.matched': 'matched “{{term}}”',

  // ── First-run nudge ─────────────────────────────────────────────────────────────────
  // Stored as plain text with a real `&`. The panel's JSX wrote `&amp;`; React escapes text
  // children itself, so an entity in a catalog would render as the literal characters `&amp;`.
  'onboarding.setStation': 'Set your callsign & station in Settings →',

  // ── Adaptive-reveal nudge ───────────────────────────────────────────────────────────
  // The rich-text case: emphasis around two interpolated values, inside one sentence whose
  // word order a translator must be free to change.
  'reveal.prompt': '<b>{{achievement}}</b> — turn on <b>{{feature}}</b>?',
  'reveal.enable': 'Enable',
  'reveal.notNow': 'Not now',

  // ── Getting started guide (Help ▸ Getting started) ──────────────────────────────────
  // The densest prose surface in the app, and the reason the markup-marker path exists:
  // nearly every sentence here emphasises a control name mid-sentence. Each `<b>`/`<em>`/
  // `<code>` below is a MARKER — the element comes from the call site.
  //
  // ⚠️ Technical tokens appear INSIDE these sentences and must survive translation verbatim,
  // exactly as `ADIF OPERATOR` does in `settings.station.fdOperator.hint`: band names
  // (40 m, 20 m, 60 m), mode names (FT8, CW), the CW macro token {NAME}, file names
  // (log.adi, wsjtx_log.adi, .adi/.adif), program and product names (rigctld, Hamlib,
  // WSJT-X, JTAlert, DAX), the network address 127.0.0.1:5002, the shell command
  // `brew install hamlib`, and key names (Esc, F4, F6, Alt+1–6). What is NOT here at all:
  // frequencies and the example values the recreated wizard panels display — those live in
  // `components/GettingStartedGuide.tsx` as named constants (GUIDE_EXAMPLES), because a
  // decimal comma reaching a dial reading is an operating fault, not a wording choice.
  'gettingStarted.title': 'Getting started',
  'gettingStarted.crumb.path': 'Help ▸ Getting started',
  'gettingStarted.crumb.note':
    'Opened from the Help menu · also offered at the end of the setup wizard',
  'gettingStarted.rail.label': 'Guide steps',
  'gettingStarted.rail.head': 'The order matters',
  // Both numbers are invariant (`invariantNumber`), so no locale can group or re-point them.
  'gettingStarted.progress': 'Step {{step}} of {{total}}',
  'gettingStarted.shot.label': 'In Nexus',
  'gettingStarted.back': '← Back',
  'gettingStarted.close': 'That’s all four — close',

  'gettingStarted.rail.station.label': 'Callsign & grid',
  'gettingStarted.rail.station.blurb': 'Who and where you are',
  'gettingStarted.rail.radio.label': 'Your radio',
  'gettingStarted.rail.radio.blurb': 'Detect, pair audio, Test CAT',
  'gettingStarted.rail.license.label': 'License class',
  'gettingStarted.rail.license.blurb': 'A real transmit lockout',
  'gettingStarted.rail.log.label': 'Your ADIF log',
  'gettingStarted.rail.log.blurb': 'Where the magic happens',

  'gettingStarted.next.station': 'Next — set up the radio →',
  'gettingStarted.next.radio': 'Next — your license →',
  'gettingStarted.next.license': 'Next — the log (the important one) →',
  'gettingStarted.next.log': "That's all four",

  // The REAL wizard's step titles, shown in the dot row of each recreated panel.
  'gettingStarted.wizard.station': 'Your station',
  'gettingStarted.wizard.rig': 'Your rig',
  'gettingStarted.wizard.log': 'Your log',
  'gettingStarted.wizard.finish': 'Finish',

  // Step 1 — callsign & grid.
  'gettingStarted.station.aria': 'Step 1 of 4: Callsign and grid',
  'gettingStarted.station.eyebrow': 'Step 1 of 4 · Your station',
  'gettingStarted.station.heading': 'Who’s on the air?',
  'gettingStarted.station.lede':
    'Your callsign and your grid square. Everything location-shaped in Nexus — the propagation map, satellite passes, distances, bearings, DXpedition windows, VHF opening locality — is computed from these two fields.',
  'gettingStarted.station.where':
    'Open the first-run wizard (it appears on first launch) or <b>Settings ▸ Station</b> at any time.',
  'gettingStarted.station.callsign':
    '<b>Callsign</b> — stored uppercase. Nexus will not start PSK Reporter or the RBN/cluster feed until a valid one is set: 3–10 characters, at least one letter and one digit.',
  'gettingStarted.station.grid': '<b>Grid square</b> — your Maidenhead locator. QRZ shows yours.',
  // ⚠️ `{NAME}` is a CW macro token the expander matches literally. Safe only because
  // interpolation is `{{double}}`.
  'gettingStarted.station.name':
    '<b>Name</b> — optional, in Settings ▸ Station. Feeds the CW <code>{NAME}</code> macro and logbook autofill.',
  'gettingStarted.station.callout.label': 'Give all six characters',
  // `{{full}}` / `{{short}}` are GRID SQUARES supplied by the call site — never translated.
  'gettingStarted.station.callout.body':
    'Four characters pins you to the middle of a ~100-mile square, and every distance and bearing you will ever read is measured from that point. <code>{{full}}</code> beats <code>{{short}}</code>. A malformed locator is refused outright — the wizard will not let you past it.',
  'gettingStarted.station.shot.caption':
    'Setup wizard ▸ step 1 · re-run any time from Settings ▸ Appearance ▸ Features',
  'gettingStarted.station.shot.title': 'Who’s on the air?',
  'gettingStarted.station.shot.sub':
    'Your grid square is the anchor for everything location-based — satellite passes, propagation, the map, and DXpedition windows are all computed from it.',
  'gettingStarted.station.shot.callsignLabel': 'Callsign',
  'gettingStarted.station.shot.gridLabel': 'Grid square',
  'gettingStarted.station.shot.gridHint':
    'Maidenhead locator (qrz.com shows yours). Give all 6 — 4 characters pins you to the middle of a ~100-mile square.',
  'gettingStarted.station.shot.skip': 'I’ll set it up myself',
  'gettingStarted.station.shot.next': 'Next →',

  // Step 2 — the radio.
  'gettingStarted.radio.aria': 'Step 2 of 4: Your radio',
  'gettingStarted.radio.eyebrow': 'Step 2 of 4 · Your rig',
  'gettingStarted.radio.heading': 'How does the radio connect?',
  'gettingStarted.radio.lede':
    'One button does the archaeology. Nexus reads the USB descriptors, matches the rig model, pairs the audio CODEC, and looks for FlexRadios on your network — in a single scan.',
  'gettingStarted.radio.where':
    'In the wizard, or in <b>Settings ▸ Radio ▸ Rig & CAT</b>, click <b>Detect my radio</b>.',
  'gettingStarted.radio.pickRow':
    '<b>Pick the row that is your radio.</b> It fills the port, the Hamlib model and both audio devices at once. On a dual-UART Icom two rows describe the same rig — take the one tagged <em>CI-V port — use this one</em>.',
  'gettingStarted.radio.genericCable':
    '<b>Generic cable?</b> A CH340 reporting only “USB Serial” fills the port and audio but leaves the model blank — choose it from the dropdown.',
  'gettingStarted.radio.flex':
    '<b>FlexRadio</b> is found on the network and configured the WSJT-X-proven way: CAT through the SmartSDR CAT app at <code>127.0.0.1:5002</code>, audio through DAX.',
  'gettingStarted.radio.testCat':
    '<b>Then press Test CAT.</b> Nexus saves what you entered, starts <code>rigctld</code> for the radio, and reports the dial frequency it read back — or the specific error. Other programs share the radio through the address in Settings ▸ Radio ▸ <em>Transmit limits & sharing</em>.',
  'gettingStarted.radio.callout.label': 'Nothing found?',
  'gettingStarted.radio.callout.body':
    'USB: plug it in and power it on. Flex: it has to be on this network. Either way you can skip and set it up later — the wizard never blocks on hardware. Set <b>Tx\u00a0Level</b> (default 0.9) down until your rig’s ALC reads zero before you transmit.',
  // Two whole captions, not a stem plus two tails: "ships inside the installer" is true on
  // Windows/Linux only, and a sentence assembled from fragments cannot be re-ordered.
  'gettingStarted.radio.shot.caption': 'Setup wizard ▸ step 2 · Hamlib ships inside the installer',
  'gettingStarted.radio.shot.captionMac':
    'Setup wizard ▸ step 2 · CAT needs Homebrew Hamlib — in Terminal: brew install hamlib',
  'gettingStarted.radio.shot.title': 'How does the radio connect?',
  'gettingStarted.radio.shot.sub':
    'One detect finds everything — USB rigs and FlexRadios on the network. Skippable; Settings ▸ Radio ▸ Rig & CAT has all of this later.',
  'gettingStarted.radio.shot.detect': '🔍 Detect my radio',
  // `{{rig}}` / `{{port}}` / `{{chip}}` are hardware identifiers from the call site.
  'gettingStarted.radio.shot.row': '<b>{{rig}}</b> on {{port}}',
  'gettingStarted.radio.shot.rowCiv': '· {{chip}} · CI-V port — use this one',
  'gettingStarted.radio.shot.rowSecond': '· {{chip}} · second port, not CI-V',
  'gettingStarted.radio.shot.selected': 'Selected: {{rig}} on {{port}}',
  'gettingStarted.radio.shot.usbLabel': 'USB / Serial',
  'gettingStarted.radio.shot.usbBlurb': 'Most rigs — one cable',
  'gettingStarted.radio.shot.netLabel': 'Network',
  'gettingStarted.radio.shot.netBlurb': 'FlexRadio / remote rigctld',
  'gettingStarted.radio.shot.audioIn': 'Audio in',
  'gettingStarted.radio.shot.audioOut': 'Audio out',
  'gettingStarted.radio.shot.testCatBtn': '⚡ Test CAT',
  // ⚠️ `{{dial}}` is a FREQUENCY READING. It arrives already formatted and invariant.
  'gettingStarted.radio.shot.testCatResult': '✓ Radio answered on {{port}} — dial reads {{dial}}',

  // Step 3 — license class.
  'gettingStarted.license.aria': 'Step 3 of 4: License class',
  'gettingStarted.license.eyebrow': 'Step 3 of 4 · Your license',
  'gettingStarted.license.heading': 'Declare your class, get a real lockout',
  'gettingStarted.license.lede':
    'This one field becomes a software guard in <em>every</em> transmit path in the app. Nexus parks the dial in your licensed segments and refuses to key up outside them, with a toast that says why.',
  'gettingStarted.license.where':
    'On the wizard’s last step, under <b>What’s your license?</b>. It is persisted the moment you click it.',
  'gettingStarted.license.technician':
    'A <b>Technician</b> on 40\u00a0m is held to the CW segment — phone and FT8 TX outside it are blocked.',
  'gettingStarted.license.general':
    'A <b>General</b> on 20\u00a0m cannot transmit in the Extra-only portion at the low end.',
  'gettingStarted.license.sixty': 'The channelized <b>60\u00a0m</b> segments are included.',
  'gettingStarted.license.outsideUs':
    'Outside the US? Pick <b>Outside the US</b>. The lockout models US FCC Part 97 / ITU Region 2 only, so it is off rather than wrong.',
  'gettingStarted.license.callout.label': 'It is a safety net',
  'gettingStarted.license.callout.body':
    'You are responsible for knowing your privileges. The lockout catches the slip; it does not replace the knowledge. A fresh install defaults to <b>Open</b>, so nothing is silently restricted before you say so.',
  'gettingStarted.license.shot.caption':
    'Setup wizard ▸ step 4 · everything starts on — this is the one question',
  'gettingStarted.license.shot.title': 'What’s your license?',
  'gettingStarted.license.shot.sub':
    'Sets your transmit privileges — the app parks the dial in your licensed band segments and won’t let you transmit outside them. Pick “Outside the US” for no limits.',
  'gettingStarted.license.shot.technician': 'Technician',
  'gettingStarted.license.shot.technicianBlurb': 'US — limited HF + full VHF/UHF',
  'gettingStarted.license.shot.general': 'General',
  'gettingStarted.license.shot.generalBlurb': 'US — most HF privileges',
  'gettingStarted.license.shot.extra': 'Amateur Extra',
  'gettingStarted.license.shot.extraBlurb': 'US — full privileges',
  'gettingStarted.license.shot.outside': 'Outside the US',
  'gettingStarted.license.shot.outsideBlurb': 'No transmit limits',
  // ⚠️ `{{freq}}` is a FREQUENCY — supplied invariant by the call site, never formatted here.
  // This is a PICTURE of the real toast, not a live transmit control.
  'gettingStarted.license.shot.toast':
    'TX blocked — {{freq}} is outside your General phone privileges on 40 m.',

  // Step 4 — the ADIF log, the payoff step.
  'gettingStarted.log.aria': 'Step 4 of 4: Your ADIF log',
  'gettingStarted.log.eyebrow': 'Step 4 of 4 · The one that matters',
  'gettingStarted.log.heading': 'Bring in your existing log',
  'gettingStarted.log.lede':
    'One log, every mode. Import one ADIF file and the app knows every station you have already worked, every entity you still need, and exactly how far you are from your next award — on digital, phone and CW alike, computed offline from your own history.',
  'gettingStarted.log.without.label': 'Without a log',
  'gettingStarted.log.without.body':
    'Every callsign looks the same, whether it arrives as a decode, a cluster spot or a voice on the band. Every station is new. The Needed board has nothing to say.',
  'gettingStarted.log.b4.label': 'With it — worked before',
  'gettingStarted.log.b4.body':
    'B4 chips wherever a callsign appears — digital decodes, the Call Roster, spots, and the recall card in the Phone and CW cockpits.',
  'gettingStarted.log.needed.label': 'With it — the Needed board',
  'gettingStarted.log.needed.body':
    'New DXCC, new state, new grid, new band slot — ranked across every band and mode at once, with the evidence that says it is workable now.',
  'gettingStarted.log.awards.label': 'With it — awards',
  'gettingStarted.log.awards.body':
    'DXCC, Challenge, Honor Roll, WAS, WAZ, VUCC and IOTA — mode-agnostic and per-mode alike — the moment the import finishes.',
  'gettingStarted.log.sources.head': 'Where your ADIF comes from',
  'gettingStarted.log.sources.intro':
    'Any standard ADIF export — <code>.adi</code> or <code>.adif</code>. If you have been operating, you already have one.',
  'gettingStarted.log.sources.wsjtx':
    '<b>WSJT-X</b> — <code>wsjtx_log.adi</code> in your log directory.',
  'gettingStarted.log.sources.qrz':
    '<b>QRZ Logbook</b> or <b>LoTW</b> — download your full ADIF export.',
  'gettingStarted.log.sources.others':
    '<b>N1MM+, Log4OM, HRD, ClubLog</b> — export ADIF from the logbook.',
  'gettingStarted.log.import.head': 'Then import it',
  'gettingStarted.log.import.body':
    'Wizard step 3, or <b>Logbook ▸ Import ADIF</b> at any time. Import as many files as you like — duplicates are detected and skipped, so a second pass costs you nothing.',
  'gettingStarted.log.callout.label': 'Nothing leaves your computer',
  'gettingStarted.log.callout.body':
    'The import is local. Your log lives in <code>log.adi</code> on your own machine, and the awards engine computes against it offline. Uploading to LoTW, QRZ, ClubLog or eQSL is a separate, opt-in connector.',
  'gettingStarted.log.shot.caption':
    'Setup wizard ▸ step 3 · or Logbook ▸ Import ADIF, any time',
  'gettingStarted.log.shot.title': 'Bring in your existing log',
  'gettingStarted.log.shot.sub':
    'Nexus works best when it knows your history. Importing your ADIF log is what powers <b>worked-before</b> flags, the <b>Needed</b> board, and your <b>awards</b> progress — without it, the app starts blind and treats every station as new.',
  'gettingStarted.log.shot.import': 'Import my ADIF log…',
  // ⚠️ `{{count}}` / `{{present}}` are example QSO COUNTS, already formatted by the call site.
  'gettingStarted.log.shot.result':
    '✓ Imported <b>{{count}}</b> QSOs · {{present}} already present. Your worked-before and Needed board are now seeded.',
  'gettingStarted.log.shot.formats':
    'From WSJT-X, N1MM, Log4OM, HRD, QRZ, LoTW, ClubLog — any standard ADIF (.adi/.adif) export. Nothing leaves your computer; duplicates are detected and skipped.',
  'gettingStarted.log.closing.eyebrow': 'You’re set — now go work someone',
  // ⚠️ `{{freq}}` is a FREQUENCY — see the block header.
  'gettingStarted.log.closing.digital':
    'Open <b>Digital</b>. The dial starts on {{freq}} and the decoder runs every 15-second slot — there is no Monitor toggle to forget.',
  'gettingStarted.log.closing.decodes':
    'Within a period or two, decodes fill Band Activity — now annotated with country, B4 and <b>New / DXCC</b> badges, because you imported the log.',
  'gettingStarted.log.closing.doubleClick':
    '<b>Double-click</b> a station calling CQ. The sequencer runs the whole exchange and logs it. The same log feeds the Phone and CW cockpits, so a familiar call shows their name and your history there too.',

  'gettingStarted.wsjtx.label': 'Coming from WSJT-X? The short path',
  'gettingStarted.wsjtx.body':
    'Your muscle memory transfers — double-click semantics, <code>Esc</code> / <code>F4</code> / <code>F6</code> / <code>Alt+1–6</code>, Band Activity bottom-pinned, early decodes at 11.8\u00a0s, Fake-It split, Hound auto-move. So do your settings: point step 2 at the same rig and audio devices WSJT-X uses, and hand step 4 your <code>wsjtx_log.adi</code>. JTAlert and GridTracker keep working — Nexus speaks the full WSJT-X UDP protocol and they see it as a WSJT-X.',
  // A whole extra sentence, not a tail glued onto the one above — a translator may place it
  // wherever their language wants it.
  'gettingStarted.wsjtx.mac':
    'On a Mac, hold Fn for the F-keys — or turn on "Use F1, F2, etc. keys as standard function keys" in System Settings ▸ Keyboard, as with WSJT-X.',

  // ── Contest-assistance note ─────────────────────────────────────────────────────────
  // ⚠️ EVERY rule statement here is quoted from a primary source that was fetched and read
  // (see the header of `components/AssistanceNote.tsx`). An operator may pick a contest
  // category on the strength of this text. A translator must translate the SURROUNDING
  // prose and leave the quoted rule text, the rule numbers (VIII.2, V.A.1, V.A.2, HCAT.1.1,
  // HCAT.2.1) and the category names as they are — a paraphrase here is a rules claim.
  'assist.state.unassisted': 'UNASSISTED',
  'assist.state.assisted': 'ASSISTED',
  // Four whole sentences rather than a stem plus a " since HH:MMZ" fragment: the stamp lands
  // in a different place in different languages. `{{since}}` is a UTC time stamp — invariant.
  'assist.sources.off':
    'Assistance sources off: AI CW decoder, DX cluster / RBN, PSK Reporter needs.',
  'assist.sources.offSince':
    'Assistance sources off since {{since}}: AI CW decoder, DX cluster / RBN, PSK Reporter needs.',
  'assist.sources.on':
    'AI CW decoder, DX cluster / RBN and PSK Reporter needs are supplying callsign identification.',
  'assist.sources.onSince':
    'AI CW decoder, DX cluster / RBN and PSK Reporter needs are supplying callsign identification since {{since}}.',
  'assist.toggle.declare': 'Declare unassisted entry',
  'assist.toggle.end': 'End unassisted entry',
  'assist.why.summary': 'What this means for your contest category',
  'assist.why.cqww':
    '<b>CQ WW</b> rule VIII.2 counts “a CW decoder, DX cluster, DX spotting Web sites … local or remote call sign and frequency decoding technology (e.g., CW Skimmer or Reverse Beacon Network)” as QSO-finding assistance. Using any of them places the entry in <b>Single Operator Assisted</b> (V.A.2) instead of Single Operator (V.A.1).',
  'assist.why.arrl':
    '<b>ARRL</b> contests call it spotting assistance and name “PSKReporter, Telnet, DX spotting websites or bulletin board systems, automated multi-channel decoders”. Single Operator may not use it (HCAT.1.1: “Use of spotting assistance is not permitted.”). <b>Single Operator Unlimited</b> may (HCAT.2.1). ARRL’s glossary defines a multi-channel decoder as software that “displays multiple decoded signals at the same time”. Nexus decodes one signal at a time, so that definition does not describe its CW decoder. It does describe the cluster, RBN and PSK Reporter feeds.',
  'assist.why.ownDecodes':
    'Your <b>own radio’s decodes</b> are not assistance under either ruleset, so they keep feeding the Needed board in unassisted mode. Your outbound PSK Reporter uploads also keep running, because ARRL says “Generating spotting information for use by other stations is not considered to be spotting assistance.”',
  'assist.why.notCovered':
    '<b>What this switch does not cover:</b> POTA and SOTA activator spots still arrive. Neither ruleset names those feeds, but both define assistance broadly enough to include them, so switch off the POTA/SOTA features by hand if you are entering a contest that counts them.',
  'assist.why.checkRules':
    'Rules differ by contest and change between years, so <b>check the rules of the contest you are entering</b>. This note reports what CQ WW and ARRL currently publish. It is not a category ruling.',
  'assist.keep':
    'Your own settings are never rewritten. Ending unassisted mode restores the decoder and feeds exactly as you had them.',

  // ── Self-update — the banner and the launch/manual checks ───────────────────────────
  // ⚠️ `{{version}}`, `{{latest}}` and `{{current}}` are VERSION STRINGS (`1.7.0`) and
  // `{{percent}}` is a whole-number percentage — all invariant, none ever locale-formatted.
  'update.failed': 'Update failed',
  'update.unknownError': 'Unknown error',
  'update.installing': 'Installing {{version}} — Nexus will restart…',
  // Two complete sentences rather than one plus an appended "(85% downloaded)" fragment.
  'update.ready': 'Nexus {{version}} is ready to install',
  'update.readyDownloaded': 'Nexus {{version}} is ready to install ({{percent}}% downloaded)',
  'update.install.label': 'Install and restart',
  'update.install.title': 'Install the update and restart Nexus',
  'update.notNow.label': 'Not now',
  'update.notNow.title': 'Not now — the update stays downloaded',
  'update.available': "Nexus {{latest}} is available — you're on {{current}}",
  'update.download': 'Download',
  'update.downloadFailed': 'Could not open the download page',
  'update.checkFailed': 'Could not reach the update server to check for updates',
  'update.upToDate': "You're on the latest Nexus ({{current}})",
  'update.unreadable': "Couldn't read the latest release info",

  // ── Crash fallback & external links ─────────────────────────────────────────────────
  // `{{label}}` names the surface that crashed ("Connect", "Nexus", "The needed window");
  // `{{panel}}` is a panel id and `{{detail}}` is a raw error message — all pass through
  // verbatim, never translated.
  'crash.title': '{{label}} hit an error',
  'crash.hint':
    'The rest of Nexus is still running — the radio was not touched. Pick another section from the rail, or copy the details below into a bug report.',
  'crash.reload': 'Reload window',
  'crash.panelWindow': 'The {{panel}} window',
  'externalLink.failed': 'Could not open the link: {{detail}}',

  // ── Shared across surfaces ──────────────────────────────────────────────────────────
  // `common.*` is for words that are genuinely the same act everywhere. Resist it: a shared
  // key that two surfaces want to word differently cannot be split later without orphaning
  // both translations. When in doubt, give the surface its own key.
  'common.dismiss': 'Dismiss',
} satisfies Record<string, Message>
