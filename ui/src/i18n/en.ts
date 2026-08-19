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

  // ── The callbook lookup (QRZ, then HamQTH) ──────────────────────────────────────────
  // Shared by the Logbook form and the cockpit log strip, because looking a callsign up in
  // the callbook is genuinely one act with one wording — see the `common.*` note at the end
  // of this file for when that is and is not true.
  //
  // ⚠️ `{{call}}` is a CALLSIGN, `{{grid}}` a Maidenhead locator and `{{detail}}` a callbook
  // answer (name · grid · state) assembled from data. All three pass through verbatim.
  // `QRZ` and `QRZ.com` are the site's name, not a word: they stay as they are.
  'callbook.lookupFailed': 'QRZ lookup failed',
  'callbook.detail.grid': 'grid {{grid}}',
  'callbook.detail.found': 'found',
  // Two whole sentences rather than one plus an appended note: where the "you need a
  // subscription for this" clause belongs is a decision for each language.
  'callbook.result': 'QRZ {{call}}: {{detail}}',
  'callbook.resultNoGrid': 'QRZ {{call}}: {{detail}} · grid/state need a QRZ subscription',
  'callbook.qrzPage.title': '{{call}} on QRZ.com (opens your browser)',
  'callbook.qrzPage.failed': 'Could not open {{call}} on QRZ',

  // ── Logbook (the Logbook view) ──────────────────────────────────────────────────────
  // ⚠️ THE UNITS RULE IS DENSE HERE. Everything this surface *shows* about a contact is an
  // invariant technical token and is therefore absent from this file: callsigns, grid
  // squares, band names (20m), mode names (FT8), frequencies, RST reports, POTA/SOTA
  // references, QSL letters (L/C/E) and the Q-codes and service names printed as button
  // labels (QRZ, eQSL, CL, HL, QSL▸). Those live in `components/Logbook.tsx` as named
  // constants — LOG_EXAMPLES and the labels beside it. What IS here is the prose around
  // them, and the ADIF field names quoted inside that prose (SIG_INFO, MY_SIG_INFO) are
  // wire identifiers that must survive translation verbatim, exactly as `ADIF OPERATOR`
  // does in `settings.station.fdOperator.hint`.
  //
  // PLURALS: this view carried nine hand-rolled `n === 1 ? '' : 's'` ternaries. Every one is
  // a `{{count}}` entry below, because English's two forms are not Polish's four.
  'logbook.title': 'Logbook',
  'logbook.subtitle': 'ADIF contacts',

  'logbook.import.adif.label': 'Import ADIF',
  'logbook.import.failed': 'ADIF import failed',
  // THREE STATEMENTS, NOT THREE FRAGMENTS OF ONE SENTENCE — and the difference matters.
  // Each carries its OWN count, and one message cannot select a plural form for two counts
  // at once, so the cross-product a single string would need is unrepresentable. Each entry
  // therefore holds its own leading separator, and a locale is free to change it.
  'logbook.import.imported': {
    one: 'Imported {{count}} QSO',
    other: 'Imported {{count}} QSOs',
  },
  'logbook.import.dupes': ' ({{count}} dupes skipped)',
  'logbook.import.updated': {
    one: ' · {{count}} existing QSO updated with confirmations/credits',
    other: ' · {{count}} existing QSOs updated with confirmations/credits',
  },

  'logbook.sync.label': 'Sync confirmations',
  'logbook.sync.title':
    'Reconcile a LoTW ADIF export into the log — upgrades confirmations + credit on existing QSOs',
  'logbook.sync.failed': 'LoTW sync failed',
  'logbook.sync.done': 'Synced: {{confirmed}} newly confirmed, {{credited}} credited',
  'logbook.sync.doneUnmatched':
    'Synced: {{confirmed}} newly confirmed, {{credited}} credited · {{unmatched}} unmatched',

  'logbook.pota.label': 'Import POTA',
  'logbook.pota.title':
    'Import a pota.app hunter/activator ADIF export — stamps park references onto your matching logged QSOs. Never creates or overwrites records.',
  'logbook.pota.failed': 'POTA import failed',
  'logbook.pota.stamped': {
    one: 'POTA: {{count}} QSO stamped with park refs',
    other: 'POTA: {{count}} QSOs stamped with park refs',
  },
  'logbook.pota.already': ' · {{count}} already stamped',
  'logbook.pota.unmatched': ' · {{count}} had no matching QSO (not added)',

  'logbook.fetchLotw.label': 'Fetch LoTW',
  'logbook.fetchLotw.title':
    'Fetch confirmations directly from LoTW (no file download — uses the LoTW credentials saved in Settings ▸ Confirmations)',
  'logbook.fetchLotw.failed': 'LoTW fetch failed',
  'logbook.fetchLotw.done': 'LoTW — {{confirmed}} newly confirmed, {{credited}} credited',

  'logbook.qrzSync.label': 'Sync QRZ',
  'logbook.qrzSync.title':
    'Fetch your online QRZ Logbook and merge it here — QSOs logged elsewhere plus QRZ confirmation status (needs the QRZ Logbook API key in Settings ▸ Confirmations)',
  'logbook.qrzSync.failed': 'QRZ sync failed',
  'logbook.qrzSync.done': {
    one: 'QRZ sync — {{count}} new QSO, {{confirmed}} newly confirmed',
    other: 'QRZ sync — {{count}} new QSOs, {{confirmed}} newly confirmed',
  },

  'logbook.export.from.label': 'from',
  'logbook.export.from.title':
    'Export only QSOs on/after this UTC date (empty = from the beginning)',
  'logbook.export.to.label': 'to',
  'logbook.export.to.title': 'Export only QSOs on/before this UTC date (empty = to the end)',
  'logbook.export.adif.label': 'Export ADIF',
  'logbook.export.adif.title': 'Save the whole logbook as an ADIF file in your Downloads folder',
  'logbook.export.adif.titleRange':
    'Save the selected date range as an ADIF file in your Downloads folder',
  'logbook.export.csv.label': 'Export CSV',
  'logbook.export.csv.title': 'Save the whole logbook as a CSV spreadsheet in your Downloads folder',
  'logbook.export.csv.titleRange':
    'Save the selected date range as a CSV spreadsheet in your Downloads folder',
  'logbook.export.failed': 'Export failed',
  // `{{path}}` is a file path — data, never translated, never re-punctuated.
  'logbook.export.done': {
    one: 'Exported {{count}} QSO → {{path}}',
    other: 'Exported {{count}} QSOs → {{path}}',
  },
  'logbook.export.perOperator.label': 'Export per operator',
  // `{{operators}}` is a comma-joined list of CALLSIGNS.
  'logbook.export.perOperator.title': 'One ADIF per operator ({{operators}}) plus the combined log',
  'logbook.export.perOperator.done': 'Exported {{count}} files → Downloads',

  'logbook.lotw.upload.label': 'Upload to LoTW',
  'logbook.lotw.upload.labelCount': 'Upload to LoTW ({{count}})',
  'logbook.lotw.upload.busy': 'Uploading…',
  'logbook.lotw.upload.title':
    'Sign + upload your un-uploaded QSOs to LoTW via TQSL (set your Station Location in Settings)',
  // A second statement appended to the title above, with its own count — see the import
  // note. It keeps its leading separator for the same reason.
  'logbook.lotw.upload.timeless': {
    one: ' — {{count}} imported QSO has no time of day and can never match at LoTW, so they are not sent',
    other:
      ' — {{count}} imported QSOs have no time of day and can never match at LoTW, so they are not sent',
  },
  'logbook.lotw.upload.nothingNew': 'Nothing new to upload to LoTW',
  'logbook.lotw.upload.pending': {
    one: "Signed + uploaded {{count}} QSO to LoTW — they'll confirm as partners upload",
    other: "Signed + uploaded {{count}} QSOs to LoTW — they'll confirm as partners upload",
  },
  // The `one` form reads "1 QSO were", which is what shipped; this phase changes no visible
  // English. A translator writes their own language's agreement and is not bound by it.
  'logbook.lotw.upload.duplicate': {
    one: '{{count}} QSO were already on LoTW',
    other: '{{count}} QSOs were already on LoTW',
  },
  'logbook.lotw.upload.retry': 'LoTW unreachable — try again shortly',
  // `{{detail}}` is the server's own words, passed through untranslated.
  'logbook.lotw.upload.authFailed': 'LoTW rejected your certificate/Station Location',
  'logbook.lotw.upload.authFailedDetail':
    'LoTW rejected your certificate/Station Location: {{detail}}',
  'logbook.lotw.upload.failed': 'LoTW upload failed',
  'logbook.lotw.upload.failedDetail': 'LoTW upload failed: {{detail}}',

  // ⚠️ `{{formatted}}` is a QSO COUNT the call site has already grouped for display
  // ("1,234"). It is a count of contacts, not a technical quantity — no dial, no report, no
  // wire value — so grouping it is a display choice, not the hazard the invariant rule is
  // about. `{{count}}` (the raw number) rides alongside it purely to select the plural form.
  'logbook.markLotw.label': 'Mark on LoTW',
  'logbook.markLotw.title':
    'Already have these on LoTW (uploaded via another tool)? Mark them so Nexus stops counting them as needing upload.',
  'logbook.markLotw.aria': 'Mark as already on LoTW',
  'logbook.markLotw.heading': {
    one: 'Mark {{formatted}} QSO as already on LoTW?',
    other: 'Mark {{formatted}} QSOs as already on LoTW?',
  },
  'logbook.markLotw.body': {
    one: "Use this if you imported a log you'd already uploaded to LoTW another way (Ham2K Polo, TQSL…). It marks the {{formatted}} un-uploaded QSO as already on LoTW, so the <b>Upload to LoTW</b> count stops offering to re-send them. It only updates Nexus's own record — nothing is sent, and your LoTW account and log are untouched. New QSOs you make later still upload normally.",
    other:
      "Use this if you imported a log you'd already uploaded to LoTW another way (Ham2K Polo, TQSL…). It marks the {{formatted}} un-uploaded QSOs as already on LoTW, so the <b>Upload to LoTW</b> count stops offering to re-send them. It only updates Nexus's own record — nothing is sent, and your LoTW account and log are untouched. New QSOs you make later still upload normally.",
  },
  'logbook.markLotw.cancel': 'Cancel',
  'logbook.markLotw.confirm': 'Mark {{formatted}} as on LoTW',
  'logbook.markLotw.failed': 'Could not update LoTW state',
  'logbook.markLotw.done': {
    one: 'Marked {{formatted}} QSO as already on LoTW',
    other: 'Marked {{formatted}} QSOs as already on LoTW',
  },
  'logbook.markLotw.nothing': 'Nothing to mark',

  // The purge gate. `{{word}}` is the typed confirmation token (`DELETE`) — it is matched
  // against what the operator types, so it is a token, not prose, and it stays in the code.
  'logbook.purge.label': 'Purge log',
  'logbook.purge.title': 'Delete every contact in the local logbook (irreversible)',
  'logbook.purge.aria': 'Purge logbook',
  'logbook.purge.heading': 'Purge the entire logbook?',
  'logbook.purge.irreversible': 'Irreversible',
  'logbook.purge.warn': {
    one: "This permanently deletes <b>all {{count}} contact</b> from your local logbook and rewrites the ADIF file to empty. It does <b>not</b> remove anything you've already uploaded to LoTW, QRZ, eQSL, or ClubLog. There is no undo — export an ADIF backup first if you might want it.",
    other:
      "This permanently deletes <b>all {{count}} contacts</b> from your local logbook and rewrites the ADIF file to empty. It does <b>not</b> remove anything you've already uploaded to LoTW, QRZ, eQSL, or ClubLog. There is no undo — export an ADIF backup first if you might want it.",
  },
  'logbook.purge.syncWarn':
    'It also resets your <b>LoTW and eQSL sync position</b>, so the next sync re-downloads your whole confirmation history instead of only recent matches. That is what brings your confirmations back after a purge — but it takes considerably longer than a routine sync, so give it time to finish.',
  'logbook.purge.typeWord': 'Type <b>{{word}}</b> to confirm',
  'logbook.purge.cancel': 'Cancel',
  'logbook.purge.busy': 'Purging…',
  'logbook.purge.confirm': {
    one: 'Purge {{count}} contact',
    other: 'Purge {{count}} contacts',
  },
  'logbook.purge.failed': 'Could not purge the log',
  'logbook.purge.done': {
    one: 'Purged {{count}} contact from the log',
    other: 'Purged {{count}} contacts from the log',
  },

  // The hand-log form. Every placeholder that is a pure token (W1AW, FN31, 20m, US-1234…)
  // is in LOG_EXAMPLES, not here; the three that are human prose are.
  'logbook.form.open': 'Log QSO',
  'logbook.form.close': 'Close',
  'logbook.field.call.label': 'Call',
  'logbook.field.qrz.title': 'Look up name + grid on QRZ.com',
  'logbook.field.grid.label': 'Grid',
  'logbook.field.band.label': 'Band',
  'logbook.field.freq.label': 'Freq (MHz)',
  'logbook.field.mode.label': 'Mode',
  'logbook.field.rstSent.label': 'RST Sent',
  'logbook.field.rstRcvd.label': 'RST Rcvd',
  'logbook.field.when.label': 'Date + time (UTC)',
  'logbook.field.when.title':
    'When the contact actually happened, in UTC. Leave blank to stamp now.',
  'logbook.field.state.label': 'State',
  'logbook.field.state.title':
    'US state — drives Worked All States. A hand-logged contact has no decode to derive it from.',
  'logbook.field.txPower.label': 'TX power (W)',
  'logbook.field.parkTheirs.label': 'Park (worked)',
  'logbook.field.parkTheirs.title':
    'POTA reference of the park the station you worked was activating (ADIF SIG_INFO). Defaults to POTA.',
  'logbook.field.parkMine.label': 'Park (mine)',
  'logbook.field.parkMine.title':
    'POTA reference of YOUR own activation for this contact (ADIF MY_SIG_INFO). Defaults to POTA.',
  'logbook.field.name.label': 'Name',
  // Prose, not a token: a locale should offer a first name its operators recognise.
  'logbook.field.name.placeholder': 'Jim',
  'logbook.field.qth.label': 'QTH',
  // Also prose — QTH is free text, so the example is a place a reader recognises.
  'logbook.field.qth.placeholder': 'Dayton, OH',
  'logbook.field.comment.label': 'Comment',
  'logbook.field.comment.placeholder': 'Shared on the QSL',
  'logbook.field.notes.label': 'Notes',
  'logbook.field.notes.placeholder': 'Rig / antenna / weather / what you talked about…',
  'logbook.form.callRequired': 'Callsign is required.',
  'logbook.form.editingNote':
    'Editing — confirmations and upload state are kept, unless you change the callsign: a corrected call re-sends to every service and drops confirmations matched on the old one.',
  'logbook.form.save': 'Save',
  'logbook.form.log': 'Log',
  'logbook.form.saveFailed': 'Could not save the edit',
  'logbook.form.updated': 'Updated {{call}}',
  'logbook.form.logFailed': 'Could not log QSO',

  // The table. Column headers name a CONCEPT and are prose; every value under them is a
  // token. `{{query}}` is what the operator typed, inserted verbatim.
  'logbook.globe.loading': 'Loading globe…',
  'logbook.search.placeholder': 'Search call / grid / band / mode / date…',
  'logbook.search.clear': 'Clear',
  'logbook.filter.needsConfirmation.label': 'needs confirmation',
  'logbook.filter.needsConfirmation.title':
    "Show only contacts without an award-eligible (LoTW/paper) confirmation. Rows you've already sent a QSL request for stay here — a request is not a confirmation.",
  'logbook.column.call': 'Call',
  'logbook.column.country': 'Country',
  'logbook.column.band': 'Band',
  'logbook.column.freq': 'Freq',
  'logbook.column.mode': 'Mode',
  'logbook.column.sent': 'Sent',
  'logbook.column.rcvd': 'Rcvd',
  'logbook.column.time': 'Time (UTC)',
  'logbook.column.park': 'Park',
  'logbook.column.actions': 'Edit / delete',
  'logbook.empty': 'No logged contacts yet.',
  'logbook.emptySearch': 'No contacts match “{{query}}”.',

  // A row. `{{program}}` (POTA/SOTA/WWFF) and `{{ref}}` are references, `{{call}}` a callsign.
  'logbook.row.park.worked': '{{program}} {{ref}} (worked)',
  'logbook.row.park.mine': 'My activation: {{program}} {{ref}}',
  'logbook.row.qsl.lotw': 'LoTW confirmed (award-eligible)',
  'logbook.row.qsl.card': 'Paper card received (award-eligible)',
  'logbook.row.qsl.eqsl': 'eQSL received (NOT DXCC/WAZ/WAS-eligible)',
  'logbook.row.qsl.confirmed': 'LoTW / paper — award-eligible',
  'logbook.row.qsl.eqslOnly': 'eQSL only — not accepted for DXCC/WAZ/WAS',
  'logbook.row.qsl.none': 'Not confirmed',
  'logbook.row.spot.title':
    "Spot {{call}} to the DX cluster (pre-fills this QSO's call + frequency)",
  'logbook.row.spot.aria': 'Spot {{call}} to the DX cluster',
  'logbook.row.pushQrz.title':
    'Push {{call}} to your QRZ logbook (re-push is safe — duplicates are detected)',
  'logbook.row.pushQrz.aria': 'Push {{call}} to QRZ',
  'logbook.row.pushClublog.title':
    'Push {{call}} to ClubLog (re-push is safe — duplicates are detected)',
  'logbook.row.pushClublog.aria': 'Push {{call}} to ClubLog',
  'logbook.row.pushHrdlog.title':
    'Push {{call}} to HRDLog.net (live-logging/awards site — not an ARRL confirmation source; re-push is safe)',
  'logbook.row.pushHrdlog.aria': 'Push {{call}} to HRDLog.net',
  // The <select>'s VALUES are the ADIF QSL_SENT_VIA letters B/D/E and stay in the code; only
  // these labels are prose. They are capitalised menu items, which is why they are not the
  // same entries as the lower-case words that appear inside the sentences below.
  'logbook.row.qslSent.title':
    "Mark a QSL request sent to {{call}} (bureau/direct/electronic). A request is not a confirmation — the row stays here until it's confirmed.",
  'logbook.row.qslSent.aria': 'Mark QSL sent to {{call}}',
  'logbook.row.qslSent.bureau': 'Bureau',
  'logbook.row.qslSent.direct': 'Direct',
  'logbook.row.qslSent.electronic': 'Electronic',
  'logbook.row.edit': 'Edit {{call}}',
  'logbook.row.delete': 'Delete {{call}}',

  // The QSL-request note. Four whole sentences instead of a stem plus " via …" and " on …"
  // tails: both stamps land in a different place in different languages. `{{date}}` is a
  // UTC calendar date, already formatted invariantly by the call site.
  'logbook.qsl.via.bureau': 'bureau',
  'logbook.qsl.via.direct': 'direct',
  'logbook.qsl.via.electronic': 'electronic',
  'logbook.qsl.sent': 'QSL sent',
  'logbook.qsl.sentOn': 'QSL sent {{date}}',
  'logbook.qsl.sentVia': 'QSL sent via {{via}}',
  'logbook.qsl.sentOnVia': 'QSL sent {{date}} via {{via}}',
  'logbook.qsl.marked': 'Marked QSL sent to {{call}} ({{via}})',
  'logbook.qsl.markFailed': 'Could not mark QSL sent',

  // Manual per-QSO pushes. `{{reason}}` and `{{detail}}` are the service's own words.
  'logbook.push.qrz.ok': '✓ {{call}} pushed to QRZ logbook',
  'logbook.push.qrz.duplicate':
    '✓ {{call}} already in your QRZ logbook (duplicate) — upload chain works',
  'logbook.push.qrz.rejected': '✗ QRZ rejected {{call}}: {{reason}}',
  'logbook.push.qrz.failed': '✗ QRZ push failed: {{detail}}',
  'logbook.push.clublog.ok': '✓ {{call}} pushed to ClubLog',
  'logbook.push.clublog.duplicate':
    '✓ {{call}} already on ClubLog (duplicate) — upload chain works',
  'logbook.push.clublog.rejected': '✗ ClubLog rejected {{call}}: {{reason}}',
  'logbook.push.clublog.failed': '✗ ClubLog push failed: {{detail}}',
  'logbook.push.hrdlog.ok': '✓ {{call}} pushed to HRDLog.net',
  'logbook.push.hrdlog.duplicate':
    '✓ {{call}} already on HRDLog.net (duplicate) — upload chain works',
  'logbook.push.hrdlog.unavailable':
    'HRDLog.net unavailable — {{call}} not confirmed uploaded; try again later',
  'logbook.push.hrdlog.rejected': '✗ HRDLog.net rejected {{call}}: {{reason}}',
  'logbook.push.hrdlog.failed': '✗ HRDLog.net push failed: {{detail}}',

  // Deleting one contact. `{{band}}` is a band name — never translated.
  'logbook.delete.heading': 'Delete the QSO with {{call}} on {{band}}?',
  'logbook.delete.body': "This removes it from your log. This can't be undone.",
  'logbook.delete.confirm': 'Delete QSO',
  'logbook.delete.failed': 'Could not delete the QSO',
  'logbook.delete.done': 'Deleted {{call}}',

  // ── The log strip (the cockpits' LOG pane, "Log this QSO") ──────────────────────────
  // One component serves Phone, CW and Satellites, plus a Field Day variant, so these keys
  // name the ACT — logging the contact in front of you — not any one cockpit.
  //
  // ⚠️ Invariant here and therefore absent: callsigns, RST, grid squares, band and mode
  // names, frequencies, POTA/SOTA references, the FD class and ARRL section codes, and the
  // MHz unit. They are LOG_EXAMPLES / PARK_PROGRAMS in `components/LogEntry.tsx`. Grid
  // squares quoted INSIDE a sentence that explains the format (EN52, EN52XA, EN52XA25) do
  // stay in the message, as `ADIF OPERATOR` does elsewhere — a translator must leave them.
  'logEntry.title': 'Log this QSO',
  'logEntry.clear.label': 'Clear',
  'logEntry.clear.title': 'Clear the log fields',
  'logEntry.hunt.title':
    'This QSO will be tagged with the hunted park reference when you log it (matched by callsign).',
  'logEntry.hunt.mismatch': '(call ≠ hunt)',
  'logEntry.call.placeholder': 'Call',
  'logEntry.lookup.label': 'Lookup',
  'logEntry.lookup.title':
    'Look up name + QTH in the callbook — QRZ first, then HamQTH (grid/state need a QRZ subscription)',
  'logEntry.rstSent.label': 'Sent',
  'logEntry.rstSent.title': 'Signal report you SENT them',
  'logEntry.rstRcvd.label': 'Rcvd',
  'logEntry.rstRcvd.title': 'Signal report you RECEIVED from them',
  'logEntry.grid.placeholder': 'Grid',
  'logEntry.grid.title':
    'Their Maidenhead locator — the satellite exchange. 4, 6 or 8 characters (EN52, EN52XA or EN52XA25); auto-filled by the callbook lookup only while it is blank',
  'logEntry.grid.blocked':
    '“{{grid}}” isn’t a grid square — Nexus logs 4, 6 or 8 characters (EN52, EN52XA or EN52XA25). Fix it or clear it to log.',
  'logEntry.grid.blockedToast':
    '“{{grid}}” isn’t a grid square — enter EN52, EN52XA or EN52XA25, or clear it',
  'logEntry.grid.blockedTitle':
    'Enter a 4-, 6- or 8-character grid square (EN52, EN52XA or EN52XA25), or clear it',
  'logEntry.name.placeholder': 'Name',
  'logEntry.qth.placeholder': 'QTH (city)',
  'logEntry.state.placeholder': 'State',
  'logEntry.state.title': 'State / province — auto-filled by the QRZ lookup when available',
  'logEntry.country.placeholder': 'Country',
  'logEntry.country.title': 'DXCC entity — auto-filled from the callsign when available',
  'logEntry.comment.placeholder': 'Comment (sharable)',
  'logEntry.park.program.title': 'On-the-air program for the park/summit you worked',
  // `{{example}}` is a park/summit REFERENCE from the call site — a wire format, never
  // localised. The prose around it is the part a translator owns.
  'logEntry.park.ref.placeholderPota': 'Park ({{example}} or name)',
  'logEntry.park.ref.placeholderSota': 'Summit ({{example}})',
  'logEntry.park.ref.title':
    'Park/summit reference of the station you worked — logged to ADIF (POTA→SIG_INFO, SOTA→SOTA_REF)',
  'logEntry.park.live.label': 'live',
  'logEntry.park.live.title': 'Fetched live from the POTA directory',
  'logEntry.notes.placeholder': 'Notes (private, multi-line)…',
  'logEntry.override.toggle': 'Log a contact from another radio',
  'logEntry.override.toggleSub': '· adjust band · freq · mode · time (UTC)',
  'logEntry.override.title':
    "Log a contact you made on another radio that isn't connected to Nexus — set the band, frequency, mode, and UTC time by hand",
  'logEntry.override.date.label': 'Date (UTC)',
  'logEntry.override.time.label': 'Time (UTC)',
  'logEntry.override.band.label': 'Band',
  'logEntry.override.freq.label': 'Freq (MHz)',
  'logEntry.override.mode.label': 'Mode',
  // ⚠️ `{{freq}}` is exactly what the operator typed into the frequency box, and `{{band}}`
  // a band name. Neither is reformatted on its way through.
  'logEntry.override.offBand': '{{freq}} MHz is outside {{band}} — logged as entered',
  'logEntry.override.needFreq': 'Enter a numeric frequency',
  'logEntry.override.blockedHint': 'Enter a frequency for the override to log',
  'logEntry.override.blocked': 'Enter a valid frequency for the override, or close it',
  // Two whole sentences: a dial the band plan cannot name (QO-100 at 10 GHz) has no band
  // slot at all, and a sentence assembled around an empty slot reads as a hole.
  'logEntry.summary': 'Logs to the shared logbook as {{mode}} · {{freq}} MHz',
  'logEntry.summaryBand': 'Logs to the shared logbook as {{mode}} · {{band}} · {{freq}} MHz',
  'logEntry.log': 'Log',
  'logEntry.logged': 'Logged {{call}} ({{mode}})',
  'logEntry.logFailed': 'Could not log the QSO',
  'logEntry.spot.label': '📢 Spot',
  'logEntry.spot.title': 'Spot this call to the DX cluster (pre-fills the call + your frequency)',

  // Field Day variant. `{{class}}` and `{{section}}` are exchange codes (1D, WI) and
  // `{{mode}}` the FD mode code (CW/PH) — all wire values.
  'logEntry.fd.chip': 'FD LOG',
  'logEntry.fd.hint': '{{band}} · contacts go to the Field Day log',
  'logEntry.fd.call.label': 'Call',
  'logEntry.fd.class.label': 'Class',
  'logEntry.fd.class.title': 'Their Field Day class',
  'logEntry.fd.section.label': 'Section',
  'logEntry.fd.section.title': 'Their ARRL section',
  'logEntry.fd.log': 'Log FD',
  'logEntry.fd.needClass': 'Enter their Field Day class to log.',
  'logEntry.fd.badSection':
    'Section "{{section}}" isn\'t a known ARRL/RAC section — required to log.',
  'logEntry.fd.logged': 'FD: logged {{call}} {{class}}/{{section}} ({{mode}})',
  'logEntry.fd.failed': 'FD log failed',

  // ── Confirm-before-log prompt (WSJT-X's "Prompt me to log QSO") ─────────────────────
  // Its own area, not `logEntry.*`: this is the popup that reviews a contact the sequencer
  // already made, and its four field labels are read in a different context.
  'logPrompt.aria': 'Log QSO',
  'logPrompt.title': 'Log this QSO?',
  'logPrompt.call.label': 'Call',
  'logPrompt.grid.label': 'Grid',
  // RST is the signal-report format's name — it stays as it is inside the label.
  'logPrompt.rstSent.label': 'RST sent',
  'logPrompt.rstRcvd.label': 'RST rcvd',
  'logPrompt.discard': 'Discard',
  'logPrompt.log': 'Log QSO',

  // ── Station roster (the Stations list and its cards) ────────────────────────────────
  // ⚠️ `{{call}}` is a callsign; the SNR badge, grid, country, distance and bearing on a
  // card are all data and never pass through here. `B4` is ham shorthand printed as-is.
  'roster.title': 'Stations',
  'roster.band.label': 'Band — calling CQ',
  'roster.band.title': 'Call CQ and see open broadcasts on the band',
  'roster.recents.aria': 'Recent conversations',
  'roster.recents.head': 'Recent chats',
  'roster.recents.open': 'Open conversation with {{call}}',
  'roster.recents.offline': 'not heard recently',
  'roster.recents.archive.title': 'Delete this conversation',
  'roster.recents.archive.aria': 'Delete conversation with {{call}}',
  'roster.onBandNow': 'On the band now',
  'roster.filter.aria': 'Station filter',
  'roster.filter.all': 'All',
  'roster.filter.heardNow': 'Heard now',
  'roster.filter.beaconing': 'Beaconing',
  'roster.filter.needed': 'Needed',
  'roster.empty': 'No stations match.',
  'roster.card.doubleClick': 'Double-click to work {{call}}',
  'roster.card.open': 'Open {{call}}',
  'roster.card.b4.sameBand': 'Worked before on this band',
  'roster.card.b4.otherBand': 'Worked before (another band)',
  'roster.card.work.label': 'Work',
  'roster.card.work.title': 'Work {{call}}',
  // "Slots" are T/R periods, not clock time — the count is the number of periods since the
  // station was last decoded, so it is a plural entry, not a duration format.
  'roster.card.heard.now': 'now',
  'roster.card.heard.slots': {
    one: '{{count}} slot ago',
    other: '{{count}} slots ago',
  },
  'roster.card.heard.minutes': '{{count}} min ago',

  // ── Awards (the official tracker) ───────────────────────────────────────────────────
  // ⚠️ AWARD NAMES ARE INVARIANT TOKENS and are therefore absent from this file: DXCC,
  // Honor Roll, Challenge, 5-Band DXCC, WAZ, WAS, VUCC, Sat VUCC and IOTA are the names of
  // ARRL/CQ programmes, not words — they live in `components/AwardsView.tsx` as AWARD_NAMES.
  // So are the DXCC entity names, CQ zones, grid squares, band and mode names the tables
  // print (data), and the service names (LoTW, QRZ, ClubLog, eQSL, TQSL, Club Log). Where one
  // of those appears INSIDE a sentence below — `5BWAS`, `ARRL`, `6 m`, `50 MHz` — a translator
  // leaves it exactly as it is, the same rule `ADIF OPERATOR` follows in the Station hint.
  //
  // THE TRAILING "· N ready to submit" CLAUSE is its own entry with its own leading
  // separator, for the reason `logbook.import.dupes` is: it is a separate STATEMENT with its
  // own count, and a locale must be free to word and place it independently.
  'awards.title': 'Awards',
  'awards.subtitle': 'DXCC · computed from your log',

  'awards.load.failed.title': "Couldn't load awards",
  'awards.load.failed.detail': 'The award tally failed to compute.',
  'awards.loading.title': 'Tallying awards…',
  'awards.loading.detail': 'Resolving your log against the DXCC entity list.',
  'awards.empty.title': 'No contacts yet',
  'awards.empty.detail':
    'Log contacts or import an ADIF (Logbook → Import ADIF) to start tracking DXCC.',

  // The tiles. Each note is ONE sentence per state — the shipped text glued a conditional
  // head onto a shared tail, which no language with a different word order can reproduce.
  'awards.dxcc.note.achieved':
    'DXCC achieved ✓ · {{confirmed}} entities · {{worked}} worked · {{credited}} credited',
  'awards.dxcc.note.toGo': '{{remaining}} confirmed to go · {{worked}} worked · {{credited}} credited',
  'awards.dxcc.note.readyToSubmit': ' · {{count}} ready to submit',
  'awards.honorRoll.note.numberOne': '#1 Honor Roll ✓ — all {{total}} entities',
  'awards.honorRoll.note.achieved': 'Honor Roll ✓ · {{needed}} to #1',
  'awards.honorRoll.note.toGo':
    '{{needed}} more confirmed needed — Honor Roll entry at {{threshold}}',
  'awards.challenge.note': '{{worked}} entity×band slots worked',
  'awards.confirmed.label': 'Confirmed',
  'awards.confirmed.note':
    '{{confirmed}} of {{total}} QSOs confirmed via LoTW or card (eQSL / QRZ matches don’t count toward ARRL awards)',
  'awards.fiveBand.note':
    'weakest of the 5 classic bands (ARRL counts each band on its own) · {{worked}} worked',
  'awards.waz.note.achieved': 'Worked All Zones ✓ · {{worked}} worked',
  'awards.waz.note.toGo': '{{remaining}} zones to go · {{worked}} worked',
  'awards.was.note.achieved':
    'Worked All States ✓ · {{worked}} worked · 5BWAS weakest band {{fiveBand}}/50',
  'awards.was.note.toGo':
    '{{remaining}} states to go · {{worked}} worked · 5BWAS weakest band {{fiveBand}}/50',
  // `{{bands}}` is a joined list of BAND NAMES and `{{band}}` a single one — never translated.
  'awards.vucc.note.achieved':
    'VUCC ✓ {{bands}} · {{confirmed}} grids confirmed on all bands (tracker)',
  'awards.vucc.note.toGo':
    '{{remaining}} grids to go on {{band}} · {{confirmed}} grids confirmed on all bands (tracker)',
  'awards.vucc.note.none':
    'No 6 m-and-up grids yet · {{confirmed}} grids confirmed on all bands (tracker — VUCC itself is 50 MHz and up)',
  'awards.satVucc.note.achieved':
    'Satellite VUCC ✓ · {{worked}} grids worked · Sat DXCC {{satDxcc}} confirmed',
  'awards.satVucc.note.toGo':
    '{{remaining}} more to confirm · {{worked}} grids worked · Sat DXCC {{satDxcc}} confirmed',
  'awards.satVucc.note.tagging':
    'Pass contacts are tagged automatically when logged on the bird’s downlink (ISS excepted — no LoTW designator to derive)',
  'awards.iota.note.achieved':
    'IOTA ✓ (card-confirmed) · {{cards}} on cards — IOTA credits cards / Club Log, not LoTW',
  'awards.iota.note.worked':
    '{{worked}} worked · {{cards}} on cards — IOTA credits cards / Club Log, not LoTW',

  // The breakdown panels. Every band and mode name under these headings is data.
  'awards.bands.head': 'DXCC by band',
  'awards.grids.head': 'Grids by band (VUCC)',
  'awards.modes.head': 'DXCC by mode',
  'awards.bar.title': '{{confirmed}} confirmed / {{worked}} worked',
  'awards.bar.titleGrids': '{{confirmed}} confirmed / {{worked}} worked grids',

  // The four chase lists. `{{count}}` is the length of the list beside the heading.
  'awards.chase.entities.head': 'Confirm for a new one ({{count}})',
  'awards.chase.entities.empty': 'Every worked entity is confirmed. 🎉',
  'awards.chase.slots.head': 'Confirm for a Challenge slot ({{count}})',
  'awards.chase.slots.empty': 'No worked-but-unconfirmed band slots.',
  'awards.chase.bandTargets.head': 'Work for a band slot ({{count}})',
  'awards.chase.bandTargets.empty': 'No almost-complete entities to chase.',
  'awards.chase.was.head': 'WAS — states needed ({{count}})',
  'awards.chase.was.empty': 'All 50 states confirmed. 🎉',

  // The chase list's own filter + sort. `{{query}}` is what the operator typed.
  'awards.needList.filter.placeholder': 'filter entities…',
  'awards.needList.filter.label': 'Filter entities',
  'awards.needList.sort.alpha.label': 'A–Z',
  'awards.needList.sort.alpha.title': 'Sort A–Z',
  'awards.needList.sort.bands.label': '# bands',
  'awards.needList.sort.bands.title': 'Sort by number of bands needed',
  'awards.needList.noMatch': 'No entities match “{{query}}”.',

  // "Why isn't this credited?" — the confirmation diagnostics. `{{service}}` is a service
  // name, `{{field}}` an ADIF field name, `{{call}}` a callsign, `{{reason}}`/`{{detail}}`
  // the service's own words: all pass through verbatim.
  'awards.conf.head': "Confirmations — why isn't this credited?",
  'awards.conf.oneAway.label': 'One fix away:',
  'awards.conf.oneAway.newEntity.title':
    "{{entity}} ({{bands}}): one LoTW upload / data fix puts a NEW DXCC entity in play — the partner's confirmation still decides",
  'awards.conf.oneAway.slots.title': {
    one: "{{entity}} ({{bands}}): one LoTW upload / data fix puts {{count}} Challenge slot in play — the partner's confirmation still decides",
    other:
      "{{entity}} ({{bands}}): one LoTW upload / data fix puts {{count}} Challenge slots in play — the partner's confirmation still decides",
  },
  'awards.conf.oneAway.more': '+{{count}} more',
  'awards.conf.bucket.upload': 'Upload {{count}}',
  'awards.conf.uploading': 'Uploading…',
  'awards.conf.pushing': 'Pushing…',
  'awards.conf.reupload': 'Re-upload',
  'awards.conf.uploadToLotw': 'Upload to LoTW',
  'awards.conf.push': 'Push to {{service}}',
  'awards.conf.repush': 'Re-push to {{service}}',
  'awards.conf.push.title':
    'Pushes this QSO to your {{service}} logbook — does not count for ARRL DXCC/WAS (LoTW only)',
  'awards.conf.fixCert': 'Fix cert in TQSL',
  'awards.conf.fixLoginInSettings': 'Fix {{service}} login in Settings',
  'awards.conf.fixLogin': 'Fix {{service}} login',
  'awards.conf.fixLogin.title':
    'Opens Settings ▸ Confirmations, where the {{service}} login is saved',
  'awards.conf.waitingOn': 'Waiting on {{call}}',
  'awards.conf.reviewDup': 'Review dup #{{number}}',
  'awards.conf.fixField': 'Fix {{field}}',
  'awards.conf.bustedCall': 'Was it {{call}}?',
  'awards.conf.likely': 'likely',
  'awards.conf.waitingOnPartner': {
    one: '{{count}} QSO uploaded to LoTW — waiting on the other operator to confirm.',
    other: '{{count}} QSOs uploaded to LoTW — waiting on the other operator to confirm.',
  },
  'awards.conf.pendingLag': {
    one: '{{count}} recently-worked QSO still awaiting a confirmation — not a problem, just give it time.',
    other:
      '{{count}} recently-worked QSOs still awaiting a confirmation — not a problem, just give it time.',
  },

  // The LoTW (re)upload result line. `{{outcome}}` is a wire enum, printed as-is.
  'awards.upload.pending': 'Signed and sent {{count}} to LoTW — awaiting confirmation.',
  'awards.upload.duplicate': 'Already on LoTW ({{count}}) — nothing to re-send.',
  'awards.upload.rejected': 'LoTW rejected the upload.',
  'awards.upload.rejectedDetail': 'LoTW rejected the upload: {{detail}}.',
  'awards.upload.authFailed':
    'LoTW rejected your certificate / Station Location — fix it in TQSL, then retry.',
  'awards.upload.retry': 'LoTW was unreachable — your log is unchanged; try again shortly.',
  'awards.upload.none': 'Nothing to upload.',
  'awards.upload.finished': 'Upload finished ({{outcome}}).',

  // Per-QSO pushes from a diagnosis row. Worded for THIS surface — the Logbook's own
  // `logbook.push.*` lines say the same thing to an operator who is somewhere else.
  'awards.push.noQso': 'Could not find that QSO in the log — reload Awards and try again.',
  'awards.push.qrz.ok': '✓ {{call}} pushed to your QRZ logbook.',
  'awards.push.qrz.duplicate':
    '✓ {{call}} already in your QRZ logbook (duplicate) — nothing to re-send.',
  'awards.push.qrz.rejected': '✗ QRZ rejected {{call}}: {{reason}}',
  'awards.push.clublog.ok': '✓ {{call}} on ClubLog.',
  'awards.push.clublog.duplicate': '✓ {{call}} on ClubLog (already there).',
  'awards.push.clublog.rejected': '✗ ClubLog rejected {{call}}: {{reason}}',
  'awards.push.eqsl.ok': '✓ {{call}} sent to eQSL.',
  'awards.push.eqsl.duplicate': '✓ {{call}} sent to eQSL (already there).',
  'awards.push.eqsl.rejected': '✗ eQSL: {{detail}}',
  'awards.push.failed': '✗ {{service}} push failed: {{detail}}',

  'awards.achievements.head': 'Achievements ({{unlocked}}/{{total}})',

  // The section's two tabs. The Journey layer's own strings are `journey.*`.
  'awards.tabs.aria': 'Awards and Journey',
  'awards.tab.journey': 'Journey',
  'awards.tab.official': 'Official Awards',

  // ── The Needed board ────────────────────────────────────────────────────────────────
  // ⚠️ Callsigns, DXCC entity names, band names, mode names, CQ zones, frequencies and
  // headings are the DATA this board exists to show — none of them is here. Mode-class
  // names (Digital, CW, Phone) are mode names and stay in `components/NeededPanel.tsx`, as
  // do the POTA/SOTA filter chips: those are the programmes' own names.
  'needed.title': 'Needed now',
  'needed.countFiltered': 'of {{count}}',
  'needed.hint': 'single-click a row to QSY the radio to that band and listen',
  'needed.filters.aria': 'Filter needed alerts',
  'needed.filters.modes.aria': 'Modes shown',
  'needed.filter.toggle.title': 'Filter the board by need type, band, or mode',
  'needed.filter.toggle.active': 'Filtered',
  'needed.filter.toggle.idle': 'Filter',
  'needed.filter.clear.label': 'Clear',
  'needed.filter.clear.title': 'Clear all filters',
  'needed.filter.all': 'All',
  'needed.filter.wanted': 'Watch list',
  'needed.filter.atno': 'ATNO',
  'needed.filter.newBand': 'New band',
  'needed.filter.newMode': 'New mode',
  'needed.filter.newZone': 'New zone',
  'needed.filter.newGrid': 'New grid',
  'needed.filter.newState': 'New state',
  'needed.filter.confirm': 'Needs confirm',
  'needed.filter.dxped': 'DXped',
  // `{{mode}}` is a mode-class name — the tooltip is prose, the mode is not.
  'needed.filter.mode.show.title': 'Show {{mode}} needs',
  'needed.filter.mode.hide.title': 'Hide {{mode}} needs',
  'needed.popOut.label': '⧉ Pop out',
  'needed.popOut.title': 'Open this board in its own window (for a second monitor)',
  'needed.autoPop.label': 'open at launch',
  'needed.autoPop.title': 'Open this board in its own window automatically when the app starts',

  // The rotator strip. `{{deg}}` is a bearing in degrees — a number, never grouped.
  'needed.rotator.title': 'Antenna rotator — live heading + manual point (via rotctld)',
  'needed.rotator.azimuth.placeholder': 'az',
  'needed.rotator.azimuth.label': 'Rotator azimuth (degrees)',
  'needed.rotator.go.title': 'Turn the rotator to this azimuth',
  'needed.rotator.pointed': '↗ Rotator → {{deg}}°',
  'needed.rotator.failed': 'Rotator command failed',

  // Phone-source liveness. `{{host}}` is a hostname:port and `{{state}}` a wire enum.
  'needed.phone.live.text': 'Phone source: {{host}} · live',
  'needed.phone.live.title': 'SSB/phone spots are flowing from {{host}}.',
  'needed.phone.connected.text': 'Phone source: {{host}} · connected',
  'needed.phone.connected.title':
    'Connected to {{host}} — no phone spot yet (an empty Phone board just means nothing you need is on SSB right now).',
  'needed.phone.connecting.text': 'Phone source: {{host}} · connecting…',
  'needed.phone.connecting.title': 'Reaching the SSB cluster node {{host}}.',
  'needed.phone.down.text': 'Phone source: {{host}} · down',
  'needed.phone.down.title':
    'Lost the connection to {{host}} — no SSB/phone needs until it reconnects.',
  'needed.phone.idle.text': 'Phone source: {{host}} · idle',
  'needed.phone.idle.title': 'Connected to {{host}} but quiet — a lull in human SSB spots.',
  'needed.phone.unknown.text': 'Phone source: {{host}} · {{state}}',
  'needed.phone.unknown.title': '{{host}}: {{state}}',
  // TWO STATEMENTS, each with its OWN count — one message cannot select a plural form for
  // two counts at once, so each keeps its own leading separator (the `logbook.import.*`
  // pattern) and a locale may re-word and re-order both.
  'needed.phone.spots': {
    one: ' · {{count}} SSB spot',
    other: ' · {{count}} SSB spots',
  },
  'needed.phone.needs': {
    one: ' → {{count}} need',
    other: ' → {{count}} needs',
  },
  // The whole sentence, with the settings link as a MARKER — the element comes from the
  // call site, so the catalog cannot introduce one.
  'needed.phone.off.link': 'Phone source off — <a>turn on “DX Cluster / RBN spots”</a>',
  'needed.phone.off.plain':
    'Phone source off — turn on “DX Cluster / RBN spots” in Settings ▸ Integrations & Feeds',
  'needed.phone.off.title':
    'Phone/SSB needs come only from a human DX-cluster node. This shows when the DX Cluster feed is disabled OR no human host is set — turn on “DX Cluster / RBN spots” and add a host (e.g. ve7cc.net:23) in Settings ▸ Integrations & Feeds. RBN carries only CW + digital, never SSB.',

  // The grid. Column headings name a CONCEPT; every value under them is a token.
  'needed.grid.aria': 'Needed now — arrow to move, Enter to work or QSY',
  'needed.column.need': 'Need',
  'needed.column.call': 'Call',
  'needed.column.entity': 'Entity',
  'needed.column.band': 'Band',
  'needed.column.mode': 'Mode',
  'needed.column.zone': 'Zone',
  'needed.column.why': 'Why',
  'needed.empty.filtered': 'No alerts match the current filters — clear to see all.',
  'needed.empty':
    "Nothing needed on the air right now — needed stations (new ones, band-slots, modes, grids, POTA/SOTA) appear here as they're heard or spotted.",

  // A row. `{{freq}}` is a dial frequency the call site has already formatted invariantly.
  'needed.row.work.title': 'Work {{call}} — {{mode}} on {{band}}',
  'needed.row.work.titleFreq': 'Work {{call}} — {{mode}} on {{band}} @ {{freq}} MHz',
  'needed.row.mainWindow.title':
    '{{call}} ({{mode}}) — open the main window to work this (pop-out only QSYs the band)',
  'needed.row.qsy.titleFreq': 'QSY to {{freq}} MHz and listen for {{call}}',
  'needed.row.qsy.titleBand': 'QSY to {{band}} and listen for {{call}}',
  'needed.row.aria': '{{call}}, {{entity}}, {{band}} {{mode}}, needed {{tags}}',
  'needed.row.ariaAzimuth':
    '{{call}}, {{entity}}, about {{deg}} degrees, {{band}} {{mode}}, needed {{tags}}',
  'needed.row.point.title': 'Point the antenna at {{call}}',
  'needed.row.mode.title': 'Needed on {{mode}}',

  // ── The need vocabulary (chips + decode badges) ─────────────────────────────────────
  // ONE set of words for "why this station is worth working", shared by the board, the
  // roster, the decode feed and the map — the registry is `features/needVisuals.ts`.
  // ⚠️ POTA and SOTA are the programmes' own names and are NOT here; the CSS class and the
  // icon beside each entry are code tokens and are not here either.
  //
  // Two vocabularies, deliberately not one: the decode feed's badge and the board's chip
  // already word their tooltips differently, and a shared key could not be split later
  // without orphaning both translations.
  'need.badge.entity.label': 'NEW ONE',
  'need.badge.entity.title': 'New DXCC entity — an all-time new one',
  'need.badge.zone.label': 'ZONE',
  'need.badge.zone.title': 'New CQ zone on this band (5BWAZ)',
  'need.badge.band.label': 'BAND',
  'need.badge.band.title': 'New band-slot for this entity',
  'need.badge.mode.label': 'MODE',
  'need.badge.mode.title': 'New mode for this entity',
  'need.badge.grid.label': 'GRID',
  'need.badge.grid.title': 'New grid square on this band (VUCC is per band)',
  'need.badge.state.label': 'STATE',
  'need.badge.state.title':
    'New US state on this band (5BWAS) — a hint from the grid; confirm from the log',
  'need.badge.dxped.label': 'DXPED',
  'need.badge.dxped.title': 'Active DXpedition — limited-time window',
  'need.badge.confirm.label': 'NEEDS QSL',
  'need.badge.confirm.title':
    'This entity/zone/grid is worked on this band but not yet confirmed — a QSL from this station would close it. Not a claim about this callsign: B4 is the worked-this-call chip.',
  'need.badge.pota.title': 'Live POTA activator',
  'need.badge.sota.title': 'Live SOTA activator',
  'need.badge.wanted.label': 'WANTED',
  'need.badge.wanted.title': 'On your wanted watch list',

  // The board/roster chip. `short` is the dense-column form — a translation needs both, and
  // the short one has to stay short.
  'need.chip.newEntity.label': 'NEW ONE',
  'need.chip.newEntity.short': 'NEW',
  'need.chip.newEntity.title': 'All-time-new DXCC entity (ATNO)',
  'need.chip.newZone.label': 'ZONE',
  'need.chip.newZone.short': 'ZONE',
  'need.chip.newZone.title': 'New CQ zone on this band',
  'need.chip.newBand.label': 'BAND',
  'need.chip.newBand.short': 'BAND',
  'need.chip.newBand.title': 'New band-slot for this entity',
  'need.chip.newMode.label': 'MODE',
  'need.chip.newMode.short': 'MODE',
  'need.chip.newMode.title': 'New mode for this entity',
  'need.chip.newGrid.label': 'GRID',
  'need.chip.newGrid.short': 'GRID',
  'need.chip.newGrid.title': 'New grid square on this band',
  'need.chip.newState.label': 'STATE',
  'need.chip.newState.short': 'ST',
  'need.chip.newState.title': 'New US state on this band — best-guess from the grid',
  'need.chip.confirm.label': 'NEEDS QSL',
  'need.chip.confirm.short': 'QSL',
  'need.chip.confirm.title':
    'Worked on this band but not yet confirmed — a QSL from this station would close it',
  'need.chip.dxped.label': 'DXPED',
  'need.chip.dxped.short': 'DXP',
  'need.chip.dxped.title': 'Active announced DXpedition — a limited-time window',
  'need.chip.pota.title': "Live POTA activator — the row's call is on a park right now",
  'need.chip.sota.title': "Live SOTA activator — the row's call is on a summit right now",
  'need.chip.wanted.label': 'WANTED',
  'need.chip.wanted.short': 'WANT',
  'need.chip.wanted.title': 'On your wanted watch list',

  // ── Status roles (the colour+glyph pairing table) ───────────────────────────────────
  // `statusMeta.ts` pairs each role with a CSS token, a CVD-immune glyph and this label.
  // The token and the glyph are code; only the label is read.
  'status.newEntity.label': 'New entity (ATNO)',
  'status.newBand.label': 'New band',
  'status.newMode.label': 'New mode',
  'status.worked.label': 'Worked, unconfirmed',
  'status.confirmed.label': 'Confirmed',
  'status.dupe.label': 'Already worked',
  'status.snrStrong.label': 'Strong signal',
  'status.snrMarginal.label': 'Marginal signal',
  'status.snrWeak.label': 'Weak signal',
  'status.tx.label': 'Transmitting',
  'status.rx.label': 'Receiving',
  'status.bandOpen.label': 'Band open',
  'status.bandMarginal.label': 'Band marginal',
  'status.bandClosed.label': 'Band closed',
  'status.alertCritical.label': 'Critical',
  'status.alertWarning.label': 'Warning',
  'status.alertInfo.label': 'Info',

  // ── Journey (the beginner-first achievement layer) ──────────────────────────────────
  // ⚠️ Every title, meaning, heritage note, gate hint, unit and personal-best value on this
  // surface comes from the backend (`get_journey`) and is NOT here — those are phase-3.
  // What is here is the frame the app writes around them. `{{xp}}`, `{{qsos}}` and the
  // ladder counts arrive already formatted by the call site.
  'journey.load.failed.title': "Couldn't load your Journey",
  'journey.loading.title': 'Loading your Journey…',
  'journey.loading.detail': 'Reading your log.',
  'journey.level': 'Level {{level}}',
  'journey.levelCap': 'level',
  'journey.xpEarned.title': '{{xp}} XP earned',
  'journey.xpToLevel': '{{into}} / {{forLevel}} XP to level {{next}}',
  'journey.qsosLogged': '{{qsos}} QSOs logged',
  'journey.streak.title': 'Consecutive weeks with at least one contact',
  'journey.streak': {
    one: '{{count}} week on the air',
    other: '{{count}} weeks on the air',
  },
  // Its own statement, appended — see the `logbook.import.*` note.
  'journey.streak.pending': ' · this week pending',
  'journey.next.title': 'Your most-attainable next milestone',
  'journey.next.cap': 'Next milestone',
  'journey.next.go': '{{remaining}} to go ({{current}}/{{target}})',

  // The share cards — rendered to an image locally, never uploaded.
  'journey.share.label': '⤴ Share',
  'journey.share.title':
    'Copy a share-card image of your Journey (local render — nothing is uploaded)',
  'journey.share.feat.title':
    'Copy a share-card image of this feat (local render — nothing is uploaded)',
  // Stands in for the operator's callsign when none is set — prose, not a call.
  'journey.share.anonCall': 'MY STATION',
  'journey.share.sub': '{{qsos}} QSOs logged · {{xp}} XP',
  'journey.share.footer': 'Journey · Nexus',
  'journey.share.featFooter': '{{tier}} feat · Journey · Nexus',

  'journey.marathon.head': 'DX Marathon {{year}}',
  'journey.marathon.note':
    'Entities + zones worked this calendar year — resets every Jan 1 (CQ DX Marathon-style, personal).',
  'journey.marathon.score.title': 'Entities + zones this year',
  'journey.marathon.parts': '{{entities}} entities · {{zones}} zones',
  'journey.marathon.best': 'personal best {{score}} ({{year}})',
  'journey.marathon.bestBeaten': 'personal best {{score}} ({{year}}) — beaten!',
  'journey.marathon.bestYear': 'your best year yet',

  'journey.firsts.head': 'Firsts',
  'journey.first.locked.title': 'Locked — {{meaning}}',
  'journey.ladders.head': 'Climb toward the awards',
  'journey.ladders.note':
    'Sub-award ladders — the official awards are the capstones in the Awards tab.',
  'journey.ladder.worked': 'worked',
  'journey.ladder.confirmed': 'confirmed',
  'journey.ladder.toGo': '{{count}} to go',
  'journey.ladder.complete': 'Complete ★',
  'journey.collections.head': 'Collections',
  // `{{label}}` is the cell's own name — a band, a mode, a continent.
  'journey.cell.confirmed.title': '{{label}} — confirmed',
  'journey.cell.worked.title': '{{label}} — worked',
  'journey.cell.needed.title': '{{label}} — needed',
  'journey.feats.head': 'Feats',
  'journey.bests.head': 'Personal bests',
  'journey.tier.bronze': 'Bronze',
  'journey.tier.silver': 'Silver',
  'journey.tier.gold': 'Gold',
  'journey.tier.platinum': 'Platinum',
  'journey.tier.legendary': 'Legendary',

  // The unlock toasts. The first/rung lines are pure data (a backend title, a rung label)
  // and carry no prose, so only these two are here.
  'journey.unlock.feat': '★ {{title}} unlocked!',
  'journey.unlock.more': '+{{count}} more milestones — open Journey to see them',

  // ── Logbook statistics ──────────────────────────────────────────────────────────────
  // ⚠️ Band names, mode names, years, DXCC entity names, US state codes, continents and CQ
  // zone numbers are the DATA these cards slice — none of them is here. `LoTW` and `eQSL`
  // are service names and `DX` is ham shorthand: all three stay in `components/StatsView.tsx`.
  // The counts arrive already formatted by the call site.
  'stats.title': 'Statistics',
  'stats.failed': 'Couldn’t read the logbook — try reopening this view.',
  'stats.loading': 'Loading your logbook…',
  'stats.empty': 'No QSOs logged yet — your stats will fill in here as you work stations.',
  'stats.qsos': 'QSOs',
  'stats.uniqueCalls': 'unique calls',
  'stats.dxccEntities': 'DXCC entities',
  'stats.confirmed': 'confirmed',
  'stats.byBand.head': 'By band',
  'stats.byMode.head': 'By mode',
  'stats.byYear.head': 'By year',
  'stats.topEntities.head': 'Top DXCC entities',
  'stats.byState.head': 'Most-worked states (WAS)',
  'stats.byHour.head': 'Activity by hour (UTC)',
  // `{{hour}}` is a zero-padded UTC hour — a clock reading, formatted by the call site.
  'stats.hour.title': '{{hour}}:00 UTC — {{count}} QSOs',
  'stats.hoursMissing': {
    one: '{{formatted}} QSO not shown — imported with a date but no time of day.',
    other: '{{formatted}} QSOs not shown — imported with a date but no time of day.',
  },
  'stats.confirmations.head': 'Confirmations',
  'stats.confirmations.awardGrade': 'Award-grade',
  'stats.confirmations.paperCard': 'Paper card',
  'stats.byContinent.head': 'By continent',
  'stats.continent.entities': '· {{count}} ent',
  'stats.byZone.head': 'By CQ zone',
  'stats.zone.label': 'Zone {{zone}}',
  'stats.dxSplit.head': 'DX vs domestic',
  'stats.dxSplit.domestic': 'Domestic',
  'stats.unplaced': '{{unplaced}} of {{total}} QSOs couldn’t be placed by callsign',

  // ── Shared across surfaces ──────────────────────────────────────────────────────────
  // `common.*` is for words that are genuinely the same act everywhere. Resist it: a shared
  // key that two surfaces want to word differently cannot be split later without orphaning
  // both translations. When in doubt, give the surface its own key.
  'common.dismiss': 'Dismiss',
} satisfies Record<string, Message>
