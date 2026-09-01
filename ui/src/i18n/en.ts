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
  'update.downloadFailed':
    "Couldn't open your browser — download Nexus yourself from {{url}}",
  'update.copyLink': 'Copy link',
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
  'logbook.column.notes': 'Notes',
  'logbook.row.notes.title': 'Comment (shared on the QSL)',
  'logbook.row.notes.private': 'Private note',
  'logbook.row.notes.aria': 'has a private note',
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
  'logbook.qsl.cardMarked': 'QSL card recorded for {{call}}',
  'logbook.qsl.cardCleared': 'QSL card cleared for {{call}}',
  'logbook.row.qslRcvd.card': 'Card received',
  'logbook.row.qslRcvd.clear': 'Card NOT received',
  'logbook.row.qslSent.clear': 'QSL NOT sent',
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
  'logbook.qsl.sentCleared': 'QSL sent mark cleared for {{call}}',
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
  'logEntry.fd.dupe.own': 'Dupe: {{call}} is already in this position\'s log on {{band}} {{mode}}',
  'logEntry.fd.dupe.club': 'Club dupe: another position already worked {{call}} on {{band}} {{mode}} — logging is allowed but adds no points',

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
  'roster.countFiltered': 'of {{count}}',
  'roster.search.placeholder': 'Call or PA*…',
  'roster.search.label': 'Search stations',
  'roster.search.title':
    'Search by callsign. * and ? are wildcards — PA* finds every PA prefix, ON?AOI fills in one character. Several terms mean "any of these": PA* ON4*. Esc clears.',
  'roster.search.clear': 'Clear search',
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
  // Band names and frequencies are invariant tokens — never translate '6 m' or '50 MHz'.
  'awards.grids.filter.vucc.label': 'VUCC bands',
  'awards.grids.filter.vucc.title': 'Show only the bands ARRL awards grids on — 50 MHz and up',
  'awards.grids.filter.all.label': 'All bands',
  'awards.grids.filter.all.title':
    'Show every band you have worked a grid on, HF included — a tracker count, not award progress',
  'awards.grids.noVucc':
    'No grids on 6 m or up yet — VUCC starts at 50 MHz. Switch to All bands for the full tracker count.',
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
  'needed.column.freq': 'Freq',
  'needed.row.freq.title': 'Spotted on {{freq}} MHz',
  'needed.row.freq.none': 'No exact frequency — this need is band-level only',
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
  'need.badge.confirm.label': 'LoTW',
  'need.badge.confirm.title':
    'This entity/zone/grid is worked on this band but not yet confirmed — a LoTW match or a paper card would close it (eQSL and QRZ do not count toward awards, so they do not clear this). Not a claim about this callsign: B4 is the worked-this-call chip.',
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
  'need.chip.confirm.label': 'LoTW',
  'need.chip.confirm.short': 'LoTW',
  'need.chip.confirm.title':
    'Worked on this band but not yet confirmed — a LoTW match or a paper card closes it; eQSL and QRZ do not count toward awards',
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

  // ── The map, the globes and the propagation panes ───────────────────────────────────
  // ⚠️ THE UNITS RULE IS THE WHOLE STORY ON THESE SURFACES, because almost everything they
  // put on screen is a MEASUREMENT. Absent from this file and staying in the code: grid
  // squares, callsigns, DXCC entity and region names, band names, mode names, bearings and
  // octants, distances in km, MUF/dial frequencies in MHz, signal reports in dB, knots,
  // percentages, SFI/Kp/A/Bz/X-ray index NAMES and values, R/S/G scale letters, satellite
  // names, CQ zone numbers, POTA/SOTA references, the P.533 recommendation number and every
  // layer/projection id. A decimal comma in any of them is an operating fault, not a
  // wording choice. Unit symbols (MHz, km, dB, min) ride INSIDE the sentence that carries
  // their number, so the number and its unit can never be separated by a translation.
  //
  // ⚠️ ALSO ABSENT, AND DELIBERATELY: the prose the BACKEND writes. Workability words
  // (Excellent/Good/Fair/Marginal), the band advisor's `reason`, each insight's `plain` and
  // `technical` sentences, `windowHint`, `howToCall`, the opening `confidence` word and
  // `note`, the space-weather alert `kind`/`message`, and the modelled-window headline
  // (`best`) all arrive over the wire already worded. They are phase-3, exactly as the
  // Journey block above says of `get_journey`. Where one of them lands inside a sentence
  // written here it is an interpolated value, never a fragment to translate.
  //
  // The state WORD in `dualStateLabel` (Open / Marginal / Closed) is also NOT here: it is
  // the backend's `BandModeled` enum passed through, and `components/connect/paneFormat.ts`
  // compares against it (`!== 'Closed'`). Translating it would break that comparison from a
  // file this batch does not own. Its sub-note IS ours, and is below.

  // The 2-D Beam Map (components/MapView.tsx). Shared with the 3-D globe where the two
  // surfaces are deliberately identical — the legends and the ★-filter hint.
  'map.empty.title': 'Set your grid to see the map',
  'map.empty.detail':
    'The Beam Map centers on your Maidenhead grid — set it in Settings, then every heading and range ring is measured from your QTH.',

  'map.projection.aria': 'Projection',
  'map.projection.globe.label': 'Globe',
  'map.projection.globe.title': '3-D globe — drag to spin, wheel to zoom',
  'map.projection.beam.label': 'Beam',
  'map.projection.beam.title': 'Beam map — true headings + range rings from your QTH',
  'map.projection.world.label': 'World',
  'map.projection.world.title': 'Flat world map with shaded relief',

  'map.zoom.aria': 'Zoom',
  'map.zoom.in': 'Zoom in',
  'map.zoom.out': 'Zoom out',

  'map.colorBy.aria': 'Color spots by',
  'map.colorBy.need.label': 'Need',
  'map.colorBy.need.title': 'Color spots by what you still need',
  'map.colorBy.snr.label': 'Signal',
  'map.colorBy.snr.title': 'Color spots by signal strength',

  // Provenance of the snapshot behind the map. The loading state is an ellipsis, which is
  // not prose and stays in the component.
  'map.prov.live': 'LIVE',
  'map.prov.partial': 'PARTIAL',
  'map.prov.cached': 'CACHED',
  'map.prov.none': 'NO LIVE DATA',

  'map.reset.label': 'Reset',
  'map.reset.title': 'Reset view + layers',

  // `SP` / `LP` are the ham abbreviations for the two great-circle paths and stay in the
  // component as named constants, exactly as the Q-codes on the logbook row do.
  'map.path.aria': 'Path',
  'map.path.short.title': 'Short path',
  'map.path.long.title': 'Long path',
  // A bearing and a distance — the unit rides with its number so the two can never be
  // separated by a translation.
  'map.path.figure': '{{brg}}° · {{km}} km',

  'map.emptyHint':
    'No located stations yet — decoded stations with a grid appear here, centered on {{grid}}, colored by what you still need.',
  // Not a plural entry: the shipped English says "all 1 satellites" when exactly one bird is
  // hidden, and this phase changes no visible text. A locale writes its own agreement.
  'map.sats.allHidden':
    'None of your ★ birds are in the current elements — the ★ filter is hiding all {{count}} satellites (Layers ▸ Satellites ▸ All).',
  'map.sats.filter.aria': 'Filter satellites to ★ birds',
  'map.sats.filter.on.title':
    'Showing your ★ birds (Passes pane + globe follow) — click to show all satellites',
  'map.sats.filter.off.title':
    'Showing all satellites — click to show only your ★ birds (Passes pane + globe follow)',
  // The chip reads ★ when the filter is on and this word when it is off.
  'map.sats.filter.all': 'All',

  'map.pca.chip':
    '☢ Proton event · S{{scale}} · polar caps absorbing ~{{db}} dB @30 MHz (day) — high-lat paths degraded',

  // The flare chip and its tail. Each phase is its OWN statement appended to the chip — see
  // the `logbook.import.dupes` note — because where "recovering" belongs in the sentence is
  // a decision for each language. `{{cls}}` is the GOES class (M2.1) and `{{r}}` the R-scale.
  'map.flare.chip': '☀️ {{cls}} flare · R{{r}} · HF ≤{{haf}} MHz absorbed on dayside',
  'map.flare.phase.rising': ' · rising',
  'map.flare.phase.recovering': ' · recovering',
  'map.flare.phase.recoveringIn': ' · recovering (~{{mins}} min)',
  'map.flare.phase.fade': ' · fade (~{{mins}} min)',
  'map.flare.phase.preview': ' · PREVIEW',
  'map.flare.preview.title':
    'Simulate an X2 flare on the map for 60 s — visual preview only (no alerts). The layer otherwise draws nothing until a real M-class flare.',
  'map.flare.preview.stop': '■ stop',
  'map.flare.preview.start': '☀ preview',

  // The layer panel. The layer IDS are code; only these names are read.
  'map.layers.head': 'Layers',
  'map.layer.opacity.aria': '{{layer}} opacity',
  'map.layer.daynight.label': 'Day / night (greyline)',
  'map.layer.relief.label': 'Relief (World view)',
  'map.layer.muf.label': 'Ionosonde MUF',
  'map.layer.aurora.label': 'Aurora oval',
  'map.layer.flare.label': 'Flare blackout (D-RAP)',
  'map.layer.pca.label': 'Proton polar cap (PCA)',
  'map.layer.coast.label': 'Coastlines',
  'map.layer.states.label': 'US states',
  'map.layer.grid.label': 'Grid (20°×10°)',
  'map.layer.gridLabels.label': 'Grid labels (AA…RR)',
  'map.layer.cqzones.label': 'CQ zones',
  'map.layer.coverage.label': 'My coverage (worked)',
  'map.layer.sats.label': 'Satellites (amateur)',
  'map.layer.aprs.label': 'APRS stations',
  'map.layer.rings.label': 'Range rings',
  'map.layer.heat.label': 'Band heat (openings)',
  'map.layer.openings.label': 'Opening sectors (mode)',
  'map.layer.liveSpots.label': 'Live spots (cluster/RBN)',
  'map.layer.stations.label': 'My decodes',
  'map.layer.paths.label': 'Selected path',
  'map.layer.dxped.label': 'DXpeditions',
  'map.layer.ota.label': 'Parks on the air',

  // The two legends, rendered by BOTH the 2-D map and the 3-D globe from one component —
  // the surfaces must explain their dots identically, so they share these keys. The band
  // names on the MUF ramp's ticks are tokens and stay in the component.
  'map.legend.newDxcc': 'new DXCC',
  'map.legend.newBand': 'new band',
  'map.legend.zoneMode': 'zone/mode',
  'map.legend.confirm': 'confirm',
  'map.legend.worked': 'worked',
  'map.legend.opening': 'opening',
  'map.legend.heat.label': 'heat = band activity',
  'map.legend.heat.title': 'Colored auras = live spot density per band; pulsing = a detected opening',
  'map.legend.muf.title': 'Ionosonde MUF → band',

  'map.coverage.dim.aria': 'Coverage dimension',
  'map.coverage.dim.title': 'What to color: your worked grid squares (VUCC) or CQ zones (WAZ)',
  'map.coverage.dim.grids': 'Grids',
  'map.coverage.dim.zones': 'CQ zones',

  // Hover tooltips. The station line is pure measurement and is not here at all. Each of
  // these carries its own leading separator where it is an appended clause.
  'map.hover.workHint': ' — double-click to work',
  'map.hover.liveConfirmed': ' · live-confirmed',
  'map.hover.dxped': '{{call}} · {{entity}}{{az}} · {{need}} on {{band}} · {{likelihood}}',
  // Parks on the air. `{{badge}}` marks a park never logged; `{{approx}}` admits a
  // grid-placed marker is a ~4 km square rather than the park itself.
  'map.hover.ota': '{{activator}} · {{reference}}{{name}} · {{freq}} MHz {{mode}}{{badge}}{{approx}}',
  'map.hover.ota.new': ' · NEW PARK',
  'map.hover.ota.approx': ' · approx',
  'map.hover.muf':
    'Ionosonde · measured MUF {{muf}} MHz here (KC2G) — a data point, not a station',
  // `{{what}}` (the symbol's own label), `{{moving}}` (speed/course) and `{{note}}` (the
  // packet's free text) are assembled from data by the call site.
  'map.hover.aprs': '{{call}}{{what}} · {{how}} {{when}}{{via}}{{moving}}{{note}}',
  'map.hover.aprs.how.rf': 'heard on RF',
  'map.hover.aprs.how.inet': 'reported by APRS-IS',
  'map.hover.aprs.how.both': 'heard on RF + APRS-IS',
  'map.hover.aprs.ageSecs': '{{secs}}s ago',
  'map.hover.aprs.ageMins': '{{mins}}m ago',
  'map.hover.aprs.via': ' · via {{path}}',
  'map.hover.aprs.direct': ' · direct',

  // The map's right-edge insight rail (components/prop/MapInsightRail.tsx). `MUF` is the
  // acronym and stays in the component; the three trend arias are whole strings rather than
  // a stem plus a direction word.
  'map.insights.collapsed.title': 'Show propagation insights',
  'map.insights.pill': 'Conditions',
  'map.insights.aria': 'Propagation insights',
  'map.insights.title': 'Conditions',
  'map.insights.collapse.title': 'Collapse',
  'map.insights.muf.title':
    'Maximum Usable Frequency — the modelled DX ceiling right now; bands below it are open',
  'map.insights.muf.value': '{{muf}} MHz',
  'map.insights.muf.aria.rising': 'MUF rising',
  'map.insights.muf.aria.falling': 'MUF falling',
  'map.insights.muf.aria.steady': 'MUF steady',
  'map.insights.bands.head': 'Band conditions',
  'map.insights.outlook.head': 'Outlook',
  'map.insights.heatmap.head': 'Modelled band × hour',

  // The 3-D Connect globe (components/Globe3D.tsx). Its own layer vocabulary — shorter than
  // the 2-D map's and NOT the same list, so the two must not share keys.
  'globe.unsupported':
    "This machine's graphics can't run the 3-D globe. Switch back to the 2-D map (🌐 button) — it works everywhere.",
  'globe.spin.stop.title': 'Stop the globe spinning',
  'globe.spin.start.title': 'Spin the globe',
  'globe.spin.pause': '⏸ Spin',
  'globe.spin.play': '▶ Spin',
  'globe.layers.head': 'Layers',
  'globe.layer.spots': 'Spots',
  'globe.layer.decodes': 'My decodes',
  'globe.layer.arcs': 'Heard-me arcs',
  'globe.layer.dxped': 'DXpeditions',
  'globe.layer.heat': 'Band heat',
  'globe.layer.openings': 'Opening sectors',
  'globe.layer.flare': 'Flare blackout',
  'globe.layer.aurora': 'Aurora',
  // The MUF layer's whole name is the acronym — a token, and a constant in the component.
  'globe.layer.pca': 'Polar cap (PCA)',
  'globe.layer.greyline': 'Greyline',
  'globe.layer.sats': 'Satellites',
  'globe.layer.pass': 'Tracked pass',
  'globe.layer.rings': 'Range rings',
  'globe.layer.cqzones': 'CQ zones',
  'globe.layer.coverage': 'My coverage',
  'globe.layer.states': 'US states',
  'globe.layer.grid': 'Graticule',
  'globe.layer.lights': 'City lights',
  'globe.sats.filter.aria': 'Filter satellites to ★ birds',
  'globe.sats.filter.on.title':
    'Showing your ★ birds (Passes pane + 2-D map follow) — click to show all satellites',
  'globe.sats.filter.off.title':
    'Showing all satellites — click to show only your ★ birds (Passes pane + 2-D map follow)',
  'globe.sats.filter.all': 'All',
  // The tracked pass in words — the text equivalent of a WebGL scene a screen reader
  // cannot see. `El`/`Az`/`LOS` are the standard satellite abbreviations.
  'globe.pass.aria': 'Tracked pass: {{name}}',
  'globe.pass.elAz': 'El {{el}}° · Az {{az}}°',
  'globe.pass.range': '{{km}} km',
  'globe.pass.losIn': 'LOS in {{mmss}}',

  // The Logbook's world-of-contacts globe (components/QsoGlobe.tsx) — `logbook.globe.*`
  // because it is a band of the Logbook view, not a map surface of its own.
  'logbook.globe.spin.stop.title': 'Stop the slow rotation',
  'logbook.globe.spin.start.title': 'Start the slow rotation',
  'logbook.globe.spin.pause': '⏸ Spin',
  'logbook.globe.spin.play': '▶ Spin',
  'logbook.globe.band.title':
    "Grid squares are a per-band achievement (VUCC) — view one band's squares on their own",
  // The <option> VALUE 'all' is a token and stays in the code; only this label is read.
  'logbook.globe.band.all': 'All bands',
  'logbook.globe.count.all': {
    one: '{{count}} grid square worked',
    other: '{{count}} grid squares worked',
  },
  'logbook.globe.count.band': {
    one: '{{count}} grid square on {{band}}',
    other: '{{count}} grid squares on {{band}}',
  },

  // ── Propagation presentation (propViz.ts and the prop panes) ────────────────────────
  // `propViz.ts` is a pure formatting module: every string it returns is read straight
  // into a tooltip, so its words are looked up when the string is built, never at import.
  'prop.rarity.rare.label': 'RARE',
  'prop.rarity.rare.title': 'Rare grid — almost no land (small island or coastal sliver)',
  'prop.rarity.ultra.label': 'ULTRA',
  'prop.rarity.ultra.title':
    'Ultra-rare grid — open water: only rovers, maritime mobiles, or DXpeditions can activate it',

  // Plain-language HF impact for each space-weather index. The NUMBER stays visible beside
  // it in the component (project rule: never hide the physics), so these say only what it
  // means for the bands.
  'prop.impact.sfi.high': 'high flux — upper bands lively',
  'prop.impact.sfi.moderate': 'moderate flux — 20–15 m workable',
  'prop.impact.sfi.low': 'low flux — high bands sluggish',
  'prop.impact.kp.storm': 'geomag storm — polar paths degraded',
  'prop.impact.kp.unsettled': 'unsettled — high-lat paths soft',
  'prop.impact.kp.quiet': 'quiet field — stable paths',
  'prop.impact.bz.hardSouth': 'field hard south — storm likely, polar paths fading',
  'prop.impact.bz.south': 'field south — high-lat paths softening soon',
  'prop.impact.bz.neutral': 'field neutral/north — stable',
  'prop.impact.a.storm': 'stormy day — HF rough, polar paths out',
  'prop.impact.a.active': 'active day — paths up and down',
  'prop.impact.a.unsettled': 'unsettled day — minor fading spells',
  'prop.impact.a.quiet': 'quiet day — conditions steady',
  'prop.impact.xray.flare': 'flare — low-band shortwave fade',
  'prop.impact.xray.cClass': 'C-class — minor low-band absorption',
  'prop.impact.xray.none': 'no significant flares',

  // Live band timing. `{{when}}` is a duration the call site formats from digits and the
  // h/m unit letters; `{{at}}` is a UTC time (1500Z) — both invariant.
  'prop.bandTiming.openNowHours': 'open now · ~{{hours}}h left',
  'prop.bandTiming.openNowMins': 'open now · ~{{mins}}m left',
  'prop.bandTiming.opensIn': 'opens in ~{{when}} ({{at}})',

  // The sub-note under the dual state word — the fix that stops a quiet band reading dead.
  'prop.state.sub.active': 'active',
  'prop.state.sub.someActivity': 'some activity',
  'prop.state.sub.noneHeard': 'none heard',

  // The shared spot tooltip (2-D map AND 3-D globe read identically, by design). Each
  // optional clause is its own statement carrying its own separator.
  'prop.spotTooltip': '{{call}} · {{band}}{{mode}}{{freq}} · {{age}} ago',
  'prop.spotTooltip.heardMe': ' · heard YOU',
  'prop.spotTooltip.approx': ' · ~location',

  // The satellite tooltip. `{{star}}` is ★/☆, `{{alt}}` the live altitude clause.
  'prop.satTooltip': '{{name}} {{star}}{{alt}} · {{when}}{{click}} · dbl-click: favorite',
  'prop.satTooltip.alt': ' · alt {{km}} km',
  'prop.satTooltip.noPass': 'no pass over you in 24 h',
  'prop.satTooltip.inPass': 'IN PASS now · max {{maxEl}}°',
  'prop.satTooltip.nextPass': 'next pass {{at}} (in {{mins}} min) · max {{maxEl}}°',
  'prop.satTooltip.clickForPasses': ' — click for passes',

  // One gesture, one wording, on every pane that offers it — the band-focus tooltip.
  'prop.focusBand.title': 'Focus {{band}} on the map',

  // The band advisor.
  'prop.bands.aria': 'Band activity',
  'prop.bands.head.you': 'Bands — best for you',
  'prop.bands.head.world': 'Bands — worldwide activity',
  'prop.bands.view.aria': 'Band ranking view',
  'prop.bands.view.you.label': 'For you',
  'prop.bands.view.you.title': 'Bands ranked by what YOU can reach now (own-call + near-region)',
  'prop.bands.view.world.label': 'Worldwide',
  'prop.bands.view.world.title':
    'Bands ranked by GLOBAL activity — busy, but not necessarily workable from your QTH',
  'prop.bands.clearFocus.title': 'Clear the band focus',
  'prop.bands.focused': 'focused: {{band}} ✕',
  'prop.bands.caption.you':
    'Ranked by what you can actually reach now — your own-call paths + stations near you.',
  'prop.bands.caption.world':
    'Busiest bands worldwide — loud somewhere, not necessarily workable from your QTH.',
  'prop.bands.modelled.title': 'Modelled: {{reason}}',
  'prop.bands.people.title': 'stations that hear you / you hear',

  // The hamqsl-style condition strip. `{{sub}}` is the dual-state sub-note with its
  // separator; `{{reason}}` is the backend's own sentence.
  'prop.bandConditions.aria': 'Band conditions',
  'prop.bandConditions.cell.title': '{{band}}: {{state}}{{sub}} — {{reason}}',

  // Best band to each region.
  'prop.bestBand.stations.title': 'anchored stations (⇄ = both directions)',

  // Continent × band activity matrix.
  'prop.activityMatrix.corner.aria': 'band',
  'prop.activityMatrix.cell.title': {
    one: '{{region}} {{band}}: {{count}} stn ({{hearMe}} hear you, {{iHear}} you hear)',
    other: '{{region}} {{band}}: {{count}} stns ({{hearMe}} hear you, {{iHear}} you hear)',
  },
  'prop.activityMatrix.cell.empty': '{{region}} {{band}}: —',

  // NCDXF/IARU beacon monitor — `{{qth}}` is the beacon's own city.
  'prop.beacons.title': '{{qth}} · {{freq}} MHz',

  // Getting-out compass rose.
  'prop.getout.aria': 'Compass rose of where your signal is reaching',
  'prop.getout.spoke': {
    one: '{{octant}}: {{count}} station, out to {{km}} km',
    other: '{{octant}}: {{count}} stations, out to {{km}} km',
  },

  // The predictive insight feed — both sentences in each row come from the backend.
  'prop.insightFeed.aria': 'Predictive insights',

  // Band × UTC-hour likelihood heatmap. The per-cell tooltip is band, hour and percent —
  // pure measurement, so it is not here.
  'prop.heatmap.aria': 'Band by UTC-hour contact-likelihood heatmap',
  'prop.heatmap.band.title': '{{band}} — {{workability}} · {{pct}}% of the day usable (modelled)',
  'prop.heatmap.legend': 'less likely → more likely',

  // Live measured ionosonde MUF.
  'prop.measuredMuf.none': 'No live ionosonde MUF right now.',
  'prop.measuredMuf.value': '{{muf}} MHz',
  'prop.measuredMuf.fof2': 'foF2 {{value}}',

  // SWPC R/S/G scales. The letters R, S and G are the scale names and stay in the code.
  'prop.scales.none': 'No live space-weather scales right now.',
  'prop.scales.r.title': 'Radio blackout — HF absorption on sunlit paths',
  'prop.scales.s.title': 'Solar radiation storm — polar HF',
  'prop.scales.g.title': 'Geomagnetic storm — high-lat paths + aurora',
  'prop.scales.tomorrow': 'G{{level}}↗ tmrw',
  'prop.scales.tomorrow.title': "Tomorrow's forecast geomagnetic level",

  // Space-weather gauges. The index NAMES (SFI, Kp, A, X-ray, Bz) are technical tokens and
  // stay in the component; these are the Simple-mode plain-English glosses.
  'prop.spaceWx.aria': 'Space weather',
  'prop.spaceWx.gloss.sfi':
    'Solar Flux Index — how energized the ionosphere is. Higher opens the upper HF bands (20–10 m). ~70 is low; 150+ is great.',
  'prop.spaceWx.gloss.kp':
    'Geomagnetic activity, 0–9. Low is calm and good for DX; 5+ is a storm that fades the high bands and polar paths.',
  'prop.spaceWx.gloss.a':
    'A-index — a daily summary of geomagnetic disturbance. Lower is quieter and better for DX.',
  'prop.spaceWx.gloss.xray':
    'Solar X-ray flare level (A/B/C/M/X). An M- or X-class flare can briefly black out the low bands.',

  // The greyline pane. The two next-terminator lines are whole sentences, not a stem plus
  // "Sunrise"/"Sunset" — that word is the subject, and a translator must be free to move it.
  'prop.greyline.noGrid': 'Set your grid in Settings to see your greyline windows.',
  'prop.greyline.next.rise': '<b>Sunrise</b> greyline in <b>{{when}}</b> ({{at}})',
  'prop.greyline.next.set': '<b>Sunset</b> greyline in <b>{{when}}</b> ({{at}})',
  'prop.greyline.favors': '◐ greyline favors {{bands}}',
  'prop.greyline.onGrey': 'On the greyline now (point your beam): {{entities}}',
  'prop.greyline.none': 'No greyline DX paths lit right now.',

  // The loud 6 m/VHF opening strip.
  'prop.opening.focus.title': 'Focus {{band}} on the map — where IS this opening?',
  'prop.opening.bandOpen': '{{band}} OPEN',
  'prop.opening.new': 'NEW',
  'prop.opening.detail':
    'point {{octant}} · ~{{km}} km · {{stations}} stations{{reciprocal}} · {{confidence}}{{opened}}',
  'prop.opening.reciprocal': ' ({{count}} 2-way)',
  'prop.opening.opened': ' · opened {{ago}}',
  'prop.opening.ago.justNow': 'just now',
  'prop.opening.ago.mins': '{{mins}}m ago',
  'prop.opening.ago.hours': '{{hours}}h ago',

  // The openings log — the historical record. Column names are prose; every value under
  // them is a token.
  'prop.openingsLog.filter.aria': 'Filter openings by band',
  'prop.openingsLog.filter.all': 'All',
  'prop.openingsLog.count': { one: '{{count}} opening', other: '{{count}} openings' },
  'prop.openingsLog.empty': 'No {{filter}} openings recorded yet.',
  'prop.openingsLog.sort.title': 'Sort by {{column}}',
  'prop.openingsLog.column.band': 'Band',
  'prop.openingsLog.column.mode': 'Mode',
  'prop.openingsLog.column.when': 'When',
  'prop.openingsLog.column.duration': 'Dur',
  'prop.openingsLog.column.dx': 'DX',
  'prop.openingsLog.column.stations': 'Stns',
  'prop.openingsLog.duration.partial.title': 'Already open at app start — duration under-counts',
  'prop.openingsLog.dx.title': 'Longest path seen during the opening',
  'prop.openingsLog.dx': '~{{km}} km {{octant}}',
  'prop.openingsLog.stations.title': 'Most stations heard in one window',
  'prop.openingsLog.stations': '{{count}} stns',

  // The opening TOASTS (openingAlert.ts). Every one is a whole sentence: the tier decides
  // which fires, and a fragment shared between two tiers could not carry their different
  // urgency into another language.
  'prop.openingAlert.sporadicE':
    '⚡ {{band}} SPORADIC-E — rare & brief, point {{octant}} NOW · DX ~{{km}} km · {{stations}} stns',
  'prop.openingAlert.aurora':
    '🌌 {{band}} AURORA — beam NORTH (not at the station); signals sound raspy/buzzy, CW & SSB work best',
  'prop.openingAlert.f2':
    '⚡ {{band}} F2 opening — real DX, point {{octant}} · ~{{km}} km · {{stations}} stns',
  'prop.openingAlert.tropo':
    '📡 {{band}} tropo opening — DX to ~{{km}} km, point {{octant}} · {{stations}} stns',
  'prop.openingAlert.generic': '⚡ {{band}} open — point {{octant}} · {{stations}} stns',
  // Geomagnetic storm heads-up (stormAlert.ts). A storm is hours-to-days of degraded
  // HF, unlike a flare's minutes — the copy says what it means for operating, and the
  // forecast line is explicitly a forecast.
  'prop.stormAlert.now': '🧲 Geomagnetic storm G{{g}} (Kp {{kp}}) — HF degraded, worst on polar paths; aurora possible on VHF',
  'prop.stormAlert.forecast': '🧲 NOAA expects G{{g}} (Kp {{kp}}) from {{when}} — HF likely degraded then',
  'prop.openingAlert.thin':
    '📻 {{band}} possible {{mode}} — thin evidence: {{stations}} stns to ~{{km}} km {{octant}}; may not be audible by ear',

  // ── Chasing (the chase panes and the ranked chase feed) ─────────────────────────────
  'chase.row.show.title': 'Show {{call}} on the map',
  'chase.row.point.title': 'Point the antenna at {{call}}',
  'chase.row.work.title': 'Rig jumps to this band/mode/frequency; the cockpit opens',
  'chase.row.work.label': '▶ Work',
  'chase.age.secs': '{{secs}}s ago',
  'chase.age.mins': '{{mins}}m ago',
  // The row's action line. `{{workability}}` and `{{window}}` are the backend's words.
  'chase.open.now': '{{band}} is open ({{workability}}) — call now',
  'chase.open.marginal': '{{band}} marginal',
  'chase.open.closed': '{{band}} closed now · best {{window}}',
  'chase.open.best': ' · best {{window}}',

  'chase.feed.dxped.label': 'DXP',
  'chase.feed.dxped.title': 'DXpedition',
  'chase.feed.endsSoon.label': 'last days',
  'chase.feed.endsSoon.title': 'This operation ends within 3 days',
  'chase.feed.gem.rare.title': 'Rare grid — almost no land',
  'chase.feed.gem.ultra.title': 'Ultra-rare grid — open water',
  'chase.feed.empty':
    'Nothing chase-worthy right now — targets appear as needed stations are heard or expeditions come on the air.',
  'chase.feed.summary': '{{count}} to chase ({{openness}}). Top: {{top}}{{entity}} — {{why}}.',
  'chase.feed.workableNow': '{{count}} workable now',
  'chase.feed.noneOpen': 'none open this minute',
  // "Why chase this now" — one whole line per case, plus two appended clauses.
  'chase.why.spotted': 'on the air now (spotted) — {{band}}',
  'chase.why.modelledOpen': '{{band}} path modelled open — {{likelihood}}',
  'chase.why.likelihood': '{{band}} {{likelihood}}',
  'chase.why.best': ' · best {{best}}',
  'chase.why.lastDays': '{{why}} · last days!',
  'chase.why.openNow': '{{band}} open now — call it',
  'chase.why.closedBest': '{{band}} closed now · best {{window}}',
  'chase.why.heardOn': 'heard on {{band}}',

  // ── DXpeditions (the board, the calendar, the chase flag and the wake-me alarm) ──────
  'dxped.loading.title': 'Reading the expedition feeds…',
  'dxped.loading.detail': "Fetching the announced-operations calendar and who's active now.",
  'dxped.hero.onAir': {
    one: '{{count}} DXpedition on the air now · {{announced}} announced',
    other: '{{count}} DXpeditions on the air now · {{announced}} announced',
  },
  'dxped.hero.noneOnAir': 'No expeditions on the air right now — {{announced}} announced and coming',
  'dxped.hero.none': 'No expeditions announced right now',
  'dxped.prov.title': 'Data provenance',
  'dxped.prov.live': 'LIVE',
  'dxped.prov.partial': 'PARTIAL',
  'dxped.prov.cached': 'CACHED {{mins}}m',
  'dxped.prov.none': 'NO LIVE DATA',
  'dxped.popOut.title': 'Open DXpeditions in its own window (for a second monitor)',
  'dxped.popOut.label': '⧉ Pop out',
  'dxped.workNow.aria': 'Workable now',
  'dxped.workNow.head': 'Work now — needed × on the air',
  'dxped.workNow.none':
    'Nothing you need is workable right now. New ones appear here the moment a needed expedition is on a band with a real path to you.',
  'dxped.showOnMap.title': 'Open Connect with this expedition selected on the map',
  'dxped.showOnMap.label': '◎ show on map',
  'dxped.calendar.empty': 'The forward calendar is empty — announced operations land here.',

  // One card. `P.533` is the ITU recommendation's number and stays in the component.
  'dxped.engine.modelled': 'modelled',
  'dxped.card.live.title': 'Live PSK Reporter spots confirm this band toward the DX region',
  'dxped.card.live.label': 'live spots',
  'dxped.card.geo': '{{octant}}{{az}} · {{km}} km',
  'dxped.card.bestShot': 'Best shot: {{band}} {{workability}} {{window}}',
  'dxped.card.details.title': 'The full 24h × band reliability grid for this path',
  'dxped.card.details.show': '▸ details',
  'dxped.card.details.hide': '▾ details',
  'dxped.card.work.title': 'Jump the rig to {{band}} and open the right cockpit',
  'dxped.card.work.label': '▶ Work {{band}}',

  // The ★ chase toggle — the card and the calendar row are the same control, so one key.
  'dxped.chase.toggle.on.title':
    'Chasing — you get an alert when your window opens and they are spotted. Click to stop.',
  'dxped.chase.toggle.off.title':
    'Chase this expedition — alert me when my modelled window opens and live spots confirm them',
  'dxped.chase.open.loud': '🎯 {{call}} window open NOW — {{band}}, spotted on the air',
  'dxped.chase.open.quiet': '{{call}}: modelled window open ({{best}}) — not yet spotted',
  'dxped.chase.work': 'Work',

  // The wake-me alarm. Two whole statements for the window rather than a stem plus a time.
  'dxped.alarm.toggle.on.title':
    'Alarm armed — a loud in-app wake-up fires {{lead}} min before your modelled window opens. Click to disarm.',
  'dxped.alarm.toggle.off.title':
    'Wake me — arm a loud in-app alarm for when your modelled window to this expedition opens',
  'dxped.alarm.lead.title': 'How long before the window opens to wake you',
  'dxped.alarm.lead.aria': 'Alarm lead time',
  'dxped.alarm.lead.option': '{{mins}} min',
  'dxped.alarm.toast': '⏰ {{call}} — your modelled window {{opens}} · {{best}}',
  'dxped.alarm.openNow': 'is OPEN now',
  'dxped.alarm.opensAt': 'opens {{at}} (~{{mins}} min)',
  // Stops the repeating BEEP, not a transmission — this is not a stop-line control.
  'dxped.alarm.stop': 'Stop alarm',

  // The forward calendar. Weekday and month abbreviations are date formatting, not catalog
  // prose, and stay in the components with the rest of the date handling.
  'dxped.calendar.aria': 'DXpedition calendar',
  'dxped.calendar.head': 'DXpedition calendar — when to plan your chase',
  'dxped.calendar.view.aria': 'Calendar view',
  'dxped.calendar.view.month': 'Calendar',
  'dxped.calendar.view.list': 'Details',
  'dxped.calendar.onAir': 'on the air',
  'dxped.calendar.week.title': 'Your modelled best shot for each of the next 7 days — plan the chase',
  'dxped.calendar.day.title': '{{day}}: {{best}}',
  'dxped.calendar.day.noPath': 'no modelled path',
  'dxped.calendar.day.offAir': '{{day}}: not on the air',
  'dxped.calendar.openFailed': 'Could not open the page for {{call}}',

  // Where a calendar entry sends the operator. `QRZ` is the site's name and stays in the
  // module; the tooltip always names the real destination.
  'dxped.link.website': 'Website',
  'dxped.link.site.title': "Open the expedition's website — {{url}}",
  'dxped.link.qrz.title': 'No website announced — open their QRZ page instead ({{url}})',

  // "What should I chase, and when" — the digest above the calendar.
  'dxped.digest.aria': 'What to chase',
  'dxped.digest.head': 'What to chase',
  'dxped.digest.onAir': 'ON THE AIR',
  'dxped.digest.onAirNow': 'on the air now',
  'dxped.digest.startsTomorrow': 'starts tomorrow',
  'dxped.digest.startsInDays': 'starts in {{days}} days',
  'dxped.digest.bestDays': 'best {{days}}',

  // The month grid. The bar's own detail line is call, entity, dates, bands and modes —
  // all tokens — so only these two appended clauses and the overflow control are here.
  'dxped.month.aria': 'DXpedition month grid',
  'dxped.month.today': 'TODAY',
  'dxped.month.bar.chasing': ' · chasing',
  'dxped.month.bar.opens': ' · opens {{url}}',
  'dxped.month.more.title': {
    one: '{{count}} more operation on this day — show the whole week',
    other: '{{count}} more operations on this day — show the whole week',
  },
  'dxped.month.showFewer': 'show fewer',

  // ── Share cards (rendered locally, never uploaded) ──────────────────────────────────
  'share.copied': 'Share card copied — paste it anywhere',
  'share.saved': 'Share card saved → {{path}}',
  'share.saveFailed': 'Could not save the share card: {{detail}}',

  // ── Spots, watchlists and the display filters ───────────────────────────────────────
  // ⚠️ A SPOT IS ALL TOKENS. Everything these surfaces SHOW about one is invariant and is
  // therefore absent from this file: callsigns and spotter callsigns, DXCC entity names, US
  // state codes, band names, mode and submode names (CW, SSB, FT8, FT4, RTTY, PSK), dial
  // frequencies in MHz, the prefixes and grid squares an operator types into a watch/hide
  // filter, and the comment the spotter sent. `de` (the cluster's "from"), the P/S/✈/B badge
  // glyphs and the POTA/SOTA programme names live in the components as named constants.
  // Where a token appears INSIDE a sentence below — VP8*, NCDXF, W1AW, DXCC, CQ, QRM, QSY,
  // FCC Part 97, VUCC/FFMA, 6 m — a translator leaves it exactly as it is, the same rule
  // `ADIF OPERATOR` follows in the Station hint.
  //
  // Two boards, deliberately not one set of keys: the Needed board (`needed.*`) is the
  // curated "what to work" list and this is the firehose, and they already word their
  // filters and empty states differently.
  'spots.title': 'Spots',
  'spots.countFiltered': 'of {{count}}',
  'spots.hint': 'every spot on the air — single-click to work it',
  'spots.search.placeholder': 'Search call · entity · spotter · freq…  (PA* works)',
  'spots.search.label': 'Search spots',
  'spots.search.clear': 'Clear search',
  'spots.popOut.label': '⧉ Pop out',
  'spots.popOut.title': 'Open in its own window',

  'spots.filters.aria': 'Filter spots',
  'spots.filters.modes.aria': 'Modes shown',
  'spots.filters.states.aria': 'US states shown',
  'spots.filter.toggle.title': 'Filter spots by band, mode, state, or privileges',
  'spots.filter.toggle.active': 'Filtered',
  'spots.filter.toggle.idle': 'Filter',
  // `{{mode}}` is a mode name and `{{state}}` a US state code — the tooltip is prose, the
  // token in it is not.
  'spots.filter.mode.show.title': 'Show {{mode}} spots',
  'spots.filter.mode.hide.title': 'Hide {{mode}} spots',
  'spots.filter.state.title':
    "Show only {{state}} spots (state resolved from stations you've heard before)",
  'spots.filter.local.label': 'Heard on my continent',
  'spots.filter.local.hidden': 'Heard on my continent · {{count}} hidden',
  'spots.filter.local.title':
    'Show only spots someone on your continent actually heard. A station reported solely from another continent says nothing about a path from your station — the same test the Needed board uses. Turn it off for the worldwide cluster feed.',
  'spots.filter.privileges.label': 'My privileges',
  'spots.filter.privileges.title':
    'Show only spots you may transmit to under your license class (Settings ▸ license). Open class sees everything either way.',
  'spots.filter.clear.label': 'Clear',
  'spots.filter.clear.title': 'Clear all filters',

  // The grid. Column headings name a CONCEPT; every value under them is a token.
  'spots.column.age': 'Age',
  'spots.column.call': 'Call',
  'spots.column.entity': 'Entity',
  'spots.column.state': 'St',
  'spots.column.band': 'Band',
  'spots.column.freq': 'Freq',
  'spots.column.mode': 'Mode',
  'spots.column.spotter': 'Spotter',
  'spots.column.comment': 'Comment',
  'spots.empty.filtered': 'No spots match the current filters — clear to see all.',
  'spots.empty': 'No spots yet — cluster/RBN spots appear here as they arrive.',
  // ⚠️ `{{freq}}` is a dial frequency the call site has already formatted invariantly. Two
  // whole tooltips, not a stem plus a "Work …" head: a row you cannot QSY to says something
  // different, and where the callsign belongs in each is a decision for each language.
  'spots.row.work.title': 'Work {{call}} — {{mode}} @ {{freq}} MHz (spotted by {{spotter}})',
  'spots.row.title': '{{call}} @ {{freq}} MHz (spotted by {{spotter}})',
  'spots.row.mode.title': '{{mode}} spot',
  'spots.row.mode.submode.title': '{{submode}} spot ({{mode}})',

  // Composing a spot for the cluster (opened from a log row or a cockpit).
  'spots.post.aria': 'Spot a callsign',
  'spots.post.title': 'Spot to the DX cluster',
  'spots.post.call.label': 'Callsign',
  'spots.post.freq.label': 'Frequency (MHz)',
  'spots.post.comment.label': 'Comment',
  // An example cluster comment. The mode name, the split offset and the report are tokens a
  // translator leaves; the word between them is theirs.
  'spots.post.comment.placeholder': 'e.g. FT8 up 2 · loud · 599',
  'spots.post.cancel': 'Cancel',
  'spots.post.submit': 'Spot',
  'spots.post.busy': 'Spotting…',
  'spots.post.done': 'Spotted {{call}} on the cluster',
  'spots.post.failed': 'Spot failed',

  // The colour + type key, rendered by BOTH the band strip and the band map from one
  // component — the two surfaces must explain their dots identically, so they share these
  // keys. The need COLOURS are named by `need.chip.*`; these are the type badges beside them.
  'spots.legend.aria': 'Spot colour + type key',
  'spots.legend.pota.title': 'Live POTA activator — the call is on a park now',
  'spots.legend.sota.title': 'Live SOTA activator — the call is on a summit now',
  'spots.legend.dxped.label': 'DXped',
  'spots.legend.dxped.title': 'Active announced DXpedition — a limited-time window',
  'spots.legend.beacon.label': 'Beacon',
  'spots.legend.beacon.title':
    'One-way transmission (NCDXF/IARU beacon or W1AW bulletin) — real propagation evidence, but it never answers, so it is never scored as a need',
  // The badge WORDS, read out of the shared badge tables into a spot's tooltip. POTA and
  // SOTA are the programmes' own names and stay in the component beside them.
  'spots.type.dxped': 'DXpedition',
  'spots.beacon.ncdxf': 'NCDXF beacon — one-way, not workable',
  'spots.beacon.w1aw': 'W1AW bulletin — one-way, not workable',

  // ── The vertical band map (the N1MM-style torn-off window) ──────────────────────────
  // ⚠️ Band and mode names, dial frequencies and every spot's own data are tokens and stay
  // in the component. `{{mode}}` here is CW or SSB — the plotted spot mode, not prose.
  'bandMap.title': 'Band map',
  'bandMap.offPlan': '{{band}} — off the band plan',
  'bandMap.empty.noPlan': 'no band-plan data for {{band}}',
  // What stands in for the band name when the dial is somewhere the plan cannot name.
  'bandMap.empty.thisFrequency': 'this frequency',
  'bandMap.count': {
    one: '{{count}} {{mode}} spot · {{band}}',
    other: '{{count}} {{mode}} spots · {{band}}',
  },
  // Its own statement with its own count and its own leading separator — see the
  // `logbook.import.dupes` note; one message cannot select a plural form for two counts.
  'bandMap.count.more': ' · {{count}} more',
  'bandMap.empty.none': 'no {{mode}} spots on {{band}} yet',
  'bandMap.legend.label': 'Legend',
  'bandMap.legend.title': 'Show/hide the colour + type key',
  'bandMap.dock.left.title': 'Dock to the left screen edge',
  'bandMap.dock.right.title': 'Dock to the right screen edge',
  'bandMap.dock.left.titleRemembered':
    'Dock this window to the left screen edge (full-height strip, remembered)',
  'bandMap.dock.right.titleRemembered':
    'Dock this window to the right screen edge (full-height strip, remembered)',
  'bandMap.track.title': '{{band}} — high at top, low at bottom (MHz)',
  'bandMap.track.title.tunable':
    '{{band}} — high at top, low at bottom (MHz). Click to tune here; scroll to tune.',
  'bandMap.shade.title': 'Your licensed phone segment on this band',
  // `{{detail}}` is the spot line — call, frequency, age, badges, spotter and comment —
  // assembled from data by the call site.
  'bandMap.spot.title': '{{detail}} — click to work',
  'bandMap.age.secs': '{{secs}}s ago',
  'bandMap.age.mins': '{{mins}}m ago',
  'bandMap.age.hours': '{{hours}}h ago',
  // The "you are here" marker. Two whole tooltips: the blocked one is a different statement,
  // not a tail. It is a READOUT, not a transmit control.
  'bandMap.dial.title': 'You: {{freq}} MHz',
  'bandMap.dial.title.blocked':
    'You: {{freq}} MHz — transmit blocked (outside your privileges)',

  // ── Pounce (the rare-one banner) ────────────────────────────────────────────────────
  // The need word comes from `need.badge.*`; this is only what shows when the alert carries
  // a tag the chip vocabulary has no entry for.
  'pounce.tag.fallback': 'NEW',
  'pounce.work.label': 'Work it',
  'pounce.work.title': 'QSY to {{call}} and start the QSO',
  'pounce.dismiss.aria': 'Dismiss alert',

  // ── The watch list ("alert me loudly when THIS shows up") ───────────────────────────
  // ⚠️ The prefixes, grid squares and entity names in the examples are TOKENS supplied by
  // the component (WATCH_EXAMPLES); the ones quoted inside the hint stay in the sentence,
  // as `ADIF OPERATOR` does elsewhere. `DXCC` is a programme name, not a word.
  'watchlist.hint':
    "Get a loud alert when a matching station is decoded — a callsign or prefix (wildcards: <code>VP8*</code>, <code>*ABC</code>), a whole DXCC entity, or a grid square (<code>FN31</code>, <code>EM7*</code>). A grid you name here alerts on every band — the HF grid-quiet default doesn't apply to squares you asked for.",
  'watchlist.item.kind.call': 'CALL',
  'watchlist.item.kind.grid': 'GRID',
  'watchlist.item.cqOnly': 'CQ only',
  'watchlist.item.remove.title': 'Remove from watch list',
  // `{{value}}` is the call, prefix, grid or entity the operator typed.
  'watchlist.item.remove.aria': 'Remove {{value}}',
  'watchlist.add.kind.aria': 'Watch kind',
  // The <option> VALUES ('call', 'dxcc', 'grid') are persisted tokens and stay in the code.
  'watchlist.add.kind.call': 'Call / prefix',
  'watchlist.add.kind.dxcc': 'DXCC entity',
  'watchlist.add.kind.grid': 'Grid square',
  'watchlist.add.value.aria': 'Watch value',
  'watchlist.add.value.placeholder.call': 'e.g. {{first}}  or  {{second}}',
  'watchlist.add.value.placeholder.grid': 'e.g. {{first}}  or  {{second}}',
  'watchlist.add.value.placeholder.dxcc': 'e.g. {{entity}}',
  'watchlist.add.cqOnly.label': 'CQ only',
  'watchlist.add.cqOnly.title': 'Only alert on a CQ call',
  'watchlist.add.submit': 'Add',

  // ── The two display filters (hide calls / hide countries) ───────────────────────────
  // Both say the same thing about themselves and say it differently, which is why they do
  // not share keys: one is about callsigns you are tired of seeing, the other about whole
  // entities. `{{examples}}` is a space-separated list of prefixes from the component.
  'hideCalls.chip.label': 'Hide calls',
  'hideCalls.chip.title':
    'Hide callsigns (or VP8*-style prefixes) from this pane — a display filter only; decoding, logging, alerts and the auto-responder are untouched',
  'hideCalls.head': 'Hide these callsigns',
  'hideCalls.placeholder': 'e.g. {{examples}}',
  'hideCalls.note':
    'Space-separated. A trailing <code>*</code> is a prefix ("VP8*" hides every VP8). A view filter only — the stations you are working and those calling you always show. To stop your auto-CQ from answering a call, Alt-double-click it instead.',

  'hideCountries.chip.label': 'Countries',
  // What the chip says instead of the count while the ticks are kept but nothing is hidden.
  'hideCountries.chip.paused': 'paused',
  'hideCountries.chip.title':
    'Hide chosen countries from this pane (a display filter — decoding, logging and alerts are untouched)',
  'hideCountries.head': 'Hide these countries',
  'hideCountries.pause': 'Pause (keep my ticks, show everything)',
  'hideCountries.other.head': 'Other country…',
  'hideCountries.search.placeholder': 'search all entities…',
  'hideCountries.note':
    'A view filter only — decoding, logging and alerts are untouched. Stations calling you, the one you are working, and new entities or band slots still show.',
  // The count is of COUNTRIES ticked, never of rows that vanished.
  'hideCountries.hidden': {
    one: '{{count}} country hidden',
    other: '{{count}} countries hidden',
  },
  'hideCountries.clear.label': 'Clear',
  'hideCountries.clear.aria': 'Clear country filter',
  'hideCountries.clear.title': 'Show every country again',

  // ── Coordinated QSY (Roam) ──────────────────────────────────────────────────────────
  // ⚠️ The role word (`initiator` / `follower` / `idle`) is the backend's own enum, printed
  // as it arrives — only the "unpaired" stand-in below is ours. Band names, channel labels
  // and dial frequencies are tokens. `Stop → home` stops the CHANNEL HOPPING, not a
  // transmission: it is not a stop-line control.
  'roam.title': 'Coordinated QSY · Roam',
  'roam.subtitle': 'move together off QRM & casual listeners',
  'roam.disclaimer':
    '<b>Not private — announced in the clear.</b> Coordinated QSY steps you and one other station to a new channel together, with the move sent as plain text (FCC Part 97 forbids encryption / obscured meaning, and your callsign IDs every 10 min). It shakes a <em>casual</em> scanner parked on the old frequency — it does <em>not</em> hide you from anyone with a wideband receiver, who can follow. Use it for anti-QRM and modest obscurity, never for secrecy.',
  'roam.enable.title': 'Coordinated QSY',
  'roam.enable.sub.on': 'Enabled — separate from your normal Chat/QSO modes.',
  'roam.enable.sub.off': 'Off by default.',
  'roam.enable.on': 'Enabled',
  'roam.enable.off': 'Enable',
  'roam.partner.title': 'Roaming partner',
  // `{{partner}}` is a callsign and `{{role}}` the backend's role word.
  'roam.partner.line': '<b>{{partner}}</b> · you are the <b>{{role}}</b>',
  'roam.partner.none': 'Select a station in the roster — you move together with that peer.',
  'roam.role.unpaired': 'unpaired',
  'roam.channels.legend': 'Channel set',
  'roam.channels.hint':
    'The initiator round-robins through these (skipping the current one). Pick at least two. Announced QSY is legal on every band.',
  'roam.channel.title': '{{label}} — {{freq}} MHz {{mode}}',
  'roam.cadence.legend': 'Hop cadence',
  'roam.cadence.aria': 'Hop cadence',
  'roam.cadence.hint':
    'How often the initiator announces a move. Conservative by default (never per-over) so it reads as a normal QSY.',
  // The offered cadences are 3, 6, 10 and 20, so English never needs a singular here; a
  // locale that needs more forms supplies them as an overlay.
  'roam.cadence.option': '{{count}} overs',
  'roam.controls.legend': 'Controls',
  'roam.moveNow.label': 'Move now',
  'roam.moveNow.title': 'Announce a move on your next over (initiator only)',
  'roam.pause.label': 'Pause',
  'roam.resume.label': 'Resume',
  'roam.pause.title': 'Hold on the current channel',
  'roam.stop.label': 'Stop → home',
  'roam.stop.title': 'Stop and return to the home channel',
  'roam.failed.configure': 'Could not update QSY set',
  'roam.failed.enable': 'Could not toggle coordinated QSY',
  'roam.failed.moveNow': 'Could not request a move',
  'roam.failed.pause': 'Could not toggle pause',
  'roam.failed.stop': 'Could not stop coordinated QSY',
  // One whole status line per state. `{{channel}}` is a band-plan token and `{{slot}}` a
  // T/R slot number.
  'roam.status.off': 'Off',
  'roam.status.paused': 'Paused · holding {{channel}}',
  'roam.status.lostSync': 'Lost sync → returning to {{home}}',
  'roam.status.next': 'Next: {{channel}}',
  'roam.status.nextSlot': 'Next: {{channel}} @ slot {{slot}}',
  'roam.status.auto': 'Auto · on {{channel}} · hopping every {{count}} overs',
  'roam.status.following': 'Following {{partner}} · on {{channel}}',
  'roam.status.idle': 'Select a station to roam with',
  // The stand-ins when the backend has not named a home channel or a partner yet.
  'roam.status.home': 'home',
  'roam.status.partner': 'partner',
  // The two Roam chips in the Tempo conversation — the launchpad's (which spells the state
  // out) and the header's (which is compact). `{{status}}` is the backend's short state:
  // a band-plan channel label, or its own `paused`. The ⇄ and ⚙ glyphs stay in the
  // component; a glyph is not prose.
  'roam.chip.title':
    'Roam — coordinated QSY: you and your partner move channels together, announced in the clear (never private). Click to enable/disable.',
  'roam.chip.launch.on': 'Roam on',
  'roam.chip.launch.on.status': 'Roam on · {{status}}',
  'roam.chip.launch.off': 'Roam off',
  'roam.chip.label': 'Roam',
  'roam.chip.label.on': 'Roam · on',
  'roam.chip.label.status': 'Roam · {{status}}',
  'roam.chip.settings.label': '⚙ Roam settings',
  'roam.chip.settings.title': 'Roam settings — channel set, hop cadence, move/pause/stop',
  'roam.chip.settings.aria': 'Roam settings',

  // ── Decode alerts (the beeps' toasts and the spoken batch summary) ──────────────────
  // ⚠️ `{{call}}` is a callsign, `{{entity}}` a DXCC entity name, `{{grid}}` a Maidenhead
  // locator and `{{what}}` the watch filter's own label — all data, none translated here.
  // `{{where}}` and `{{grid}}` arrive as whole optional CLAUSES assembled by the call site
  // (the `map.hover.aprs` pattern), because a toast with no entity must not leave a dangling
  // separator behind.
  'alerts.where': ' — {{entity}}',
  // Who the alert is about when the decode carried no callsign.
  'alerts.station': 'station',
  'alerts.watch': '⭐ Watch {{what}}: {{call}}{{where}}',
  'alerts.newDxcc': '🎯 NEW DXCC: {{call}}{{where}}',
  'alerts.myCall': '📢 {{call}} is calling you',
  'alerts.newGrid': 'New grid: {{call}}{{where}}',
  'alerts.rareGrid': '💎 {{tier}} grid{{grid}}: {{call}}{{where}}',
  'alerts.tier.rare': 'RARE',
  'alerts.tier.ultraRare': 'ULTRA-RARE',
  // The toast's own button. Answering someone who called you and working a station you found
  // are different acts, which is why they are different words.
  'alerts.action.work': 'Work',
  'alerts.action.answer': 'Answer',
  'alerts.cq': 'CQ from {{call}}{{where}}',
  // The spoken batch summary. Two statements: the count, then the CQ callers — `{{calls}}`
  // is a comma-joined list of callsigns with their entities, built by the call site.
  'alerts.batch.decodes': {
    one: '{{count}} new decode.',
    other: '{{count}} new decodes.',
  },
  'alerts.batch.cq': ' CQ from {{calls}}.',

  // ── Settings ▸ Spots & Alerts ───────────────────────────────────────────────────────
  // Paired with the panels above so the wording matches on both sides of the setting. The
  // <select> VALUES ('off', 'atno', 'hf', 'vhf', 'all' …) are persisted tokens and stay in
  // the code; only these labels are read.
  'settings.pounce.legend': 'Pounce — new-one alert',
  'settings.pounce.note':
    'Interrupts you the INSTANT a needed station appears on the cluster or RBN, rather than waiting for the spot board to refresh. A loud tone plays whether or not Nexus is the window you are looking at, and a banner offers one-click Work. Off until you switch it on. How rare "rare" is depends on your own totals: if you are chasing your first hundred entities then almost every DX spot is a new one and this would never stop talking. Start with <em>New DXCC entity only</em> once your log is far enough along that a new one is genuinely an event. Each station alerts once per band and mode.',
  'settings.pounce.threshold.label': 'Alert me for',
  'settings.pounce.threshold.off': 'Off (default)',
  'settings.pounce.threshold.atno': 'New DXCC entity only',
  'settings.pounce.threshold.atnoOrZone': 'New entity or CQ zone',
  'settings.pounce.threshold.atnoZoneOrState': 'New entity, zone, or US state',

  'settings.alerts.legend': 'Alerts',
  'settings.alerts.myCall.label': 'My call',
  'settings.alerts.myCall.hint': 'Beep + flash when someone directs a call at you.',
  'settings.alerts.cq.label': 'CQ calls',
  'settings.alerts.cq.hint': 'Alert on any decoded CQ. Off by default — CQs are constant.',
  // One band-scope vocabulary, read by all three selects — the same four choices mean the
  // same thing on each, and a translator writes them once.
  'settings.alerts.scope.off': 'Off',
  'settings.alerts.scope.hf': 'HF only',
  'settings.alerts.scope.vhf': 'VHF+ (6 m and up)',
  'settings.alerts.scope.all': 'All bands',
  'settings.alerts.dxcc.label': 'New DXCC',
  'settings.alerts.dxcc.aria': 'New DXCC alert bands',
  'settings.alerts.dxcc.hint':
    'Loud alert on a new DXCC entity — a “new one”. Does NOT alert on every decode. The band choice also decides where the NEW ONE icon is shown.',
  'settings.alerts.grid.label': 'New grid',
  'settings.alerts.grid.aria': 'New grid alert bands',
  'settings.alerts.grid.hint':
    "Quiet toast on a grid you haven't worked. Default VHF+ only — grid awards (VUCC/FFMA) start at 6 m; on HF nearly every decode is an unworked grid. The band choice also decides where the GRID icon is shown, on the roster and the decode rows.",
  'settings.alerts.rareGrid.label': 'Rare grid 💎',
  'settings.alerts.rareGrid.aria': 'Rare grid alert bands',
  'settings.alerts.rareGrid.hint':
    'The loud 💎 alert for rare/water-only grids (rovers, maritime, DXpeditions) — separate from plain grids so silencing HF chatter keeps the gems. Covers their GRID icon too.',
  'settings.alerts.watchlist.label': 'Watch list',

  // ── POTA / SOTA (the hunter board and your own activation) ──────────────────────────
  // ⚠️ THE PROGRAMMES AND THEIR REFERENCES ARE TOKENS, so they are absent from this file and
  // live in `components/PotaSotaView.tsx` as named constants: the programme names (POTA,
  // SOTA) wherever they stand alone, the view's own POTA / SOTA title, the reference EXAMPLES
  // the activation box offers (K-1234, W7A/MN-001) and the feed names (pota.app, SOTAwatch).
  // Everything a spot carries — activator callsign, park/summit reference, park name, dial
  // frequency, band and mode — is data and never passes through here either. Where a
  // programme name appears INSIDE a sentence below, a translator leaves it exactly as it is.
  'ota.subtitle': 'Hunt activators on the air now',
  'ota.popOut.title': 'Open the POTA/SOTA board in its own window (a POTA board beside a SOTA board)',
  'ota.popOut.label': '⧉ Pop out',
  // The programme picker. POTA and SOTA are names the operator reads on the programmes' own
  // sites; "Both" is an English word and this is the only part of that row a translator sees.
  'ota.program.aria': 'Program',
  'ota.program.both': 'Both',
  // Which programmes the empty state below is speaking about, as a CLAUSE — the sentence it
  // sits in is one message, so the two names and the conjunction between them arrive whole.
  'ota.programs.both': 'POTA or SOTA',

  // The hunt target — the next QSO with this call gets the park stamped on it.
  'ota.hunt.banner':
    'Hunting <b>{{reference}}</b> · <b>{{call}}</b><note> — next logged QSO with this call gets the park tagged</note>',
  'ota.hunt.clear': 'Clear hunt target',
  'ota.hunt.cleared': 'Hunt cleared',
  'ota.hunt.clearFailed': 'Could not clear hunt target',
  'ota.hunt.setFailed': 'Could not set hunt target for {{call}}',
  'ota.hunt.label': 'HUNT',
  // `{{freq}}` arrives already formatted and invariant ("14.0740 MHz").
  'ota.hunt.button.title':
    'Hunt {{call}} on {{reference}} — QSY to {{freq}} and tag next QSO',
  'ota.hunt.button.aria': 'Hunt {{call}}',

  // Your own activation. "Stop" ends the ACTIVATION — the park stamp on the QSOs you log —
  // and never a transmission; it is not a stop-line control.
  'ota.activation.label': "I'm activating:",
  'ota.activation.start': 'Start',
  'ota.activation.started': 'Activating {{program}} {{reference}} — QSOs will be park-tagged',
  'ota.activation.startFailed': 'Could not start activation',
  'ota.activation.active':
    '📻 Activating <b>{{program}} {{reference}}</b><note> · {{count}} logged — QSOs get your park tagged</note>',
  'ota.activation.stop.label': 'Stop',
  'ota.activation.stop.title': 'End activation',
  'ota.activation.ended': 'Activation ended',
  'ota.activation.stopFailed': 'Could not stop activation',

  // The offline park directory. `{{formatted}}` is a park COUNT the call site has already
  // grouped for display — it never passes through a formatter here (see `logbook.markLotw`).
  'ota.parks.have': '📖 {{formatted}} parks — searchable in the log',
  'ota.parks.none': '📖 No local park list yet',
  'ota.parks.download': 'Download',
  'ota.parks.update': 'Update',
  'ota.parks.downloaded': 'Downloaded {{formatted}} parks — searchable in the log',
  'ota.parks.downloadFailed': 'Park-list download failed',
  'ota.parks.import': 'Import CSV',
  'ota.parks.imported': 'Imported {{formatted}} parks',
  'ota.parks.importFailed': 'Import failed: {{detail}}',
  // "Hunted Parks.CSV" is the file POTA's own stats page hands you — a filename, kept verbatim.
  'ota.hunted.title':
    "Import your POTA 'Hunted Parks.CSV' (from your POTA stats page) so worked parks show correctly — the park number isn't in a CW exchange, so your log alone can't know it",
  'ota.hunted.import': 'Import Hunted Parks',
  'ota.hunted.have': 'Hunted ✓ ({{formatted}})',
  'ota.hunted.imported': 'Imported {{formatted}} hunted parks — new-park flags updated',
  'ota.hunted.importFailed': 'Hunted-parks import failed: {{detail}}',

  // The board's controls. `{{program}}` is POTA or SOTA; `{{time}}` a clock reading the call
  // site formatted itself.
  'ota.spots.failed': '{{program}} spots failed',
  'ota.refresh.label': 'Refresh',
  'ota.refresh.title': 'Refresh spots',
  'ota.lastUpdated': 'Updated {{time}}',
  'ota.filter.band.aria': 'Band filter',
  'ota.filter.band.label': 'Band',
  'ota.filter.mode.aria': 'Mode filter',
  'ota.filter.mode.label': 'Mode',
  'ota.filter.all': 'All',
  // The sort picker. The <option> VALUES ('value', 'activator' …) are persisted tokens and
  // stay in the code; these are the words beside them.
  'ota.sort.aria': 'Sort spots',
  'ota.sort.label': 'Sort',
  'ota.sort.title': 'How the spot list is ordered',
  'ota.sort.value': 'Workable now',
  'ota.sort.activator': 'Activator',
  'ota.sort.reference': 'Reference',
  'ota.sort.band': 'Band / freq',
  'ota.sort.mode': 'Mode',
  'ota.sort.asc.title': 'Ascending — click for descending',
  'ota.sort.desc.title': 'Descending — click for ascending',
  'ota.sort.asc.aria': 'Sort ascending',
  'ota.sort.desc.aria': 'Sort descending',

  // The spot list. The first two lines of a row's tooltip are programme, reference, park
  // name, frequency, mode and band — all data, assembled in the component and never here.
  'ota.loading': 'Loading…',
  'ota.empty.filtered': 'No activators match the current filters.',
  'ota.empty': 'No {{program}} activators spotted right now.',
  'ota.spot.spottedBy': 'Spotted by {{spotter}}',
  'ota.spot.bandOpen.tooltip':
    'BAND OPEN — your signal is being received on this band right now (workable)',
  'ota.badge.newPark': 'NEW PARK',
  'ota.badge.newPark.title': 'You have never logged this park/summit — a new one',
  'ota.badge.bandOpen': 'BAND OPEN',
  'ota.badge.bandOpen.title':
    'Your signal is being received on this band right now — workable',
  // `{{source}}` is the feed's own name (pota.app, SOTAwatch); HUNT is the button above.
  'ota.source.hint':
    'Live from {{source}}. Auto-refreshes every 60 s. Click HUNT to QSY and tag the next logged QSO.',

  // ── Field Day (the workspace, the scoreboard and the event countdown) ───────────────
  // ⚠️ INVARIANT AND THEREFORE ABSENT: the event names (ARRL Field Day, Winter Field Day,
  // WFD) which are the programmes' own — `FD_EVENT_NAMES` in `fdEvent.ts`; the class and
  // category codes (3A, 2O) and the H/I/M/O letters; ARRL section codes and the section and
  // division NAMES the board reads out of `features/arrlSections.ts`; the FD mode codes
  // (DIG, CW, PH); every score, count and multiplier; and the BONUS NAMES in `FD_BONUSES`,
  // which mirror the Rust table an entry is submitted against. Weekday and month
  // abbreviations are date formatting and stay in `fdEvent.ts` with the rest of it.
  'fieldDay.operator.label': 'Operator',
  'fieldDay.operator.placeholder': 'operator (call/initials)',
  'fieldDay.operator.aria': 'Field Day operator (call or initials)',
  'fieldDay.popOut.label': '⧉ Pop out',
  'fieldDay.popOut.title': 'Pop the scoreboard out to its own window (second monitor)',

  'fieldDay.score.qsos': 'QSOs',
  'fieldDay.score.sections': 'Sections',
  // Winter Field Day scores by objectives at submission, so its line states the raw count
  // instead of the ARRL power×+bonus arithmetic — a different statement, not a shortened one.
  'fieldDay.score.wfd':
    'QSO pts {{points}} · WFD objective multipliers apply at submission (not tracked here)',
  // The whole sum as ONE message: an equation assembled from eight fragments cannot be
  // reordered, and every language puts "power" and "bonuses" somewhere of its own.
  'fieldDay.score.math':
    'QSO pts <b>{{qsoPts}}</b> × power ×<b>{{powerMult}}</b> = <b>{{poweredPoints}}</b> + bonuses <b>{{bonusPoints}}</b> = <total>{{totalScore}}</total>',
  'fieldDay.state.title': 'Sequencer state',
  // The stand-in when the engine has not named a sequencer state yet; the states themselves
  // are the backend's own words, printed as they arrive.
  'fieldDay.state.idle': 'Idle',

  // The worked-sections board. `{{code}}`, `{{name}}` and `{{division}}` are the section's
  // own identifiers; two whole tooltips, because "worked" and "not worked yet" are the point
  // of the sentence rather than a tail on it.
  'fieldDay.sections.aria': 'Worked sections board',
  'fieldDay.sections.head': 'Sections',
  'fieldDay.sections.count': '{{worked}}/{{total}} sections',
  'fieldDay.sections.cell.worked.title': '{{code}} — {{name}} ({{division}}) — worked',
  'fieldDay.sections.cell.notWorked.title': '{{code}} — {{name}} ({{division}}) — not worked yet',
  'fieldDay.sections.cell.worked.aria': '{{name}}, worked',
  'fieldDay.sections.cell.notWorked.aria': '{{name}}, not worked',

  // Run vs search-and-pounce. Stored as plain text with a real `&` — React escapes text
  // children itself, so an entity here would render as the literal characters.
  'fieldDay.role.aria': 'Field Day role',
  'fieldDay.role.running': 'Running',
  'fieldDay.role.sp': 'S&P',

  // The exports. Cabrillo and ADIF are the file formats' own names and stay in the sentence.
  'fieldDay.export.busy': 'Exporting…',
  'fieldDay.export.cabrillo.label': 'Export Cabrillo',
  'fieldDay.export.cabrillo.title': 'Export Field Day log as Cabrillo (.cbr) for ARRL submission',
  'fieldDay.export.adif.label': 'Export ADIF',
  'fieldDay.export.adif.title': 'Export Field Day log as ADIF (.adi)',
  'fieldDay.export.summary.label': 'Summary',
  'fieldDay.export.summary.title':
    'Download a one-page score summary (QSOs by band/mode, sections, power, bonuses, total)',
  'fieldDay.export.dupeSheet.label': 'Dupe sheet',
  'fieldDay.export.dupeSheet.title':
    'Download a dupe / multiplier check sheet (sections + callsigns worked)',
  'fieldDay.export.done': 'Exported → {{path}}',

  // The bonus checklist. The bonus NAMES come from `FD_BONUSES`; these are the words around
  // them, and `{{points}}` is a score.
  'fieldDay.bonuses.head': 'Bonuses',
  'fieldDay.bonuses.count': '{{claimed}}/{{total}} claimed · {{points}} pts',
  'fieldDay.bonuses.aria': 'Claimed FD bonuses',
  'fieldDay.bonus.aria': '{{label}} — {{points}} pts',
  'fieldDay.bonus.pts': '{{points}} pts',

  // Three states per bonus: not planned / planned / earned. Only EARNED scores — a plan is
  // a plan, not points — so these words carry that distinction and must keep carrying it in
  // every language. `{{points}}` is a score, `{{count}}` a number of bonuses, `{{mult}}` the
  // power multiplier and `{{label}}` a bonus name from FD_BONUSES (never translated).
  'fieldDay.scoring.power.chip': '\u00d7{{mult}} power',
  'fieldDay.bonuses.planned.count': '{{count}} planned \u00b7 +{{points}} pts',
  'fieldDay.bonuses.chase.aria': 'Bonus points earned and planned',
  'fieldDay.bonuses.chase.earned': 'Earned {{points}} pts',
  'fieldDay.bonuses.chase.earned.note': 'counted in your score',
  'fieldDay.bonuses.chase.planned': 'Planned +{{points}} pts',
  'fieldDay.bonuses.chase.planned.note': 'not scored until you tick it',
  'fieldDay.bonuses.chase.potential': 'If all land {{points}} pts',
  'fieldDay.bonus.plan.off': 'Plan',
  'fieldDay.bonus.plan.on': 'Planned',
  'fieldDay.bonus.plan.aria': 'Plan {{label}} \u2014 planned bonuses do not score',
  'fieldDay.bonus.plan.title': 'Planned = you mean to earn it. It scores only once you tick the box.',

  // The log. Column headings name a CONCEPT; every value under them is a token. ARRL calls
  // the exchange field Class and WFD calls it Category — two words for two events, not one
  // word with a variant.
  'fieldDay.log.column.time': 'Time',
  'fieldDay.log.column.call': 'Call',
  'fieldDay.log.column.class': 'Class',
  'fieldDay.log.column.category': 'Category',
  'fieldDay.log.column.section': 'Section',
  'fieldDay.log.column.band': 'Band',
  'fieldDay.log.column.mode': 'Mode',
  'fieldDay.log.empty': 'No contacts logged yet.',
  'fieldDay.log.dupe.title': 'Duplicate callsign',
  'fieldDay.log.mult.title': 'New section — multiplier',
  'fieldDay.log.mult': 'Mult!',

  // The event header. One sentence with three slots rather than a label plus a suffix: the
  // date range is invariant, the status is one of the four below, and where each belongs is
  // the translator's decision.
  'fieldDay.subtitle': '{{event}}: {{dates}} · {{status}}',
  'fieldDay.status.active': 'active',
  // Only reached at two days or more, so English never needs the singular; a locale with more
  // forms supplies them as an overlay (the `roam.cadence.option` pattern).
  'fieldDay.countdown.days': 'starts in {{count}} days',
  'fieldDay.countdown.tomorrow': 'starts tomorrow',
  'fieldDay.countdown.hours': 'starts in {{count}}h',
  'fieldDay.countdown.soon': 'starting soon',
  // Which rules data is scoring — the banner's identity line. {{year}} is the ruleset's
  // rules_year, {{date}} the rules file's `generated` stamp shown as YYYY-MM-DD.
  'fieldDay.rules.line': 'Rules {{year}} · data {{date}}',
  // The warn-only rule advisories (FdAdvisories.tsx — warn, NEVER remove or disable;
  // operator ruling). {{event}} is the event's own untranslated name, {{mode}} an on-air
  // mode token, {{sources}} the live assistance-source labels — all invariant slots.
  'fieldDay.advisory.banned':
    '{{mode}} is not permitted at {{event}} ({{year}} rules) — you can still log it, but it will not count',
  'fieldDay.advisory.cluster':
    'DX cluster assistance is not permitted at {{event}} ({{year}} rules) — live now: {{sources}}',
  'fieldDay.advisory.spotting':
    'Spotting assistance is not permitted at {{event}} ({{year}} rules) — live now: {{sources}}',
  'fieldDay.club.aria': 'Club sync',
  'fieldDay.club.head': 'Club',
  'fieldDay.club.state.synced': 'Synced',
  'fieldDay.club.state.behind': 'Behind — {{queued}} to send',
  'fieldDay.club.state.offline': 'Offline — {{queued}} queued here',
  'fieldDay.club.state.title': 'Live sync state, derived from the send queue — never a guess. Contacts logged while offline are journaled and re-sent automatically.',
  'fieldDay.club.hostLine': '{{event}} · host {{call}}',
  'fieldDay.club.counters': 'Club: {{score}} pts · {{qsos}} QSOs · {{sections}} sections',
  'fieldDay.club.export.cabrillo.label': 'Club Cabrillo',
  'fieldDay.club.export.cabrillo.title': 'Export the merged club log as Cabrillo (deduped — the earliest contact wins)',
  'fieldDay.club.export.adif.label': 'Club ADIF',
  'fieldDay.club.export.adif.title': 'Export the merged club log as ADIF (deduped — the earliest contact wins)',
  'fieldDay.club.popOut.label': '⧉ Pop out board',
  'fieldDay.club.popOut.title': 'Pop the club band board out to its own window (second monitor) — who is on what band, across every position',
  'fieldDay.club.skew': 'This PC\'s clock differs from the host\'s by {{secs}} s — check this PC\'s clock',
  'fieldDay.club.error': 'Host: {{msg}}',
  'fieldDay.club.board.empty': 'No positions heard yet — every other Nexus position on this network appears here as it logs.',
  'fieldDay.club.board.column.position': 'Position',
  'fieldDay.club.bands.column.band': 'Band',
  'fieldDay.club.bands.column.who': 'Who is there',
  'fieldDay.club.bands.free': 'free',
  'fieldDay.club.bands.clash.mark': 'CLASH',
  'fieldDay.club.bands.clash.why':
    '{{band}} conflict: two positions are running {{mode}} at once. They will work each other\u2019s callers and split the run.',
  'fieldDay.club.board.column.band': 'Band',
  'fieldDay.club.board.column.mode': 'Mode',
  'fieldDay.club.board.column.operator': 'Operator',
  'fieldDay.club.board.column.qsos': 'QSOs',
  'fieldDay.club.board.column.rate': 'Rate',
  'fieldDay.club.board.stale': 'Last heard {{secs}} s ago',
  'fieldDay.club.board.unnamed': 'Unnamed position',
  'fieldDay.club.board.rate': '{{rate}}/hr',

  // ── The contest calendar (upcoming contests, from the WA7BNM calendar) ──────────────
  // ⚠️ Contest NAMES arrive from the feed and are never translated; the date + UTC time
  // stamps are date formatting and stay in the component. `WA7BNM` is the calendar keeper's
  // callsign.
  'contests.group.now': 'On the air now',
  'contests.group.soon': 'Starting soon',
  'contests.group.week': 'This week',
  'contests.group.later': 'Later',
  'contests.rules.title': 'Rules & details (WA7BNM)',
  // The one-line summary, as four whole sentences. The "in 8 h" / "in 3 d" part was a
  // fragment glued into the middle of the third one, which is exactly what cannot be
  // translated — a language that puts the time first has nowhere to put it.
  'contests.line.offline': 'Contest schedule loads once online.',
  'contests.line.none': 'No contests coming up.',
  'contests.line.onNow': 'On now: {{name}} (until {{until}}).',
  'contests.line.next.now': 'Next: {{name}} now ({{date}}).',
  'contests.line.next.hours': 'Next: {{name}} in {{count}} h ({{date}}).',
  'contests.line.next.days': 'Next: {{name}} in {{count}} d ({{date}}).',

  // ── Settings ▸ Contesting ───────────────────────────────────────────────────────────
  // Paired with the surfaces above so the wording matches on both sides of the setting.
  // ⚠️ Invariant and staying in the panel: the class/category placeholders (1D, 2O), the
  // section placeholder (WI) and the section list itself, which is data.
  'settings.contestCategory.legend': 'Contest Category',
  'settings.contestCategory.unassisted.label': 'Unassisted entry',
  // Two whole accessible names — the switch does one of two different things, and neither is
  // the other with a word swapped.
  'settings.contestCategory.unassisted.aria.declare': 'Declare an unassisted contest entry',
  'settings.contestCategory.unassisted.aria.end': 'End an unassisted contest entry',
  'settings.contestCategory.unassisted.hint':
    'Turns off the AI CW decoder, DX cluster / RBN spots and the PSK Reporter needs feed together, and records the change with a timestamp. Takes effect at once. Your own settings for each of those are left alone and come back when you switch this off.',
  // The journal. Each entry's own note and the source names in it are the engine's words,
  // printed as they arrive; the UNASSISTED / assisted state and the empty stand-in are ours.
  'settings.contestCategory.journal.label': 'Assistance record',
  'settings.contestCategory.journal.unassisted': 'UNASSISTED',
  'settings.contestCategory.journal.assisted': 'assisted',
  'settings.contestCategory.journal.noSources': 'nothing active',
  'settings.contestCategory.journal.hint':
    'Kept in <code>assistance_journal.json</code> beside your settings, so it survives restarts. Newest first.',

  'settings.fieldDay.legend': 'Field Day Setup',
  'settings.fieldDay.mode.label': 'Field Day mode',
  'settings.fieldDay.mode.aria.enable': 'Enable Field Day mode',
  'settings.fieldDay.mode.aria.disable': 'Disable Field Day mode',
  'settings.fieldDay.mode.hint':
    'Turn on for Field Day weekend — reveals the Field Day workspace and the Class/Section exchange across all modes. Off the rest of the year. Fill in Class + Section below to start operating. Save to apply.',
  'settings.fieldDay.needExchange':
    "<b>Set your Class + Section to start operating.</b> Field Day mode is on, but the station won't enter Field Day until both are filled in below.",
  'settings.fieldDay.event.label': 'Event',
  'settings.fieldDay.event.aria': 'Field Day event',
  'settings.fieldDay.event.hint':
    "Which event you're operating in — affects scoring labels and export headers.",
  // The exchange field, named as its own event names it. The letters quoted in the hints are
  // the codes an operator sends — a translator leaves every one of them.
  'settings.fieldDay.class.label': 'FD Class',
  'settings.fieldDay.class.hint':
    'Number of transmitters + class letter: A=club/group portable, B=1–2 person portable, C=mobile, D=home (mains power), E=home (emergency power), F=EOC. E.g. 3A = 3 transmitters, club portable.',
  'settings.fieldDay.category.label': 'WFD Category',
  'settings.fieldDay.category.hint':
    'Transmitters + location: H=Home, I=Indoor, M=Mobile, O=Outdoor (e.g. 2O = 2 transmitters, outdoor).',
  'settings.fieldDay.section.label': 'ARRL Section',
  // `{{section}}` is what the operator typed and `{{count}}` the size of the section list.
  'settings.fieldDay.section.invalid':
    '“{{section}}” isn\'t a known ARRL/RAC section — pick one from the list.',
  'settings.fieldDay.section.hint':
    'Your ARRL / RAC section (e.g. WI, ENY, ONN). Start typing the code or a state name and pick from the list — validated against all {{count}} sections. Required for the Cabrillo log.',
  // The power multiplier. The ×5/×2/×1 factors and the watt figures are technical quantities
  // that stay exactly as they are inside these labels; only the words around them move.
  'settings.fieldDay.power.label': 'Power multiplier',
  'settings.fieldDay.power.aria': 'Field Day power multiplier',
  'settings.fieldDay.power.qrp.label': '×5 QRP / battery',
  'settings.fieldDay.power.qrp.hint': 'Runs entirely on battery or other natural power, ≤5W output',
  'settings.fieldDay.power.hundred.label': '×2 ≤100W',
  'settings.fieldDay.power.hundred.hint': '100W or less from any power source',
  'settings.fieldDay.power.high.label': '×1 >100W',
  'settings.fieldDay.power.high.hint': 'Over 100W — commercial/generator power',
  'settings.fieldDay.power.hint':
    'Multiplies your QSO points. QRP/battery = ×5 (ARRL bonus for going off-grid). Choose before the event.',

  // Rules-data currency (fd-rules.json — the scoring parameters, event windows, bonus menu
  // and section list behind both events). {{date}} is the rules file's `generated` stamp
  // shown as YYYY-MM-DD; every "applies at next launch" is literal (the table is set once
  // at launch), like the country file's. No cron: this pre-event button is the refresh path.
  'settings.fdRules.update.action': 'Check for rules updates',
  'settings.fdRules.update.busy': 'Checking…',
  'settings.fdRules.update.done':
    'Rules data downloaded — applies at next launch (data {{date}}).',
  'settings.fdRules.update.current': 'Rules data is already current (data {{date}}).',
  'settings.fdRules.update.failed': 'Rules update failed: {{detail}}',
  'settings.fdRules.status': '{{year}} rules · data {{date}}',
  'settings.fdRules.pending': 'Update downloaded (data {{date}}) — applies at next launch.',
  'settings.fdRules.stale': 'Rules data is from {{year}} — check for updates before the event.',
  'settings.fdRules.empty': 'Built-in rules data active.',
  'settings.fdRules.hint':
    'Scoring parameters, event windows, bonuses and sections for both Field Day events. Checked on demand; a downloaded update applies at the next launch.',
  // ── Settings ▸ Contesting ▸ Who's who at this event ─────────────────────────────────
  // Three names, three different jobs, and until now nothing said so: the club call lived on
  // Station, the position name under a networking heading on Contesting, the operator on
  // Station and on the Field Day dashboard. The club report behind this section is an operator
  // asking what the position name was even for. Each hint therefore names its own job in terms
  // that separate it from the other two — what goes on the air, where you are sitting, who is
  // sitting there — and every one of these rows edits the SAME setting as its other home.
  'settings.fdWho.legend': "Who's who at this event",
  'settings.fdWho.note':
    'Three names, three different jobs — the club call goes on the air, the position is which tent you are sitting in, and the operator is whoever is at the key right now. Changing one of them here changes it everywhere in Nexus: the callsign and the operator are the same two boxes the Station tab holds.',

  'settings.fdWho.call.label': 'Callsign on the air',
  'settings.fdWho.call.hint':
    'The call that goes on the air and onto every contact you log — at a club event that is the club\'s call, the same one at every position on site.',

  'settings.fdWho.position.label': 'Position name',
  'settings.fdWho.position.placeholder': 'CW tent',
  'settings.fdWho.position.hint':
    'Which tent, trailer or table this station is — it names you on the club band board so everyone can see which position is on which band, and it never goes on the air.',

  // ⚠️ `OPERATOR` is the ADIF field name — a wire identifier. Keep it verbatim.
  'settings.fdWho.operator.label': 'Operator at the key',
  'settings.fdWho.operator.placeholder': 'blank = the callsign above',
  'settings.fdWho.operator.hint':
    'Whoever is running this position right now — change it every time someone takes the seat, and their contacts are stamped with it (ADIF OPERATOR) so the club can split the log by operator afterwards.',

  'settings.fdClub.legend': 'Field Day Club Sync',
  'settings.fdClub.host.label': 'Host a club event',
  'settings.fdClub.host.hint': 'Merges every position\'s contacts into one club log on this PC — and opens a port on your local network (the only time Nexus listens beyond this computer).',
  'settings.fdClub.host.note': 'Positions on this network can now find and join this event. There is no join password — a club site LAN is trusted, and anyone on it could add rows to the club log, which you will see. If this PC dies, enable hosting on any other position: everyone re-joins and nothing is lost.',
  'settings.fdClub.host.aria.enable': 'Enable club event hosting',
  'settings.fdClub.host.aria.disable': 'Disable club event hosting',
  'settings.fdClub.eventName.label': 'Event name',
  'settings.fdClub.eventName.placeholder': 'W9ABC Field Day',
  'settings.fdClub.eventName.hint': 'Shown to joining positions and in discovery.',
  'settings.fdClub.hostPort.label': 'Host port',
  'settings.fdClub.hostPort.hint': 'TCP port for the club sync (default 42073).',
  'settings.fdClub.join.label': 'Join event at',
  'settings.fdClub.join.hint': 'host:port of the club host — use Find club events, or type it from the host\'s screen.',
  'settings.fdClub.join.hostingHint': 'Hosting — this position joins its own event automatically.',
  'settings.fdClub.discover.action': 'Find club events',
  'settings.fdClub.discover.busy': 'Listening…',
  'settings.fdClub.discover.empty': 'Nothing heard in 2 s — same network? Some Wi-Fi blocks discovery; type the host address instead.',
  'settings.fdClub.discover.pick.label': '{{event}} — {{host}}',
  'settings.fdClub.discover.pick.title': 'Use {{host}} as the join address',
  // ---- Connect on the TV (the read-only LAN page). The toggle IS the LAN opt-in,
  // so the copy has to name what it exposes and to whom — its threat model is not the
  // Field Day scoreboard's, which is defensible partly because a contest log is
  // already broadcast in clear on the air.
  'settings.connectWeb.legend': 'Connect on a TV',
  'settings.connectWeb.label': 'Serve Connect on this network',
  'settings.connectWeb.hint':
    'Serves the full Connect view — the map with every layer, the panes, live openings — read-only, to any browser on your network: a shack TV, a tablet, a phone. Nothing can be changed from it.',
  'settings.connectWeb.exposes':
    'While this is on, anyone on your network can see your callsign, grid square and the propagation picture — including the callsigns of stations heard and spotted. Your log, your needs board and the frequency you are on are never sent.',
  'settings.connectWeb.aria.enable': 'Serve Connect on the local network',
  'settings.connectWeb.aria.disable': 'Stop serving Connect on the local network',
  'settings.connectWeb.port.label': 'Port',
  'settings.connectWeb.port.hint': 'Separate from the Field Day scoreboard, so both can run at once.',
  'settings.connectWeb.url.label': 'Open this on the TV',
  'settings.connectWeb.url.copy': 'Copy',
  'settings.connectWeb.url.pending': 'Starting…',
  'settings.fdBoard.label': 'Spectator scoreboard',
  'settings.fdBoard.hint': 'Serves a read-only scoreboard page for a TV or projector on this network — nothing to install on the TV, and viewers can only look. The first enable may pop a Windows Firewall prompt; allow it or viewers see nothing.',
  'settings.fdBoard.aria.enable': 'Enable the spectator scoreboard',
  'settings.fdBoard.aria.disable': 'Disable the spectator scoreboard',
  'settings.fdBoard.port.label': 'Board port',
  'settings.fdBoard.port.hint': 'TCP port the scoreboard page is served on (default 7373).',
  'settings.fdBoard.url.label': 'On the TV, open',
  'settings.fdBoard.url.copy': 'Copy',
  'settings.fdBoard.url.pending': 'Starting up — save settings, then this row shows the address.',
  'settings.fdBoard.hostOnly': 'Live data appears when this position hosts the club event; otherwise the page points viewers to the host.',

  // ── Satellites ──────────────────────────────────────────────────────────────────────
  // The Satellites section, the Connect Passes pane, and the nine composers behind them.
  //
  // The units rule is nearly the whole review here, and it lands on the SKY and the DIAL.
  // Staying in the code, never entries: bird names, NORAD ids, TLE epochs, every uplink and
  // downlink frequency, the SatNOGS transponder descriptions and their per-leg mode names,
  // grids, azimuths, elevations, ranges, altitudes, the compass letters, the mode names the
  // radio binding prints (FM/SSB), and the `value` of every <select> on these screens.
  //
  // Abbreviations an operator reads OFF AN INSTRUMENT stay verbatim where they sit inside a
  // sentence: AOS, LOS, TCA, TLE, NORAD, VFO, MHz/kHz/Hz, km, and the ± × Δ ↑ ↓ symbols are
  // amateur-satellite vocabulary, the same in every language, and a translator leaves them
  // exactly as they are. The sky dome's own plate text (`az 143°` / `el 47°`, `▲ AOS`,
  // `▼ LOS`) is not here at all: those plates are SIZED from the string, in viewBox units,
  // by arithmetic the component documents at length — they are instrument tick labels.

  // What a bird's catalog record says about it (features/satHealth.ts) — one chip, rendered
  // by every surface that names a bird. SatNOGS is the database's own name.
  'sat.health.silent.label': 'silent',
  'sat.health.silent.title':
    'In orbit, but the catalog lists no live amateur transmitter — the pass geometry is real, there is nothing to work on it.',
  'sat.health.dead.label': 'dead',
  'sat.health.dead.title':
    'SatNOGS reports this bird silent. Its passes are still computed from real elements; working it is not expected.',
  'sat.health.reentered.label': 're-entered',
  'sat.health.reentered.title':
    'SatNOGS reports this bird has re-entered — it is gone. Any pass shown is modelled from the last elements on file.',
  'sat.health.preLaunch.label': 'pre-launch',
  'sat.health.preLaunch.title':
    'SatNOGS has this bird on record but not yet deployed — nothing to work until it launches.',
  // An upstream status this build has never seen. `{{status}}` is the SOURCE's own word,
  // printed as it arrived — only the frame around it moves.
  'sat.health.unknown.title': 'SatNOGS reports this bird\'s status as "{{status}}".',
  'sat.health.noElements.label': 'no elements',
  'sat.health.noElements.title':
    'No source carries orbital elements for this bird right now, so its passes cannot be computed. It stays starred; rows return when elements do.',
  'sat.health.staleElements.label': 'stale elements',
  'sat.health.staleElements.title':
    'The newest elements on file are past the 30-day ceiling where SGP4 accuracy is gone. Refresh elements, or wait for the next automatic refresh.',
  'sat.health.noPosition.label': 'no position',
  'sat.health.noPosition.title':
    'Current elements, but the propagator refused a position for them — a decaying orbit does this. Nothing is being hidden; there is genuinely no place to draw.',

  // The set-wide element bands (features/elementBands.ts) — one wording for the Satellites
  // header, the Now-Bar lane, the Connect Passes pane, the Settings fieldset and the refresh
  // toast, so two surfaces can never describe one catalog differently. `14 d` and `30 d` are
  // the thresholds themselves.
  'sat.elements.pastLine': '{{past}} of {{total}} past 14 d',
  'sat.elements.parts.aging': '{{count}} past 14 d',
  'sat.elements.parts.heldBack': {
    one: '{{count}} sits out past 30 d',
    other: '{{count}} sit out past 30 d',
  },
  // English says "birds" at every count, one included. That wording predates this migration
  // and the phase changes no visible text; a locale that needs forms adds them here.
  'sat.elements.summary.birds': '{{count}} birds',
  'sat.elements.sentence.aging': {
    one: '{{count}} bird is past the 14-day line and drifting.',
    other: '{{count}} birds are past the 14-day line and drifting.',
  },
  'sat.elements.sentence.heldBack': {
    one: '{{count}} bird is past 30 days and sits out until fresh elements arrive.',
    other: '{{count}} birds are past 30 days and sit out until fresh elements arrive.',
  },

  // The Now-Bar `sat` lane (features/satLane.ts): a two- or three-word chip, and the detail
  // that carries the fix. Celestrak and hamradiotools.io are the sources' own names.
  'sat.lane.blocked.message': 'Sat: Celestrak blocked',
  'sat.lane.blocked.detail':
    'Celestrak refused direct element fetches (HTTP 403/404) — direct attempts are stopped for 24 h. The hamradiotools.io mirror keeps retrying; elements may age until it lands.',
  'sat.lane.unusable.message': 'Sat: elements unusable',
  'sat.lane.unusable.detail':
    'Every cached element set is over 30 days old — satellite surfaces refuse to point or tune on them. Refresh in Settings ▸ Radio ▸ Orbital elements, or import a fresh file.',
  'sat.lane.stale.message': 'Sat: elements {{days}} d old',
  'sat.lane.stale.detail':
    'Orbital elements are past the 14-day stale line — pass times, pointing and Doppler drift with age. Refresh from the Satellites section or Settings ▸ Radio ▸ Orbital elements.',
  'sat.lane.mostlyStale.message': 'Sat: {{label}}',
  'sat.lane.mostlyStale.detail':
    "Most of the element sets you hold are past the 14-day stale line — {{label}}. The typical bird is still current, so the age reads calm; the rest of the catalog's pass times, pointing and Doppler are drifting. Refresh from the Satellites section or Settings ▸ Radio ▸ Orbital elements.",

  // The satellite VFO-mapping enumeration (features/satVfo.ts) — LABELS ONLY. Every `value`
  // ('off', 'a-down-b-up', …) is a persisted token and stays in the code, and so do the VFO
  // names and the rig model in the one example: A, B, Main and Sub are printed on the
  // radio's own front panel, and IC-9700 is what it says on the box.
  'sat.vfoMap.off': 'Not set — downlink only',
  'sat.vfoMap.downlinkOnly': 'Downlink only (receive)',
  'sat.vfoMap.uplinkOnly': 'Uplink only (transmit)',
  'sat.vfoMap.aDownBUp': 'VFO A = downlink, VFO B = uplink',
  'sat.vfoMap.aUpBDown': 'VFO A = uplink, VFO B = downlink',
  'sat.vfoMap.mainDownSubUp': 'Main = downlink, Sub = uplink (IC-9700 full duplex)',
  'sat.vfoMap.mainUpSubDown': 'Main = uplink, Sub = downlink',

  // The ⏰ per-pass alarm (features/satAlarm.ts). TWO whole messages rather than one sentence
  // with a variable clause dropped into the middle: the bird is up, or it is about to rise.
  'sat.alarm.up': '⏰ {{name}} is UP now (LOS {{los}}) · max {{maxEl}}°',
  'sat.alarm.rises': '⏰ {{name}} rises {{aos}} (~{{mins}} min) · max {{maxEl}}°',
  'sat.alarm.stop': 'Stop alarm',

  // ISS SSTV auto-arm (features/issAutoArm.ts). The dial and the mode are interpolated from
  // the module's own constants and never become text: a decimal comma reaching 145.800 is an
  // operating fault, not a cosmetic one. ISS and SSTV are the station's and the mode's names.
  'sat.iss.optOut': 'ISS auto-arm off — SSTV disarmed, dial restored',
  'sat.iss.los': 'ISS LOS — SSTV disarmed, dial restored',
  'sat.iss.armed': 'ISS overhead — tuned {{dial}} {{mode}}, SSTV armed',

  // The manual element-refresh result (features/tleMessages.ts) — one composer for the
  // Update-now toast and the Settings "Last refresh" line, so the two can never drift.
  // `{{source}}` is the serving source's own name, printed as it arrives.
  'sat.refresh.mirrorDown': 'Mirror unreachable — fetched from Celestrak: {{count}} birds',
  'sat.refresh.ok': 'Orbital elements updated — {{count}} birds ({{source}})',
  'sat.refresh.blocked':
    'Celestrak refused direct element fetches — direct attempts are stopped for 24 h; the mirror keeps retrying.',
  'sat.refresh.mirrorUnreachableCurrent':
    "The element mirror isn't reachable (it goes live with the next release); your elements are {{age}} d old — current.",
  'sat.refresh.mirrorUnreachableStale':
    'The element mirror is unreachable and your elements are {{days}} d old — import a fresh element file or retry later.',
  'sat.refresh.mirrorUnreachableEmpty':
    'The element mirror is unreachable and no usable elements are cached — import an element file to get the satellite surfaces running.',
  'sat.refresh.failed':
    'Element update failed — no source delivered a usable set; retry shortly or import an element file.',

  // AOS/LOS alerts for the armed pass track (features/satPassAlert.ts).
  'sat.passAlert.rotorLost':
    '🛰 {{name}}: the rotator stopped answering — point it yourself. The pass, the transponder and Doppler keep running.',
  'sat.passAlert.inProgress': '🛰 {{name}} pass in progress — {{mins}} min left, max el {{maxEl}}°',
  'sat.passAlert.aos': '🛰 AOS {{name}} — point {{az}}°, max el {{maxEl}}°, {{mins}} min pass',
  // The LOS report is a HEAD plus up to two whole sentences about what came back. Each is a
  // complete statement joined with a space — never a fragment glued into the middle of one.
  'sat.passAlert.los': '🛰 {{name}} pass complete — LOS.',
  'sat.passAlert.los.dial': 'Dial handed back.',
  'sat.passAlert.los.split': 'Uplink split released.',
  'sat.passAlert.los.rotorPark': 'Rotor parking.',
  'sat.passAlert.los.rotorReady': 'Rotor moving to the ready position.',

  // The Connect ▸ Passes pane (components/prop/SatPassesPane.tsx) — the compact glance view.
  // `sat.passesPane.line` is also the pane's Basic projection, so it is one whole sentence.
  'sat.passesPane.noElements': 'No orbital elements yet — satellite data loads once online.',
  'sat.passesPane.noPasses':
    'No passes over your QTH in the next 24 h (set your grid in Settings?).',
  'sat.passesPane.line': 'Next: {{name}} {{when}}, max {{maxEl}}° {{aos}}→{{los}}.',
  'sat.passesPane.now': 'now',
  'sat.passesPane.inMins': 'in {{mins}} min',
  'sat.passesPane.favFilter.aria': 'Filter to ★ birds',
  'sat.passesPane.favFilter.on':
    'Showing your ★ birds (map + globe follow) — click to show all satellites',
  'sat.passesPane.favFilter.off':
    'Showing all satellites — click to show only your ★ birds (map + globe follow)',
  'sat.passesPane.favFilter.all': 'All',
  'sat.passesPane.stale.title': 'Orbital elements decay; pass times drift as they age',
  'sat.passesPane.stale': 'stale elements ({{days}} d) — times are approximate',
  'sat.passesPane.mostlyStale.title':
    'The typical bird in your catalog is still current, so the times below are good for it — but most of the set is past the 14-day line, or held back past 30 days. Refresh elements from the Satellites section.',
  'sat.passesPane.mostlyStale': '{{label}} — most times are approximate',
  'sat.passesPane.favEmpty': 'No passes for your ★ birds in the next 24 h.',
  'sat.passesPane.chase.on': 'Chasing — sorts first, footprint ring on the map. Click to stop.',
  'sat.passesPane.chase.off':
    'Chase this bird — sort its passes first + draw its footprint on the map',
  'sat.passesPane.peakEl.title': 'Peak elevation {{el}}° — higher = longer, stronger pass',
  'sat.passesPane.arc.title': 'Rise → set compass directions',
  'sat.passesPane.duration': '{{mins}} min',

  // ── Satellites ▸ the section ─────────────────────────────────────────────────────────
  'sat.head.title': 'Satellites',
  'sat.head.sub': 'passes over your grid',
  'sat.head.bands.title':
    'Elements past the 14-day line still fly the pass, with drifting times; elements past 30 days are not used at all — each of those birds is listed, with its reason, in the Birds list.',
  'sat.head.stale.title':
    'Orbital elements are {{days}} days old — pass times and Doppler drift with element age. Click to refresh now; they also refresh automatically every 6 h when the network allows.',
  // The chip voice is uppercase, so the unit is spelled out ("9 d" would render "9 D"), and
  // it is always plural — stale starts past 14 days. `TLE` is the element set's own name.
  'sat.head.stale.chip': 'TLE {{days}} days — STALE',
  'sat.head.stale.refreshing': '· refreshing…',
  'sat.head.stale.refresh': '· refresh',
  'sat.head.quiet.title':
    'Orbital elements are {{days}} days old (the median of the {{sets}} sets in use). They refresh automatically every 6 h when the network allows; click to refresh now.',
  'sat.head.quiet.titleEmpty':
    'No usable orbital elements are cached. Click to fetch them now; you can also import a file under Settings ▸ Radio ▸ Orbital elements.',
  'sat.head.quiet.refreshing': '⟳ refreshing…',
  'sat.head.quiet.refresh': '⟳ refresh elements',
  'sat.head.popOut.title': 'Open in its own window',
  'sat.head.seed.notice':
    'Starred {{count}} active birds to get you started — change them any time with the ★ beside any bird.',
  'sat.head.seed.dismiss.aria': 'Dismiss the starred-birds notice',
  'sat.head.seed.dismiss.title': 'Dismiss — your ★ birds stay exactly as they are',

  // The pass-quality ladder — the section's ONE vocabulary for "how good is this pass",
  // read by the Next/Best strip and by the discovery band's elevation cell.
  'sat.quality.overhead': 'overhead pass',
  'sat.quality.high': 'high pass',
  'sat.quality.workable': 'workable pass',
  'sat.quality.low': 'low horizon pass',

  // The countdown to a pass, and the two states a pass itself can be in.
  'sat.countdown.now': 'NOW',
  'sat.countdown.mins': 'in {{mins}} min',
  'sat.countdown.hours': 'in {{hours}} h',
  'sat.state.inPass': 'IN PASS — {{mins}} min to LOS',
  'sat.state.prepositioning': 'slewing to the AOS azimuth',

  // The why-line under a strip row. ONE sentence: the fragments it used to be could not be
  // reordered by a language that words "already up, 47° high pass, 11 min" differently.
  'sat.why': '{{when}} — {{el}}° {{quality}}, {{dur}} min, {{aos}}→{{los}}{{status}}',
  // Reported by the backscan rather than modelled: the pass out-lasted the scan window, so
  // its rise time is unknown and its duration is a lower bound (the code marks it `+`).
  'sat.why.alreadyUp': 'already up',
  'sat.why.alive': ' · reported alive (SatNOGS)',
  'sat.why.reported': ' · reported {{status}} (SatNOGS)',

  // What a pass would EARN, in the app's need-chip vocabulary.
  'sat.earn.entities.title': {
    one: "{{count}} never-worked entity reachable through this pass's footprint: {{names}}{{more}}",
    other:
      "{{count}} never-worked entities reachable through this pass's footprint: {{names}}{{more}}",
  },
  // Satellite VUCC is the award programme's own name — an operator applies for it by that
  // name, and a translated one names nothing.
  'sat.earn.grids.title': {
    one: "{{count}} new Satellite VUCC grid reachable through this pass's footprint: {{grids}}{{more}}",
    other:
      "{{count}} new Satellite VUCC grids reachable through this pass's footprint: {{grids}}{{more}}",
  },
  'sat.earn.more': ' +{{count}} more',

  // The sky dome. Only the WORDS are here — every angle, the compass letters and the plate
  // text the geometry is sized from stay in the component.
  'sat.dome.aria.bird':
    'Sky dome for {{name}}, north up. Satellite at azimuth {{az}} degrees, elevation {{el}} degrees.',
  'sat.dome.aria.track':
    'Sky dome for {{name}}, north up. Pass track from azimuth {{aos}} to {{los}} degrees, maximum elevation {{maxEl}} degrees.',
  'sat.dome.aos.title': 'AOS — rises at {{az}} ({{wind}}) {{time}}',
  'sat.dome.los.title': 'LOS — sets at {{az}} ({{wind}}) {{time}}',
  'sat.dome.ghost.title': 'Antenna: commanded az/el (not a rotator read-back)',
  'sat.dome.ghost.azOnly.title':
    'Antenna: azimuth {{az}} commanded — az-only rotator, no elevation sent',
  'sat.dome.bird.title': '{{name}} — az {{az}} el {{el}}',
  'sat.dome.azEl': 'az {{az}} el {{el}}',
  'sat.dome.antenna.noCommand': 'armed — no rotor command sent yet',
  'sat.dome.antenna.azOnly': 'az {{az}} · elevation not commanded (az-only rotator)',
  'sat.dome.readout.satellite': 'Satellite',
  'sat.dome.readout.range': 'Range',
  'sat.dome.readout.range.title': 'Slant range — how far the bird is FROM YOU.',
  'sat.dome.readout.km': '{{km}} km',
  'sat.dome.readout.rangeRate': ' · {{rate}} km/s {{trend}}',
  'sat.dome.readout.closing': 'closing',
  'sat.dome.readout.opening': 'opening',
  'sat.dome.readout.altitude': 'Altitude',
  'sat.dome.readout.altitude.title':
    'Altitude — how far above the earth the bird is. Not range (its distance from you).',
  'sat.dome.readout.antenna': 'Antenna',
  'sat.dome.readout.antenna.title':
    'What the rotator was COMMANDED — not a read-back. Δ is the true angular gap to the bird.',
  'sat.dome.readout.riseSet': 'Rise / set',

  // The AOS · TCA · LOS timeline under the dome.
  'sat.timeline.aos': 'AOS {{time}}',
  'sat.timeline.los': 'LOS {{time}}',
  'sat.timeline.peak': '{{tca}}max {{maxEl}}°',
  'sat.timeline.tca': 'TCA {{time}} · ',
  'sat.timeline.nextPass': 'next pass {{countdown}}',

  // The Doppler readout. When nothing is being tuned it says WHY, in the words of the thing
  // the operator would have to change.
  'sat.doppler.head': 'Doppler',
  'sat.doppler.none.off':
    'Doppler is off — nothing is being tuned (Settings ▸ Radio ▸ Satellite Doppler).',
  'sat.doppler.none.pickConfirm':
    'No transponder selected — pick one below; once your mapping is confirmed for this radio, Doppler tunes your uplink and the dial stays yours.',
  'sat.doppler.none.pickUplink':
    'No transponder selected — pick one below and Doppler tunes your uplink; the dial stays yours.',
  'sat.doppler.none.pick': 'No transponder selected — pick one below to put the dial under Doppler.',
  'sat.doppler.none.unconfirmed':
    'Your uplink-only mapping is not confirmed for this radio — nothing is being tuned.',
  'sat.doppler.none.beforeAos':
    'Doppler corrects from AOS — nothing to correct until the bird is up.',
  'sat.doppler.none.noTuning': 'Doppler has not reported a tuning for this pass yet.',
  // ONE sentence, two surfaces: the Doppler readout's reason and the chooser's state line
  // say the same thing about the same bird, and they must never drift apart.
  'sat.uplinkOnly.noLeg':
    'Your uplink-only mapping has nothing to drive here — this bird has no separate uplink. Nothing is being tuned.',
  'sat.leg.downlink': '↓ Downlink',
  'sat.leg.uplink': '↑ Uplink',
  'sat.doppler.txMode.title':
    "The TX (split) VFO's sideband — this bird's uplink runs {{mode}} while the downlink does not, and the radio's TX leg is set to match. Commanded by the engine with the Doppler tuning; shown here so a swapped sideband is never a surprise.",
  // The inverting mark and its lesson — one chip and one tooltip, rendered by the Doppler
  // head, the readiness rail and every transponder card.
  'sat.inverting.label': 'INVERTING',
  'sat.inverting.title':
    'Inverting linear transponder: tune the downlink UP and your uplink goes DOWN, and the sidebands swap (LSB up, USB down).',

  // The transponder passband strip.
  'sat.passband.head': 'Passband',
  'sat.passband.inverting': 'inverting — tune up, transmit down',
  'sat.passband.nonInverting': 'non-inverting — both legs move the same way',
  'sat.passband.inverting.word': 'inverting',
  'sat.passband.nonInverting.word': 'non-inverting',
  'sat.passband.aria':
    'Transponder passband, {{mode}}, ±{{half}} kHz either side of centre. Downlink {{down}} from centre, uplink {{up}} from centre.',
  'sat.passband.lane.downlink': 'Downlink',
  'sat.passband.lane.uplink': 'Uplink',
  'sat.passband.mark.title':
    '{{glyph}} {{leg}}{{freq}} — offset {{offset}} from passband centre{{clamped}}',
  'sat.passband.mark.freq': ' — {{hz}} Hz',
  'sat.passband.mark.clamped': ' (outside the passband — the mark is parked on the edge)',
  'sat.passband.axis.title': 'kHz from passband centre',
  'sat.passband.noWidth':
    'No passband to tune inside — this is a single channel, or SatNOGS carries no width for it. There is no axis to draw; the offsets below are still exact.',
  'sat.passband.readout.doppler': 'Doppler {{shift}}',
  'sat.passband.readout.offset': 'offset {{offset}}',
  'sat.passband.readout.width': '±{{half}} kHz from centre',

  // SatNOGS `type` in operator words, for the chooser cards.
  'sat.kind.beacon': 'beacon',
  'sat.kind.linear': 'linear',
  'sat.kind.fmRepeater': 'FM repeater',

  // The readiness rail — five gates, each not-ready row carrying its own fix.
  'sat.rail.gate.pass': 'Pass',
  'sat.rail.gate.rotor': 'Rotor',
  'sat.rail.gate.transponder': 'Transponder',
  'sat.rail.gate.doppler': 'Doppler',
  'sat.rail.gate.elements': 'Elements',
  'sat.rail.pass.armedIn': 'armed — AOS in {{mins}} min',
  'sat.rail.pass.armed': 'armed',
  'sat.rail.stop.title':
    'Stop this track (rotor halts if it holds one; the dial is handed back)',
  'sat.rail.rotor.lost':
    'stopped answering — pointing is yours; the pass, Doppler and your transponder keep running',
  'sat.rail.rotor.notInTrack': 'not in this track — re-arm to take the rotor',
  'sat.rail.rotor.none': 'no rotator configured — Settings ▸ Radio ▸ Rotator',
  'sat.rail.rotor.tracking': 'tracking · cmd az {{az}}{{el}}',
  'sat.rail.rotor.azOnly': ' (az only)',
  'sat.rail.rotor.armed': 'armed — takes the rotor 5 min before AOS',
  'sat.rail.transponder.none': 'none — the dial stays yours',
  'sat.rail.transponder.auto': ' picked for you',
  'sat.rail.transponder.change': 'change',
  'sat.rail.transponder.pick': 'pick',
  'sat.rail.transponder.goTo': 'Go to the transponder chooser below',
  'sat.rail.transponder.nothingToPick':
    'No transmitters listed for this bird (SatNOGS) — nothing to pick',
  'sat.rail.doppler.off': 'off — nothing is being tuned',
  'sat.rail.doppler.waitingConfirm':
    'waiting for a transponder — once your mapping is confirmed for this radio, Doppler tunes your uplink; the dial stays yours',
  'sat.rail.doppler.waitingUplink':
    'waiting for a transponder — then Doppler tunes your uplink; the dial stays yours',
  'sat.rail.doppler.waiting': 'waiting for a transponder — then the downlink follows the bird',
  'sat.rail.doppler.simplex': 'on — one channel: both legs ride the same dial',
  'sat.rail.doppler.simplexUplinkOnly':
    'one channel and an uplink-only mapping — nothing is being tuned; the dial stays yours',
  'sat.rail.doppler.both': 'correcting the downlink and the uplink',
  'sat.rail.doppler.downlink': 'correcting the downlink. {{next}}',
  'sat.rail.doppler.uplink': 'correcting the uplink — the dial stays yours',
  'sat.rail.doppler.uplinkOnly': 'uplink-only mapping, nothing is being tuned. {{next}}',
  // What ONE confirmation would hand over, named for the radio that would receive it.
  'sat.rail.doppler.yourRadio': 'your radio',
  'sat.rail.uplink.confirm': 'Confirm the uplink and Doppler drives {{rig}} as {{pair}}.',
  'sat.rail.uplink.confirmMapping':
    'Confirm {{mapping}} for {{rig}} and Doppler drives its uplink.',
  'sat.rail.uplink.switchMapping':
    'Your mapping cannot carry this pass on {{rig}} — switch it to {{pair}} and Doppler drives the uplink.',
  'sat.rail.uplink.ask': 'Pick which VFO carries your uplink to have Doppler tune that too.',
  'sat.rail.uplink.yours': 'The transmit VFO stays yours.',
  'sat.rail.doppler.turnOn': 'turn on',
  'sat.rail.doppler.turnOn.title':
    'Turn Doppler correction back on (the same switch as Settings ▸ Radio ▸ Satellite Doppler)',
  'sat.rail.uplink.switch.label': 'switch mapping',
  'sat.rail.uplink.confirm.label': 'confirm uplink',
  'sat.rail.uplink.switch.title':
    'Switch {{rig}} to {{pair}}. Your mapping cannot carry this pass on this radio — nothing was written to it — and this is the layout Nexus drives here. Nothing changes until you click, and a wrong mapping transmits on your own downlink.',
  'sat.rail.uplink.confirm.title':
    'Confirm {{pair}} for {{rig}}. Nexus read this from your radio model; nothing reaches your transmit VFO until you confirm it, and a wrong mapping transmits on your own downlink.',
  'sat.rail.uplink.confirmMapping.title':
    'Confirm {{mapping}} — your chosen mapping — for {{rig}}. Nothing reaches its transmit VFO until you confirm it for this radio, and a wrong mapping transmits on your own downlink.',
  'sat.rail.vfoMap.aria': 'Satellite VFO mapping',
  'sat.rail.vfoMap.title':
    'Which VFO carries your uplink — match it to how your radio is wired. A wrong mapping transmits on your own downlink, into the satellite\'s output passband, on top of everyone else working the bird. Picking one confirms it for the radio Doppler is driving. Every mapping except Uplink only keeps the downlink corrected; Uplink only hands your one VFO to the transmit leg.',
  'sat.rail.elements.current': '{{age}} d old — current',
  'sat.rail.elements.stale': '{{days}} days old — pointing and Doppler drift',
  'sat.rail.elements.refresh': 'refresh',
  'sat.rail.elements.refresh.title':
    'Fetch fresh orbital elements now. This armed pass keeps its frozen set — re-arm to track with the fresh one.',

  // Which rig will move (the radio binding line), and the way back onto the bird.
  'sat.binding.name': 'Radio',
  'sat.binding.thisRadio': 'this radio',
  'sat.binding.legs': ' — {{legs}} MHz{{note}}',
  'sat.binding.note': ' — {{note}}',
  'sat.binding.pegged': '🔒 pinned',
  'sat.binding.unpegged': '🔓 pin this radio',
  'sat.binding.pegged.title':
    'Peg-lock is ON — this bird stays on the active radio; band+mode routing will not hand it to another rig. Click to unlock.',
  'sat.binding.unpegged.title':
    'Peg-lock is OFF — a pick routes to the radio that owns the band and mode class. Click to pin the active radio instead.',
  'sat.lockOn.name': 'Dial',
  'sat.lockOn.state': 'moved off the transponder? put the radio back on the bird',
  'sat.lockOn.label': 'Lock on',
  'sat.lockOn.title':
    'Re-run your transponder pick — routes, sets the band and mode, writes both legs, and re-centres you in the passband. Use it after moving the dial off the transponder by hand, or when the rig came back somewhere else.',

  // The tracking badge on the arm bar.
  'sat.badge.rotorLost.title':
    'The rotator stopped answering, so this track let it go — point the antenna yourself. The pass clock, the transponder and Doppler carry on to LOS.',
  'sat.badge.passOnly.title':
    'Pass timing only — nothing is driven: no rotor in this track, and Doppler is not driving the dial (correction switched off, no transponder held, or an uplink-only mapping that is not driving this pass). The pass clock and geometry still run.',
  'sat.badge.dopplerOnly.title': 'No rotator in this track — Doppler {{what}}; nothing moves an antenna',
  'sat.badge.dopplerOnly.dialNow': 'is steering the radio dial',
  'sat.badge.dopplerOnly.dialAtAos': 'takes the radio dial at AOS',
  'sat.badge.dopplerOnly.splitNow': 'is steering the TX (split) VFO — the dial stays yours',
  'sat.badge.dopplerOnly.splitAtAos': 'takes the TX (split) VFO at AOS — the dial stays yours',
  'sat.badge.rotorWaiting.title':
    'The rotor has NOT been commanded yet — auto-track takes it 5 min before AOS',
  'sat.badge.rotorDriving.title':
    'Auto-track is driving the rotor — angles shown are what was COMMANDED (rotctld read-back lives on the rotor strip/pane)',
  'sat.badge.open.title':
    "Open this pass's detail (sky dome, readiness rail) — tracking is not affected",
  'sat.badge.armed': 'armed',
  'sat.badge.tracking': 'tracking',
  'sat.badge.mode.rotorLost': ' · rotor stopped answering',
  'sat.badge.mode.dopplerOnly': ' · Doppler only',
  'sat.badge.mode.passOnly': ' · pass timing only',
  'sat.badge.mode.rotorOnly': ' · rotor only',
  'sat.badge.risesAz': 'rises az {{az}}°',
  'sat.badge.cmdAz': 'cmd az {{az}}° {{el}}',
  'sat.badge.azOnly': '(az only)',
  // The one stop shared by the rail and the badge — the same act, so the same word.
  'sat.track.stop': '■ stop',
  'sat.track.stop.rotor.title': 'Stop auto-tracking (rotor halts)',
  'sat.track.stop.noRotor.title': 'Stop this track (no rotor involved; the dial is handed back)',
  'sat.detail.close.aria': "Close this bird's detail",
  'sat.detail.close.title':
    'Close — a tracked pass keeps tracking; the badge on this bar brings you back',

  // The gate ladder and the Next/Best strip.
  'sat.noGrid':
    'Set your grid square (Settings ▸ Station) first — passes are computed over YOUR location, and without a locator there is nothing honest to show.',
  'sat.strip.next.label': 'Next',
  'sat.strip.next.why':
    'The two soonest workable passes (10° peak or better), any bird — a pass already in progress leads',
  'sat.strip.best.label': 'Best 24 h',
  'sat.strip.best.why':
    'The two highest-quality passes in the next 24 h — max elevation first, duration breaking ties',
  'sat.strip.row.title': '{{name}} — {{why}}',
  'sat.strip.fav.title': 'One of your ★ birds',
  'sat.work.label': '▶ Work this pass',
  'sat.work.title.rotor':
    'Work this pass: opens the bird, picks its transponder, arms rotor auto-track + the pass clock (Doppler tunes when its switches are on)',
  'sat.work.title.noRotor':
    'Work this pass: opens the bird, picks its transponder, starts the pass clock + Doppler (no rotator configured — nothing will move)',
  'sat.work.short': '▶ Work',

  // The 48 h schedule, its sortable headers and the discovery band beneath it.
  'sat.schedule.head': 'Schedule — favorites, next {{hours}} h',
  'sat.schedule.sortBy': 'Sort by {{label}}',
  'sat.schedule.column.bird': 'Bird',
  'sat.schedule.column.aos': 'AOS local',
  'sat.schedule.column.maxEl': 'Max el',
  'sat.schedule.column.dur': 'Dur',
  'sat.schedule.column.path': 'Path',
  'sat.schedule.column.status': 'Status',
  'sat.schedule.column.needed': 'Needed',
  'sat.schedule.empty.noFavs':
    'No ★ birds yet — star a bird (☆ in the rows below, or in the Birds list) and its passes, alarms and needed-grid chips appear here.',
  'sat.schedule.empty.noPasses':
    'No upcoming passes for your favorites in the next {{hours}} h{{why}}.',
  'sat.schedule.empty.noElements':
    ' — waiting for orbital elements (first fetch needs the network once)',
  'sat.schedule.empty.excluded':
    ' (birds whose elements are older than 30 days are excluded until a refresh)',
  'sat.schedule.unstar.title':
    'Unstar removes the bird from this schedule and disarms its alarm',
  'sat.schedule.alive.label': 'alive',
  'sat.schedule.alive.title': 'SatNOGS community reports it transmitting',
  'sat.schedule.dead.title':
    'SatNOGS reports it silent/re-entered — geometry still shown, working it is unlikely',
  'sat.schedule.alarm.on': 'Alarm armed — click to disarm',
  'sat.schedule.alarm.off': 'Wake me before this bird rises (per-bird, survives restarts)',
  'sat.schedule.alarm.lead.title': 'Lead time before AOS',
  'sat.schedule.stop.title': 'Stop this track',
  'sat.discovery.chip': 'Other birds overhead · {{count}} workable · 24 h',
  'sat.discovery.chip.title':
    'Birds outside your ★ set flying a workable pass (10° peak or better) over your grid in the next 24 h — one row per bird, its best pass. Star one to move it into your schedule.',
  'sat.discovery.bar':
    "Other birds — each row is that bird's best pass in the next 24 h · ☆ moves it into your schedule",
  'sat.discovery.star.title':
    'Star to move this bird into your schedule above — it gains needed-grid chips, the pass alarm and the 48 h view',
  'sat.discovery.workable.title': 'Workable passes (10° peak or better) in the next 24 h',
  'sat.discovery.workable': '{{count}} in 24 h',
  'sat.discovery.altitude.title': 'Current altitude',
  'sat.discovery.altitude': '{{km}} km up',
  'sat.discovery.clamped.title':
    'Rose before the 6 h scan window — its true rise time is unknown here',
  'sat.discovery.showAll': 'show all {{count}} ▾',
  'sat.discovery.empty':
    'no other birds with a workable pass (10° peak or better) over your grid in the next 24 h',

  // The radio quadrant: what the dial is doing, and what to point it at.
  'sat.radio.frequencies.head': 'Frequencies',
  'sat.radio.idle.noBird':
    'Open a bird to see its transponders; arm a pass and the live dial, the Doppler shift and your place in the passband read out here.',
  'sat.radio.idle.noTrack':
    'Arm a pass (▶ Work this pass) and the live dial, the Doppler shift and your place in the passband read out here.',
  'sat.transponder.head': 'Transponder',
  'sat.transponder.list.aria': 'Transponder — where Doppler puts the dial',
  'sat.transponder.none.aria': 'Work no transponder — leave the dial to me',
  'sat.transponder.none.label': 'None — leave the dial to me',
  'sat.transponder.card.aria': 'Work {{description}}',
  'sat.transponder.auto': 'picked for you — change it here if this is not the one',
  'sat.transponder.showAll': 'show all {{count}} ▾',
  'sat.transponder.showAll.title': 'Show every workable transmitter SatNOGS lists for this bird',
  'sat.transponder.dead': 'reported dead (SatNOGS) — not workable',
  'sat.transponder.showDead': 'show {{count}} inactive',
  'sat.transponder.showDead.title':
    'Transmitters SatNOGS reports dead/re-entered — shown for the record, never workable',
  // The TX-sideband note. `{{tx}}`, `{{up}}` and `{{down}}` are the record's own mode names.
  'sat.transponder.txMode.commanded':
    'TX sideband: the uplink (split) VFO is set to <b>{{tx}}</b> — the downlink stays {{down}} while Doppler runs this pass.',
  'sat.transponder.txMode.notCommanded':
    'This bird lists {{up}} up / {{down}} down (SatNOGS) — the TX sideband is not being commanded for this pass ({{why}}).',
  'sat.transponder.txMode.why.dopplerOff': 'Doppler correction is off',
  'sat.transponder.txMode.why.notDriving': 'Doppler is not driving the uplink on this radio',
  'sat.transponder.txMode.why.shared': 'the legs share a sideband, or the mode is yours',
  'sat.transponder.txMode.forecast':
    'TX sideband: this bird runs {{up}} up / {{down}} down (SatNOGS). Once your uplink mapping is confirmed for the radio in use, the TX (split) VFO is set to match while a tracked pass runs.',
  'sat.transponder.state.dopplerOff':
    'Doppler correction is off, so nothing is being tuned. Turn it on in Settings ▸ Radio ▸ Satellite Doppler.',
  'sat.transponder.state.uplinkOnly.driving':
    'Your uplink-only mapping keeps the dial yours — only the transmit VFO is tuned.',
  'sat.transponder.state.uplinkOnly.unconfirmed':
    'Your uplink-only mapping is not confirmed for this radio — nothing is being tuned. Confirm it on the pass rail.',
  'sat.transponder.state.uplinkOnly.pending':
    'Your uplink-only mapping keeps the dial yours — the transmit VFO is tuned once it is confirmed for the radio in use and a tracked pass runs.',
  'sat.transponder.state.tuning':
    'Doppler tunes this transponder while auto-track is following the pass.',
  'sat.transponder.otherBird':
    'Doppler holds a transponder on {{name}}. Picking one here takes the dial from it.',
  // ⚠️ The attribution itself: the source's name and the licence identifier are what the
  // CC-BY-SA terms require to be reproduced, so they stay exactly as they are.
  'sat.credit.satnogs': 'frequencies & status: SatNOGS DB (CC-BY-SA 4.0)',
  'sat.credit.noBird': 'Open a bird to choose which transponder Doppler puts the dial on.',
  'sat.credit.noData': 'no transponder data yet — fetched from SatNOGS DB when online',
  'sat.credit.noTransmitters': 'no transmitters listed for this bird (SatNOGS DB)',

  // The pass line that stands in for the dome when there is no pass in the detail's window.
  'sat.passline.beyond': 'next pass over you rises {{time}} ({{countdown}})',
  'sat.passline.none': 'no pass over you in the next 24 h',

  // The log strip's note. ⚠️ PROP_MODE and SAT_NAME are ADIF field names — wire identifiers
  // an operator types into their own log — and LoTW and VUCC are the programmes' own names.
  'sat.log.note':
    'Logs an ordinary contact from your dial, exactly as the Phone and CW log panels do. It is <b>not</b> tagged as a satellite QSO: Nexus does not write the ADIF PROP_MODE and SAT_NAME fields yet, so the contact counts toward neither LoTW satellite credit <b>nor Nexus’s own satellite totals</b>. Add <b>both</b> fields yourself if you want that credit — one without the other is refused at signing, and on 2 m the grid otherwise counts toward your terrestrial VUCC, which a satellite contact does not earn.',
  'sat.log.note.title':
    'Logs an ordinary contact from your dial, exactly as the Phone and CW log panels do. It is NOT tagged as a satellite QSO: Nexus does not write the ADIF PROP_MODE and SAT_NAME fields yet, so the contact counts toward neither LoTW satellite credit nor Nexus\'s own satellite totals. Add BOTH fields yourself if you want that credit — one without the other is refused at signing, and on 2 m the grid otherwise counts toward your terrestrial VUCC, which a satellite contact does not earn.',

  // The Birds catalog.
  'sat.birds.head': 'Birds ({{count}})',
  'sat.birds.search.placeholder': 'search…',
  'sat.birds.star.title': '★ favorites drive the schedule, the map emphasis, and alarms',
  'sat.birds.alt': 'alt {{km}} km',
  'sat.birds.alt.title':
    'Altitude — how far above the earth the bird is right now. Not range (its distance from you).',
  'sat.birds.empty': 'no elements yet — first fetch needs the network once',

  // The toasts the section raises, and the >14 d arm confirm.
  'sat.toast.track': 'Pass track {{name}}: {{doing}}',
  'sat.toast.track.rotor': 'armed — the rotor stays yours until 5 min before AOS',
  'sat.toast.track.doppler': 'armed — no rotor in this track; Doppler takes the dial at AOS',
  'sat.toast.track.passOnly': 'armed — pass timing only; the dial stays yours',
  'sat.toast.track.following': 'following the pass',
  'sat.toast.track.nothing': 'Nothing to track — no matching pass to arm',
  'sat.toast.track.failed': 'Track failed: {{error}}',
  'sat.toast.transponder.cleared': 'Transponder cleared — the dial is yours',
  'sat.toast.transponder.working': 'Working {{name}} {{label}}{{auto}}',
  'sat.toast.transponder.working.auto': ' (picked for you — change below)',
  'sat.toast.transponder.failed': 'Transponder not selected: {{error}}',
  'sat.toast.doppler.failed': 'Doppler setting: {{error}}',
  'sat.toast.peg.failed': 'Peg-lock: {{error}}',
  'sat.toast.vfoMap.failed': 'VFO mapping: {{error}}',
  'sat.armConfirm.title': 'Stale orbital elements',
  'sat.armConfirm.body':
    '{{name}}: elements are {{days}} days old — pointing and Doppler will be off.',
  'sat.armConfirm.refreshing': 'Refreshing…',
  'sat.armConfirm.refresh': 'Refresh elements',
  'sat.armConfirm.armAnyway': 'Arm anyway',
  'sat.armConfirm.cancel': 'Cancel',

  // ── Memories (the saved-channel manager and the cockpit MEM strip) ──────────────────
  // ⚠️ Almost everything an operator READS on this screen is a unit, and none of it is here:
  // the dial and TX frequencies, offsets, CTCSS tones and DTCS codes, the mode names and the
  // mode/CTCSS datalists, callsigns, the group names the operator typed, the HF and VHF/UHF
  // section labels, and the POTA/SOTA programme name (a constant in the component, as it is
  // in `needVisuals.ts`). Weekday abbreviations are date formatting rather than prose and stay
  // in the component too. The `value` of every <select> is the STORED token; only its label is
  // here — and the export file name is built from a slug, never from the words below.
  'memories.aria': 'Memories',

  // The channel kinds. Each entry's key is the value the memory stores.
  'memories.kind.repeater': 'Repeater',
  'memories.kind.simplex': 'Simplex',
  'memories.kind.hfnet': 'HF net',
  'memories.kind.calling': 'Calling',
  'memories.kind.digital': 'Digital',
  'memories.kind.satellite': 'Satellite',
  'memories.kind.emcomm': 'EmComm',
  'memories.kind.reference': 'Reference',
  'memories.kind.other': 'Other',

  // The sidebar: the three built-in views, then the operator's own groups.
  'memories.side.all': 'All memories',
  'memories.side.fav': '★ Favorites',
  'memories.side.nets': 'Nets',
  'memories.side.group.rename.title': 'Rename group',
  'memories.side.group.delete.title': 'Delete group (memories stay)',
  'memories.side.newGroup.placeholder': 'New group…',

  // One WHOLE placeholder per view rather than "Search " plus a name: a name spliced into a
  // sentence cannot be reordered, and lower-casing a translated noun is wrong in every
  // language that capitalises them. The group's own name is the operator's text.
  'memories.search.placeholder.all': 'Search all…',
  'memories.search.placeholder.fav': 'Search favorites…',
  'memories.search.placeholder.nets': 'Search nets…',
  'memories.search.placeholder.group': 'Search {{group}}…',
  'memories.search.placeholder.groupless': 'Search group…',

  // The toolbar. `{{freq}}` and `{{mode}}` are the live dial — tokens, never formatted.
  'memories.toolbar.list.label': 'List',
  'memories.toolbar.list.title': 'List view — clean rows with an inline editor',
  'memories.toolbar.grid.label': 'Grid',
  'memories.toolbar.grid.title': 'Grid view — the CHIRP-style spreadsheet',
  'memories.toolbar.save.label': '＋ Save {{freq}} {{mode}}',
  'memories.toolbar.save.title': 'Save the current dial frequency + mode as a memory',
  'memories.toolbar.new.label': '＋ New',
  'memories.toolbar.new.title': 'Add a memory by hand',
  'memories.toolbar.import.label': 'Import CSV',
  'memories.toolbar.import.title': 'Import a CHIRP CSV (duplicates are skipped)',
  'memories.toolbar.export.label': 'Export CSV ({{count}})',
  'memories.toolbar.export.title': {
    one: 'Export the {{count}} shown channel as a CHIRP CSV (imports into ~1,000 radio models)',
    other: 'Export the {{count}} shown channels as a CHIRP CSV (imports into ~1,000 radio models)',
  },
  'memories.toolbar.popOut.label': '↗ Pop out',
  'memories.toolbar.popOut.title': 'Pop Memories out into its own window (multi-monitor)',
  'memories.toolbar.packs.label': 'Packs',
  'memories.toolbar.packs.title':
    'Install curated channel sets — nets, calling frequencies, POTA, digital',

  // The empty state. The hint names two buttons that are entries of their own; a translation
  // has to keep the three consistent, which is why they read as the same words here.
  'memories.empty.none': 'No memories yet.',
  'memories.empty.hint':
    'Start with a <b>starter pack</b> — nets, calling frequencies, POTA, and digital watering holes, ready to go. Or save the current frequency with <b>＋ Save</b>, import a CHIRP CSV, or send repeaters here from the Program section. Star a memory (★) and it shows on the MEM strip in every cockpit.',
  'memories.empty.browsePacks': 'Browse starter packs',
  'memories.empty.noMatch': 'Nothing matches this view.',

  // The CHIRP-style grid. The column headers name technical quantities and the UNIT inside
  // one (MHz) is a token — reword the header around it, never the unit.
  'memories.grid.column.favorite': 'Favorite',
  'memories.grid.column.name': 'Name',
  'memories.grid.column.rx': 'RX MHz',
  'memories.grid.column.mode': 'Mode',
  'memories.grid.column.offset': 'Offset',
  'memories.grid.column.tone': 'Tone',
  'memories.grid.column.kind': 'Kind',
  'memories.grid.column.actions': 'Actions',
  'memories.grid.tune.label': 'Tune',
  'memories.grid.tune.title': 'Tune to this memory',

  // A row in either view. Recall is a RETUNE — it moves the dial, it never transmits.
  'memories.row.star.title': 'Star (show on cockpit strips)',
  'memories.row.unstar.title': 'Unstar (remove from cockpit strips)',
  'memories.row.main.title': 'Tune to {{freq}} MHz {{mode}}',
  'memories.row.moveUp.aria': 'Move {{name}} up',
  'memories.row.moveDown.aria': 'Move {{name}} down',
  'memories.row.moveUp.title': 'Move up',
  'memories.row.moveDown.title': 'Move down',
  'memories.row.moveUp.rank.title': 'Move up one rank (1–{{limit}} are the strip)',
  'memories.row.moveDown.rank.title': 'Move down one rank (1–{{limit}} are the strip)',
  'memories.row.tune.label': 'Tune',
  'memories.row.tune.title':
    'Tune — sets frequency, mode, offset, and tone, and opens the right cockpit',
  'memories.row.edit.title': 'Edit',
  'memories.row.delete.title': 'Delete this memory',
  // The ★ view's rank badge. `{{hotkey}}` is the keyboard chord, printed by `modChord`.
  'memories.rank.on.title': 'Chip {{rank}} on the cockpit MEM strip{{hotkey}}',
  'memories.rank.off.title':
    'Rank {{rank}} — past the {{limit}} chips the cockpit MEM strip shows. Move it up with ▲.',

  // The inline editor. TSQL, DTCS, CTCSS and the +/− signs are the radio's own vocabulary;
  // only the words around them are prose.
  'memories.editor.name.label': 'Name',
  'memories.editor.kind.label': 'Kind',
  'memories.editor.rx.label': 'RX MHz',
  'memories.editor.mode.label': 'Mode',
  'memories.editor.offset.label': 'Offset',
  'memories.editor.offset.simplex': 'Simplex',
  'memories.editor.offset.plus': '+ up',
  'memories.editor.offset.minus': '− down',
  'memories.editor.offset.split': 'Odd split',
  'memories.editor.offsetMhz.label': 'Offset MHz',
  'memories.editor.txMhz.label': 'TX MHz',
  'memories.editor.tone.label': 'Tone',
  'memories.editor.tone.none': 'None',
  'memories.editor.tone.tone': 'Tone (encode)',
  'memories.editor.tone.tsql': 'TSQL (enc+dec)',
  'memories.editor.tone.dtcs': 'DTCS',
  'memories.editor.ctcss.label': 'CTCSS Hz',
  'memories.editor.dtcs.label': 'DTCS code',
  'memories.editor.callsign.label': 'Callsign',
  'memories.editor.days.label': 'Days',
  'memories.editor.days.aria': 'Net days (UTC)',
  'memories.editor.start.label': 'Start (UTC)',
  'memories.editor.remind.label': 'Remind me',
  'memories.editor.remind.aria': 'Enable a reminder for this net',
  'memories.editor.lead.aria': 'Reminder lead time in minutes',
  'memories.editor.lead.unit': 'min before (UTC schedule)',
  'memories.editor.notes.label': 'Notes',
  'memories.editor.groups.label': 'Groups',
  'memories.editor.groups.aria': 'Group membership',
  'memories.editor.done': 'Done',

  // The mode and CTCSS pickers. Every choice is always offered (a datalist filtered them by
  // what was already in the field, which left one mode reachable), and "Other…" is the escape
  // that lets a mode or tone we do not list — a CHIRP import carries them — still be typed in.
  'memories.picker.other': 'Other…',

  // The pinned add panel ＋ New opens. The channel exists from the moment it is pressed, so
  // backing out keeps it and Discard is what throws it away.
  'memories.add.title': 'New memory',
  'memories.add.hint': 'Enter saves · Esc closes — the channel is kept either way',
  'memories.add.discard': 'Discard',
  'memories.add.discard.title': 'Delete this new memory and close the panel',

  // Selecting rows and deleting them together. Every count here is the SELECTED ROWS ON
  // SCREEN — a bulk delete never touches a row the operator has narrowed away.
  'memories.select.row.aria': 'Select {{name}}',
  'memories.select.all.aria': 'Select all shown',
  'memories.select.all.label': 'All shown',
  'memories.select.count': {
    one: '{{count}} selected',
    other: '{{count}} selected',
  },
  // Shown INSTEAD of the plain count when part of the selection has been narrowed out of
  // view: the numbers on this bar are the visible selection, so without this the count
  // silently disagrees with how many rows are actually ticked.
  'memories.select.countHidden': '{{count}} selected · {{hidden}} not in view',
  'memories.select.clear': 'Clear',
  'memories.select.delete.label': 'Delete {{count}}',
  'memories.select.delete.title': 'Delete the selected memories in this view',
  'memories.select.confirm.title': {
    one: 'Delete {{count}} memory?',
    other: 'Delete {{count}} memories?',
  },
  'memories.select.confirm.body':
    'They leave every group and the cockpit strips too. The toast that follows can undo it.',
  'memories.select.confirm.ok': {
    one: 'Delete {{count}} memory',
    other: 'Delete {{count}} memories',
  },
  'memories.select.deleted': {
    one: 'Deleted {{count}} memory',
    other: 'Deleted {{count}} memories',
  },
  'memories.select.undo': 'Undo',
  'memories.select.restored': {
    one: 'Restored {{count}} memory',
    other: 'Restored {{count}} memories',
  },

  // Starter packs. The pack's name, description and region are the pack's own data.
  'memories.packs.title': 'Starter packs',
  'memories.packs.close.aria': 'Close',
  'memories.packs.sub':
    'One-click channel sets. Duplicates are skipped, so installing again is safe. Net schedules are UTC and approximate — enable a reminder per net.',
  'memories.packs.meta': '{{count}} channels · {{region}}',
  'memories.packs.install': 'Install',
  'memories.packs.update': 'Update',
  // Two counts in one report, so each clause is its own entry with its own plural — one
  // message cannot select a form for both (the `logbook.import.dupes` ruling).
  'memories.packs.toast': '{{pack}} — {{parts}}',
  'memories.packs.toast.added': {
    one: 'added {{count}} channel',
    other: 'added {{count}} channels',
  },
  'memories.packs.toast.refreshed': {
    one: 'refreshed {{count}} channel',
    other: 'refreshed {{count}} channels',
  },
  'memories.packs.toast.upToDate': '{{pack}} — already up to date',

  // What the section says after it acts.
  'memories.toast.saved': 'Saved {{freq}} {{mode}}',
  'memories.toast.alreadySaved': '{{freq}} {{mode}} is already saved',
  'memories.export.empty': 'Nothing to export in this view',
  'memories.export.done': {
    one: 'Exported {{count}} channel → {{path}}',
    other: 'Exported {{count}} channels → {{path}}',
  },
  'memories.import.notChirp': 'No channels found — is this a CHIRP CSV?',
  'memories.import.done': {
    one: 'Imported {{count}} channel',
    other: 'Imported {{count}} channels',
  },
  // Its own statement, with its own count and its own leading separator.
  'memories.import.dupes': {
    one: ' ({{count}} duplicate skipped)',
    other: ' ({{count}} duplicates skipped)',
  },

  // The cockpit MEM strip. `{{hotkey}}` is a keyboard chord and `{{tone}}` the clause below.
  'memories.strip.aria': 'Memory quick recall',
  'memories.strip.label': 'MEM',
  'memories.strip.label.title': 'Memory quick recall — your ★-starred memories',
  'memories.strip.save.title': 'Save {{freq}} {{mode}} as a favorite memory',
  'memories.strip.chip.title': '{{name}} — {{freq}} MHz {{mode}}{{tone}} (click to tune{{hotkey}})',
  'memories.strip.chip.tone': ' · tone {{hz}}',
  'memories.strip.manage.title':
    'Open Memories — manage channels, groups, nets, and CHIRP import/export',
  'memories.strip.manage.title.overflow': {
    one: 'Open Memories — {{count}} more favorite past the {{limit}} this strip shows. Re-rank them with ▲▼ under ★ Favorites.',
    other:
      'Open Memories — {{count}} more favorites past the {{limit}} this strip shows. Re-rank them with ▲▼ under ★ Favorites.',
  },

  // ── Program (the radio-programming workbench) ───────────────────────────────────────
  // ⚠️ The machines' own data is not here: callsigns, output frequencies, offsets, tones,
  // band chips, the DMR/D-STAR/Fusion badges, distances and octants, cities and states, and
  // the channel names as the radio will show them. Nor are four things the component keeps as
  // constants: the two directories' names, the ATTRIBUTION each requires (also written into
  // the exported CSV, so it cannot vary by locale), the example grid and frequency, and the
  // persisted project name. The rig models in the "Max name" list are tokens in
  // `features/radioprog.ts`.
  'program.title': 'Program',
  'program.sub':
    'Build channel lists for your radios — repeaters near a location, exported for CHIRP or tuned on your rig',

  // Where to search from.
  'program.origin.aria': 'Search origin',
  'program.origin.label': 'Near',
  // `{{grid}}` is the station locator with its separator, or nothing at all.
  'program.origin.station.label': 'My station {{grid}}',
  'program.origin.station.title': 'Your station grid from Settings',
  'program.origin.grid.label': 'Grid',
  'program.origin.grid.aria': 'Grid square',
  'program.origin.city.label': 'City',
  'program.origin.city.aria': 'City',
  // A HUMAN example, not a technical one: a locale should offer a place its operators know.
  'program.origin.city.placeholder': 'Gatlinburg, TN',
  'program.city.search': 'Search',
  'program.city.searching': 'Searching…',
  'program.city.noMatch': 'No places matched — try "City, State"',
  'program.city.matches.aria': 'Matching places',
  'program.recent.aria': 'Recent searches',
  'program.recent.label': 'Recent',
  'program.recent.chip.title': 'Reuse this search origin',

  // How far. Every radius is formatted by `units.ts` at the display edge.
  'program.radius.aria': 'Search radius',
  'program.radius.label': 'Radius',
  'program.radius.auto.label': 'Auto',
  'program.radius.auto.title': "Radius from the selected bands' realistic repeater reach",
  'program.radius.auto.hint': '= {{radius}} ({{bands}})',
  'program.radius.auto.allBands': 'all bands',

  // The fetch, and what the directory answered with.
  'program.fetch.label': '⟳ Fetch repeaters',
  'program.fetch.busy': '⟳ Fetching…',
  'program.fetch.title': 'Fetch repeaters within {{radius}}',
  'program.fetch.title.noOrigin': 'Pick a valid origin first (grid or city)',
  'program.fetch.retry': 'Retry',
  'program.stamp.title': 'Directory data age (cached per source, weekly)',
  'program.stamp.stale': ' · stale (fetch failed, cached data shown)',
  'program.age.mins': '{{mins}}m ago',
  'program.age.hours': '{{hours}}h ago',
  'program.age.days': '{{days}}d ago',
  // `{{source}}` is the directory's own name and `{{band}}` a band name — both tokens.
  'program.coverageGap':
    '{{source}} lists no <b>{{band}}</b> repeaters here, which is unusual for an area that has any — its rural coverage is patchy, so this list is probably missing machines. Adding a RepeaterBook API token in <b>Settings ▸ Integrations</b> fills the gap.',

  // Narrowing the results. The band chips and the FM chip are mode/band names, not prose.
  'program.filters.aria': 'Result filters',
  'program.filters.allBands': 'All',
  'program.filters.fm.title': 'FM repeaters only — what v1 programs',
  'program.filters.digital.label': '+Digital',
  'program.filters.digital.title':
    'Also list DMR / D-STAR / Fusion machines (badged; programming them comes later)',
  'program.filters.onAir.label': 'On-air only',
  'program.filters.onAir.title': 'Hide machines the directory marks off-air',
  'program.filters.search.placeholder': 'Filter call / city…',
  'program.filters.search.aria': 'Filter results',
  'program.count': '{{shown}} of {{total}} shown · nearest first',
  'program.addAll.label': '＋ Add all shown',
  'program.addAll.confirm.title': 'Add {{count}} channels?',
  'program.addAll.confirm.ok': 'Add channels',

  // The results list, and what it says when it has nothing. Two whole sentences for the
  // empty case: where the mode word sits belongs to the translator.
  'program.results.aria': 'Repeaters',
  'program.results.prompt':
    'Pick a location and press <b>Fetch repeaters</b> — results land here; ADD the machines you want on your radio.',
  'program.results.none': 'No repeaters within {{radius}}.',
  'program.results.none.fm': 'No FM repeaters within {{radius}}.',
  'program.results.tryWider': 'Try {{radius}}',
  'program.results.showDigital': 'Show digital',
  'program.row.offAir': 'OFF-AIR',
  'program.row.star.title':
    'Star this repeater — saves it to Memories and the cockpit MEM strip for one-click tuning',
  'program.row.unstar.title':
    'Unstar — keeps the channel in Memories, drops it off the cockpit strip',
  // Tune is a RETUNE of the CAT rig — frequency, shift, offset and tone. It never transmits.
  'program.row.tune.label': 'Tune',
  'program.row.tune.title': 'Tune your CAT rig to this repeater now (FM + shift + offset + tone)',
  'program.row.add.label': '＋ Add',
  'program.row.added.label': '✓ Added',
  'program.row.add.title': 'Add to the channel list',
  'program.row.remove.title': 'Remove from the channel list',
  'program.row.add.digital.title':
    'Digital repeater — programming DMR/D-STAR/Fusion comes in a later version',

  // The channel list being built — the artifact this section exists to produce.
  'program.builder.title': 'Channel list',
  'program.builder.nameCap.label': 'Max name',
  'program.builder.nameCap.title':
    "Your radio's channel-name length — auto names re-derive to fit (hand-edited names are kept)",
  'program.builder.startAt.label': 'Start at',
  'program.builder.startAt.title': 'First memory slot number (keep your existing channels)',
  'program.builder.empty':
    'No channels yet — fetch repeaters on the left and ADD the ones you want on your radio.',
  'program.chan.name.aria': 'Channel {{n}} name',
  'program.chan.dup.title': 'Duplicate name — the radio will show two identical channels',
  // `{{cap}}` is a character count and `{{name}}` the name as the radio will display it.
  'program.chan.over.title': 'Longer than {{cap}} — exports as "{{name}}"',
  'program.chan.moveUp.aria': 'Move up',
  'program.chan.moveDown.aria': 'Move down',
  'program.chan.remove.aria': 'Remove',

  // Getting the list out — by hand, by CSV, or into Nexus's own memory bank.
  'program.deliver.byHand.label': 'Add by hand…',
  'program.deliver.byHand.title':
    "Type in a repeater or simplex channel the directory doesn't have (or has wrong)",
  'program.deliver.import.label': 'Import CHIRP CSV…',
  'program.deliver.import.title':
    'Import a CHIRP CSV — the same format Export for CHIRP writes, and what CHIRP itself saves',
  'program.deliver.exportChirp.label': 'Export for CHIRP…',
  'program.deliver.exportChirp.title':
    'Save a CHIRP-ready CSV — CHIRP (free) flashes nearly every radio from it',
  'program.deliver.exportCsv.label': 'Export CSV',
  'program.deliver.exportCsv.title': 'Plain CSV — spreadsheets, Anytone CPS, RT Systems',
  'program.deliver.saveBank.label': 'Save to Memory Bank',
  'program.deliver.saveBank.title':
    "Save these channels into Nexus's own memory bank (the Phone cockpit's MEMORY recall list) — recall retunes the rig with shift + tone",
  'program.deliver.clear.label': 'Clear',
  'program.clear.confirm.title': 'Clear the whole channel list?',
  'program.clear.confirm.ok': 'Clear list',

  // Entering a machine the directory has wrong or missing. `{{example}}` is a dial frequency
  // the component supplies; the +/− and the tone unit are the radio's vocabulary.
  'program.manual.freq': 'Frequency (MHz), e.g. {{example}}',
  'program.manual.name': 'Name (blank = frequency)',
  'program.manual.offset': 'Offset: + / - / blank for simplex',
  'program.manual.tone': 'CTCSS tone Hz (blank = none)',
  // ⚠️ `Frequency` and `Mode` here are CHIRP's own CSV column names — wire identifiers.
  'program.import.notChirp': 'Not a CHIRP CSV — need a header row with Frequency and Mode',
  'program.import.done': 'Imported {{count}} channels from CHIRP CSV',
  'program.export.noFm': 'No FM channels in the list — digital channels export in a later version',
  'program.export.saved': 'Saved {{path}}',
  // ⚠️ `CHIRP ▸ File ▸ Import` is another program's menu path — keep it as CHIRP prints it.
  'program.export.saved.chirp':
    'Saved {{path}} — open CHIRP ▸ File ▸ Import, then upload to your radio',

  // What the workbench says after it acts. The shift is a sign and a number of MHz.
  'program.tune.done': 'Tuned {{freq}} {{mode}} — {{shift}}{{tone}}',
  'program.tune.simplex': 'simplex',
  'program.tune.tone': ' · tone {{hz}}',
  'program.star.unstarred': '{{name}} unstarred — still in Memories',
  'program.star.starred': '{{name}} starred — already in Memories',
  'program.star.saved': '{{name}} ★ — on the cockpit MEM strip and in Memories',
  // The already-there clause is INTERPOLATED into the sentence, not glued after it: it
  // carries the second count, which one message cannot pluralise beside the first.
  'program.saveBank.done': {
    one: '{{count}} channel saved to Memories{{dupes}} — star ★ the ones you want on the cockpit MEM strip',
    other:
      '{{count}} channels saved to Memories{{dupes}} — star ★ the ones you want on the cockpit MEM strip',
  },
  'program.saveBank.dupes': ' ({{count}} already there)',
  'program.saveBank.allDupes': 'All of these are already in Memories',
  'program.saveBank.noFm': 'No FM channels to save',

  // The how-to before the first CHIRP export. The menu paths are CHIRP's own.
  'program.chirp.title': 'Flash with CHIRP',
  'program.chirp.description':
    'Nexus builds the list; CHIRP drives the cable. One list, every radio you own.',
  'program.chirp.step.save': 'Nexus saves a CHIRP-ready CSV to your Downloads.',
  'program.chirp.step.import':
    'Open CHIRP (free, ~1,000 radios) → <b>File ▸ Import</b> and pick the file.',
  'program.chirp.step.upload':
    'Connect your programming cable → <b>Radio ▸ Upload To Radio</b>.',
  'program.chirp.link': 'Get CHIRP ↗',
  'program.chirp.dontShow': "Don't show this again — just save the file",
  'program.chirp.save': 'Save the CSV',

  // ── The mid-QSO recall card ─────────────────────────────────────────────────────────
  // ⚠️ The callsign, the grid squares, the band and mode of each prior contact, the RST pair,
  // the operator's own comments and the distance/bearing line are all data and stay in the
  // component — as do the month abbreviations, which are date formatting.
  'recall.qrz.title': 'Open {{call}} on QRZ (browser)',
  'recall.qrz.error': 'Could not open {{call}} on QRZ',
  // Names the button the operator can see: the log strip's callbook button reads "Lookup".
  'recall.where.empty': 'Tab or press Lookup for name / QTH',
  // Two whole statements, and the conjunction between the two squares is inside a message of
  // its own — a language that pairs them differently can only do it if it can translate the
  // joining word.
  'recall.geo.title': 'Great-circle distance · true bearing from your QTH',
  'recall.geo.title.approx':
    'Great-circle distance · true bearing from your QTH — approximate: computed from the center of {{squares}}',
  'recall.geo.approx.mine':
    'your {{grid}} square (set a 6-character grid in Settings to sharpen it)',
  'recall.geo.approx.theirs': 'their {{grid}} square',
  'recall.geo.approx.both': '{{mine}} and {{theirs}}',
  'recall.dupe.label': 'Dupe {{band}}',
  'recall.dupe.title':
    'Already worked on {{band}} — logging now would be a dupe. Counts any mode on the band unless Settings’ “match mode too” is on.',
  'recall.confirmed.title': '{{confirmed}} of {{count}} prior QSOs confirmed',
  // ⚠️ DXCC is the award programme's own name — a token inside the sentence.
  'recall.need.entity': 'New DXCC!',
  'recall.need.band': 'New band-slot',
  'recall.need.mode': 'New mode-slot',
  'recall.need.title': 'Worth working — a new one for your log',
  'recall.note.title': 'Your most recent note on this station',
  'recall.log.head': 'Previous contacts',
  'recall.log.aria': 'Previous contacts with {{call}}',
  'recall.log.row.title': 'Show {{call}} in the Logbook',

  // ── The band controls (the licensed-band picker, the frequency control) ─────────────
  // ⚠️ Band names are both the LABEL and the VALUE of the pickers' options — `pickBand` sends
  // the value to the engine — so neither is here. Nor are the channel presets, their dial
  // frequencies, the HF/VHF/UHF group names or the USB/FM mode buttons. The 🔒 chip is a
  // readout of `txAllowed`: it reports that the engine is already blocking transmit, and is
  // not a transmit control.
  'bandPicker.select.title':
    'Band — your last frequency on this band in this mode this session, else the start of your licensed segment',
  'bandPicker.txLock.label': '🔒 TX locked',
  'bandPicker.txLock.splitTitle':
    'TX locked — your split transmit frequency, {{tx}} MHz, is outside your license privileges. Receiving on {{rx}} MHz is fine.',
  'bandPicker.txLock.title':
    'This frequency/mode is outside your license privileges — transmit is blocked. Pick a band above, or change your license class in Settings.',

  'freq.control.aria': 'Frequency control',
  'freq.channel.label': 'Band / Channel',
  'freq.channel.aria': 'Band channel preset',
  'freq.channel.title': 'Pick a band-plan channel',
  'freq.channel.presets': '— Presets —',
  // A band this licence class cannot transmit on is still LISTED and still tunable — no
  // licence restricts receiving. The suffix says what you will and will not be able to do.
  // ---- The TV page's chrome (src/tv/ConnectTv.tsx — the LAN-served full Connect view).
  'tv.readonly': 'read-only',
  'tv.noLink': 'no link to Nexus',
  'tv.stale': 'data {{min}} min old',
  'tv.waiting': 'Waiting for the first propagation picture from Nexus…',
  'freq.channel.rxOnly': 'receive only',
  'freq.channel.rxOnly.title':
    'Your licence class has no transmit privileges on this band. You can tune here and listen; transmitting will be refused.',
  'freq.channel.custom': '{{band}} (custom)',
  'freq.dial.label': 'Dial (MHz)',
  'freq.band.title': 'Current band',
  'freq.mode.aria': 'Phone mode',

  // The shared dial readout (`FrequencyReadout`), used by every cockpit header, the top bar,
  // Settings and the memory rows.
  // ⚠️ The number itself, the `MHz` unit and the per-digit step labels (`100 Hz` … `10 MHz`) are
  // measurements built in the component and never here. `megahertz` IS spelled out below: it is
  // what a screen reader must say, and `MHz` is read as three letters.
  'freq.readout.dial.label': 'Dial frequency (MHz)',
  'freq.readout.title.editable': 'Click to enter a frequency (MHz)',
  'freq.readout.title.digitTune':
    'Scroll a digit to tune it (100 Hz … 10 MHz) · ←/→ pick a digit, ↑/↓ spin it · click to type a frequency (MHz)',
  'freq.readout.announce.dial': '{{dial}} megahertz',
  'freq.readout.announce.digit': '{{step}} digit',

  // ── The band-activity strip (the cockpits' "Band activity" pane) ────────────────────
  // ⚠️ Band and mode names, the scale's edge frequencies, the dial reading and every value in
  // a spot's tooltip are tokens and stay in the component. `{{mode}}` below is CW or SSB.
  // Deliberately NOT shared with the band map's entries, which read the same in English
  // today: the two surfaces word their own tooltips, and a shared key could not be split
  // later without orphaning both translations.
  'bandStrip.offPlan': '{{band}} — off the band plan',
  'bandStrip.empty.noPlan': 'no band-plan data for {{band}}',
  'bandStrip.empty.thisFrequency': 'this frequency',
  'bandStrip.count': {
    one: '{{count}} {{mode}} spot · {{band}}',
    other: '{{count}} {{mode}} spots · {{band}}',
  },
  'bandStrip.empty.none': 'no {{mode}} spots on {{band}} yet',
  'bandStrip.legend.label': 'Legend',
  'bandStrip.legend.title': 'Show/hide the colour + type key',
  'bandStrip.popout.label': '⧉ Band map',
  'bandStrip.popout.title': 'Open the vertical band map in its own window',
  'bandStrip.track.title': '{{band}}: {{lo}}–{{hi}} MHz',
  'bandStrip.track.title.tunable': '{{band}}: {{lo}}–{{hi}} MHz — scroll to tune',
  'bandStrip.shade.title': 'Your licensed phone segment on this band',
  // `{{detail}}` is the spot line — call, frequency, age, badges, spotter and comment.
  'bandStrip.spot.title': '{{detail}} — click to work',
  'bandStrip.age.secs': '{{secs}}s ago',
  'bandStrip.age.mins': '{{mins}}m ago',
  'bandStrip.age.hours': '{{hours}}h ago',
  // The "you are here" marker: two whole tooltips, because the blocked one is a different
  // statement rather than a tail. It is a readout, not a transmit control.
  'bandStrip.dial.title': 'You: {{freq}} MHz',
  'bandStrip.dial.title.blocked':
    'You: {{freq}} MHz — transmit blocked (outside your privileges)',

  // ── Multi-radio (the launch picker and the switcher pills) ──────────────────────────
  // ⚠️ Each radio's profile name, its band and its dial frequency are interpolated as tokens.
  // The peg lock pins which radio a band change may move; it is not a transmit control.
  'radios.picker.aria': 'Choose radio',
  'radios.picker.title': 'Which radio?',
  'radios.picker.sub':
    'You have two radios running at once. Pick the radio this window will operate — you can open a second window for the other. They share one logbook.',
  'radios.picker.inUse.title': '{{name}} is already open in another window',
  'radios.picker.inUse.tag': 'in use',
  'radios.picker.choose.title': 'Operate {{name}}',
  'radios.picker.single': 'Use one radio (follow bands on a single window)',

  'radios.switcher.aria': 'Active radio',
  'radios.switcher.active.title': '{{name}} — active radio ({{band}} · {{freq}} MHz)',
  'radios.switcher.switch.title': 'Switch to {{name}} (last on {{band}} · {{freq}} MHz)',
  // ⚠️ CAT is the rig-control protocol's own name, here and in the two entries below it.
  'radios.switcher.catDead.title':
    'Switch to {{name}} — ⚠ CAT not responding (check its rig, cable, and COM port)',
  'radios.switcher.catDead.aria': 'CAT not responding',
  'radios.switcher.catDead.band': 'no CAT',
  'radios.peg.on.label': '🔒 Pegged',
  'radios.peg.off.label': '🔓 Peg',
  'radios.peg.on.title':
    'Peg-lock ON — the active radio stays put; selecting a band won’t auto-switch radios. Click to unlock.',
  'radios.peg.off.title':
    'Peg-lock OFF — selecting a band may auto-switch to the radio that covers it. Click to pin the active radio.',

  // ── Settings ▸ Radio — the pre-save rig checks (`rigFormChecks.ts`) ─────────────────
  // ⚠️ `{{port}}` is the device name exactly as the OS enumerated it (`COM5`,
  // `/dev/cu.usbserial-A`), and `/dev/cu.…`, CAT, PTT and None/VOX are the settings' own
  // vocabulary — every one of them names a thing the operator has to pick by that name.
  'settings.radio.check.noPort': 'No serial port chosen — a rig model is set, so CAT needs a port.',
  'settings.radio.check.portMissing':
    '{{port}} is not connected right now — check the rig is powered on, or pick another port.',
  'settings.radio.check.dialIn':
    '{{port}} is a dial-in device and will hang waiting for carrier. Use the matching /dev/cu.… port instead.',
  'settings.radio.check.catNoModel':
    'PTT is set to CAT but the rig model is None/VOX — pick your rig model, or choose a different PTT method.',

  // ── Settings ▸ the panel shell (chrome, tab rail, Save) ─────────────────────────────
  // The frame every Settings tab renders inside. `{{id}}` is the build stamp — an identifier,
  // never a formatted number — and "Nexus", "Test CAT" and "Save" name things the operator
  // reads on screen, so they stay exactly as they are inside these sentences.
  'settings.panel.title': 'Settings',
  'settings.panel.subtitle': 'operator, rig & network',
  'settings.panel.loading': 'Loading settings…',
  'settings.panel.build': 'build {{id}}',
  'settings.panel.build.title':
    "This install's build stamp — confirm a fresh install actually took",
  'settings.panel.update.label': 'Check for updates',
  'settings.panel.update.title': 'Check for a newer Nexus release',
  'settings.panel.tabs.aria': 'Settings sections',
  'settings.panel.save': 'Save',
  'settings.panel.saving': 'Saving…',
  'settings.panel.saved': 'Saved',

  // The tab rail. ⚠️ Phone, CW and Digital are MODE NAMES — invariant tokens — so they are
  // deliberately ABSENT here and render from the panel's own literal list in every language;
  // `SETTINGS_TABS` in `components/SettingsPanel.tsx` says so at the array.
  'settings.tabs.station': 'Station',
  'settings.tabs.radio': 'Radio',
  'settings.tabs.spots': 'Spots & Alerts',
  'settings.tabs.logging': 'Logging & Connectors',
  'settings.tabs.contesting': 'Contesting',
  'settings.tabs.appearance': 'Appearance',
  'settings.tabs.configurations': 'Config',

  // The Config tab. `<em>` marks the OLD location in the note — an operator who knew where these
  // used to be needs to be told they moved, once, rather than left to hunt.
  'settings.configurations.legend': 'Backup & reset',
  'settings.configurations.note':
    'Your whole setup in one file — for a new computer, before a rebuild, or to get back to a known-good state. These used to live under <em>Radio → Transmit limits & sharing</em>, where nobody found them.',

  // Reset sits beside Backup in the Config tab because its confirm points AT the backup: the
  // machinery that makes this reversible is one control away, so the wording names it rather
  // than just warning. Keyed under `configurations`, not `transmit` — a translator reads the key
  // for context, and this is not a transmit setting.
  'settings.configurations.reset.label': 'Start over',
  'settings.configurations.reset.action': 'Reset all settings…',
  'settings.configurations.reset.title': 'Erase all settings and return to factory defaults',
  'settings.configurations.reset.hint':
    'Erases your radios, audio devices, callsign and preferences. Your <b>logbook is not touched</b>, and stored passwords stay in your keychain (clear those individually under Logging & Connectors). Back up first — this cannot be undone.',
  // What Save says when the form is refused. The rig checks' own wording lives above, in
  // `settings.radio.check.*` — this is only the panel's fallback when one carries no message.
  'settings.save.callsignFirst': 'Enter your callsign on the Station tab before saving.',
  'settings.save.fdPositionName': 'Name this position on the Contesting tab before hosting or joining a club event — the club band board shows this name.',
  'settings.save.checkRadio': 'Check the radio settings.',
  'settings.save.failed': 'Could not save settings.',

  // ── Settings ▸ the panel's own toasts, confirms and live applies ────────────────────
  // Raised by the panel's handlers rather than by any one section, so they are grouped here
  // by the act. ⚠️ Every interpolated value is a TOKEN and stays one: `{{file}}` a file name,
  // `{{device}}` / `{{port}}` the OS's own device and port names, `{{ip}}` an address,
  // `{{name}}` a radio or profile name the operator typed, `{{id}}` a radio number. LoTW,
  // eQSL, QRZ, HamQTH, ClubLog, HRDLog.net, RepeaterBook, hearham.com, Cloudlog, SmartSDR,
  // DAX, CAT and CI-V are the names of the services and protocols themselves.
  'settings.backup.restore.confirm.title': 'Replace your current setup with {{file}}?',
  'settings.backup.restore.confirm.body':
    'Your radios, preferences, memory channels, watchlist and chase sets will be replaced. Your contact log is not affected. This cannot be undone.',
  'settings.backup.restore.confirm.action': 'Restore',
  'settings.backup.restore.done': 'Settings restored — check your radio and Test CAT',
  'settings.backup.restore.failed': 'Restore failed',
  // Reset's dialog says what SURVIVES as well as what goes: "reset" reads as total, and the two
  // things operators fear for — the log and their stored passwords — are exactly the two this
  // does not touch. Saying so in the dialog is what makes the confirm answerable.
  'settings.backup.reset.confirm.title': 'Reset all settings to factory defaults?',
  'settings.backup.reset.confirm.body':
    'Your radios, audio devices, callsign and preferences will be erased. Your contact log is not affected, and stored passwords stay in your keychain. This cannot be undone — back up first if you have not.',
  'settings.backup.reset.confirm.action': 'Reset',
  'settings.backup.reset.done': 'Settings reset to defaults',
  'settings.backup.reset.failed': 'Could not reset the configuration',

  'settings.audio.rxGain.failed': 'Could not apply RX gain',
  'settings.audio.txPower.failed': 'Could not set TX power',
  // What one sound-card option READS when the saved device is not among the ones we offer.
  // The device NAME is the OS's, and it is never translated — only the note after it is.
  'settings.audio.device.notInList': '{{device}} — saved, not in the list',

  'settings.satellites.vfoMap.failed': 'Could not confirm the VFO mapping',

  'settings.workingFreq.reset.confirm.title': 'Clear all working-frequency overrides?',
  'settings.workingFreq.reset.confirm.body': 'The stock WSJT-X frequency table is restored.',
  'settings.workingFreq.reset.confirm.action': 'Clear overrides',

  // The radio roster. `settings.radios.thisRadio` and `.unnamed` are the stand-ins a radio
  // with no name of its own is called by; they fill the `{{name}}` slot, so the sentence
  // around them stays one whole sentence.
  'settings.radios.thisRadio': 'this radio',
  'settings.radios.unnamed': 'radio {{id}}',
  'settings.radios.add.failed': 'Could not add a radio',
  'settings.radios.remove.confirm.title': 'Remove {{name}}?',
  'settings.radios.remove.confirm.body':
    "This deletes its CAT/audio config, its rigctld port and its band coverage. Your contact log is not affected. This can't be undone.",
  'settings.radios.remove.confirm.action': 'Remove radio',
  'settings.radios.remove.failed': 'Could not remove the radio',
  'settings.radios.rename.failed': 'Could not rename the radio',
  'settings.radios.bands.failed': 'Could not set band coverage',
  'settings.radios.default.failed': 'Could not set the default radio',
  'settings.radios.switch.failed': 'Could not switch radios',
  // Two confirms, one wording for the button. Kept as separate entries because they guard
  // two different acts — editing another radio's config, and moving the station onto it.
  'settings.radios.edit.confirm.title':
    'Discard unsaved changes to the radio you were editing?',
  'settings.radios.edit.confirm.body': 'The edits you have not saved for that radio are lost.',
  'settings.radios.edit.confirm.action': 'Discard and switch',
  'settings.radios.makeActive.confirm.title':
    'Discard unsaved changes and switch the operating radio?',
  'settings.radios.makeActive.confirm.body':
    'The carrier is dropped before the swap. Unsaved edits to the radio you were editing are lost.',
  'settings.radios.makeActive.confirm.action': 'Discard and switch',
  'settings.routing.rules.failed': 'Could not save the routing rules',

  // Radio detection. `settings.detect.unknownRadio` is the word an unidentified device is
  // called by in the three "Applied …" reports — it fills `{{device}}`, never a fragment.
  'settings.detect.usb.failed': 'USB radio detection failed',
  'settings.detect.flex.scanFailed': 'Flex LAN scan: {{error}}',
  'settings.detect.none':
    'No radios found — USB: plug in + power on; Flex: must be on this network.',
  'settings.detect.unknownRadio': 'radio',
  'settings.detect.applied.interface':
    'Applied {{device}} on {{port}} — now pick your Rig Model, then Save',
  'settings.detect.applied.identifying':
    'Applied {{device}} on {{port}} — identifying via Auto-test…',
  'settings.detect.applied.review': 'Applied {{device}} on {{port}} — review + Save settings',
  // ⚠️ The port numbers (5002, 60001) are what the operator types into SmartSDR CAT and into
  // Network Address. They are dial settings, not prose, and stay exactly as written.
  'settings.detect.flex.applied':
    'Applied {{radio}} at {{ip}} — SmartSDR CAT (slice A, port 5002); native panadapter/DAX ready to enable below. Review + Save, then Test CAT. Second slice? Use port 60001.',
  'settings.detect.flex.found':
    'Found {{radio}} at {{ip}} — model and radio IP applied. SmartSDR CAT is Windows-only, so set Network Address yourself: the address of a Windows PC on this network running SmartSDR CAT (slice A is its port 5002). Native panadapter/DAX below talk to the radio directly and need no such PC.',

  'settings.cat.callsignRequired': 'Callsign is required.',
  'settings.cat.savedNotTested':
    "Saved to {{name}}. CAT can only be tested on the radio you're operating — make {{name}} active to test it.",
  'settings.cat.test.failed': 'Could not run the CAT test.',
  'settings.cat.autoTest.failed': 'Could not run the port auto-test.',

  'settings.profiles.saved': 'Profile "{{name}}" saved',
  'settings.profiles.loaded': 'Loaded profile "{{name}}"',

  // The credential stores and the two-way syncs. The sync reports are ONE sentence each with
  // the optional clauses interpolated whole, never glued from fragments; `{{count}}` picks the
  // plural form and `Intl.PluralRules` owns which form that is.
  'settings.connections.test.testing': 'testing…',
  'settings.connections.sync.unmatched': ' · {{count}} unmatched',
  'settings.connections.lotw.password.saveFailed': 'Could not save the LoTW password',
  'settings.connections.lotw.password.saved': 'LoTW password saved to the system keychain',
  'settings.connections.lotw.password.clearFailed': 'Could not clear the LoTW password',
  'settings.connections.lotw.password.cleared': 'LoTW password cleared from the keychain',
  'settings.connections.lotw.sync.failed': 'LoTW sync failed',
  'settings.connections.lotw.sync.done':
    'LoTW: {{confirmed}} newly confirmed, {{credited}} credited{{promoted}}{{unmatched}}',
  'settings.connections.lotw.sync.promoted': {
    one: ' · {{count}} upload now on file',
    other: ' · {{count}} uploads now on file',
  },
  'settings.connections.eqsl.password.saveFailed': 'Could not save the eQSL password',
  'settings.connections.eqsl.password.saved': 'eQSL password saved — auto-upload to eQSL is ON',
  'settings.connections.eqsl.password.clearFailed': 'Could not clear the eQSL password',
  'settings.connections.eqsl.password.cleared':
    'eQSL password cleared — auto-upload to eQSL is off',
  'settings.connections.eqsl.sync.failed': 'eQSL sync failed',
  // ⚠️ DXCC and WAS are award programme names — a translator leaves both exactly as they are.
  'settings.connections.eqsl.sync.done':
    'eQSL: {{confirmed}} newly confirmed (not DXCC/WAS credit){{unmatched}}',
  'settings.connections.qrz.password.saveFailed': 'Could not save the QRZ password',
  'settings.connections.qrz.password.saved': 'QRZ password saved to the system keychain',
  'settings.connections.qrz.password.clearFailed': 'Could not clear the QRZ password',
  'settings.connections.qrz.password.cleared': 'QRZ password cleared from the keychain',
  'settings.connections.qrz.key.saveFailed': 'Could not save the QRZ Logbook key',
  'settings.connections.qrz.key.saved': 'QRZ Logbook key saved — auto-upload to QRZ is ON',
  'settings.connections.qrz.key.clearFailed': 'Could not clear the QRZ Logbook key',
  'settings.connections.qrz.key.cleared': 'QRZ Logbook key cleared — auto-upload to QRZ is off',
  'settings.connections.qrz.sync.failed': 'QRZ sync failed',
  // ⚠️ QSO is the hobby's own word for a contact — it does not inflect into another language.
  'settings.connections.qrz.sync.done': {
    one: 'QRZ: {{count}} new QSO, {{confirmed}} newly confirmed{{unmatched}}',
    other: 'QRZ: {{count}} new QSOs, {{confirmed}} newly confirmed{{unmatched}}',
  },
  'settings.connections.hamqth.password.saveFailed': 'Could not save the HamQTH password',
  'settings.connections.hamqth.password.saved': 'HamQTH password saved to the system keychain',
  'settings.connections.hamqth.password.clearFailed': 'Could not clear the HamQTH password',
  'settings.connections.hamqth.password.cleared': 'HamQTH password cleared from the keychain',
  'settings.connections.clublog.password.saveFailed': 'Could not save the ClubLog password',
  'settings.connections.clublog.password.saved':
    'ClubLog app-password saved — auto-upload to ClubLog is ON',
  'settings.connections.clublog.password.clearFailed': 'Could not clear the ClubLog password',
  'settings.connections.clublog.password.cleared':
    'ClubLog password cleared — auto-upload to ClubLog is off',
  'settings.connections.hrdlog.code.saveFailed': 'Could not save the HRDLog.net upload code',
  'settings.connections.hrdlog.code.saved':
    'HRDLog.net code saved — auto-upload to HRDLog.net is ON',
  'settings.connections.hrdlog.code.clearFailed': 'Could not clear the HRDLog.net upload code',
  'settings.connections.hrdlog.code.cleared':
    'HRDLog.net code cleared — auto-upload to HRDLog.net is off',
  'settings.connections.repeaterbook.token.saveFailed': 'Could not save the RepeaterBook token',
  'settings.connections.repeaterbook.token.saved':
    'RepeaterBook token saved — the Program section now uses RepeaterBook',
  'settings.connections.repeaterbook.token.clearFailed':
    'Could not clear the RepeaterBook token',
  'settings.connections.repeaterbook.token.cleared':
    'RepeaterBook token cleared — the Program section falls back to hearham.com',
  'settings.connections.cloudlog.key.saveFailed': 'Could not save the Cloudlog API key',
  'settings.connections.cloudlog.key.saved': 'Cloudlog API key saved to the keychain',
  'settings.connections.cloudlog.key.clearFailed': 'Could not clear the Cloudlog API key',
  'settings.connections.cloudlog.key.cleared': 'Cloudlog API key cleared from the keychain',

  // ── Settings ▸ Radio ▸ Radios (the roster, band coverage, band+mode routing) ────────
  // ⚠️ EVERY INTERPOLATED VALUE HERE IS A TOKEN AND STAYS ONE: `{{name}}` is a radio's own
  // name, `{{rig}}` a Hamlib model name, `{{cat}}` a COM port / host:port / OmniRig slot,
  // `{{ip}}` an address, `{{audio}}` the OS's own sound-device name, `{{port}}` a TCP port,
  // `{{bands}}` a list of band names and `{{mode}}` a routing mode class. The band and mode
  // names written into the sentences (2 m, FT8, FM, APRS) are that same vocabulary and are
  // not translated either — they name what the operator selects. The FIVE ROUTING MODE
  // CLASSES are deliberately absent from this file: they are mode names, and the panel's
  // `ROUTE_MODES` must match the Rust `RouteMode::label`, so they stay in the code.
  'settings.radios.legend': 'Radios',
  // The two stand-ins a radio with no name of its own is called by inside a sentence; they
  // fill a `{{name}}` slot, so the sentence around them stays one whole sentence.
  'settings.radios.anotherRadio': 'another radio',
  'settings.radios.selectedRadio': 'the selected radio',
  'settings.radios.editingNote':
    "<b>Editing {{name}}</b> — not your operating radio. <b>Save</b> writes only this radio's CAT / audio config; your active radio and station-wide settings are untouched.",
  'settings.radios.name.placeholder': 'Radio name',
  'settings.radios.active.badge': 'Active',
  'settings.radios.active.title': 'Your operating radio.',
  'settings.radios.editing.badge': 'Editing',
  'settings.radios.editing.title': 'The Rig / CAT + Audio form below is editing this radio.',
  'settings.radios.edit.action': 'Edit',
  'settings.radios.edit.title':
    "Edit this radio's CAT / audio below — WITHOUT changing your operating radio (no swap, no dropped carrier).",
  'settings.radios.makeActive.action': 'Make active',
  'settings.radios.makeActive.title':
    'Make this your operating radio (swaps rigs; drops any carrier first).',
  'settings.radios.remove.action': 'Remove',
  'settings.radios.remove.title': 'Remove this radio from the roster',
  'settings.radios.remove.title.blocked':
    'This is your operating radio — make another radio active first, then remove this one.',
  // The roster card's summary line, and it is ONE entry because the words BETWEEN the tokens
  // are labels — "CAT", "audio", "CAT helper port" — that a language ordering them differently
  // cannot be served four fragments of. The Flex clause is optional and is interpolated whole.
  'settings.radios.card.meta':
    '{{rig}} · CAT {{cat}}{{flex}} · audio {{audio}} · CAT helper port {{port}}',
  'settings.radios.card.meta.flex': ' · Flex radio {{ip}}',
  'settings.radios.card.omnirig': 'Set up in OmniRig',
  'settings.radios.card.noModel': 'No rig model set',
  'settings.radios.card.noAddress': 'no address',
  'settings.radios.card.noPort': 'no COM port',
  'settings.radios.card.audioDefault': 'default',
  'settings.radios.bands.hint': 'Covers bands (for auto band-routing; none = covers all):',
  'settings.radios.add.action': '+ Add radio',
  'settings.radios.hint.multi':
    "The Rig / CAT + Audio settings below edit “{{name}}”. Each radio has its OWN CAT + audio — click “Edit” on any radio to configure it WITHOUT changing the one you're operating on; “Make active” swaps your operating radio.",
  'settings.radios.hint.single':
    'Run two rigs at once — e.g. an HF radio plus a VHF/UHF radio on a different antenna? Add a second radio; you can then Edit either one without interrupting the one you are operating on. Newcomers can ignore this.',
  // The accessible name is its own entry, not a slice of the hint below it: a hint is a whole
  // sentence and a control's name is a name, and the two are free to differ in a language.
  'settings.radios.simultaneous.aria': 'Run both radios at the same time',
  'settings.radios.simultaneous.hint':
    '<b>Run both radios at the same time</b> — launch Nexus and it asks which radio this window drives; open a second window for the other. Both share one logbook. Leave off if you only ever use one radio at a time (you can still switch between them from the top bar).',

  // Band + mode routing. `{{n}}` is a rule's position in the table, counted from 1.
  'settings.routing.intro':
    '<b>Route by band AND mode</b> — band coverage above sends a whole band to one radio. Add rules here when TWO radios share a band and the MODE decides which one: 2 m FT8 to the digital rig, 2 m FM and APRS to the FM rig. Rules are checked top to bottom and the FIRST match wins; anything no rule matches falls back to band coverage, then to the default radio.',
  'settings.routing.empty': "No rules — routing is by band only (today's behavior).",
  'settings.routing.rule.mode.aria': 'Rule {{n}} mode',
  'settings.routing.rule.mode.any': 'Any mode',
  // "Satellite" is a CONTEXT designation, not a sixth mode class (the panel says so at the
  // dropdown), so unlike the five mode classes beside it, it is prose and moves.
  'settings.routing.rule.satellite': 'Satellite',
  'settings.routing.rule.satellite.title':
    'Satellite passes only: picking a transponder checks Satellite rules before the mode rules, so the FM & APRS rule can keep terrestrial packet while satellites go to the sat rig. Terrestrial tunes never match this rule.',
  'settings.routing.rule.radio.aria': 'Rule {{n}} radio',
  'settings.routing.rule.up.title': 'Check this rule earlier (first match wins)',
  'settings.routing.rule.up.aria': 'Move rule {{n}} up',
  'settings.routing.rule.down.title': 'Check this rule later',
  'settings.routing.rule.down.aria': 'Move rule {{n}} down',
  'settings.routing.rule.remove.aria': 'Remove rule {{n}}',
  // One line, three holes — never three glued fragments. `.anyMode` is the mid-sentence form
  // of `.rule.mode.any` and is a separate entry on purpose: lower-casing a translated noun is
  // wrong wherever nouns capitalise.
  'settings.routing.rule.summary': '{{bands}} · {{mode}} → {{radio}}',
  'settings.routing.rule.summary.anyBand': 'Any band',
  'settings.routing.rule.summary.anyMode': 'any mode',
  'settings.routing.rule.summary.radio': 'Radio {{id}}',
  'settings.routing.add.action': '+ Add routing rule',
  'settings.routing.default.label': 'Everything else',
  'settings.routing.default.aria': 'Default radio',
  'settings.routing.default.stay': 'Stay on the current radio',
  'settings.routing.test.label': 'Test a band + mode',
  'settings.routing.test.band.aria': 'Test band',
  'settings.routing.test.mode.aria': 'Test mode',
  'settings.routing.test.action': 'Where would this go?',
  'settings.routing.test.result': '{{band}} {{mode}} → <b>{{radio}}</b>',

  // ── Settings ▸ Radio ▸ Profiles ─────────────────────────────────────────────────────
  // A whole-station config saved under a name the operator types. The example name is human
  // prose — a locale should offer one its operators recognise — while VHF inside it is the
  // band class and stays as written.
  'settings.profiles.legend': 'Profiles',
  'settings.profiles.list.label': 'Saved profiles',
  'settings.profiles.list.none': '— Select a profile —',
  'settings.profiles.list.hint': 'Switch a whole rig / antenna / CAT / band setup in one move.',
  'settings.profiles.load.action': 'Load',
  'settings.profiles.load.title':
    'Apply this profile — merged onto your current settings. Your callsign, license class, radio roster and sync history never come from a profile, and anything the profile predates keeps its current value.',
  'settings.profiles.delete.action': 'Delete',
  'settings.profiles.save.label': 'Save current as',
  'settings.profiles.save.placeholder': 'e.g. Portable VHF',
  'settings.profiles.save.action': 'Save',
  'settings.profiles.save.hint': 'Snapshots the current settings under a name.',

  // ── Settings ▸ Radio ▸ Rig & CAT ────────────────────────────────────────────────────
  // THE DENSEST INVARIANT-TOKEN SURFACE IN SETTINGS, so read the rule before adding an entry
  // here. Everything an operator TYPES OR PICKS on this screen is a token and is NOT in this
  // file: COM / `/dev/cu.…` device names, baud rates, Hamlib model numbers and model names,
  // host:port addresses, IPs, TCP port numbers, CI-V addresses and OmniRig's own RIG 1 / RIG 2
  // slot names. So is the `value` of every <select> — only the LABEL moved. What the sentences
  // below DO carry, verbatim and untranslated, are the names of things the operator has to go
  // and find somewhere else: CAT, PTT, RTS, DTR, VOX, rigctld, Hamlib, OmniRig, SmartSDR CAT,
  // DAX, VITA-49, Thetis, PowerSDR, piHPSDR, Digirig, SO2R, CI-V, DATA-U / USB-D / PKTUSB,
  // SERIAL-B, CP210x, "Enhanced", the Icom menu path and the rig and interface model names.
  'settings.rigControl.legend': 'Rig & CAT',

  // PTT. A configuration <select>, not a transmit control: the labels are prose, the stored
  // `value` is the token the radio loop reads, and nothing about keying changed.
  'settings.rigControl.ptt.label': 'PTT Method',
  'settings.rigControl.ptt.hint': 'How transmit is keyed.',
  'settings.rigControl.ptt.cat': 'CAT (via rigctld)',
  'settings.rigControl.ptt.rts': 'Serial RTS',
  'settings.rigControl.ptt.dtr': 'Serial DTR',
  'settings.rigControl.ptt.vox': 'VOX (no keying)',
  'settings.rigControl.pttPort.label': 'PTT Serial Port',
  // `{{example}}` is the platform's own device-name example, supplied by the panel as an
  // invariant token (RIG_EXAMPLES) — a "localised" COM16 names no port on any machine.
  'settings.rigControl.pttPort.placeholder': 'e.g. {{example}} — blank = use the CAT port',
  'settings.rigControl.pttPort.hint':
    'COM port your RTS/DTR keying line is on — e.g. an SO2R controller (u2R/MK2R) that routes PTT on its own port, separate from CAT. Leave blank if keying shares the CAT port, which is how a single-cable interface like a Digirig Mobile is wired; CAT keeps working either way. <b>Per radio</b>: each rig on an SO2R box has its own keying port, and this one follows the radio you switch to.',
  'settings.rigControl.catRts.label': 'Interface keys RTS on the CAT port',
  'settings.rigControl.catRts.hint':
    'Tick this if your interface keys the radio from the CAT port’s RTS line — a Digirig Mobile and most other one-cable interfaces are wired this way. Nexus then holds RTS down, instead of leaving it up where it puts some rigs into transmit the moment the port opens. <b>If your radio transmits as soon as Nexus starts, this is the setting.</b> Leave it off if a plain serial cable goes straight to the radio: the radio may be using that line for flow control, and taking it away can cost you CAT.',

  // Detection. `{{radio}}`, `{{ip}}`, `{{port}}`, `{{chip}}` and `{{device}}` are all names the
  // OS or the radio reported; `{{note}}` is the backend's own sentence about a driver, passed
  // through as a value and translated in phase 3, never here. The two "· …" clauses are whole
  // optional clauses with their own separator, exactly as the sync reports above are.
  'settings.rigControl.detect.label': 'Zero-config setup',
  'settings.rigControl.detect.action': 'Detect my radio',
  'settings.rigControl.detect.scanning': 'Scanning…',
  'settings.rigControl.detect.use': 'Use this',
  'settings.rigControl.detect.hint':
    'One scan for everything: USB radios (fills model, port, sound device) AND FlexRadios on the network (fills the SmartSDR CAT config). Review, then Save.',
  'settings.rigControl.detect.flex.name': '{{radio}} — network',
  'settings.rigControl.detect.flex.meta':
    '{{ip}} · via SmartSDR CAT on this PC (slice A, TCP 5002)',
  'settings.rigControl.detect.unknownRadio': 'Unknown radio',
  'settings.rigControl.detect.civ.isCiv': ' · CI-V port — use this one',
  'settings.rigControl.detect.civ.notCiv': ' · second port, not CI-V',
  'settings.rigControl.detect.interface':
    'This is an interface cable, not a radio — pick your rig in <em>Rig Model</em> below. {{note}}',
  'settings.rigControl.detect.noModel':
    '⚠ Found the port but not the exact model — normal when the rig sits behind a generic USB bridge chip that reports only its own name (common on Icom, Yaesu, Kenwood, Elecraft, and Xiegu). Pick your rig in <em>Rig Model</em> below, or click <em>Auto-test</em> (it tries the common rigs to find the right port + baud for you).',
  'settings.rigControl.detect.driverLink': 'driver ↗',

  // The rig picker. The model NAMES and NUMBERS are Hamlib's and never move; the count line is
  // a plural, so `Intl.PluralRules` picks the form instead of a hand-rolled `s`.
  'settings.rigControl.rigModel.label': 'Rig Model',
  'settings.rigControl.rigModel.filter.placeholder': 'Find a rig — type a name or model number',
  'settings.rigControl.rigModel.filter.aria': 'Filter the rig model list',
  'settings.rigControl.rigModel.filter.none':
    'No model matches — clear the box, or enter the model number directly.',
  'settings.rigControl.rigModel.filter.count': {
    one: '{{count}} model match.',
    other: '{{count}} models match.',
  },
  'settings.rigControl.rigModel.none': '— None —',
  'settings.rigControl.rigModel.number.placeholder': 'or enter model #',
  'settings.rigControl.rigModel.number.aria': 'Enter a Hamlib rig model number directly',
  'settings.rigControl.rigModel.showAll.aria': 'Show all Hamlib rig models',
  // The loading clause is interpolated whole rather than glued on, so a language that marks
  // the state elsewhere in the sentence can move it.
  'settings.rigControl.rigModel.showAll.hint':
    'Show all models{{loading}} — the list above defaults to ~50 curated common rigs; check this for the full Hamlib catalog.',
  'settings.rigControl.rigModel.showAll.loading': ' (loading…)',
  'settings.rigControl.rigModel.hint':
    'Hamlib rig model. Not listed? Type its model number directly — Hamlib may still support it even without a friendly name here.',

  // Connection kind. `{{availability}}` is the whole closing sentence about OmniRig on this
  // platform — one of the two entries below it, never a fragment.
  'settings.rigControl.conn.label': 'Connection',
  'settings.rigControl.conn.serial': 'Serial (USB / COM port)',
  'settings.rigControl.conn.network': 'Network (host:port — SDR software, or a remote rig)',
  'settings.rigControl.conn.omnirig': 'OmniRig (the radio is set up in OmniRig)',
  'settings.rigControl.conn.omnirig.unavailable': 'OmniRig — Windows only',
  'settings.rigControl.conn.hint':
    'Serial for a rig on a USB/COM port (most rigs, including Xiegu). Network for anything serving CAT over TCP: an SDR program on this PC (Thetis, PowerSDR, SmartSDR CAT, piHPSDR), or a remote rigctld. The <b>Rig Model</b> still picks which CAT dialect is spoken — for an SDR, choose the program you launched, not the board inside the radio. An Icom on its <b>LAN port</b> comes in this way too: Icom\'s network protocol is its own, so run wfview (or RS-BA1) against the radio and point Nexus at wfview\'s rigctld server with Rig Model <b>NET rigctl</b>.',
  'settings.rigControl.conn.omnirig.hint':
    "<b>OmniRig</b> hands rig control to VE3NEA's OmniRig server, the one most Windows logging and contest programs already use. Set the radio up <em>in OmniRig</em> — rig type, COM port, baud — and Nexus talks to it there, so the Rig Model, Serial Port and Baud above are not used. {{availability}}",
  'settings.rigControl.conn.omnirig.unavailable.why':
    'It is greyed out here because OmniRig is a Windows program and this is not Windows.',
  'settings.rigControl.conn.omnirig.install':
    'Install and run OmniRig first; Nexus will not start without it.',
  // ⚠️ RIG 1 / RIG 2 are the labels OmniRig's OWN window uses. They are how the operator
  // matches this picker to that window, so they stay in the panel, untranslated.
  'settings.rigControl.omnirig.label': 'OmniRig radio',
  'settings.rigControl.omnirig.aria': 'OmniRig rig slot',
  'settings.rigControl.omnirig.hint':
    "Which of OmniRig's two radios this Nexus radio drives. OmniRig's own window labels them RIG 1 and RIG 2 — pick the one whose rig type matches this radio. A two-radio station can put one on each.",

  // Network address. Every port number in these sentences is a number the operator types into
  // another program's field — a dial setting, not prose — and stays exactly as written.
  'settings.rigControl.netAddr.label': 'Network Address',
  'settings.rigControl.dax.action': '⚡ Pair DAX audio ({{device}})',
  'settings.rigControl.dax.title':
    "SmartSDR's DAX virtual audio devices were detected — one click sets them as Nexus's audio in/out (bit-clean digital audio, no sound card)",
  'settings.rigControl.dax.paired': 'DAX paired: {{input}} → in, {{output}} → out',
  'settings.rigControl.netAddr.hint':
    "host:port. For a Flex: the WSJT-X-proven path is the SmartSDR CAT app on THIS PC — its DEFAULT TCP port 5002 is directed at slice A, so 127.0.0.1:5002 with the FLEX-6xxx / 8xxx model works out of the box; audio rides DAX. Multi-slice: SmartSDR CAT's per-slice ports are B=60001, C=60002, D=60003 — Nexus drives ONE slice, so enter the port of the slice you run digital on. (Direct-to-radio :4992 needs Hamlib's experimental native model and failed on real hardware.) Other rigs: a remote rigctld's host:port with their normal model.",
  'settings.rigControl.netAddr.sdrPorts':
    'Running an SDR program? Read the port out of the program, don\'t guess it: <b>Thetis</b> → Setup ▸ Serial/Network/Midi CAT ▸ <em>TCP/IP CAT Server</em> (its own box, factory 13013 — not the <em>TCI Server</em> box beside it, factory 50001; TCI is a different protocol and the Hamlib we ship has no backend for it, so pick a CAT profile such as "Thetis" and use the CAT server port); <b>SmartSDR CAT</b> → 5002; <b>piHPSDR</b> → 19090. Whatever it shows, type that.',

  // The serial port and its two rig-specific notes. Each note is ONE sentence per platform,
  // because what it says is a different answer, not a different ending; the Xiegu COM-number
  // clause is the one genuinely optional half and is interpolated whole.
  'settings.rigControl.serialPort.label': 'Serial Port',
  'settings.rigControl.serialPort.placeholder': 'Select or type, e.g. {{example}}',
  'settings.rigControl.serialPort.refresh.action': 'Refresh',
  'settings.rigControl.serialPort.refresh.title': 'Re-scan serial ports',
  'settings.rigControl.serialPort.autoTest.action': 'Auto-test',
  'settings.rigControl.serialPort.autoTest.title':
    'Probe each USB port (read-only — never transmits) and auto-select the one that drives your rig',
  'settings.rigControl.serialPort.hint.mac':
    'Serial device (/dev/cu.…) for rig control — or Auto-test to find it.',
  'settings.rigControl.serialPort.hint.other':
    'COM / serial device for rig control — or Auto-test to find it.',
  'settings.rigControl.serialPort.xiegu':
    '<b>Xiegu:</b> the radio makes two serial ports — CAT is on the <b>SERIAL-B</b> one{{note}}.',
  'settings.rigControl.serialPort.xiegu.comNumber': ' (often the higher COM number)',
  'settings.rigControl.serialPort.icom.mac':
    '<b>Icom:</b> this radio makes two /dev/cu.* ports and only one speaks CI-V — with the Silicon Labs VCP driver it is usually the first of the pair (plain <b>cu.SLAB_USBtoUART</b>; the dead twin gets a numeric suffix). The other one never answers.',
  'settings.rigControl.serialPort.icom.other':
    '<b>Icom:</b> this radio makes two COM ports and only one speaks CI-V — in Device Manager it is the CP210x port marked <b>Enhanced</b> (Icom\'s driver: “Serial Port A (CI-V)”). The “Standard” / “Serial Port B” one never answers.',

  // ⚠️ THE BAUD RATES THEMSELVES ARE NOT HERE AND NEVER WILL BE. The picker renders them from
  // `STANDARD_BAUDS`; the two written into this hint are the rates a rig's own menu offers,
  // spelled the way that menu spells them.
  'settings.rigControl.baud.label': 'Baud',
  'settings.rigControl.baud.hint':
    "Serial baud rate — match your rig's CAT setting (most modern rigs: 38,400 or 115,200). Native Icom CI-V scope needs 115,200 here <em>and</em> on the rig.",

  // Split operation. The three choices are WSJT-X's own Split Operation wording (the panel
  // keeps parity with it) and the hint names each one, so the two move together.
  'settings.rigControl.split.label': 'Split operation',
  'settings.rigControl.split.none': 'None',
  'settings.rigControl.split.rig': 'Rig',
  'settings.rigControl.split.fakeit': 'Fake It',
  'settings.rigControl.split.hint':
    'Keeps your transmitted audio between 1500–2000 Hz by shifting the TX dial in 500 Hz steps, so audio harmonics fall outside the transmit filter — cleaner signal. Rig = uses VFO B split. Fake It = retunes the VFO around each over (works on any CAT rig). None = stock WSJT-X default, transmits at the raw audio offset.',

  // The multiplier beside this label (×1.00) is a number and is rendered by the panel.
  'settings.rigControl.wheel.label': 'Wheel tuning sensitivity',
  'settings.rigControl.wheel.aria': 'Mouse-wheel tuning sensitivity',
  'settings.rigControl.wheel.hint':
    'How far the dial moves per mouse-wheel notch. Lower it if a high-resolution or free-spin mouse tunes too far per flick; raise it to tune faster. Applies to the frequency readout and the Phone/CW scope wheel.',

  // ── Settings ▸ Radio ▸ Rig & CAT ▸ Advanced ─────────────────────────────────────────
  'settings.rigControl.advanced.title': 'Advanced',
  'settings.rigControl.serialHandshake.label': 'Serial handshake',
  'settings.rigControl.serialHandshake.auto': 'Auto (recommended)',
  'settings.rigControl.serialHandshake.none': 'None — no flow control',
  'settings.rigControl.serialHandshake.hardware': 'Hardware (RTS/CTS)',
  'settings.rigControl.serialHandshake.xonxoff': 'XON/XOFF (software)',
  'settings.rigControl.serialHandshake.hint':
    "Tells Nexus what your cable actually does, instead of letting it guess. Leave it on Auto unless you have a rig that keys at launch — Auto changes nothing about how your station works today. If you change it and CAT stops working, put it back on Auto: on some rigs Hamlib quietly refuses the setting and then answers without ever having opened the radio, so you get a CAT light and a rig that ignores you.",
  'settings.rigControl.pttLineState.label': 'Keying line at startup',
  'settings.rigControl.pttLineState.auto': 'Auto (recommended)',
  'settings.rigControl.pttLineState.untouched': 'Never touch it',
  'settings.rigControl.pttLineState.low': 'Hold it low',
  'settings.rigControl.pttLineState.high': 'Hold it high',
  'settings.rigControl.pttLineState.hint':
    "For the one fault this exists to fix: a rig that keys at launch, before you have touched anything. Leave it on Auto unless that is happening to you — Auto changes nothing about how your station works today. If you change it and CAT stops working, put it back on Auto. Only you can see whether the rig unkeys; nobody can test this from our end, so treat it as something to try on a rig you are watching.",
  'settings.digital.tunePower.label': 'Tune power (%)',
  'settings.digital.tunePower.hint':
    "The power a tune-up keys at — leave it empty and Nexus never touches your power, which is what it does today. It can only turn the rig DOWN, never up: it keys at whichever is lower, this or the power you are already running, so 50 % here while you run 25 % still tunes at 25 %. On a 100 W rig, 10 % is about 10 W — enough for an antenna tuner, kind to a loop.",
  'settings.rigControl.rigctldPort.label': 'rigctld TCP Port',
  'settings.rigControl.rigctldPort.hint': 'Port Nexus launches rigctld on.',
  'settings.rigControl.plainSsb.label': 'Data modes use plain SSB',
  'settings.rigControl.plainSsb.hint':
    "<b>Leave this off unless you know you need it.</b> Nexus normally puts the radio in its DATA submode (DATA-U / USB-D / PKTUSB) for FT8, FT4, RTTY-AFSK and SSTV, because on most rigs that is the only mode where the USB codec reaches the transmitter. Turn this on and Nexus commands plain <b>USB/LSB</b> for those modes instead, and stays there — through band changes and when you call a station. Correct if your transmit audio goes in the <b>microphone</b> path, as with an interface wired to the mic jack (some RIGblaster models) — or if you simply prefer plain USB to the DATA submode (for its wider receive passband, say) and your rig is set to send its USB-codec audio in SSB, which on many modern rigs (FT-991A, IC-7300 and the like) is a single menu item. Either way the rig has to put the audio you're feeding onto the air in plain SSB: where it does not — the codec feeds only the data port and nothing carries in SSB — plain SSB takes audio from the mic and the radio transmits <b>no RF at all</b>, a red TX light and nothing on the air. <b>Per radio</b>, since it depends on how that rig is cabled and set. True FSK RTTY is unaffected — it keeps the rig's own RTTY mode.",
  'settings.rigControl.icomNative.label': 'Native Icom CI-V (early access)',
  'settings.rigControl.icomNative.hint':
    'Nexus drives this Icom\'s CI-V directly instead of launching rigctld — unlocking the rig\'s real spectrum scope in the waterfall ("CI-V RF") and instant dial tracking. The scope needs <b>115200 baud, set the same on BOTH the radio and Nexus</b>: (1) on the rig, Menu ▸ SET ▸ Connectors ▸ CI-V ▸ "CI-V USB Baud Rate" = <b>115200</b>; (2) on the rig, same menu, "CI-V USB Port" = "Unlink from [REMOTE]"; (3) the <b>Baud</b> field above = <b>115200</b> to match. Below that the rig refuses to stream the scope (CAT still works; the panadapter just stays off). Save to apply; turn off any time to return to the classic Hamlib path.',
  'settings.rigControl.flexPan.label': 'Flex native panadapter (early access)',
  'settings.rigControl.flexPan.hint':
    "Stream this FlexRadio's real SmartSDR panadapter (VITA-49 FFT) into the cockpit scope — the RF spectrum around your dial, with the Flex-pan span/ref controls. <b>Unverified on hardware</b>, so it's opt-in: needs the Flex IP set (from Find Radios) and SmartSDR reachable on this network. If the scope stays blank or the app hitches, turn it back off. Save to apply.",
  'settings.rigControl.flexAudio.label': 'Flex native DAX audio (early access)',
  'settings.rigControl.flexAudio.hint':
    'Carry this FlexRadio\'s audio straight over the network (VITA-49 DAX) instead of the "DAX Audio RX" / "DAX TX" sound devices — which are <b>invisible under Remote Desktop</b>. <b>Both directions:</b> the decoders read the rig\'s receive audio directly, and transmit audio goes out over DAX too, which disconnects the rig\'s microphone while this is on. Turning it off, switching radio or quitting Nexus puts the mic back. <b>Unverified on hardware</b>, opt-in: needs the Flex IP set and SmartSDR reachable. If decodes or transmit stop, turn it back off. Save to apply.',
  // ⚠️ `{{path}}` is a file path the backend chose. Markers are parsed BEFORE values are
  // substituted, so a path that happens to contain `<b>` is text, never markup.
  'settings.rigControl.civLog.label': 'CI-V bus diagnostic log',
  'settings.rigControl.civLog.failed': 'Could not toggle the CI-V diagnostic log',
  'settings.rigControl.civLog.recording':
    "<b>Recording</b> to <code>{{path}}</code> — this keeps running while you're on other screens, so go to the FT8 or Phone cockpit and reproduce the issue (Tune or transmit) now. Come back and turn it off when done, then send that file. It shows exactly what's on the bus during the fault.",
  'settings.rigControl.civLog.idle':
    'Records every byte to/from the radio on the native CI-V path to a file in your Downloads — a support tool for hardware-only issues like the IC-9700 PTT flicker. Turn on, reproduce the problem, turn off, then send the file.',
  'settings.rigControl.flexIp.label': 'Flex radio IP (native panadapter + DAX)',
  'settings.rigControl.flexIp.hint':
    "Your FlexRadio's LAN IP (SmartSDR API, port 4992) — turns on the native RF panadapter. This is the <em>radio's</em> address, not the SmartSDR-CAT port above.",
  'settings.rigControl.sharingPort.label': 'Sharing port',
  'settings.rigControl.sharingPort.hint':
    'The "Share this radio" address other programs connect to (Hamlib NET rigctl default 4532). Change it only if something else on this computer already owns the port.',

  // Test CAT reads the rig's frequency back. It is a read-only probe, not a transmit control.
  'settings.rigControl.testCat.action': 'Test CAT',
  'settings.rigControl.testCat.testing': 'Testing…',
  'settings.rigControl.testCat.title': 'Save settings, connect to the rig, and read its frequency',

  // ── Settings ▸ Radio ▸ Audio ────────────────────────────────────────────────────────
  // ⚠️ THE DEVICE NAMES ARE NOT HERE AND NEVER WILL BE. Every entry in these pickers is
  // whatever the OS enumerated ("USB Audio CODEC", "Speakers (Realtek)", a DAX channel) — the
  // panel renders them straight, and `settings.audio.device.notInList` (with the sync toasts
  // above) is the only prose that goes near one. Sample rates, dB readings, the × gain
  // multiplier and the drive percentage are numbers the panel formats invariantly.
  'settings.audio.legend': 'Audio',
  // The multi-radio banner: ONE sentence with the radio's own name interpolated, never
  // "for" + name + "." glued — that ordering does not survive translation. `{{radio}}` is a
  // profile name the operator typed; the fallback below is prose because there is no name yet.
  'settings.audio.multiRadio.note':
    '🎚 Audio devices below are for <b>{{radio}}</b>. Each radio has its OWN input/output — click “Edit” on another radio (in Radios above) to set its audio. The live RX audio + waterfall follow whichever radio is active.',
  'settings.audio.multiRadio.selectedRadio': 'the selected radio',
  // The empty `value` of these pickers, whose label says "let the OS choose". The VALUE is ''
  // in every language; only this label moves. Shared with the wizard's two audio pickers.
  'settings.audio.device.systemDefault': 'System default',
  'settings.audio.input.label': 'Input Device (RX)',
  'settings.audio.input.hint': 'Sound card carrying receive audio.',
  'settings.audio.refresh.action': 'Refresh',
  'settings.audio.refresh.title': 'Re-scan audio devices',
  'settings.audio.output.label': 'Output Device (TX)',
  'settings.audio.output.hint': 'Sound card feeding the rig (transmit).',
  'settings.audio.spectrum.label': 'Live input spectrum',
  'settings.audio.spectrum.idle':
    'Flat — no audio on the selected input. Check the device above (radio on? right codec?).',
  'settings.audio.spectrum.hint':
    'What the selected input hears, live — band noise should show as a moving floor. Confirms the RIGHT device before you leave Settings.',
  // ⚠️ TX POWER IS THE DRIVE-LEVEL SLIDER, NOT A TRANSMIT CONTROL. It sets how hard the sound
  // card feeds the rig; it cannot key, unkey or stop anything, and it is on no cockpit's
  // stop-line census. Same reading as PTT Method above — the words moved, the path did not.
  'settings.audio.txPower.label': 'Tx Power',
  'settings.audio.txPower.aria': 'Transmit drive level',
  'settings.audio.txPower.hint':
    "The audio <b>drive</b> into the rig — the SAME control as the cockpit <b>Pwr</b> slider (they always match now). Trim down until your rig's ALC is just zero. This is <em>not</em> the rig's RF watts — set those on the radio.",
  'settings.audio.rxLevel.label': 'RX Level',
  'settings.audio.rxLevel.meter': 'RX audio level',
  'settings.audio.rxLevel.hint':
    "A dB scale like WSJT-X — aim for around 30 dB. Anything from ~15–60 dB decodes fine; red means too hot (back off RX Gain or the rig's audio).",
  'settings.audio.rxGain.label': 'RX Gain',
  'settings.audio.rxGain.aria': 'RX capture gain',
  'settings.audio.rxGain.hint':
    'Boost a quiet interface until RX Level reads around 30 dB — the meter responds as you release the slider. Leave at ×1.0 unless the meter reads low (under ~15 dB) — FT8 decodes on a small signal, so you rarely need much.',

  // ── Settings ▸ Radio ▸ Headphone monitor ────────────────────────────────────────────
  // "System default" inside these sentences names the picker entry above, so it reads the
  // same word the operator just chose — translate them together.
  // ⚠️ THE WORD "MONITOR" IS NOT USED HERE, deliberately, though the settings behind it are still
  // named `monitor_*`. In amateur practice MONITOR means listening to your own TRANSMITTED audio —
  // it is what MONI on the rig does. This plays the RECEIVED audio out of a computer device. An
  // experienced operator read the old label and asked whether it would put his own voice back in
  // his ears (2026-08-22); it would not, which is precisely the problem with the old wording.
  'settings.headphoneMonitor.legend': 'Receive audio on this computer',
  'settings.headphoneMonitor.enable.label': 'Play receive audio here',
  'settings.headphoneMonitor.enable.aria': 'Play receive audio on this computer',
  'settings.headphoneMonitor.enable.hint':
    "Plays the RECEIVED audio — exactly what the decoder hears — out of a device on this computer, for level and RFI diagnosis or simply to listen to the band. This is not a transmit monitor: it never plays your own voice back. Off by default; UNVERIFIED on-air until the attended session.",
  // NAMES THE DESTINATION, not the direction — "Output device" collided with
  // `settings.audio.output.label` ("Output Device (TX)") on the same tab, so the Radio page
  // showed two pickers differing by a parenthetical and a capital letter. Under the section
  // heading it read clearly; scanning the tab it did not (kd9taw, #157).
  'settings.headphoneMonitor.device.label': 'Headphones or speakers',
  'settings.headphoneMonitor.device.hint':
    "Your headphones or speakers — must NOT be the rig's TX output device.",
  'settings.headphoneMonitor.level.label': 'Listening level',
  'settings.headphoneMonitor.level.aria': 'Receive listening level',
  'settings.headphoneMonitor.level.hint':
    'Headphone listening volume (does not affect TX).',

  // ── Settings ▸ Radio ▸ Satellite Doppler ────────────────────────────────────────────
  // ⚠️ The MAPPING LABELS are not here — they live in `features/satVfo.ts` beside the wire
  // value each one stores, because the Satellites readiness rail shows the same list and two
  // copies of a list that decides WHERE THE RADIO TRANSMITS would be a wrong-uplink generator.
  // Hz and ms are unit symbols and the numbers in these hints (20 Hz, 1000 ms) are the facts
  // themselves; VFO, AOS, LOS, SSB and CAT are the hobby's own vocabulary and stay verbatim.
  'settings.satelliteDoppler.legend': 'Satellite Doppler',
  'settings.satelliteDoppler.note':
    'Corrects both legs of a pass: the downlink you listen on and the uplink you transmit on. Nexus tunes only while auto-track is following a pass and you have picked a transponder in the Satellites section. The downlink needs no setup here; the uplink is confirmed once per radio, on the pass itself.',
  'settings.satelliteDoppler.enable.label': 'Doppler correction',
  'settings.satelliteDoppler.enable.aria': 'Enable satellite Doppler correction',
  'settings.satelliteDoppler.enable.hint':
    'Retunes the radio through a pass so you stay on the station you are working. On: the downlink follows the bird as soon as you arm a pass and hold a transponder. Clearing this stops both legs.',
  'settings.satelliteDoppler.vfoMap.label': 'VFO mapping',
  'settings.satelliteDoppler.vfoMap.aria': 'Satellite VFO mapping',
  'settings.satelliteDoppler.vfoMap.otherRadio':
    'The uplink mapping is confirmed per radio, for the radio you are operating. Confirm it for this radio on the pass rail during a pass, or make it the active radio first.',
  'settings.satelliteDoppler.vfoMap.hint':
    "Which VFO carries your uplink. Match this to how your radio is wired. <b>A wrong mapping transmits on your own downlink</b> — into the satellite's output passband, on top of everyone else working the bird. Picking one applies immediately and confirms it for the radio you are operating; a second radio gets its own confirmation on the pass rail. Every mapping except Uplink only keeps the downlink corrected.",
  'settings.satelliteDoppler.minShift.label': 'Minimum shift (Hz)',
  'settings.satelliteDoppler.minShift.aria': 'Minimum Doppler shift before retuning (Hz)',
  'settings.satelliteDoppler.minShift.hint':
    'Corrections smaller than this are not sent. 20 Hz is inaudible on SSB and keeps the CAT link quiet. 0 sends every update.',
  'settings.satelliteDoppler.interval.label': 'Update interval (ms)',
  'settings.satelliteDoppler.interval.aria': 'Doppler update interval (milliseconds)',
  'settings.satelliteDoppler.interval.hint':
    'Shortest gap between corrections. 1000 ms is what a low-orbit pass needs. Shorter fights your own tuning knob and saturates a serial CAT link.',
  'settings.satelliteDoppler.passSounds.label': 'Pass alert sounds',
  'settings.satelliteDoppler.passSounds.aria': 'Audible tones at pass start and end',
  'settings.satelliteDoppler.passSounds.hint':
    'A rising tone the moment an armed pass starts and a falling one when it ends, alongside the popup — hear AOS with your hands on the rotor. On by default; clearing this silences only the tones, never the popups.',

  // ── Settings ▸ Radio ▸ Orbital elements ─────────────────────────────────────────────
  // ⚠️ TLE, Keplerian elements, SupGP, the epoch, CelesTrak, SatNOGS, AMSAT and the CC BY-SA
  // 4.0 licence name are all invariant — they name a data format, its sources and the terms
  // they ship under. So is `{{source}}`, which is the backend's own one-word provenance
  // ("mirror", "celestrak", "import"), and `{{date}}`, an ISO date the panel slices out
  // invariantly — a locale-formatted element date would name a different day in half the
  // world. The status line is a LIST of independent chips joined with " · " (the shape
  // `elementBandSummary` already uses), not a sentence, so each chip is its own entry.
  'settings.orbitalElements.legend': 'Orbital elements',
  'settings.orbitalElements.update.action': 'Update now',
  'settings.orbitalElements.update.busy': 'Updating…',
  'settings.orbitalElements.import.action': 'Import from file',
  'settings.orbitalElements.import.busy': 'Importing…',
  'settings.orbitalElements.import.title':
    "Import a downloaded element file (Celestrak TLE, AMSAT keps, a new launch's SupGP set) — the offline-shack escape hatch. Imports persist across refreshes; the newest epoch per satellite wins.",
  'settings.orbitalElements.import.ok':
    'Elements imported — {{imported}} imported, {{total}} total',
  // `{{error}}` is the raw failure, passed through as a value and translated in phase 3.
  'settings.orbitalElements.import.failed': 'Element import failed: {{error}}',
  // English says "birds" at every count; the param is named `count` so a locale that needs
  // plural forms can supply them without the English wording changing.
  'settings.orbitalElements.status.birds': '{{count}} birds',
  'settings.orbitalElements.status.fetched': 'fetched {{date}}',
  'settings.orbitalElements.status.neverFetched': 'never fetched',
  'settings.orbitalElements.status.imported': '{{count}} imported',
  'settings.orbitalElements.status.empty':
    'Not loaded yet — fetched on first launch, then refreshed every 6 h.',
  'settings.orbitalElements.hint':
    'Keplerian elements (TLEs) for the amateur satellites — pass times, pointing and Doppler all come from them. Refreshed every 6 h from hamradiotools.io: the bird list comes from the SatNOGS database (CC BY-SA 4.0), the elements from CelesTrak and SatNOGS. Import a file for an offline shack or a just-launched bird.',
  // `{{detail}}` is `tleRefreshMessage`'s operator-voiced sentence — already a catalog string
  // (batch 7), composed there and interpolated whole here.
  'settings.orbitalElements.lastRefresh': 'Last refresh: {{detail}}',

  // ── Settings ▸ Radio ▸ Amplifier ────────────────────────────────────────────────────
  // ⚠️ NOT HERE, and none of it may move: the AMPLIFIER FAMILY NAMES ("SPE Expert 1.3K-FA /
  // 2K-FA", "Elecraft KPA500 / KPA1500") and the serial device examples. Those are
  // manufacturers' product names and OS device paths — a translated one names no amplifier
  // anyone owns and no port any machine has. `SWR` and `ATU` are the vocabulary of the thing
  // being configured and stay verbatim inside these sentences.
  //
  // NO PLURAL ENTRIES IN THIS BLOCK. es.ts and fr.ts carry English's plural entries flattened
  // into single concatenated strings, so a `{one, other}` key added here would ship rendering
  // both forms at once in two languages, and no guard sees it.
  'settings.amplifier.legend': 'Amplifier',
  'settings.amplifier.note':
    'Read-only status from a linear on its own serial port — power out, SWR, temperature and any alarm. Nexus never commands the amplifier: it only reads it.',
  'settings.amplifier.model.label': 'Amplifier',
  'settings.amplifier.model.none': 'None',
  'settings.amplifier.model.hint':
    'Place the Amplifier pane in Connect to see the readings. Nothing here changes how the radio transmits.',
  'settings.amplifier.follow.label': 'Follow the radio\u2019s band',
  'settings.amplifier.follow.hint':
    'Step the amplifier to the band you are on, without being asked. Off by default \u2014 this is the one amplifier control that acts on its own. It never moves the amplifier while you are transmitting, and it steps one band at a time, checking where the amplifier actually is after each one rather than assuming it got there. \u26a0\ufe0f If your amplifier already follows the radio through its own band-data cable, as most SPE installations do, leave this off: the hardware is doing the same job, and two things steering one band is worse than either alone.',
  'settings.amplifier.port.label': 'Amplifier port',
  'settings.amplifier.port.hint':
    'Its own port, not the one CAT uses — a serial port can only be open once, so sharing it stops the radio connecting. The speed is worked out for you.',

  // ── Settings ▸ Radio ▸ Rotator ──────────────────────────────────────────────────────
  // ⚠️ NOT HERE, and none of it may move: the ROTATOR MODEL NAMES and their Hamlib model
  // numbers (`ROTATOR_MODELS` — checked against the generated caps fixture, so they cannot
  // even be typed from a manual), the serial-port device examples, every baud rate, and the
  // azimuth/elevation degrees an operator types. `rotctld`, `rotctl -l`, `Hamlib`, `az`, `el`,
  // `LOS` and the `°` symbol are the vocabulary of the thing being configured and stay
  // verbatim inside these sentences.
  'settings.rotator.legend': 'Rotator',
  'settings.rotator.note':
    'The rotator itself, and its pointing manners. The manners apply to satellite auto-track.',
  'settings.rotator.model.label': 'Rotator model',
  'settings.rotator.model.none': 'None',
  'settings.rotator.model.other': 'Other Hamlib model #…',
  'settings.rotator.model.number.placeholder': 'Hamlib rotator model number (rotctl -l lists them)',
  'settings.rotator.model.number.aria': 'Hamlib rotator model number',
  'settings.rotator.model.hint':
    'Nexus runs the control daemon (rotctld) for you, the same way it does CAT. Then use the Rotor pane in Connect, ↗ on Needed rows, or the compass anywhere.',
  'settings.rotator.port.label': 'Rotator port & baud',
  'settings.rotator.port.aria': 'Rotator serial port',
  'settings.rotator.baud.aria': 'Rotator baud rate',
  'settings.rotator.baud.title': 'Serial baud rate for the rotator controller',
  // ⚠️ `{{rate}}` and `{{set}}` ARRIVE AS ALREADY-FORMATTED STRINGS, and that is a defect this
  // batch deliberately did not fix: the panel still builds them with `toLocaleString()`, so a
  // German install reads "9.600" for a baud rate — the same known hole the Rig & CAT baud
  // picker has, named in `i18n.invariant.test.ts`'s "what this guard does NOT prove". Passing
  // the numbers instead would render "9600" and change visible English, which this phase's
  // contract forbids. It wants the same change that fixes the baud picker.
  'settings.rotator.baud.hint.any':
    'Match the rate your controller is set to — Hamlib does not offer one fixed rate for this model.',
  'settings.rotator.baud.hint.fixed':
    'This controller runs at {{rate}} baud — the rate its Hamlib backend declares. Leave it here.',
  'settings.rotator.baud.hint.wrong':
    '<b>This controller runs at {{rate}} baud, not {{set}}</b> — at the wrong rate it never answers and reads as broken hardware. Set {{rate}}, or re-pick the model above to fill it in.',
  'settings.rotator.external.label': 'External rotctld (advanced)',
  // `{{example}}` is a host:port the panel supplies as an invariant token.
  'settings.rotator.external.placeholder': 'host:port — e.g. {{example}}',
  'settings.rotator.external.aria': 'External rotctld address (advanced)',
  'settings.rotator.external.hint':
    'Point Nexus at a rotctld you run yourself (or one on another machine). It OVERRIDES the model and port above and stops the integrated daemon. Needs the port — a bare host name is not an address.',
  'settings.rotator.park.label': 'Park position (° az / el)',
  'settings.rotator.park.az.aria': 'Park azimuth (degrees)',
  'settings.rotator.park.el.aria': 'Park elevation (degrees)',
  'settings.rotator.park.hint':
    'The stow position — wind-safe, or wherever your mast rests. Used only when After a pass is set to Park.',
  'settings.rotator.ready.label': 'Ready position (° az / el)',
  'settings.rotator.ready.az.aria': 'Ready azimuth (degrees)',
  'settings.rotator.ready.el.aria': 'Ready elevation (degrees)',
  'settings.rotator.ready.hint':
    'Where the antenna waits for the next pass. Used only when After a pass is set to Ready.',
  // The three choices below are a <select>: each stored `value` ('stop', 'park', 'ready') is
  // the token, and only the label moved. The hint names all three, so they translate together.
  'settings.rotator.postPass.label': 'After a pass',
  'settings.rotator.postPass.aria': 'What the rotator does after a pass',
  'settings.rotator.postPass.stop': 'Stop — leave the antenna where the pass ended',
  'settings.rotator.postPass.park': 'Park — drive to the park position',
  'settings.rotator.postPass.ready': 'Ready — drive to the ready position',
  'settings.rotator.postPass.hint':
    'Stop is the default and moves nothing: the antenna stays pointed where the bird set. Park and Ready drive the rotator on their own at LOS, so set those positions above first.',
  'settings.rotator.tolerance.label': 'Tolerance (° az / el)',
  'settings.rotator.tolerance.az.aria': 'Azimuth tolerance (degrees)',
  'settings.rotator.tolerance.el.aria': 'Elevation tolerance (degrees)',
  'settings.rotator.tolerance.hint':
    "A new target closer than this is not commanded. Without a deadband the rotator hunts and the relays chatter for the whole pass. 2° is about a G-5500's own resolution.",
  'settings.rotator.calibration.label': 'Calibration trim (° az / el)',
  'settings.rotator.calibration.az.aria': 'Azimuth calibration trim (degrees)',
  'settings.rotator.calibration.el.aria': 'Elevation calibration trim (degrees)',
  'settings.rotator.calibration.hint':
    'Added to every command. Use it when the controller reads one heading and the boom points at another.',
  'settings.rotator.flip.label': 'Allow flip',
  'settings.rotator.flip.aria': 'Allow the rotator to flip past 90 degrees elevation',
  'settings.rotator.flip.hint':
    'Takes a high pass by turning azimuth 180° and running elevation past 90°, instead of swinging the mast around at the top of the pass. Off by default: <b>many rotators cannot mechanically go past 90° elevation</b>. Check your controller before turning this on.',

  // ── Setup health (the three-dot strip: Settings ▸ Radio and the wizard's verify stage) ──
  // ⚠️ ONE STRING FROM THIS STRIP IS NOT HERE, and it is deliberate: Prove TX keys a tune
  // carrier, so its label, tooltip and consent prompt move with the transmit-path batch. The
  // dots themselves are STATUS, not controls. Each state is a whole phrase rather than a
  // "Rig" + state glue — the word order is not universal and the state word is not always
  // last. The dB reading and the forward watts are measurements the strip formats invariantly.
  'setup.health.title': 'Setup health',
  'setup.health.rig.title.link': 'CAT not tested yet — open Rig Control',
  'setup.health.rig.title.plain': 'CAT not tested yet — use Test CAT below',
  'setup.health.rig.responding': 'Rig responding',
  'setup.health.rig.notAnswering': 'Rig not answering',
  'setup.health.rig.untested': 'Rig untested',
  'setup.health.rx.title.ok': 'Receiving audio',
  'setup.health.rx.title.link': 'No RX audio — open the audio device settings',
  'setup.health.rx.title.plain': 'No RX audio — check the audio device below',
  'setup.health.rx.error': 'RX audio error',
  'setup.health.rx.reading': 'RX audio {{db}} dB',
  'setup.health.rx.none': 'RX audio —',
  'setup.health.tx.title.keying':
    'Keying a tune carrier — forward power confirms the CAT → PTT → RF path',
  'setup.health.tx.title.on': 'Transmit is enabled',
  'setup.health.tx.title.off': 'Transmit is off',
  'setup.health.tx.keying.power': 'TX keying · {{watts}} W',
  'setup.health.tx.keying.waiting': 'TX keying…',
  'setup.health.tx.on': 'TX on',
  'setup.health.tx.off': 'TX off',

  // ── The setup wizard (first run, and the Settings ▸ re-open path) ───────────────────
  // The wizard's steps ARE the Radio settings above, so the two were migrated together and
  // must be reworded together — a wizard that says one thing and Settings another is how the
  // pair drifted before. The same invariant-token line holds here as on that tab: device and
  // port names, model names and numbers, baud rates, IPs and host:port addresses, callsign and
  // grid examples, the ADIF file extensions and the program names inside these sentences
  // (WSJT-X, N1MM, Log4OM, HRD, QRZ, LoTW, ClubLog, SmartSDR CAT, DAX) all stay in the
  // component. So does every `value` this dialog writes — the license `id`, the connection
  // kind, the pack id.
  //
  // ⚠️ A twin of the license block below lives at `gettingStarted.license.shot.*`: the guide
  // shows a PICTURE of this step. They are separate keys on purpose (one is a depiction, one
  // is the live control) — reword them together.
  'setup.title': 'Set up Nexus',
  'setup.steps.station': 'Your station',
  'setup.steps.rig': 'Your rig',
  'setup.steps.log': 'Your log',
  'setup.steps.finish': 'Finish',
  'setup.steps.aria': 'Step {{n}} of {{total}}: {{title}}',

  // Step 1 — station identity. The callsign and grid EXAMPLES are tokens and stay in the
  // component; `{{short}}` / `{{long}}` are two of them, quoted inside the error sentence.
  'setup.station.title': 'Who’s on the air?',
  'setup.station.sub':
    'Your grid square is the anchor for everything location-based — satellite passes, propagation, the map, and DXpedition windows are all computed from it.',
  'setup.station.callsign.label': 'Callsign',
  'setup.station.grid.label': 'Grid square',
  'setup.station.grid.invalid':
    'Not a Maidenhead locator — 4 or 6 characters, like {{short}} or {{long}}.',
  'setup.station.grid.hint':
    'Maidenhead locator (qrz.com shows yours). Give all 6 — 4 characters pins you to the middle of a ~100-mile square, and every distance and bearing is measured from there.',

  // Step 2 — rig & audio. `{{radio}}`, `{{port}}`, `{{ip}}`, `{{chip}}` and `{{device}}` are
  // names the OS or the radio reported; `{{error}}` and the probe's own `detail` line are raw
  // answers passed through as values. The "· …" clauses carry their own separator because
  // each is appended to a line the caller built.
  'setup.rig.title': 'How does the radio connect?',
  'setup.rig.sub':
    'One detect finds everything — USB rigs and FlexRadios on the network. Skippable; Settings ▸ Radio ▸ Rig & CAT has all of this later (including Test CAT).',
  'setup.rig.detect.action': '🔍 Detect my radio',
  'setup.rig.detect.busy': 'Detecting…',
  'setup.rig.detect.failed':
    'Detection hit an error: {{error}}. You can still pick your rig by hand below, or skip and set it up later.',
  'setup.rig.detect.empty':
    'Nothing found — USB: plug in + power on; Flex: must be on this network. Or skip and set it up later.',
  // The two halves of one scan can fail independently; each is a whole sentence, and the
  // panel joins whichever failed with " · " before the sentence above quotes the lot.
  'setup.rig.detect.usbFailed': 'USB scan failed: {{error}}',
  'setup.rig.detect.flexFailed': 'Flex network scan failed: {{error}}',
  'setup.rig.detect.flex.row': '<b>{{radio}}</b> on the network ({{ip}})',
  'setup.rig.detect.flex.via': ' · via SmartSDR CAT',
  'setup.rig.detect.rig.row': '<b>{{radio}}</b> on {{port}}',
  'setup.rig.flexNote.windows':
    '{{radio}} at {{ip}} — CAT set via SmartSDR CAT (slice A, port 5002; a second slice uses 60001), radio address saved for the native panadapter/DAX. Test CAT below.',
  'setup.rig.flexNote.other':
    '{{radio}} at {{ip}} — model and radio address saved. SmartSDR CAT is Windows-only: put the address of a Windows PC running it (slice A = its port 5002) in Network Address below, then Test CAT.',
  'setup.rig.selected': 'Selected: {{radio}} on {{port}}{{baud}}',
  'setup.rig.selected.baud': ' @ {{baud}} baud',
  'setup.rig.selected.unnamedRadio': 'radio',
  'setup.rig.autoTest.action': '🔎 Auto-test my ports',
  'setup.rig.autoTest.busy': 'Testing ports — {{seconds}}s (can take up to a minute)…',
  'setup.rig.autoTest.title':
    'Probes every USB port until a radio answers — read-only, never transmits',
  'setup.rig.autoTest.failed': 'Auto-test failed: {{error}}',
  'setup.rig.model.label': 'Which radio is this?',
  'setup.rig.model.placeholder': 'Pick your rig model…',
  // The bare model NUMBER an out-of-catalog rig is known by — the number stays a token.
  'setup.rig.model.unnamed': 'Model {{model}}',
  'setup.rig.model.unknownResponder': 'unknown',
  'setup.rig.model.seeded':
    'The port answered, but the exact model is a guess ({{radio}} responded). Pick your radio so its real command set is used.',
  'setup.rig.model.confirm':
    'Confirm the exact model — fixed-rate rigs get their baud set automatically.',
  'setup.rig.conn.serial.label': 'USB / Serial',
  'setup.rig.conn.serial.blurb': 'Most rigs — one cable',
  'setup.rig.conn.network.label': 'Network',
  'setup.rig.conn.network.blurb': 'FlexRadio / remote rigctld',
  'setup.rig.address.label': 'Address',
  'setup.rig.network.hint':
    'A found Flex configures the WSJT-X-proven path: CAT through the SmartSDR CAT app on this PC — its default TCP port 5002 drives slice A (per-slice ports: B=60001, C=60002) — and audio through DAX. Other network rigs: pick their model later in Settings ▸ Radio ▸ Rig & CAT.',
  'setup.rig.dax.title':
    "SmartSDR's DAX virtual audio devices were detected — pairs them as Nexus's audio in/out",
  'setup.rig.audioIn.label': 'Audio in',
  'setup.rig.audioOut.label': 'Audio out',
  'setup.rig.testCat.action': '⚡ Test CAT',
  'setup.rig.testCat.busy': 'Testing…',
  'setup.rig.testCat.title':
    "Saves what you've entered so far, then asks the radio for its frequency",
  'setup.rig.second.action': '＋ I have a second radio',
  'setup.rig.second.title': 'Adds a radio profile and probes the remaining USB ports for it',
  'setup.rig.second.probing':
    'Probing the other ports — {{seconds}}s (radio 1’s port is skipped; this can take up to a minute)…',
  'setup.rig.second.saved': 'Second radio: {{radio}} on {{port}} — saved to its own profile.',
  'setup.rig.second.addFailed': "Couldn't add the radio: {{error}}",
  'setup.rig.second.model.label': 'Which radio is the second one?',
  'setup.rig.second.model.placeholder': 'Pick the model…',
  'setup.rig.second.model.seeded':
    'The port answered but the exact model is a guess — pick the radio so its real command set is used.',
  // `<a>` is the "swap them" BUTTON, supplied by the call site — the catalog names the span,
  // never the element. The non-breaking space keeps the link off a line of its own.
  'setup.rig.second.swap':
    'Both radios use identical USB sound cards, shared out one each. If you later see the <em>wrong</em> rig’s meters move when audio plays,\u00a0<a>swap them</a>.',
  'setup.rig.second.twoWindows':
    'To run both radios at the same time, open Nexus twice — each window drives one radio (the launcher asks which).',

  // Step 3 — the ADIF import. `.adi` / `.adif` are file extensions and the logger names are
  // products; both stay verbatim. The counts are plurals, so `Intl.PluralRules` picks the
  // form instead of the hand-rolled `s` this step used to carry.
  'setup.log.title': 'Bring in your existing log',
  'setup.log.sub':
    "Nexus works best when it knows your history. Importing your ADIF log is what powers <b>worked-before</b> flags, the <b>Needed</b> board (new DXCC / states / grids), and your <b>awards</b> progress — without it, the app starts blind and treats every station as new. This is optional and you can import anytime from the Logbook, but it's the single biggest thing that makes the app useful on day one.",
  'setup.log.import.action': 'Import my ADIF log…',
  'setup.log.import.again': 'Import another ADIF file',
  'setup.log.import.busy': 'Importing…',
  'setup.log.import.failed': 'Import failed: {{error}}',
  'setup.log.result.imported': {
    one: '✓ Imported <b>{{count}}</b> QSO{{dupes}}. Your worked-before and Needed board are now seeded.',
    other:
      '✓ Imported <b>{{count}}</b> QSOs{{dupes}}. Your worked-before and Needed board are now seeded.',
  },
  'setup.log.result.dupes': '{{count}} already present',
  'setup.log.result.allDupes': "✓ All {{count}} QSOs were already in your log — you're seeded.",
  'setup.log.result.none':
    '⚠ No QSOs found in that file — is it a standard ADIF (.adi/.adif) export?',
  'setup.log.sources':
    'From WSJT-X, N1MM, Log4OM, HRD, QRZ, LoTW, ClubLog — any standard ADIF (.adi/.adif) export. Nothing leaves your computer; duplicates are detected and skipped.',

  // Step 4 — license, starter packs, the walkthrough offer. The license CLASS NAMES are prose
  // here exactly as they are in Settings ▸ Station (`settings.station.licenseClass.*`); the
  // `id` beside each one is the token that turns the privilege lockout on.
  'setup.finish.title': 'You get everything',
  'setup.finish.sub':
    'Every mode and every section starts ON — FT8/FT4, Phone, CW, RTTY, SSTV, APRS, satellites, the maps, the lot. Nexus is one program instead of six; there’s nothing to unlock. If you ever want a leaner app, trim sections in Settings.',
  'setup.finish.license.title': 'What’s your license?',
  'setup.finish.license.sub':
    'Sets your transmit privileges — the app parks the dial in your licensed band segments and won’t let you transmit outside them. Pick “Outside the US” for no limits.',
  'setup.finish.license.technician': 'Technician',
  'setup.finish.license.technician.blurb': 'US — limited HF + full VHF/UHF',
  'setup.finish.license.general': 'General',
  'setup.finish.license.general.blurb': 'US — most HF privileges',
  'setup.finish.license.extra': 'Amateur Extra',
  'setup.finish.license.extra.blurb': 'US — full privileges',
  'setup.finish.license.open': 'Outside the US',
  'setup.finish.license.open.blurb': 'No transmit limits',
  'setup.finish.packs.title': 'Start with some channels?',
  'setup.finish.packs.sub':
    'Optional — a ready-made set of common frequencies and nets, added to your Memories. Change or remove any of them later in the Memories section.',
  // `{{region}}` is the pack's own region name, carried as data by `features/packs.ts`.
  'setup.finish.packs.meta': '{{count}} channels · {{region}}',
  'setup.finish.guide.title': 'Want a walkthrough of what you just set up?',
  'setup.finish.guide.label': 'Show me Getting started',
  'setup.finish.guide.blurb': 'The four things, in order — opens when this closes',

  'setup.nav.back': '← Back',
  'setup.nav.skip': 'I’ll set it up myself',
  'setup.nav.next': 'Next →',
  'setup.nav.finish': 'Finish — everything on',

  // ── Settings ▸ Radio ▸ Transmit limits & sharing ────────────────────────────────────
  // Transmit POLICY and station plumbing: what the licence lets the operator do, a per-mode
  // power ceiling, the setup backup, and sharing the radio with another program. These are
  // CONFIGURATION on the transmit path — the batch-13 ruling — and none of them is a control
  // that keys, gates or stops an over, so they move here while the cockpit's TX controls wait.
  //
  // ⚠️ NOT HERE, and none of it may move: the three MODE names on the power caps, the `%` they
  // are set in, the loopback ADDRESS and its port, and the PROGRAM names in the sharing hints
  // (VarAC, FreeDV, WSJT-X, JS8Call, fldigi, Hamlib, rigctld) — other people's software, named
  // as they name themselves. The backup FILE NAME is built invariantly in the panel for the
  // batch-8 reason: a translated word in it would leave a non-Latin locale with
  // `nexus-settings-.json`.
  'settings.transmit.legend': 'Transmit limits & sharing',

  'settings.transmit.bandEdgeTones.label': 'Band-edge tones',
  'settings.transmit.bandEdgeTones.hint':
    'A short audio cue when the dial crosses your license privileges — a rising "ding" back in band, a falling "dong" past an edge. Applies on every mode.',

  'settings.transmit.powerCaps.label': 'Max power by mode (safety)',
  'settings.transmit.powerCaps.hint':
    'A ceiling on RF output per mode — leave blank for full power. FT8/FT4/RTTY run ~100% duty cycle, so capping the Digital modes (e.g. 30%) protects your finals and any amplifier. The rig is brought down to the cap the moment you enter a capped mode, not only when you touch the power slider.',

  // `Test CAT`, `Rig Model` and `Serial Port` are the names of controls in Rig & CAT, and
  // `rigctld` is the daemon's own; the call site supplies the emphasis for each.
  'settings.transmit.note':
    "Saving applies your rig settings live (no restart). <b>Test CAT</b> saves, launches the bundled <code>rigctld</code> (Hamlib ships with Nexus on Windows — no separate install), and reads your rig's frequency to confirm CAT. For CAT, pick your <em>Rig Model</em> and <em>Serial Port</em>; serial RTS/DTR and VOX need no model.",

  'settings.transmit.backup.label': 'Back up your setup',
  'settings.transmit.backup.action': 'Back up',
  'settings.transmit.backup.title':
    'Save your radios, preferences, memory channels and watchlist to a file',
  'settings.transmit.backup.hint':
    "Your radios, operating preferences, memory channels, watchlist and chase sets in one file — for a new computer, or before a rebuild. <b>It holds no passwords or API keys</b>: those stay in your operating system's keychain, so a restore asks for them again, and the file is safe to keep on a USB stick. Your contact log is separate — export that from the Logbook. Restoring replaces your current setup.",
  // `{{path}}` is where the file landed — a path, printed verbatim.
  'settings.transmit.backup.done': 'Settings backed up → {{path}}',
  'settings.transmit.backup.failed': 'Backup failed',
  'settings.transmit.restore.action': 'Restore…',
  'settings.transmit.restore.title': 'Replace your current setup with a saved backup',


  'settings.transmit.share.label': 'Share this radio with other programs',
  'settings.transmit.share.copy.action': 'Copy',
  'settings.transmit.share.copy.title': 'Copy the address to paste into the other program',
  'settings.transmit.share.hint.on':
    'Nexus itself answers at this address, so <b>VarAC</b>, <b>FreeDV</b>, <b>WSJT-X</b>, <b>JS8Call</b> and <b>fldigi</b> can use the radio while Nexus runs — pick the rig <em>Hamlib NET rigctl</em> (VarAC and FreeDV call it a network or rigctld connection), give it the address above, and leave their serial port blank. They stay connected even while Nexus tests or reconfigures the radio link, and they follow whichever radio is active. Only for this computer — the address is not reachable from the network. Takes effect right away. Both programs can command the rig, so expect them to argue if you tune in both at once.',
  'settings.transmit.share.hint.off':
    'Off — other programs cannot use the radio while Nexus runs. Turn it on and point them at the address that appears here (<em>Hamlib NET rigctl</em>).',
  // `{{address}}` is the per-radio loopback address the panel builds — a wire value.
  'settings.transmit.share.hint.direct':
    'Driving a specific radio that is <em>not</em> the active one: use its direct address <code>{{address}}</code> (per-radio; this link drops briefly whenever Nexus reconfigures that radio).',

  'settings.transmit.foreignPtt.label': 'Other programs may key transmit',
  'settings.transmit.foreignPtt.hint':
    'On: a shared program (WSJT-X, VarAC) can key the rig, exactly as it could when it owned the CAT cable — every Nexus transmit safeguard still applies. Off: shared programs tune and read but never transmit.',

  // ── Settings ▸ Digital (FT8/FT4) ────────────────────────────────────────────────────
  //
  // ⚠️ TWO SUB-GROUPS OF THIS SECTION ARE MISSING FROM THIS FILE ON PURPOSE. "Transmit &
  // Sequencing" and "Auto-CQ & Caller Selection" are the FT-mode TX / timing / QSO-management
  // surface — the T/R period, the TX watchdog, disable-after-73, double-click-arms-TX, the
  // tune timeout, the CQ budget, the blocked-caller list and the best-caller pick. Every label
  // and accessible name in them stays written in `SettingsPanel.tsx` until the transmit-path
  // batch moves them with the stop-line sweeps re-run. What follows is the REST of the
  // section (Logging Behavior, Decoder, Station Housekeeping) and the six weak-signal mode
  // sections under it.
  //
  // ⚠️ EVERY NUMBER BELOW IS AN INVARIANT TECHNICAL QUANTITY and must survive translation
  // exactly as written — the decoder's 200–2900 Hz passband and its 4000 Hz ceiling, the
  // ±25 Hz single-decode window, Hound's 1000 Hz split, ~0.5 s of clock error, every T/R
  // period in seconds, the tone-spacing multipliers (2x … 16x), the WSPR dBm ladder and
  // MSK144's 72 ms frame. So is every NAME an operator matches against another program or
  // puts on the air: the mode names (FT8, FT4, JT65, MSK144, WSPR, FST4, FST4W, Q65,
  // TempoFast/TempoDeep), the A…E submode letters, the Q-codes (QSO, CQ, QRM), the message
  // tokens (RRR, RR73, 73), the redundancy versions RV0/RV1/RV2, and WSJT-X's own option
  // names quoted from its interface. None of them is ever produced by a formatter — they are
  // written here as the prose around them is, and re-pointing or grouping one (a decimal
  // comma, a thousands separator) would be an operating fault, not a cosmetic one.
  'settings.digital.legend': 'Digital (FT8/FT4)',

  // Logging Behavior. "Prompt me to log QSO" is the WSJT-X checkbox this one mirrors, quoted
  // from that program's interface; RRR / RR73 / 73 are the messages themselves.
  'settings.digital.logging.title': 'Logging Behavior',
  'settings.digital.autoLog.label': 'Auto-log QSOs',
  'settings.digital.autoLog.hint': 'Automatically log completed contacts to the ADIF logbook.',
  'settings.digital.promptToLog.label': 'Prompt before logging',
  'settings.digital.promptToLog.hint':
    'Show a confirm-and-edit popup when a QSO completes instead of logging silently (WSJT-X “Prompt me to log QSO”). No effect unless Auto-log is on.',
  'settings.digital.preferRrr.label': 'Roger with RRR (not RR73)',
  'settings.digital.preferRrr.hint':
    'Acknowledge the final report with a bare RRR (partner still owes a 73) instead of the combined RR73. Off = RR73 (modern FT8 practice).',
  'settings.digital.clearDxAfterLog.label': 'Clear DX call after logging',
  'settings.digital.clearDxAfterLog.hint':
    'Wipe the DX Call / DX Grid fields once a contact is logged (WSJT-X option, off by default).',

  // Decoder. The depth chips and the DXpedition chips are a radio group whose accessible name
  // is the field's own label, so each pair reads from ONE entry rather than two identical ones.
  // ⚠️ "Hound" is not here: it is WSJT-X's name for the calling side of a DXpedition QSO, a
  // role token exactly as Fox is, and it stays in the component beside its <option> value.
  'settings.digital.decoder.title': 'Decoder',
  'settings.digital.decodeDepth.label': 'Decode depth',
  'settings.digital.decodeDepth.fast': 'Fast',
  'settings.digital.decodeDepth.normal': 'Normal',
  'settings.digital.decodeDepth.deep': 'Deep',
  'settings.digital.decodeDepth.hint':
    'Deep finds the most signals (WSJT-X default); Fast saves CPU on old hardware. All Decoder settings drive the native decoder — on a WSJT-X UDP source (companion mode) decodes arrive already made and none of them apply.',
  // F low / F high name the two edges of the search range. `F` is the frequency symbol and the
  // Hz beside it is a unit; the word next to each is what moves.
  'settings.digital.passband.label': 'Decoder passband (Hz)',
  'settings.digital.passband.low': 'F low',
  'settings.digital.passband.low.aria': 'Decoder F low (Hz)',
  'settings.digital.passband.high': 'F high',
  'settings.digital.passband.high.aria': 'Decoder F high (Hz)',
  'settings.digital.passband.hint':
    "The decoder's search range. Default 200–2900 Hz. Raise F high toward 4000 Hz to decode stations calling above ~2.9 kHz (common on crowded FT8 bands); lower the range to focus on a narrow filter or dodge strong close-in QRM.",
  'settings.digital.apDecode.label': 'A-priori (AP) decoding — FT8',
  'settings.digital.apDecode.hint':
    'Retry marginal signals against hypotheses built from your call, the DX call and the QSO state (WSJT-X "Enable AP", on by default) — including the cross-cycle replay of last cycle\'s QSOs. FT8 only: FT4\'s AP is part of its Normal/Deep depth and has no separate switch.',
  'settings.digital.apCqOnly.label': 'AP: CQ hypothesis only',
  'settings.digital.apCqOnly.hint':
    'Limit AP to the "CQ" guess — no MyCall/DxCall hypotheses (FT8 and FT4). WSJT-X switches to this by itself after 5 minutes without transmitting, as a guard against stale-context false decodes; here it is your explicit choice. Off = full AP, the stock behavior.',
  'settings.digital.singleDecode.label': 'Single decode',
  'settings.digital.singleDecode.hint':
    'Decode only within ±25 Hz of your green RX marker (the same one-station window WSJT-X uses for a double-click re-decode) instead of the whole passband — isolates one weak station and saves CPU. FT8 and FT4 only: 50 Hz is narrower than a single JT65, Q65 or MSK144 signal, so those modes keep the full passband. Applies while the RX marker sits inside the passband above; off = full passband, the stock behavior.',
  'settings.digital.dxpedition.label': 'DXpedition mode',
  'settings.digital.dxpedition.off': 'Off',
  'settings.digital.dxpedition.hint':
    "Off = normal FT8/FT4 operation. Hound = DXpedition pile-up discipline (calls above 1000 Hz; your report auto-moves to the Fox's frequency).",

  // Station Housekeeping. ⚠️ The beacon toggle is a CONFIGURATION control, not a transmit
  // control — it chooses whether the station announces itself, and it can neither key nor
  // unkey anything. Same reading as Tx Power's drive slider in batch 13; nothing here is on
  // any cockpit's stop-line census. The watts placeholder ("e.g. 100") is a power value and
  // stays in the component, as the example addresses in Rig & CAT do.
  'settings.digital.housekeeping.title': 'Station Housekeeping',
  'settings.digital.journeyStreak.label': 'Journey — track a weekly streak',
  'settings.digital.journeyStreak.hint':
    'Off by default. A gentle “weeks on the air” counter on the Journey board — never a daily streak, never a penalty for a break.',
  'settings.digital.beacon.label': 'Beacon — announce presence (CQ)',
  'settings.digital.beacon.hint':
    "Off = passive (hunt & pounce): Nexus listens and only transmits when you act. On = periodically calls CQ to announce you're on frequency.",
  'settings.digital.harq.label': 'IR-HARQ — combine retransmissions',
  'settings.digital.harq.hint':
    'On (default) = a weak frame that fails is recovered by joint-combining its retransmissions (RV0+RV1+RV2), and unacknowledged QSO overs escalate redundancy. Off = RV0-only (each frame decoded on its own).',
  'settings.digital.clockCheck.label': 'Clock check (NTP)',
  'settings.digital.clockCheck.hint':
    'Periodically check your PC clock against an NTP server and show the offset in the top bar. TempoFast/TempoDeep are slot-timed to UTC — keep it within ~0.5 s (NTP / time.is; off-grid: GPS). Turn off for fully-offline operation (no network calls).',
  'settings.digital.stationPower.label': 'Station power (W)',
  'settings.digital.stationPower.hint':
    'Your transmit power in watts — unlocks the Journey miles-per-watt & QRP feats. Leave blank if unknown.',
  // The units preference. The <option> VALUES ('auto', 'metric', 'imperial') are persisted
  // tokens and stay in the component; km, °C, mi and °F are unit symbols inside these labels.
  'settings.digital.units.label': 'Units',
  'settings.digital.units.auto': 'Automatic (from your system)',
  'settings.digital.units.metric': 'Metric (km, °C)',
  'settings.digital.units.imperial': 'Imperial (mi, °F)',
  'settings.digital.units.hint':
    "Distances, temperature and wind speed. Automatic follows your operating system's region. Applies everywhere in the app immediately.",

  // ── Settings ▸ JT65 ─────────────────────────────────────────────────────────────────
  // ⚠️ `60\u00a0s` is a NON-BREAKING SPACE, written as an escape so it cannot be lost to a
  // careless edit: the period and its unit must not be split across a line. A translation
  // keeps it. A, B and C are the submode names both stations have to agree on.
  'settings.jt65.legend': 'JT65 — classic EME',
  'settings.jt65.submode.label': 'Submode (tone spacing)',
  'settings.jt65.submode.a': 'A — HF standard, narrowest',
  'settings.jt65.submode.b': 'B — 2x spacing',
  'settings.jt65.submode.c': 'C — 4x spacing, most Doppler-tolerant',
  'settings.jt65.submode.hint':
    'JT65 always uses a 60\u00a0s T/R period, so spacing is the only choice. A is what you want on HF; EME operators move up to B or C as Doppler spread on the higher bands smears the tones. Both stations must use the same submode.',
  'settings.jt65.hint':
    'The classic weak-signal and moonbounce mode, decoded and transmitted. Messages are the older 22-character format, not the 37-character one FT8 and friends use.',

  // ── Settings ▸ MSK144 ───────────────────────────────────────────────────────────────
  // The two periods with no prose beside them (10 s, and the plain rows in FST4 below) are
  // pure tokens and stay in the component — there is nothing in them to translate.
  'settings.msk144.legend': 'MSK144 — meteor scatter',
  'settings.msk144.period.label': 'T/R period',
  'settings.msk144.period.fast': '5 s — fast turnaround, big showers',
  'settings.msk144.period.standard': '15 s — the 6 m standard',
  'settings.msk144.period.sparse': '30 s — sparse pings, more to stack',
  'settings.msk144.period.hint':
    'MSK144 sends a 72\u00a0ms message over and over, so a single meteor trail lasting a tenth of a second can carry the whole thing. Shorter periods turn the exchange around faster during a shower; longer ones give the decoder more frames to stack when pings are sparse. Both stations must use the same period.',
  'settings.msk144.hint':
    'MSK144 transmits for nearly the whole period, sending the same 72 ms frame hundreds of times — that is how meteor scatter works, and a contact can take many minutes of apparent silence. The audio frequency is fixed at a 1500 Hz centre; the signal is 1 kHz wide, so there is nowhere to tune it. Shorthand (MSK40) messages are off, matching WSJT-X’s default.',

  // ── Settings ▸ Beacons — WSPR & FST4W ───────────────────────────────────────────────
  // ⚠️ The dBm ladder (23 = 200 mW … 43 = 20 W) is what gets PUBLISHED to a propagation
  // database and read back by other operators, so those numbers are as invariant as a dial
  // reading. Two fields answer in two whole sentences rather than a stem plus a tail: when the
  // rotation is scheduling, "ignored" and "how many stations are in it" are different
  // statements, and which clause leads is the translator's decision.
  'settings.beacons.legend': 'Beacons — WSPR & FST4W',
  'settings.beacons.txPercent.label': 'Transmit %',
  'settings.beacons.txPercent.title':
    'Round Robin is scheduling — this percentage is not used. Set the slot to 0 to schedule by percentage.',
  'settings.beacons.txPercent.hint.roundRobin':
    '<b>Ignored while Round Robin is active</b> — the rotation decides which intervals transmit. Set the slot to 0 to schedule by percentage again.',
  'settings.beacons.txPercent.hint':
    'Fraction of intervals to transmit on. 0 = listen only. A beacon that transmits every interval hears nothing, so a minority is the convention — 20–30% is typical. Below 40% Nexus also avoids back-to-back transmissions while still hitting the rate you asked for.',
  'settings.beacons.power.label': 'Transmit power (dBm)',
  'settings.beacons.power.hint':
    '<b>Required, and it has to be real.</b> WSPR reports are published to a public propagation database that other operators draw conclusions from, so a wrong figure corrupts their data as well as yours. The beacon stays silent until this is set. 23 = 200 mW, 30 = 1 W, 37 = 5 W, 43 = 20 W.',
  'settings.beacons.rrSlot.label': 'FST4W Round Robin slot',
  'settings.beacons.rrSlot.hint':
    '0 = use the transmit-% schedule. Otherwise your slot in a coordinated rotation: stations agreeing on the same slot count and each taking a different slot never transmit at the same time, because the assignment is fixed by UTC. A rotation needs at least 2 slots.',
  'settings.beacons.rrSlots.label': 'Round Robin slots',
  'settings.beacons.rrSlots.hint.degenerate':
    '<b>A one-station rotation is no rotation</b> — Round Robin needs at least 2 slots, so it is off and the transmit-% schedule applies.',
  'settings.beacons.rrSlots.hint': 'How many stations are in the rotation. Ignored when the slot is 0.',
  'settings.beacons.hint':
    'Beacons transmit your callsign, grid and power — there is no QSO sequence, so Call CQ and S&P are inactive on these tiers. Transmit still has to be armed as usual; the schedule never keys a radio whose transmit you have not enabled.',

  // ── Settings ▸ FST4 (QSO) / FST4W (beacon) ──────────────────────────────────────────
  // ⚠️ `<...>` inside the hint is what a HASHED CALLSIGN actually looks like on screen — the
  // literal characters, wrapped by a <code> element the call site supplies. It is not a marker
  // and the renderer cannot read it as one, so it survives translation as the text it is.
  'settings.fst4.legend': 'FST4 (QSO) / FST4W (beacon)',
  'settings.fst4.period.label': 'T/R period',
  'settings.fst4.period.shortestBeacon': '120 s — shortest FST4W beacon interval',
  'settings.fst4.period.deepest': '1800 s — deepest',
  'settings.fst4.period.hint':
    'Shared by both tiers. Longer periods hear weaker signals at fewer exchanges per hour. FST4W beacons run at 120/300/900/1800 s; FST4 QSO work is usually 15–60 s. Both stations (or the beacon you are listening for) must be on the same period.',
  'settings.fst4.hint':
    '<b>FST4</b> is the QSO mode; <b>FST4W</b> is the WSPR-like beacon mode — pick which one on the tier selector. Nexus decodes both and transmits neither. Note that FST4W hashed callsigns show as <code><...></code>: the lookup table upstream fills from a file this build does not carry.',

  // ── Settings ▸ Q65 ──────────────────────────────────────────────────────────────────
  // A…E are the submode names, and the multiplier beside each one is the tone spacing it
  // buys; both stations have to be on the same pair, so both are tokens inside these labels.
  'settings.q65.legend': 'Q65 — EME / VHF+ scatter',
  'settings.q65.period.label': 'T/R period',
  'settings.q65.period.tropo': '15 s — troposcatter',
  'settings.q65.period.meteor': '30 s — 6 m meteor / ionoscatter',
  'settings.q65.period.eme': '60 s — EME (most common)',
  'settings.q65.period.deepEme': '120 s — deep EME',
  'settings.q65.period.microwaveEme': '300 s — deepest, microwave EME',
  'settings.q65.period.hint':
    'Longer periods integrate longer and hear weaker signals, at one exchange per period. Both stations must use the <b>same</b> period. Changing this changes the decode frame length, so it takes effect on the next slot.',
  'settings.q65.submode.label': 'Submode (tone spacing)',
  'settings.q65.submode.a': 'A — narrowest, most sensitive',
  'settings.q65.submode.b': 'B — 2x spacing',
  'settings.q65.submode.c': 'C — 4x spacing',
  'settings.q65.submode.d': 'D — 8x spacing',
  'settings.q65.submode.e': 'E — 16x spacing, most Doppler-tolerant',
  'settings.q65.submode.hint':
    'Wider spacing survives more Doppler spread and frequency drift but costs sensitivity. Move up the letters as the path degrades — EME on the higher bands usually needs B or C.',
  'settings.q65.hint':
    'Q65 transmits and receives. The period and submode set both what you hear and what you send, and BOTH STATIONS MUST MATCH — a correspondent on a different period or submode will not decode you.',

  // ── Settings ▸ Quick-reply macros ───────────────────────────────────────────────────
  // ⚠️ The QSO set's label is NOT here: QSO is a Q-code, the same three letters in every
  // language, so it is a token in the component — exactly as CQ inside "Band / CQ" is a token
  // inside a label whose other word is prose. The macro TEXT itself is the operator's own and
  // was never translatable.
  'settings.quickReply.legend': 'Quick-reply macros',
  'settings.quickReply.chat.label': 'Chat',
  'settings.quickReply.chat.hint': 'Comma-separated chips for Chat.',
  'settings.quickReply.qso.hint': 'Chips for sequenced QSOs.',
  'settings.quickReply.band.label': 'Band / CQ',
  'settings.quickReply.band.hint': 'Open broadcasts — the Call CQ launchpad + band feed.',

  // ════════════════════════════════════════════════════════════════════════════════════
  // Settings ▸ Phone · CW · RTTY · PSK · SSTV · APRS · Working Frequencies
  //
  // ⚠️ SEVEN LEGENDS BELOW ARE MODE NAMES — Phone, CW, RTTY, PSK, SSTV, APRS. A locale keeps
  // every one of them exactly as written; they are entries at all only because the settings
  // registry guard reads a `<legend>` either literally or through this catalog and accepts
  // nothing else (`settings/registry.test.ts`). Same concession `aprs.source.rf.label` makes,
  // and for the same mechanical reason.
  //
  // ⚠️ EVERY NUMBER AND EVERY DEVICE NAME BELOW IS AN INVARIANT TECHNICAL QUANTITY, written
  // inside the entry and listed here so a translator can see the whole set at once: the CW
  // sidetone's 300–1200 Hz range, WinKey's 1200 baud, RTTY's 45.45 / 75 baud and its 170 Hz
  // shift, APRS-IS's port 14580 and 150 km radius, its 43-character comment cap and its
  // 2000-station ceiling, SSTV's 290 seconds of key-down, and the repeater offsets (2 m
  // 600 k, 70 cm 5 M). So is every NAME an operator matches against hardware or another
  // program: the modes and sidebands (SSB, USB, LSB, FM, FT8, PSK31, AFSK, FSK, Baudot), the
  // SSTV mode names (Scottie 1, Martin 1, PD-120), the rig and interface models (IC-756PRO
  // III, FTDX10, K1EL, WinKeyer, Timewave Navigator, microHAM, Buxcomm), the serial control
  // lines (DTR, RTS), ARISS, NWS, and the Tier 2 rotate hostnames. None of them is ever
  // produced by a formatter.
  //
  // ⚠️ WHAT IS NOT WRITTEN HERE, because a decimal comma in one would be an operating fault:
  // the ISS SSTV downlink, the North American APRS channel and every stock working frequency
  // reach these sentences as INTERPOLATED VALUES from the panel, formatted invariantly. So do
  // the COM-port examples, the callsign/SSID and watched-call examples, the default digipeater
  // path, the APRS-IS host and the §97.119 citation — each a named constant in the component.
  // ════════════════════════════════════════════════════════════════════════════════════

  // ── Settings ▸ Phone (SSB / FM) ─────────────────────────────────────────────────────
  // The CTCSS picker's tones are rendered from `CTCSS_TONES`, never from a string here; only
  // its "Off" row is prose.
  'settings.phone.legend': 'Phone (SSB / FM)',
  'settings.phone.mode.title': 'Mode',
  'settings.phone.mode.label': 'Phone mode',
  'settings.phone.mode.ssb': 'SSB (USB/LSB by band)',
  'settings.phone.mode.fm': 'FM (VHF/UHF + repeaters)',
  'settings.phone.mode.hint': 'FM drives the rig to FM + the shift/tone below.',
  'settings.phone.shift.label': 'Repeater shift',
  'settings.phone.shift.simplex': 'Simplex (no shift)',
  'settings.phone.shift.plus': 'Plus (+)',
  'settings.phone.shift.minus': 'Minus (−)',
  'settings.phone.shift.hint': 'Offset is the band standard (2 m 600 k, 70 cm 5 M…).',
  'settings.phone.ctcss.label': 'CTCSS (PL) tone',
  'settings.phone.ctcss.off': 'Off',
  'settings.phone.ctcss.hint': 'Repeater access tone (PL).',
  'settings.phone.mic.title': 'Microphone',
  'settings.phone.mic.hint':
    'Mic gain and voice-keyer message recording are in the Phone cockpit (live CAT + one-touch record).',
  'settings.phone.voiceMic.label': 'Voice mic (recording)',
  'settings.phone.voiceMic.default': 'Same as audio input (default)',
  'settings.phone.voiceMic.hint':
    "Mic used when RECORDING a voice-keyer message. Default records from the audio input device — but on a digital setup that's the rig's RX audio, so you'd record the band, not your voice. Pick your actual mic here. If it can't open, recording falls back to the input device (never silent).",

  // ── Settings ▸ CW ───────────────────────────────────────────────────────────────────
  // ⚠️ `keyer.hint` HAS NO SPACE between "K1EL." and `<b>Soundcard</b>`. That is what the
  // panel has always rendered — the JSX it came from lost the space at a line break — and
  // this phase changes no visible text. Putting it back is a wording change and belongs in
  // its own commit, not smuggled in behind a migration.
  //
  // ⚠️ The macro TOKENS in the two hints below — {MYCALL}, {NAME}, {MYGRID}, {MYSTATE},
  // {RST}, {HISNAME}, {HISSTATE} and the bare `!` — are what the expander matches LITERALLY.
  // They must survive translation character for character, which is the whole reason this
  // catalog interpolates on `{{double}}` braces: single braces would eat every one of them.
  // The F1…F8 key names and the F1→F2→F3→F4 order are tokens too.
  'settings.cw.legend': 'CW',
  'settings.cw.keyer.title': 'Keyer',
  'settings.cw.keyer.label': 'Keyer backend',
  'settings.cw.keyer.cat': 'CAT — the rig keys CW (Hamlib send_morse; newer rigs only)',
  'settings.cw.keyer.serial': "Serial keyline (DTR/RTS) — key the rig's KEY jack",
  'settings.cw.keyer.winkeyer': 'WinKeyer — K1EL hardware keyer',
  'settings.cw.keyer.soundcard': 'Soundcard — audio tone through a data mode (workaround)',
  'settings.cw.keyer.hint':
    "How Nexus sends CW. <b>CAT</b> uses the rig's internal keyer, but older rigs (e.g. IC-756PRO III) don't support it. <b>Serial keyline</b> toggles DTR/RTS into the rig's KEY jack (rig in CW, rig shapes the signal — the clean N1MM/fldigi method, needs only a keying cable). <b>WinKeyer</b> drives a K1EL.<b>Soundcard</b> keys an audio tone — a workaround; set drive so ALC reads zero. <b>It takes your radio out of CW</b> into a data mode (DATA-U/DATA-L, so the tone reaches the transmitter instead of the mic jack); pick any other keyer and CW mode comes straight back. Also switchable live from the CW cockpit.",
  'settings.cw.keyer.unproven':
    "CAT CW keying is <b>unproven on this radio</b>. Its Hamlib backend sends a different keying command from the one other radios use, and it reports success either way — so if nothing is transmitted, Nexus cannot tell you. If CW doesn't go out, use the Serial keyline, WinKeyer, or Soundcard keyer.",
  'settings.cw.pitch.label': 'Sidetone pitch (Hz)',
  'settings.cw.pitch.hint':
    'CW tone pitch (300–1200 Hz) — the soundcard keyer tone and the CW scope zero-beat marker.',
  'settings.cw.winkeyerPort.label': 'WinKeyer port',
  'settings.cw.winkeyerPort.placeholder': '{{example}} — K1EL WinKeyer serial port',
  'settings.cw.winkeyerPort.hint':
    "The serial port your WinKey presents. 1200 baud. A WinKey micro inside a multi-function interface (Timewave Navigator, microHAM) counts — use that device's CW/WinKey port, not its CAT port.",
  'settings.cw.keyPort.label': 'Keyline serial port',
  'settings.cw.keyPort.placeholder': '{{example}} — the keying interface (separate from CAT)',
  'settings.cw.keyPort.hint':
    "The USB-to-serial into your keying interface (Buxcomm, a homebrew DTR cable, …) that plugs into the rig's KEY jack. Must be a SEPARATE port from CAT. Set the rig to CW and its key-jack to straight-key / bug. An interface with a WinKey chip in it — Timewave Navigator, microHAM, a K1EL WinKeyer — does <b>not</b> key on DTR: pick the WinKeyer backend above instead and give it that device's CW port.",
  'settings.cw.keyLine.label': 'Keying line',
  'settings.cw.keyLine.dtr': 'DTR (the CW convention)',
  'settings.cw.keyLine.hint':
    'Which control line keys the rig. DTR is standard (RTS = PTT); flip to RTS if your interface is wired the other way.',
  'settings.cw.idAfter73.label': 'CW ID after 73',
  'settings.cw.idAfter73.hint':
    'Keys your callsign in CW once the final 73 has fully left the air (stock WSJT-X option, default off). Uses the normal CW keying path — PTT + tone — after the FT8 over, never on top of it.',

  // The F-key macro editor. Each key's ROLE is prose (what the key is FOR); F1's role is the
  // Q-code it sends and stays a token in the panel, exactly as the QSO macro set does.
  'settings.cw.macros.title': 'Macros (F-key profiles)',
  'settings.cw.macros.label': 'CW cockpit F-keys',
  'settings.cw.macros.profiles.aria': 'Active CW macro profile',
  'settings.cw.macros.profiles.unnamed': 'Profile {{n}}',
  'settings.cw.macros.profiles.add': 'New',
  'settings.cw.macros.profiles.addPrompt': 'New CW macro profile name:',
  'settings.cw.macros.profiles.rename': 'Rename',
  'settings.cw.macros.profiles.renamePrompt': 'Rename CW macro profile:',
  'settings.cw.macros.profiles.delete': 'Delete',
  'settings.cw.macros.profiles.deleteTitle': 'Delete this profile',
  'settings.cw.macros.profiles.keepOne': 'Keep at least one profile',
  'settings.cw.macros.builtin.hint':
    'Using the built-in F1–F8 set. Customize to make them your own (labels + templates; tokens: {MYCALL} {RST} {NAME} and ! = the worked call).',
  'settings.cw.macros.customize': 'Customize',
  'settings.cw.macros.row.label.aria': '{{key}} label',
  'settings.cw.macros.row.text.aria': '{{key}} text',
  'settings.cw.macros.tokens.hint':
    "Tokens: {MYCALL} {NAME} {MYGRID} {MYSTATE} {RST} · ! = the worked call · {HISNAME} {HISSTATE} = the worked station's QRZ name/state (fill in Settings ▸ Station for {MYSTATE}; the rest auto-fill from the copilot / roster click + QRZ lookup). Each key KEEPS its role — the Guided copilot's next-step highlight follows the role, so customized text still rolls through F1→F2→F3→F4 exactly as before. Keep the ! token wherever you want the other station's call inserted. Save to apply.",
  'settings.cw.macros.reset': 'Reset to defaults',
  'settings.cw.macros.role.answer': 'Answer a station',
  'settings.cw.macros.role.report': 'Send report',
  'settings.cw.macros.role.signOff': 'Sign off (73)',
  'settings.cw.macros.role.myCall': 'My call',
  'settings.cw.macros.role.hisCall': 'His call',
  'settings.cw.macros.role.askRepeat': 'Ask repeat',
  'settings.cw.macros.role.query': 'Query',

  // ── Settings ▸ RTTY ─────────────────────────────────────────────────────────────────
  // The 425 and 850 Hz shifts are rows with nothing in them but a number, so they stay in
  // the panel; only the 170 Hz row and the two baud rows carry prose beside the figure.
  'settings.rtty.legend': 'RTTY',
  'settings.rtty.rxAutoArm.label': 'Start receiving when RTTY opens',
  'settings.rtty.rxAutoArm.hint':
    'The RTTY screen starts the decoder as soon as you open it — tune a signal and the text prints, no setup. Turn this off to arm the receiver by hand (the Arm RX button in the decoded-text pane). This arms the RECEIVER only — transmitting is never armed for you.',
  'settings.rtty.keying.title': 'Keying',
  'settings.rtty.backend.label': 'Keying backend',
  'settings.rtty.backend.afsk': 'AFSK — soundcard tones through the rig in LSB (default)',
  'settings.rtty.backend.fsk': 'True FSK — serial keyline (DTR/RTS), rig in RTTY mode',
  'settings.rtty.backend.hint':
    "How Nexus transmits RTTY. <b>AFSK</b> plays the two-tone waveform through the same TX audio path as FT8 (soundcard-clocked = jitter-free; set drive so ALC reads just zero). <b>True FSK</b> bit-bangs the rig's FSK input over a serial control line with the rig in RTTY mode — unlocking its narrow RTTY filters (e.g. the FTDX10's) — with PTT on CAT or its own line. Software FSK timing is casual/Field-Day grade; AFSK is the timing-cleanest path.",
  'settings.rtty.fskPort.label': 'FSK serial port',
  'settings.rtty.fskPort.placeholder': "{{example}} — e.g. the FTDX10's USB Enhanced COM",
  'settings.rtty.fskPort.hint':
    "The port whose control line feeds the rig's FSK input. Empty = the CAT serial port.",
  'settings.rtty.fskLine.label': 'FSK data line',
  'settings.rtty.fskLine.dtr': 'DTR (the common wiring — RTS stays free for PTT)',
  'settings.rtty.fskLine.hint':
    'Which control line carries the data bits. PTT must ride its OWN path — CAT PTT or the separate PTT line, never this one; Nexus refuses a send if they collide.',
  'settings.rtty.signal.title': 'Signal',
  'settings.rtty.baud.label': 'Baud rate',
  'settings.rtty.baud.hf': '45.45 — the HF standard',
  'settings.rtty.baud.vhf': '75 — VHF / some nets',
  'settings.rtty.baud.hint':
    'Drives the TX bit clock and the RX demodulator (true 45.45, never rounded to 45).',
  'settings.rtty.shift.label': 'Shift (Hz)',
  'settings.rtty.shift.hf': '170 — the HF standard',
  'settings.rtty.shift.hint': 'Mark/space spacing — the TX tone pair and the RX demodulator both.',
  'settings.rtty.reverse.label': 'Reverse (swap mark/space)',
  'settings.rtty.reverse.hint':
    'The convention is LSB with mark on the lower audio tone. Turn this on when deliberately running the opposite sideband (e.g. AFSK in USB/DATA-U) so the on-air sense stays correct — applies to TX and the RX decoder.',

  // ── Settings ▸ PSK ──────────────────────────────────────────────────────────────────
  'settings.psk.legend': 'PSK',
  'settings.psk.rxAutoArm.label': 'Start receiving when PSK opens',
  'settings.psk.rxAutoArm.hint':
    'The PSK screen starts the decoder as soon as you open it — click a trace on the waterfall and the text prints, no setup. Turn this off to arm the receiver by hand (the Arm RX button in the decoded-text pane). Stopping the receiver yourself is already remembered for the rest of the session. This arms the RECEIVER only — transmitting is never armed for you.',

  // ── Settings ▸ SSTV ─────────────────────────────────────────────────────────────────
  // The transmit-mode picker's own rows are built from `SSTV_TX_MODES` — a mode name, its
  // duration and its raster — and are data, not prose. `{{freq}}` is the ISS downlink and
  // `{{rule}}` the FCC citation; both arrive as constants from the panel. The quoted
  // "My picture already shows my callsign" is the SSTV screen's own tick-box, quoted here so
  // the two surfaces read as one.
  'settings.sstv.legend': 'SSTV',
  'settings.sstv.receiving.title': 'Receiving',
  'settings.sstv.rxAutoArm.label': 'Start receiving when SSTV opens',
  'settings.sstv.rxAutoArm.hint':
    'The SSTV screen starts the decoder as soon as you open it, so a picture on the band decodes without arming anything. Turn this off to arm the receiver by hand (the Arm button in the SSTV header). Stopping the receiver yourself is already remembered for the rest of the session.',
  'settings.sstv.issAutoArm.label': 'ISS SSTV auto-arm',
  'settings.sstv.issAutoArm.hint':
    'Auto-arm SSTV for ISS passes — tunes {{freq}} FM and arms the decoder when the ISS is overhead, restores your dial at LOS. Off by default. A pass arm is an explicit act, so it works whether or not the switch above is on.',
  'settings.sstv.transmitting.title': 'Transmitting',
  'settings.sstv.txMode.label': 'Transmit mode',
  'settings.sstv.txMode.auto': 'Automatic — Scottie 1 on HF, PD-120 on 2 m (ARISS)',
  'settings.sstv.txMode.hint':
    'This is the mode the SSTV screen starts on; you can still change it there for one picture. <b>Automatic</b> follows the band: HF gets Scottie 1 (the NA calling-frequency convention — Martin 1 is the EU one), 2 m gets PD-120, which is what ARISS transmits.',
  'settings.sstv.txPower.label': 'Transmit power',
  'settings.sstv.txPower.aria': 'SSTV transmit power percent',
  'settings.sstv.txPower.hint':
    'The drive the SSTV screen starts on, and the level an image is sent at. Leave it blank and Nexus never touches your power. SSTV is up to 290 seconds of continuous key-down at full duty, so most operators run it well below their SSB drive. Your Phone power cap still applies on top of this.',
  'settings.sstv.callsignNote':
    'Your callsign is burned into the top-left of every picture you transmit, and there is no switch for it: an SSTV over is one long carrier of picture-only audio, so the picture is the identification ({{rule}}). Send is refused until you have set a callsign in Settings ▸ Station. If a picture already shows your call — a pre-made QSO card — tick “My picture already shows my callsign” in the SSTV screen: that is per-picture on purpose and resets with every new image.',

  // ── Settings ▸ APRS ─────────────────────────────────────────────────────────────────
  // The channel picker's rows and the beacon symbols come from `aprsBeacon.ts` — a dial
  // reading and a symbol name, both values. The SSID CONVENTIONS below are prose: they are
  // what the community reads each suffix to mean, and the number beside every one of them is
  // the stored value, printed by the panel.
  'settings.aprs.legend': 'APRS',
  'settings.aprs.rf.title': 'Over the air',
  'settings.aprs.channel.label': 'Channel (RF)',
  'settings.aprs.channel.derived': 'Automatic — {{freq}} from your grid',
  'settings.aprs.channel.default': 'Automatic — {{freq}} (set your grid on the Station tab)',
  'settings.aprs.channel.custom': '{{freq}} · custom',
  'settings.aprs.channel.hint':
    'The 2 m FM channel APRS runs on, which is regional. <b>Automatic</b> follows your grid square, so moving to another region lands you on the right channel with nothing to configure — the number it picked is shown above. The boundaries are approximate; pick a channel here to pin it for good.',
  'settings.aprs.symbol.label': 'Beacon symbol',
  'settings.aprs.symbol.hint':
    'The icon other stations see on the map for your beacon. Digipeater and iGate come from the alternate symbol table and are what a fixed station running as infrastructure should show.',
  'settings.aprs.comment.label': 'Beacon comment',
  'settings.aprs.comment.hint':
    'Free text carried with your position — a name, a net, a URL. This goes on the air, and APRS caps it at 43 characters.',
  'settings.aprs.path.label': 'Digipeater path',
  'settings.aprs.path.hint':
    'Which digipeaters may repeat your beacon. <code>{{path}}</code> is the near-universal default — one hop through a local fill-in digi, then one wide hop. Leave it empty to transmit direct, with no digipeaters at all.',
  'settings.aprs.ssid.label': 'Beacon SSID',
  'settings.aprs.ssid.fromCallsign': 'From my callsign',
  'settings.aprs.ssid.hint':
    'The suffix on your callsign in every APRS frame you send, which is how other operators tell your mobile from your home station. <b>From my callsign</b> uses whatever your callsign already spells out — so if you have set it to <code>{{example}}</code> on the Station tab, that is what goes out.',
  'settings.aprs.ssid.fixed': 'fixed station',
  'settings.aprs.ssid.genericSecondary': 'generic / secondary',
  'settings.aprs.ssid.generic': 'generic',
  'settings.aprs.ssid.phone': 'phone / tablet',
  'settings.aprs.ssid.satellite': 'satellite / special',
  'settings.aprs.ssid.handheld': 'handheld',
  'settings.aprs.ssid.boat': 'boat / marine mobile',
  'settings.aprs.ssid.mobile': 'mobile (car)',
  'settings.aprs.ssid.igate': 'iGate / internet',
  'settings.aprs.ssid.balloon': 'balloon / aircraft',
  'settings.aprs.ssid.tracker': 'tracker',
  'settings.aprs.ssid.weather': 'weather station',
  'settings.aprs.ssid.truck': 'truck / freight',

  // The internet feed. ⚠️ `uplink.hint.call` and `uplink.hint.noCall` are TWO WHOLE
  // SENTENCES, not a stem and two tails: "Publishes under KD9TAW" and "Publishes under your
  // callsign" are different statements, and a language that orders them differently cannot
  // be served by gluing a fragment on.
  'settings.aprs.is.title': 'APRS-IS (internet feed)',
  'settings.aprs.is.enabled.label': 'APRS-IS feed',
  'settings.aprs.is.enabled.hint':
    'Plot stations the internet reports alongside the ones your own antenna hears — each one tagged so you can always tell which is which. Runs whether or not the APRS decoder is armed: it uses no radio and never transmits. If internet stations appear while your receiver stays silent, the fault is in the RF chain.',
  'settings.aprs.is.host.label': 'Server',
  'settings.aprs.is.host.hint':
    'Your regional Tier 2 rotate is best — noam / soam / euro / asia / aunz .aprs2.net. <code>{{host}}</code> works anywhere.',
  'settings.aprs.is.port.label': 'Port',
  'settings.aprs.is.port.hint':
    '14580 is the filtered port clients and iGates should use. The full-feed ports would send you the entire planet.',
  'settings.aprs.is.radius.label': 'Radius (km)',
  'settings.aprs.is.radius.hint':
    'How far around your grid square to subscribe. APRS is a local mode; 150 km is a generous 2 m-plus-digipeater horizon. 0 = no distance limit (busy).',
  'settings.aprs.is.watchCalls.label': 'Watched calls',
  'settings.aprs.is.watchCalls.hint':
    'Comma separated. These come through from anywhere on earth, however far outside your radius they are — the club tracker on a road trip, a friend chasing a summit.',
  'settings.aprs.is.weather.label': 'Weather stations',
  'settings.aprs.is.weather.hint': 'Include weather reports in the feed.',
  'settings.aprs.is.objects.label': 'Objects & items',
  'settings.aprs.is.objects.hint':
    'Repeaters, NWS alerts and event markers other stations have placed on the map.',
  'settings.aprs.is.messages.label': 'Messages',
  'settings.aprs.is.messages.hint':
    'Show APRS text messages from the feed. Display only — replying to an internet message is not wired up.',
  'settings.aprs.stationTtl.label': 'Keep stations for (min)',
  'settings.aprs.stationTtl.hint':
    'How long a station stays on the map after its last packet. Stations start to fade at a third of this. An hour by default: fixed stations often beacon only every ten to thirty minutes, and a shorter window makes the slow ones blink off between their own beacons. 0 keeps every station forever (no fade, no removal — the 2000-station ceiling still applies).',
  'settings.aprs.is.uplink.label': 'Receive-only iGate',
  'settings.aprs.is.uplink.hint.call':
    'Contribute packets <b>your own antenna hears</b> to APRS-IS, so stations in your area reach the global map through you. Publishes under <b>{{call}}</b>, so it is a separate choice from watching the feed, and it needs the APRS decoder running to have anything to send. Nexus never sends the other way: gating the internet back onto the air means transmitting unattended.',
  'settings.aprs.is.uplink.hint.noCall':
    'Contribute packets <b>your own antenna hears</b> to APRS-IS, so stations in your area reach the global map through you. Publishes under your callsign, so it is a separate choice from watching the feed, and it needs the APRS decoder running to have anything to send. Nexus never sends the other way: gating the internet back onto the air means transmitting unattended.',

  // ── Settings ▸ Working Frequencies ──────────────────────────────────────────────────
  // ⚠️ NOT ONE FREQUENCY IS WRITTEN HERE. The stock table, the overrides and the "stock is …"
  // tooltip all interpolate a dial reading the panel formats with `toFixed(6)`, and the band
  // and mode pickers offer `FREQ_BANDS` / `FREQ_MODES` verbatim. The three column headings
  // are the WORDS "band" and "mode", which is why they are entries while the values under
  // them are not.
  'settings.workingFrequencies.legend': 'Working Frequencies',
  'settings.workingFrequencies.note':
    'The dial frequency used when a band/mode is selected. These are <b>overrides</b> of the stock WSJT-X working-frequency table — leave the list empty to use stock everywhere. An override replaces the stock row for its band + mode (e.g. to move FT8 to an alternate sub-band).',
  'settings.workingFrequencies.stock.label': 'Standard table (read-only)',
  'settings.workingFrequencies.stock.band': 'Band',
  'settings.workingFrequencies.stock.mode': 'Mode',
  'settings.workingFrequencies.stock.dial': 'Dial (MHz)',
  'settings.workingFrequencies.stock.overrideTitle': 'Your override — stock is {{mhz}} MHz',
  'settings.workingFrequencies.stock.overrideTag': 'override',
  'settings.workingFrequencies.stock.hint':
    'WSJT-X stock dial frequencies. A row with an active override shows your value (highlighted) instead of the stock one.',
  'settings.workingFrequencies.overrides.label': 'Your overrides',
  'settings.workingFrequencies.overrides.none': 'None — the stock table is in effect.',
  'settings.workingFrequencies.overrides.band.aria': 'Override {{n}} band',
  'settings.workingFrequencies.overrides.mode.aria': 'Override {{n}} mode',
  'settings.workingFrequencies.overrides.mhz.aria': 'Override {{n}} dial frequency in MHz',
  'settings.workingFrequencies.overrides.remove.aria': 'Remove the {{band}} {{mode}} override',
  'settings.workingFrequencies.overrides.remove.title': 'Remove this override',
  'settings.workingFrequencies.overrides.duplicate': 'duplicate band + mode — the last row wins',
  'settings.workingFrequencies.overrides.add': 'Add override',
  'settings.workingFrequencies.overrides.reset': 'Reset to standard',
  'settings.workingFrequencies.overrides.hint':
    'MHz is the dial (suppressed-carrier) frequency. Save to apply — band switches then use your value for that band + mode.',

  // ── Settings ▸ Logging & Connectors ▸ Connections ───────────────────────────────────
  // The connector health grid and its event log. ⚠️ NOT here, and deliberately: the state
  // word beside each dot and the "failed 10m ago …" line come from `settings/connHealth.ts`,
  // the connector NAME and identity are data, and every event message in the log is written
  // by the connector that raised it. They are values this surface renders, not its prose.
  // `ACTION=STATUS` is the QRZ API's own parameter and `logbook.qrz.com ▸ Settings ▸ API` is
  // a path through QRZ's website — both stay exactly as written in every language.
  'settings.connections.legend': 'Connections',
  'settings.connections.test.busy': 'Testing…',
  'settings.connections.qrz.test.action': 'Test',
  'settings.connections.qrz.test.title':
    'Round-trips the QRZ Logbook API (ACTION=STATUS) — proves the key works without logging anything',
  // ⚠️ `{{detail}}` is the round trip's own answer — inserted verbatim, never translated. Two
  // whole messages rather than a shared "✓/✗" stem plus a tail: the failure carries a second
  // sentence the success has no use for, and where it belongs is the translator's decision.
  'settings.connections.qrz.test.ok': '✓ QRZ Logbook reachable: {{detail}}',
  'settings.connections.qrz.test.fail':
    '✗ QRZ test failed: {{detail}} (Uploads need the per-logbook <b>API key</b> from logbook.qrz.com ▸ Settings ▸ API — not your QRZ password.)',
  'settings.connections.log.title': 'Connection log',
  'settings.connections.log.hint': 'every save, sync, push, and failure lands here',
  'settings.connections.log.empty':
    'No events yet this session — save a credential or run a sync and it shows here.',

  // ── Settings ▸ Logging & Connectors ▸ Worked-before (B4) & dupes ────────────────────
  // ⚠️ B4 is the hobby's own shorthand for "worked before" and Dupe is what the badge is
  // called; band names (40m), mode names (FT8, phone) and the program name WSJT-X are
  // invariant. The legend holds a real `&` — the JSX wrote `&amp;`, but React escapes a text
  // child itself, so an entity here would render as the literal characters.
  'settings.b4.legend': 'Worked-before (B4) & dupes',
  'settings.b4.matchMode.label': 'Match mode too',
  'settings.b4.matchMode.hint':
    'Off (the default, and WSJT-X’s): working a station on 40m marks them B4-on-band for 40m in every mode, and the log strip’s Dupe badge counts any mode on the band. On: 40m FT8 and 40m phone are separate contacts — the solid B4 chip and the Dupe badge require the mode to match as well. The hollow B4 chip (worked anywhere) is unaffected either way.',

  // ── Settings ▸ Logging & Connectors ▸ Integrations & Feeds ─────────────────────────
  // ⚠️ Everything this section names on the wire is invariant and stays in the panel: the
  // UDP addresses and ports it offers as field examples, the file names (ALL.TXT, .wav) and
  // the program names (WSJT-X, JTDX, JTAlert, GridTracker, HRD, OpenHamClock) inside these
  // sentences. Two labels are invariant as WHOLE strings and are still in the panel because
  // they are nothing but names — `WSJT-X UDP API` and `PSK Reporter`, the same rule that
  // leaves the Phone/CW/Digital tab labels literal. So are the cluster presets, which name
  // real nodes.
  'settings.integrations.legend': 'Integrations & Feeds',
  'settings.integrations.local.title': 'Local APIs & Loggers',
  'settings.integrations.wsjtxUdp.hint': 'for JTAlert / GridTracker / loggers',
  'settings.integrations.udpAddr.label': 'UDP Address',
  'settings.integrations.udpAddr.hint': 'host:port for the UDP feed',
  'settings.integrations.hrdLogging.label': 'Ham Radio Deluxe logging',
  'settings.integrations.hrdLogging.hint':
    "push each QSO to HRD Logbook over its QSO-Forwarding UDP port (HRD must be running; don't also run JTAlert/QSO Relay into HRD or you'll double-log)",
  'settings.integrations.hrdAddr.label': 'HRD UDP Address',
  'settings.integrations.hrdAddr.hint':
    'HRD QSO-Forwarding host:port (default 127.0.0.1:2333)',
  'settings.integrations.hrd.linkUp': '● HRD reachable — contacts are forwarding',
  // ⚠️ `contact(s)` is left exactly as it shipped. The plural path would render "1 contact"
  // where this renders "1 contact(s)", and changing the English is a wording decision, not a
  // migration. `{{count}}` is a queue depth the panel formats invariantly.
  'settings.integrations.hrd.linkDown':
    '○ HRD not reachable — {{count}} contact(s) queued, will send when HRD is back',
  'settings.integrations.companionAddr.label': 'Companion UDP address',
  'settings.integrations.companionAddr.hint':
    'Where Nexus listens for WSJT-X/JTDX in Companion source mode.',
  'settings.integrations.allTxt.label': 'Write ALL.TXT decode log',
  'settings.integrations.allTxt.hint':
    'WSJT-X-format decode log for GridTracker / loggers to tail. Written only while this is on, and it first appears after the next decode.',
  // A whole extra sentence, not a tail glued onto the hint above — it appears only once the
  // file exists, and a translator may place it wherever their language wants it. `{{path}}`
  // is a file system path: verbatim, always.
  'settings.integrations.allTxt.path': 'Saved at <code>{{path}}</code>.',
  'settings.integrations.allTxt.reveal': 'Reveal in folder',
  'settings.integrations.qsoWav.label': 'Save a WAV per logged QSO',
  'settings.integrations.qsoWav.hint': 'Auto-records the last ~60 s of RX audio on log.',
  'settings.integrations.qsoWav.path':
    'Saved in <code>{{path}}</code>, created the first time you record.',
  'settings.integrations.qsoWav.reveal': 'Open recordings folder',
  // The <option> VALUES ('none', 'decodes', 'all') are persisted tokens and stay in the panel.
  'settings.integrations.saveWav.label': 'Save received audio (.wav per period)',
  'settings.integrations.saveWav.none': 'None (default)',
  'settings.integrations.saveWav.decodes': 'Save periods with decodes',
  'settings.integrations.saveWav.all': 'Save all periods',
  'settings.integrations.saveWav.hint':
    'WAVs land in recordings/periods (12 kHz mono, ~360 KB each). "All" writes ~2 GB/day of continuous monitoring — use for decoder debugging, not always-on.',
  'settings.integrations.spotSources.title': 'Spot Sources',
  'settings.integrations.pskreporter.hint': 'upload spots to the global map',
  'settings.integrations.clusterSpots.label': 'DX Cluster / RBN spots',
  'settings.integrations.clusterSpots.hint':
    'Surface "new ones" from the Reverse Beacon Network on the Needed board + Connect. Takes effect on restart.',
  'settings.integrations.clusterNodes.label': 'Phone/SSB cluster nodes',
  'settings.integrations.clusterNodes.empty':
    'No nodes — add one below to get SSB/phone needs (RBN only carries CW + digital).',
  'settings.integrations.clusterNodes.remove.title': 'Remove this cluster node',
  // ⚠️ `{{host}}` is a node address. Two whole accessible names rather than one with a
  // "node" fragment substituted in: a fragment cannot be declined, and half the languages
  // this will be read in decline it.
  'settings.integrations.clusterNodes.remove.aria': 'Remove {{host}}',
  'settings.integrations.clusterNodes.remove.ariaBlank': 'Remove node',
  'settings.integrations.clusterNodes.add.option': '+ Add a known node…',
  'settings.integrations.clusterNodes.addCustom.title': 'Add a custom node row',
  'settings.integrations.clusterNodes.addCustom.action': '+ Custom',
  'settings.integrations.clusterNodes.hint':
    'We connect to ALL listed nodes and union their human SSB/phone spots — more nodes = wider phone coverage (RBN CW + digital connect automatically; RBN endpoints are ignored here). An added node connects on the next Save; removing one takes effect on restart.',
  'settings.integrations.propagation.title': 'Propagation',
  'settings.integrations.openingWatch.label': 'Near-region opening watch',
  'settings.integrations.openingWatch.hint':
    'Watch VHF/10 m activity near your QTH (not just your own contacts) so openings flag "open around you" before you\'ve worked anyone. Takes effect on restart.',
  // ⚠️ `ITU-R P.533` is the recommendation's number — the same token the propagation panes
  // keep as a constant (ENGINE_P533). The <option> values ('heuristic', 'p533') are persisted
  // and stay in the panel.
  'settings.integrations.propEngine.label': 'Prediction engine',
  'settings.integrations.propEngine.heuristic': 'Modelled (fast heuristic)',
  'settings.integrations.propEngine.p533': 'ITU-R P.533 (full physics)',
  'settings.integrations.propEngine.hint':
    'Drives the per-station path outlook + 24h band×hour grid. P.533 is the real circuit-reliability method (validated against the ITU reference; ~0.1 s per prediction, uses your station power). Live spots always win over any model.',
  // The collapsed disclosure inside Integrations & Feeds. ⚠️ dBi, TX and RX are units and the
  // radio's own two states.
  'settings.antennaGain.title': 'Antenna gain (advanced)',
  'settings.antennaGain.label': 'Antenna gain (dBi) — TX / RX',
  'settings.antennaGain.tx.aria': 'TX antenna gain (dBi)',
  'settings.antennaGain.rx.aria': 'RX antenna gain (dBi)',
  'settings.antennaGain.hint':
    'Used by the P.533 link budget only. 0 = a simple wire/vertical (isotropic); a 3-element yagi ≈ 6–8. Honest v1: a plain dB shift — no pattern or takeoff-angle modelling, and the fast heuristic ignores it.',

  // ── Settings ▸ Logging & Connectors ▸ DXKeeper ─────────────────────────────────────
  // ⚠️ The legend (`DXKeeper (DXLab Suite)`) and the `DXLab Base Port` label are still in the
  // panel, and for two different reasons. The legend is nothing but product names. The label
  // NAMES A CONTROL IN ANOTHER APPLICATION'S ENGLISH INTERFACE — the hint below it sends the
  // operator to DXKeeper's own Network Service panel to read the number off a field called
  // "Base Port", and a translated label sends them looking for a field that is not there.
  // The quoted `Base Port`, `Auto upload` and `QSL Configuration` inside these sentences are
  // the same thing and stay verbatim.
  'settings.dxkeeper.note':
    'Pushes each logged QSO into <b>DXKeeper</b> over its TCP Network Service. Enable it in DXKeeper under <em>Configuration ▸ Defaults ▸ Network Service</em> first.',
  'settings.dxkeeper.host.label': 'DXKeeper host',
  // `{{example}}` is an IP address the panel supplies as an invariant token (LOGGER_EXAMPLES).
  'settings.dxkeeper.host.placeholder': '{{example}} (empty = off)',
  'settings.dxkeeper.host.hint': 'Usually 127.0.0.1 — same PC. Leave blank to disable.',
  // `{{port}}` is base + 1, computed and interpolated invariantly.
  'settings.dxkeeper.basePort.hint':
    "The <em>Base Port</em> from DXKeeper's Network Service panel (default 52000). DXKeeper itself listens on <b>{{port}}</b> — Nexus adds the 1 for you.",
  'settings.dxkeeper.uploads.label': 'Let DXKeeper do the uploads',
  'settings.dxkeeper.uploads.hint':
    'Off by default: Nexus already uploads to LoTW / eQSL / ClubLog / QRZ, so turning this on would upload every QSO twice. Note DXKeeper ignores this for Club Log and QRZ if <em>Auto upload</em> is ticked on its own QSL Configuration tab — untick it there.',

  // ── Settings ▸ Logging & Connectors ▸ N3FJP ────────────────────────────────────────
  // ⚠️ `N3FJP`, `ACLog`, `Field Day Contest Log` and `Network Status Display` are the club
  // logger's own names; `ENTER` is the key its scoring path is named after and `ADDDIRECT` is
  // the API command that replaces it. All of them are wire-level or product names and stay as
  // written. The host/port EXAMPLES are invariant tokens in the panel.
  'settings.n3fjp.legend': 'N3FJP Integration (club master log)',
  'settings.n3fjp.note':
    "Each FD contact lands in the club's <b>N3FJP Field Day Contest Log</b> the moment you log it — so the whole club's score updates in real time. Run N3FJP on the master computer; point Nexus at its IP + port (default 1100).",
  'settings.n3fjp.host.label': 'N3FJP host',
  'settings.n3fjp.host.placeholder': '{{example}} (empty = off)',
  'settings.n3fjp.host.hint':
    'IP or hostname of the master log computer. Leave blank to disable.',
  'settings.n3fjp.port.label': 'N3FJP port',
  'settings.n3fjp.port.hint': "N3FJP's API TCP port (default 1100).",
  'settings.n3fjp.useEnter.label': 'Use ENTER for Field Day scoring',
  'settings.n3fjp.useEnter.hint':
    "Log each FD contact with N3FJP's <b>ENTER</b> sequence, which scores the contest — the correct path. Turn off to fall back to a plain <code>ADDDIRECT</code> insert (may not score). On by default.",
  'settings.n3fjp.reportBand.label': 'Report my band to N3FJP',
  'settings.n3fjp.reportBand.hint':
    "Tell N3FJP which band you're on (no CAT needed), so the club's Network Status Display band board shows this position. Off by default.",
  'settings.n3fjp.forwardAll.label': 'Forward every QSO',
  'settings.n3fjp.forwardAll.hint':
    "Also push <b>every</b> logged QSO (not just Field Day) to N3FJP ACLog on the host above — everyday general logging. N3FJP dedupes, so it's safe to run alongside the Field-Day push.",
  'settings.n3fjp.test.label': 'Connection test',
  'settings.n3fjp.test.title': 'Save settings, then test the N3FJP TCP connection',
  'settings.n3fjp.test.action': 'Test N3FJP',
  'settings.n3fjp.test.hint':
    'Run this at the club site before the event starts to confirm the API link works.',

  // ── Settings ▸ Logging & Connectors ▸ N1MM+ ────────────────────────────────────────
  // ⚠️ The port number 12060 is the contest logger's own broadcast port and stays in the
  // sentence that names it. The address the toggle fills in for a blank field is a VALUE that
  // is written to settings, not an example, and never came near this file.
  'settings.n1mm.legend': 'N1MM+ Integration',
  'settings.n1mm.addr.label': 'N1MM contact broadcast address',
  'settings.n1mm.addr.placeholder': '{{example}} (empty = off)',
  'settings.n1mm.addr.hint':
    'Where the N1MM contact packets go (host:port, UDP). Name the port — consumers stack on one host, and 12060 is often already taken by another logger. Leave blank to disable.',
  // The state sentence that follows the hint — one whole sentence per state, because what it
  // says is a different answer, not a different ending. The second one names the toggle below
  // it: reword the toggle's label and this sentence moves with it.
  'settings.n1mm.addr.sending': 'Sending for every logged QSO.',
  'settings.n1mm.addr.idle':
    'An address alone sends nothing outside a Field Day event — turn on Broadcast every QSO below for everyday logging.',
  'settings.n1mm.broadcastAll.label': 'Broadcast every QSO',
  'settings.n1mm.broadcastAll.hint':
    'Send the contact packet for <b>every</b> logged QSO, not just Field Day — point OpenHamClock or GridTracker at the address above and each contact plots on its map as you log it. One packet per QSO: this never doubles up with the Field Day broadcast, so it is safe to leave on through an event. Off by default; with it off, packets go out <em>only</em> while a Field Day event is running.',

  // ── Settings ▸ Logging & Connectors ▸ LoTW users list ──────────────────────────────
  // ⚠️ `{{count}}` arrives ALREADY FORMATTED by the panel (the grouping is that call site's,
  // not this file's — `t()` has no locale-aware formatter and never will), `{{date}}` is an
  // ISO day stamp, `{{detail}}` is a fetch error, and `L` is the mark a decode line carries.
  // LoTW, ARRL and WSJT-X are names.
  'settings.lotwUsers.legend': 'LoTW users list',
  'settings.lotwUsers.fetch.action': 'Fetch now',
  'settings.lotwUsers.fetch.busy': 'Fetching…',
  'settings.lotwUsers.fetch.done': 'LoTW list loaded — {{count}} calls',
  'settings.lotwUsers.fetch.failed': 'LoTW list fetch failed: {{detail}}',
  'settings.lotwUsers.status': '{{count}} calls · fetched {{date}}',
  'settings.lotwUsers.empty':
    'Not fetched yet — decode lists gain an L mark on calls that upload to LoTW.',
  'settings.lotwUsers.maxAge.label': 'Count as a LoTW user if uploaded within (days)',
  'settings.lotwUsers.maxAge.hint':
    'ARRL\'s activity list updates weekly — refetching more often just returns "unchanged". Manual fetch by design (WSJT-X convention).',

  // ── Settings ▸ Logging & Connectors ▸ Callsign → state database ────────────────────
  // ⚠️ `Callsign→state` names the index itself and keeps its arrow; `New State` is the need
  // category the Needed board lights up; FCC is the licensing authority and hamradiotools.io
  // is where the file comes from. `{{count}}` is formatted by the panel, as above.
  'settings.callsignState.legend': 'Callsign → state database',
  'settings.callsignState.update.action': 'Update now',
  'settings.callsignState.update.busy': 'Updating…',
  'settings.callsignState.update.done':
    'Callsign→state database updated — {{count}} US calls',
  'settings.callsignState.update.failed': 'Callsign→state update failed: {{detail}}',
  'settings.callsignState.status': '{{count}} US calls · fetched {{date}}',
  'settings.callsignState.empty':
    'Not loaded yet — downloads on first launch, then auto-refreshes weekly.',
  'settings.callsignState.hint':
    'A callsign→state index (from the FCC license file) so a New State lights up on cluster / CW / SSB spots that carry no grid. Refreshed weekly from hamradiotools.io; a live decode grid refines it for rovers.',

  // ── Settings ▸ Logging & Connectors ▸ Country file (DXCC) ─────────────────────────
  // ⚠️ `AD1C` (the file's maintainer), `cty.dat` and `DXCC` are invariant tokens in every
  // catalog. `{{ver}}` is an AD1C release date the panel formats as YYYY-MM-DD; the resolver
  // is set once at launch, so every "applies at next launch" is literal, not caution.
  'settings.countryFile.legend': 'Country file (DXCC)',
  'settings.countryFile.update.action': 'Update country file',
  'settings.countryFile.update.busy': 'Updating…',
  'settings.countryFile.update.done':
    'Country file downloaded — AD1C {{ver}} applies at next launch.',
  'settings.countryFile.update.current': 'Country file is already current — AD1C {{ver}}.',
  'settings.countryFile.update.failed': 'Country file update failed: {{detail}}',
  'settings.countryFile.status': '{{count}} entities · AD1C {{ver}} · fetched {{date}}',
  'settings.countryFile.statusBuiltIn': '{{count}} entities · AD1C {{ver}} (built-in)',
  'settings.countryFile.empty': 'Built-in country file active.',
  'settings.countryFile.pending':
    'Update downloaded (AD1C {{ver}}) — applies at next launch.',
  'settings.countryFile.hint':
    'The AD1C cty.dat country file maps callsigns to DXCC entities — the country on decode rows, the Needed board and the log. Checked weekly; a downloaded update applies at the next launch.',

  // ── Settings ▸ Logging & Connectors ▸ Confirmations ────────────────────────────────
  // The QSL services, one featgroup each. Everything below is a LABEL or a HINT: no key,
  // password, token or upload code is read, written or interpolated by any entry here, and the
  // Set/Forget buttons these words sit on call the same handlers they always did.
  //
  // ⚠️ WHAT STAYED IN THE PANEL, and it is nearly all names. The featgroup titles (`LoTW`,
  // `eQSL`, `QRZ`, `HamQTH`, `ClubLog`, `HRDLog`, `RepeaterBook`, `Cloudlog / Wavelog`) are the
  // services' own names and nothing else, so they never came here — a translated service name
  // names no service. The token-shaped placeholders (`rbuapp_…`, the example Cloudlog URL, the
  // profile id `1`) are invariant values in `CONFIRMATION_EXAMPLES`.
  //
  // ⚠️ WHAT IS VERBATIM INSIDE THESE SENTENCES, for the same reason the DXKeeper hints keep
  // `Base Port`: each one NAMES SOMETHING THE OPERATOR MUST FIND SOMEWHERE ELSE, spelled the way
  // that other place spells it. `TQSL` and its `Station Location`, its `-l` argument and the
  // `"use the location in the ADIF file"` setting; `STATION_CALLSIGN` / `MY_GRIDSQUARE`, which
  // are ADIF field names; ClubLog's `Application Password` and `App Passwords` page;
  // RepeaterBook's `API Apps` page; HRDLog's `Options`; Cloudlog's `Account ▸ API Keys` and
  // `Station Locations`; the host names (`eQSL.cc`, `QRZ.com`, `HamQTH.com`, `HRDLog.net`,
  // `hearham.com`, `clublog.org/requestapikey.php`); `DXCC`, `WAS` and `ARRL`, which name award
  // programmes and the authority that grants them; and `Upload to LoTW (N)`, whose `(N)` is the
  // Logbook's own key. `{{when}}` arrives ALREADY FORMATTED from the panel — the same rule the
  // LoTW-users `{{count}}` follows.
  'settings.confirmations.legend': 'Confirmations',

  // The Set/Forget pair sits under seven credentials with the same two words and, four times
  // over, the same tooltip. One entry each: it is one control, said once.
  'settings.confirmations.credential.set.action': 'Set',
  'settings.confirmations.credential.forget.action': 'Forget',
  'settings.confirmations.credential.forget.title':
    'Remove the stored password from the system keychain',

  'settings.confirmations.lotw.username.label': 'LoTW username',
  'settings.confirmations.lotw.username.placeholder': 'your LoTW account login',
  'settings.confirmations.lotw.username.hint':
    'Often your callsign, but not always — use your LoTW account login. Save settings to apply.',
  'settings.confirmations.lotw.password.label': 'LoTW password',
  'settings.confirmations.lotw.password.placeholder': 'LoTW website password',
  // Names the Set button beside it: reword that and this sentence moves with it.
  'settings.confirmations.lotw.password.hint':
    'Your LoTW <b>website</b> password (not your TQSL certificate password). Stored in the OS keychain, never on disk; not shown again after you click Set.',
  'settings.confirmations.lotw.sync.label': 'LoTW confirmations',
  'settings.confirmations.lotw.sync.action': 'Download confirmations',
  'settings.confirmations.lotw.sync.busy': 'Downloading…',
  'settings.confirmations.lotw.sync.hint':
    'This only pulls confirmations <b>down</b>. To send your contacts <em>to</em> LoTW, use <b>Upload to LoTW (N)</b> in the Logbook. Pulls new confirmations into your log and marks which of your uploads LoTW now holds on file (so they read “waiting on the other op,” not “never uploaded”). The first pull covers your whole history (can be slow); later ones are incremental.',
  'settings.confirmations.lotw.stationLocation.label': 'LoTW Station Location',
  'settings.confirmations.lotw.stationLocation.placeholder': 'exact TQSL Station Location name',
  'settings.confirmations.lotw.stationLocation.hint':
    'For <b>uploading</b> to LoTW (the "Upload to LoTW" button in the Logbook). Signing is done by your installed <b>TQSL</b> against this named Station Location — set it up in TQSL first; the name must match exactly. No certificate or password is stored by Nexus.',
  'settings.confirmations.lotw.adifLocation.label': 'Sign from ADIF location (travelers)',
  'settings.confirmations.lotw.adifLocation.aria': 'Sign LoTW uploads from the ADIF location',
  'settings.confirmations.lotw.adifLocation.hint':
    'Turn on if you set TQSL to <em>"use the location in the ADIF file"</em> and don\'t create named Station Locations (handy if you travel). Nexus then stamps your call + grid (STATION_CALLSIGN / MY_GRIDSQUARE) into the upload and omits the <code>-l</code> argument, so TQSL signs from those and the Station Location above isn\'t required. <b>The whole batch is signed from your current grid above</b>, so if you operate from more than one location, upload <em>before</em> you move — otherwise earlier contacts are signed with the new grid.',
  'settings.confirmations.lotw.tqslPath.label': 'TQSL path (optional)',
  'settings.confirmations.lotw.tqslPath.placeholder': 'auto-detect (leave blank)',
  'settings.confirmations.lotw.tqslPath.hint':
    'Only if TQSL is installed somewhere non-standard; otherwise leave blank to auto-detect.',
  'settings.confirmations.lotw.autoUpload.label': 'Upload to LoTW automatically',
  'settings.confirmations.lotw.autoUpload.hint':
    "Every few hours, Nexus hands your un-uploaded contacts to TQSL in one batch and TQSL signs and sends them — the same thing the Logbook's <b>Upload to LoTW</b> button does, on a timer. Needs TQSL installed and the Station Location above. If a batch is refused, this stops and waits for you rather than retrying; save any LoTW setting to start it again.",
  // A whole sentence, not a clause: it says why the switch above is dead, and it is only ever
  // shown while it is. Same for the run stamp — one sentence per state, never a glued ending.
  'settings.confirmations.lotw.autoUpload.blocked':
    'Unavailable while “Sign from ADIF location” is on: an unattended batch would sign older contacts with wherever you are NOW.',
  'settings.confirmations.lotw.autoUpload.lastRun': 'Last run: {{when}}.',

  'settings.confirmations.eqsl.username.label': 'eQSL username',
  'settings.confirmations.eqsl.username.placeholder': 'your eQSL.cc account login',
  'settings.confirmations.eqsl.username.hint':
    'Your eQSL.cc login (often your callsign). Save settings to apply.',
  'settings.confirmations.eqsl.password.label': 'eQSL password',
  'settings.confirmations.eqsl.password.placeholder': 'eQSL.cc account password',
  'settings.confirmations.eqsl.password.hint':
    'Stored in the OS keychain, never on disk; not shown again after you click Set.',
  'settings.confirmations.eqsl.sync.label': 'eQSL confirmations',
  'settings.confirmations.eqsl.sync.action': 'Sync eQSL now',
  'settings.confirmations.eqsl.sync.busy': 'Syncing…',
  'settings.confirmations.eqsl.sync.hint':
    "Download eQSL confirmations into your log. These count as confirmations but <b>not</b> for DXCC/WAS (ARRL doesn't accept eQSL) — a separate tier.",
  'settings.confirmations.eqsl.upload.label': 'Auto-upload QSOs to eQSL',
  'settings.confirmations.eqsl.upload.hint':
    'Upload each logged QSO to eQSL.cc as you log it (needs the eQSL username + password above).',

  'settings.confirmations.qrz.username.label': 'QRZ username',
  'settings.confirmations.qrz.username.placeholder': 'your QRZ.com account login',
  'settings.confirmations.qrz.username.hint':
    "Used to look up a callsign's name + grid when logging. Save settings to apply.",
  'settings.confirmations.qrz.password.label': 'QRZ password',
  'settings.confirmations.qrz.password.placeholder': 'QRZ.com account password',
  'settings.confirmations.qrz.password.hint':
    'Your QRZ.com login password — <b>this is what powers callbook lookups</b> (name, QTH, grid), and it is separate from the Logbook API key below (that key only uploads QSOs). Stored in the OS keychain, never on disk. Grid & state need a QRZ XML subscription; free accounts return only name/address/country.',
  'settings.confirmations.qrz.apiKey.label': 'QRZ Logbook API key',
  'settings.confirmations.qrz.apiKey.placeholder': 'from your QRZ logbook settings page',
  'settings.confirmations.qrz.apiKey.forget.title':
    'Remove the stored Logbook key from the system keychain',
  'settings.confirmations.qrz.apiKey.hint':
    "A <b>separate</b> key (not the login password) from your QRZ logbook's settings page — used to upload logged QSOs.",
  'settings.confirmations.qrz.upload.label': 'Auto-upload QSOs to QRZ',
  'settings.confirmations.qrz.upload.hint':
    'Push each logged QSO to your QRZ logbook (needs the Logbook API key above).',
  'settings.confirmations.qrz.autoSync.label': 'Pull confirmations automatically',
  'settings.confirmations.qrz.autoSync.hint':
    'As people confirm on QRZ, the confirmations flow in on their own — no need to press Sync. After the first run only what CHANGED is fetched. QRZ confirmations show as confirmed but never count toward DXCC or WAS, which need LoTW or a card.',
  'settings.confirmations.qrz.autoSync.lastPull': 'Last pull: {{when}}.',
  'settings.confirmations.qrz.sync.label': 'Two-way sync',
  'settings.confirmations.qrz.sync.title':
    'FETCH your online QRZ logbook and merge it in — pulls QSOs you logged elsewhere plus their confirmations',
  'settings.confirmations.qrz.sync.action': 'Sync from QRZ now',
  'settings.confirmations.qrz.sync.busy': 'Syncing…',
  'settings.confirmations.qrz.sync.hint':
    'Pull your QRZ logbook <b>down</b> — adds QSOs you logged elsewhere (e.g. a phone app in the field) and marks QRZ-confirmed contacts. QRZ confirmations count as confirmations but <b>not</b> for DXCC/WAS. Safe to run repeatedly (deduped). Needs the Logbook API key above.',

  'settings.confirmations.hamqth.username.label': 'HamQTH username',
  'settings.confirmations.hamqth.username.placeholder': 'your HamQTH.com account login',
  'settings.confirmations.hamqth.username.hint':
    "A <b>free</b> callbook used as a fallback when QRZ isn't configured or has no match — a HamQTH account returns name, grid & US state at no charge. Save settings to apply.",
  'settings.confirmations.hamqth.password.label': 'HamQTH password',
  'settings.confirmations.hamqth.password.placeholder': 'HamQTH.com account password',
  'settings.confirmations.hamqth.password.hint': 'Stored in the OS keychain, never on disk.',

  'settings.confirmations.clublog.email.label': 'ClubLog email',
  'settings.confirmations.clublog.email.placeholder':
    'your ClubLog account email (not a callsign)',
  'settings.confirmations.clublog.email.hint':
    'Your ClubLog login email. Save settings to apply.',
  'settings.confirmations.clublog.callsign.label': 'ClubLog callsign',
  'settings.confirmations.clublog.callsign.placeholder': 'defaults to your callsign',
  'settings.confirmations.clublog.callsign.hint':
    'The ClubLog logbook to upload into (empty = your callsign).',
  'settings.confirmations.clublog.password.label': 'ClubLog app-password',
  'settings.confirmations.clublog.password.placeholder': 'a ClubLog Application Password',
  'settings.confirmations.clublog.password.forget.title':
    'Remove the stored ClubLog password from the system keychain',
  'settings.confirmations.clublog.password.hint':
    'Use a ClubLog <b>Application Password</b> (Settings → App Passwords), not your main password. Stored in the OS keychain.',
  'settings.confirmations.clublog.apiKey.label': 'ClubLog API key (application-level)',
  'settings.confirmations.clublog.apiKey.placeholder':
    'blank = use the key bundled with this build (if any)',
  'settings.confirmations.clublog.apiKey.hint':
    "This is the <b>application</b> credential, not yours — official installer builds bundle one, and you only need email + app-password above. Building from source? Request a free key at clublog.org/requestapikey.php and paste it here (open-source can't ship one — ClubLog auto-revokes published keys).",
  'settings.confirmations.clublog.upload.label': 'Auto-upload QSOs to ClubLog',
  'settings.confirmations.clublog.upload.hint':
    'Push each logged QSO to ClubLog in real time (needs the email + app-password above; official builds bundle the API key).',

  'settings.confirmations.hrdlog.code.label': 'HRDLog.net upload code',
  'settings.confirmations.hrdlog.code.placeholder': 'your hrdlog.net upload code',
  'settings.confirmations.hrdlog.code.forget.title':
    'Remove the stored HRDLog.net code from the system keychain',
  'settings.confirmations.hrdlog.code.hint':
    'The upload code from your HRDLog.net account (Options → your code). Uploads log under your station callsign. Stored in the OS keychain. This is the online HRDLog.net service — separate from the HRD Logbook UDP push under Logging.',
  'settings.confirmations.hrdlog.upload.label': 'Auto-upload QSOs to HRDLog.net',
  'settings.confirmations.hrdlog.upload.hint':
    'Push each logged QSO to HRDLog.net (needs the upload code above). HRDLog.net is a live-logging and awards site — it is <b>not</b> an ARRL confirmation source, so an upload here never earns DXCC/WAS credit.',

  'settings.confirmations.repeaterbook.token.label': 'RepeaterBook API token',
  'settings.confirmations.repeaterbook.token.forget.title':
    'Remove the stored RepeaterBook token from the system keychain',
  'settings.confirmations.repeaterbook.token.hint':
    "Optional. Without a token the <b>Program</b> section uses the open hearham.com directory. Add a personal token (from your RepeaterBook account's <b>API Apps</b> page) to pull from RepeaterBook.com under your own account instead. Stored in the OS keychain. Shared RepeaterBook access for every Nexus user is pending RepeaterBook's approval; if RepeaterBook is unreachable, Program falls back to hearham.com.",

  'settings.confirmations.cloudlog.note':
    'Auto-forward each logged QSO to your self-hosted <b>Cloudlog</b> or <b>Wavelog</b> logbook (HTTP). The API key is a per-instance token for your own server — enter it, your station-profile id, and turn on the toggle.',
  'settings.confirmations.cloudlog.url.label': 'Base URL',
  'settings.confirmations.cloudlog.url.hint':
    'Your Cloudlog/Wavelog site root. Leave blank to disable.',
  'settings.confirmations.cloudlog.stationId.label': 'Station profile id',
  'settings.confirmations.cloudlog.stationId.hint':
    'The station-location profile to log against (Cloudlog ▸ Station Locations).',
  'settings.confirmations.cloudlog.apiKey.label': 'API key',
  'settings.confirmations.cloudlog.apiKey.placeholder': 'your instance API key',
  'settings.confirmations.cloudlog.apiKey.forget.title':
    'Remove the stored Cloudlog key from the system keychain',
  'settings.confirmations.cloudlog.apiKey.hint':
    'Cloudlog ▸ Account ▸ API Keys — a key with read/write. Stored in the OS keychain, never on disk.',
  'settings.confirmations.cloudlog.upload.label': 'Auto-forward QSOs',
  'settings.confirmations.cloudlog.upload.hint':
    "Push every logged QSO to the instance above as it's logged.",

  // ── Settings ▸ Appearance ▸ Workspace ───────────────────────────────────────────────
  // ⚠️ Every scale here is a PERCENT of a technical quantity, so it is interpolated as an
  // invariant number and the `%` stays glued to it. The chips themselves (`100%`, `175%`)
  // are numbers alone and never enter this file.
  'settings.workspace.legend': 'Workspace',
  'settings.workspace.language.label': 'Language',
  'settings.workspace.language.hint':
    'The language Nexus writes in. Frequencies, signal reports, callsigns, grid squares, band and mode names are never translated or reformatted — a dial reads the same in every language.',
  'settings.workspace.theme.label': 'Theme',
  'settings.workspace.theme.hint':
    'Light reads best outdoors in daylight; the top bar’s Field chip boosts contrast and size in whichever theme you use.',
  'settings.workspace.scale.label': 'UI scale',
  'settings.workspace.scale.mode.aria': 'UI scale mode',
  'settings.workspace.scale.auto': 'Auto (fit)',
  'settings.workspace.scale.manual': 'Manual',
  'settings.workspace.scale.aria': 'UI scale',
  // `&apos;` in the JSX was a plain ASCII apostrophe; `&rsquo;` above was a typographic one.
  // They are different characters and the rendered text must not change, so both are kept.
  'settings.workspace.scale.cap.label': "Max scale (auto won't exceed)",
  'settings.workspace.scale.cap.aria': 'Maximum UI scale',
  'settings.workspace.scale.cap.unreachable':
    'This window only fits up to {{fits}}% — a larger window or monitor unlocks {{wanted}}%.',
  // Three WHOLE messages rather than a shared opening plus a tail: the second sentence is a
  // different answer in each case, and a translator needs to read the one they are writing.
  'settings.workspace.scale.auto.hint.tooSmall':
    "Fits the whole interface to the window so nothing is cut off (currently {{scale}}%). This window maxes out at {{fits}}% — raising the cap can't help until you enlarge the window, or switch to Manual to force a bigger scale.",
  'settings.workspace.scale.auto.hint.limited':
    'Fits the whole interface to the window so nothing is cut off (currently {{scale}}%). This window fits up to {{fits}}%; bigger caps need a larger window or monitor. The waterfall stays sharp.',
  'settings.workspace.scale.auto.hint.full':
    'Fits the whole interface to the window so nothing is cut off (currently {{scale}}%). The waterfall stays sharp. Raise the max for big monitors.',
  'settings.workspace.scale.manual.hint':
    'Fixed scale. Switch to Auto to fit the interface to the window automatically.',
  'settings.workspace.density.label': 'Density',
  'settings.workspace.density.aria': 'Information density',
  'settings.workspace.density.standard': 'Comfortable',
  'settings.workspace.density.dense': 'Compact',
  'settings.workspace.density.hint':
    'How tightly rows and controls pack. Compact fits more on screen.',
  'settings.workspace.panes.label': 'Pane sizes',
  'settings.workspace.panes.reset': 'Reset pane sizes',
  'settings.workspace.panes.hint': 'Restore the default left/right pane widths.',

  // ── Settings ▸ Appearance ▸ Features ────────────────────────────────────────────────
  // ⚠️ Each feature's NAME and one-line description, and the category headings they are
  // grouped under, come from `features/registry.ts` and are interpolated or rendered as
  // values — they are not in this file, and they move with that registry. What is here is
  // the section's own chrome. The Field Day master's label and its two accessible names are
  // shared with Settings ▸ Contesting above (`settings.fieldDay.mode.*`): one toggle, one
  // wording, two places it is offered — only the hint differs, so only the hint is here.
  'settings.features.legend': 'Features',
  'settings.features.alwaysOn': 'always on',
  'settings.features.toggle.aria.enable': 'Enable {{feature}}',
  'settings.features.toggle.aria.disable': 'Disable {{feature}}',
  'settings.features.dependsOn': 'Turning on also enables {{feature}}.',
  'settings.features.profile.label': 'Profile',
  'settings.features.profile.aria': 'Feature profile',
  'settings.features.profile.confirm.title': 'Switch to “{{profile}}”?',
  'settings.features.profile.confirm.body': 'This replaces your custom feature set.',
  'settings.features.profile.confirm.action': 'Switch',
  'settings.features.profile.custom.label': 'Custom',
  'settings.features.profile.custom.title':
    'Custom — a blended feature set (manual toggles or multiple goals)',
  'settings.features.profile.hint.custom':
    'Custom — a blended feature set. Pick a single goal above to reset to its defaults.',
  'settings.features.profile.hint.preset':
    'Pick a goal to set sensible defaults — every feature stays toggleable below. Switching profiles re-applies its set.',
  'settings.features.rerunWizard': 'Re-run setup…',
  'settings.features.core.title': 'Core — always on',
  'settings.features.fieldDay.hint':
    'Turn on for Field Day weekend — reveals the Field Day workspace, the Class/Section exchange across all modes, and the setup tab. Off the rest of the year (nothing shows). Stays on across restarts until you turn it off; Save settings to apply.',

  // ── Settings ▸ Appearance ▸ Accessibility ───────────────────────────────────────────
  // ⚠️ The <select> VALUES ('off', 'needed', 'all') are persisted tokens and stay in the
  // panel; only these labels are read. CQ is a Q-code and TX / RX are the radio's own two
  // states — a translator leaves all three exactly as they are.
  'settings.accessibility.legend': 'Accessibility & eyes-free',
  'settings.accessibility.note':
    'Speech and sound cues for operating by ear (screen-reader users, or anyone who wants audible feedback). The keyboard and screen-reader labels throughout Nexus are always on — these settings only control what comes out of the speakers.',
  'settings.accessibility.announce.label': 'Announce decodes (screen reader)',
  'settings.accessibility.announce.off': 'Off',
  'settings.accessibility.announce.needed': 'Needed only (calling you / new / watched)',
  'settings.accessibility.announce.all': 'All (adds a per-cycle CQ summary)',
  'settings.accessibility.announce.hint':
    'What a screen reader speaks as decodes arrive. Silent without a reader running. "Needed" mirrors your alerts; "All" adds a spoken batch summary each cycle.',
  'settings.accessibility.txRxEarcon.label': 'TX / RX earcon',
  'settings.accessibility.txRxEarcon.hint':
    'A rising tone when you key up, falling when you unkey — know your TX state by ear.',
  'settings.accessibility.decodeTick.label': 'Decode-batch tick',
  'settings.accessibility.decodeTick.hint':
    "A soft tick each cycle new signals are decoded — the band's rhythm, eyes-free.",

  // ── Connect — the board, its pane grid, and the one-line pane projections ───────────
  // ⚠️ THE UNITS RULE OWNS THIS SURFACE. Band and mode names, bearings (`~47°`), MUF and
  // dial frequencies, distances, octants, SFI/Kp readings, beacon callsigns, the P.533
  // engine name, and every word the BACKEND sends — the advisory headline, the workability
  // (`Fair`), the dual-state word (`Open`), the insight text, the Kp impact sentence and the
  // getting-out direction summary — arrive as values and are never translated here. Two
  // intent chips are named for a programme and a band (`POTA/SOTA`, `6m/VHF`), so those two
  // labels stay in `ConnectView.tsx` as tokens; the other two are prose and are below.
  'connect.intent.aria': 'What are you doing?',
  'connect.intent.dx.label': 'Chase DX',
  'connect.intent.dx.title': 'Beam map, need-colored, live openings',
  'connect.intent.pota.title': 'World view, park/summit activators',
  'connect.intent.casual.label': 'Ragchew',
  'connect.intent.casual.title': 'Who can I hear — signal-colored, calm',
  'connect.intent.vhf.title': 'Openings front-and-center (Es / F2 / aurora)',
  'connect.map3d.title.on':
    'Using the 3D WebGL globe — click for the 2D map (works on any PC)',
  'connect.map3d.title.off': 'Switch to the 3D WebGL globe (best on higher-end PCs)',
  'connect.globe3d.loading': 'Loading 3D globe…',
  'connect.popOut.label': '⧉ Pop out',
  'connect.popOut.title': 'Open Connect in its own window (for a second monitor)',

  // The pane frame: one grid slot's header. `{{slot}}` is the slot id (`left1`, `bottom3`),
  // and the B2/B3 picker groups are named by their tier code — neither is prose.
  'connect.slot.pick.aria': 'Choose what the {{slot}} slot shows',
  'connect.slot.pick.title': 'Choose what this slot shows',
  'connect.slot.group.core': 'Panels',

  // Pane names, as they read in the picker and in each pane's header.
  'connect.pane.advisory.title': 'Conditions',
  'connect.pane.bandAdvisor.title': 'Band Advisor',
  'connect.pane.selection.title': 'Selection',
  'connect.pane.outlook.title': 'Band Outlook',
  'connect.pane.openings.title': 'Openings',
  'connect.pane.openingsLog.title': 'Openings Log',
  // ---- The three-day planetary-K outlook (Connect pane + the map's storm cue).
  // `kind` is SWPC's own word for how a sample was arrived at; only "observed" is a
  // measurement, so the wording must never turn a forecast into a reading.
  'connect.pane.kpOutlook.title': 'Kp outlook',
  'connect.pane.kpOutlook.basic': 'Three-day planetary-K forecast from NOAA — when the bands settle down.',
  'connect.kp.unavailable': 'No Kp outlook yet — NOAA has not answered.',
  'connect.kp.noForward': 'NOAA has published no forecast beyond now.',
  'connect.kp.now': 'Now Kp {{kp}} ({{when}}, measured)',
  'connect.kp.peak': 'Worst ahead: Kp {{kp}} at {{when}}',
  'connect.kp.onset': 'Storm level (Kp {{kp}}) expected from {{when}}',
  'connect.kp.relief': 'Settling below storm level around {{when}}',
  'connect.kp.chart.aria': 'Planetary K index, measured hours then forecast',
  'connect.kp.bar.title': '{{when}} · Kp {{kp}} · {{kind}}{{scale}}',
  'connect.kp.kind.observed': 'measured',
  'connect.kp.kind.estimated': 'estimated by NOAA',
  'connect.kp.kind.predicted': 'forecast',
  'connect.pane.spacewx.title': 'Space Wx',
  'connect.pane.getout.title': 'Getting Out',
  'connect.pane.bestband.title': 'Best Band → Region',
  'connect.pane.activity.title': 'Activity Matrix',
  'connect.pane.beacons.title': 'NCDXF Beacons',
  'connect.pane.insights.title': 'Insights',
  'connect.pane.chase.title': 'Chase',
  'connect.pane.greyline.title': 'Greyline',
  'connect.pane.bandHours.title': '24h Band×Hour',
  'connect.pane.esNowcast.title': 'Sporadic-E',
  'connect.pane.measuredMuf.title': 'Measured MUF',
  'connect.pane.chaseFeed.title': 'Chase Feed',
  'connect.pane.satPasses.title': 'Satellite Passes',
  'connect.pane.rotor.title': 'Rotor',
  'connect.pane.amp.title': 'Amplifier',
  'connect.pane.scope.title': 'Band Scope',
  'connect.pane.contests.title': 'Contests',

  // The five self-fetching panes describe themselves: their data lives inside the
  // component, so their one-line projection is a standing hint rather than a reading.
  'connect.pane.openingsLog.basic':
    'A historical record of every detected band opening (6m/2m tropo, Es, aurora) builds here.',
  'connect.pane.satPasses.basic':
    'Upcoming amateur-satellite passes over your QTH appear here once orbital elements load.',
  'connect.pane.rotor.basic':
    'Rotator control appears here once you pick a rotator model and port in Settings ▸ Radio ▸ Rotator.',
  'connect.pane.amp.basic':
    'Amplifier readings appear here once you pick an amplifier and its port in Settings ▸ Radio ▸ Amplifier.',
  'connect.pane.scope.basic':
    "A live spectrum of the active radio's passband — band noise and signals at a glance.",
  'connect.pane.scope.idle': "Flat — the radio's audio isn't reaching Nexus right now.",
  'connect.pane.contests.basic': 'Upcoming HF/VHF contests (WA7BNM) appear here once online.',

  // Where a snapshot came from. The words are the chip; the freshness is a number.
  'connect.prov.title': 'Data provenance',
  'connect.prov.live': 'LIVE',
  'connect.prov.partial': 'PARTIAL',
  'connect.prov.cached': 'CACHED {{mins}}m',
  'connect.prov.offline': 'NO LIVE DATA',

  // The selection panel. Each ` · `-separated item is a datum of its own, not a clause of
  // one sentence, so each has its own key and the separator stays in the component.
  'connect.selection.clear.title': 'Clear selection',
  'connect.selection.age.secs': '{{secs}}s ago',
  'connect.selection.age.mins': '{{mins}}m ago',
  'connect.selection.heardYou': 'heard YOU',
  'connect.selection.approx': '~location',
  'connect.selection.decoded': 'decoded here',
  'connect.selection.workedBefore': 'worked before',
  'connect.selection.bestShot': 'Best shot: {{window}}',
  'connect.selection.work.title':
    "Rig jumps to this spot's band/mode/frequency; the right cockpit opens",
  'connect.selection.work.label': '▶ Work {{band}}',
  // The engine behind a prediction. `P.533` is the ITU recommendation's number and stays in
  // the component beside it.
  'connect.engine.modelled': 'modelled',
  'connect.engine.outlook': 'modelled · DX',

  'connect.path.heading': 'Path to {{call}}',
  'connect.path.muf.title':
    "Maximum Usable Frequency — the path's ceiling right now. Bands below it are open; bands above it are closed.",
  'connect.path.muf': 'Ceiling (MUF): <b>{{muf}} MHz</b>',
  'connect.path.none': 'No HF band modelled workable on this path right now.',
  'connect.path.greyline.title': 'Greyline (terminator) opening',
  'connect.path.modeChip.title':
    '{{mode}}: ~{{pct}}% of days this circuit works right now (P.533)',
  'connect.outlook.heading': 'Band outlook',
  'connect.outlook.muf.title':
    'Maximum Usable Frequency — the modeled ceiling to long-haul DX right now. Bands below it are open; above it, closed.',
  'connect.outlook.none': 'No HF band modelled workable to DX right now.',

  'connect.getout.heading': 'Am I getting out?',
  'connect.getout.none': 'No reception reports yet — call CQ, then watch who hears you.',
  'connect.getout.summary': '<b>{{count}}</b> hearing you · furthest <b>{{km}} km</b>',
  'connect.getout.select.title': 'Select {{call}} on the map',

  // ── The Basic projections — one whole sentence per pane, per state ──────────────────
  // Every one of these was assembled from fragments. They are whole sentences now, with
  // the variable part interpolated, because a sentence glued from clauses cannot be
  // translated into a language that orders them differently.
  'connect.basic.loading': 'Reading the band…',
  'connect.basic.offline': 'No live propagation data right now.',
  'connect.basic.bandAdvisor.offline': 'No live band data yet.',
  'connect.basic.bandAdvisor.none': 'No bands modelled open right now.',
  'connect.basic.bandAdvisor.best': 'Best band now: {{band}} ({{word}}).',
  'connect.basic.bandAdvisor.bestRegion': 'Best band now: {{band}} to {{region}} ({{word}}).',
  'connect.basic.selection.none': 'Tap a station, spot, or DXpedition on the map.',
  // Two whole sentences — "and is hearing you" changes what the line SAYS, so it is inside
  // the message, not glued after it. The heading and the band are known only sometimes, so
  // each is an optional clause carrying its own separator, exactly as the map's spot
  // tooltip does above (`prop.spotTooltip.*`).
  'connect.basic.selection': '{{call}} — {{who}}{{az}}{{band}}.',
  'connect.basic.selection.hearing': '{{call}} — {{who}}{{az}}{{band}}, and is hearing you.',
  'connect.basic.selection.az': ' {{az}}',
  'connect.basic.selection.band': ' on {{band}}',
  'connect.basic.outlook.none.call': 'No HF band modelled workable to {{call}} right now.',
  'connect.basic.outlook.none.dx': 'No HF band modelled workable for DX right now.',
  'connect.basic.outlook.path': '{{band}} is your best path to {{call}} now — {{window}}.',
  'connect.basic.outlook.best': 'Best DX band now: {{band}} ({{workability}}).',
  'connect.basic.openings.none': 'No band openings right now.',
  // The station count reads `stns` at every count today; a locale that needs a singular
  // supplies one as an overlay. English is left exactly as it shipped.
  'connect.basic.openings': '{{band}} OPEN {{octant}} — ~{{km}} km, {{stations}} stns.',
  'connect.basic.spaceWx.unavailable': 'Space weather unavailable.',
  'connect.basic.spaceWx': 'SFI {{sfi}}, Kp {{kp}}: {{impact}}.',
  'connect.basic.spaceWx.flare':
    'SFI {{sfi}}, Kp {{kp}}: {{impact}}; {{xray}} flare in progress.',
  'connect.basic.spaceWx.blackout':
    'SFI {{sfi}}, Kp {{kp}}: {{impact}}; R{{scale}} radio blackout.',
  'connect.basic.spaceWx.flareBlackout':
    'SFI {{sfi}}, Kp {{kp}}: {{impact}}; {{xray}} flare in progress; R{{scale}} radio blackout.',
  'connect.basic.getout.dir': '{{count}} hearing you — {{dir}}.',
  'connect.basic.getout.furthest': '{{count}} hearing you — furthest {{km}} km.',
  'connect.basic.bestband.none': 'No region reachable on any band yet.',
  'connect.basic.bestband': 'To {{region}}: try {{band}} ({{word}}).',
  'connect.basic.activity.offline': 'No live activity data right now.',
  'connect.basic.activity.none': 'Quiet on all bands — no activity around you yet.',
  'connect.basic.activity.top': {
    one: 'Hottest: {{band}} to {{region}} ({{count}} stn).',
    other: 'Hottest: {{band}} to {{region}} ({{count}} stns).',
  },
  // `{{list}}` is the beacon callsigns with their bands — tokens, built in the component.
  'connect.basic.beacons.heard': 'Beacons heard: {{list}}.',
  'connect.basic.beacons.now': 'Beacons now: {{list}}.',
  'connect.basic.insights.none': 'No notable changes right now.',
  'connect.basic.greyline.noGrid': 'Set your grid in Settings to see your greyline windows.',
  'connect.basic.greyline.sunrise':
    'Your sunrise greyline in {{when}} — watch 160/80/40m long-path.',
  'connect.basic.greyline.sunset':
    'Your sunset greyline in {{when}} — watch 160/80/40m long-path.',
  'connect.basic.greyline.in.hours': '{{hours}}h {{mins}}m',
  'connect.basic.greyline.in.mins': '{{mins}}m',
  'connect.basic.bandHours.none': 'No workable bands modelled in the next 24 h.',
  'connect.basic.bandHours.peak': '{{band}} peaks {{hour}}Z ({{pct}}%).',
  'connect.basic.muf.noData': 'No live ionosonde data right now.',
  'connect.basic.muf.noneNearby': 'No ionosonde MUF reported nearby.',
  'connect.basic.muf.nearby': 'Measured MUF nearby: {{mhz}} MHz ({{mins}} min old).',
  'connect.basic.es.open': {
    one: '{{band}} OPEN {{octant}} — ~{{km}} km {{mode}}, {{count}} stn.',
    other: '{{band}} OPEN {{octant}} — ~{{km}} km {{mode}}, {{count}} stns.',
  },
  // ⚠️ `{{freq}}` is the 6 m Es calling frequency, interpolated rather than written into
  // the sentence: a literal frequency in a catalog is one a translator can reformat.
  'connect.basic.es.season': 'Es season: watch {{freq}} for sudden 6m DX.',
  'connect.basic.es.quiet': '6m quiet — outside Es season.',

  // ── Tempo — the conversation, its composer, the bubbles and the CQ launchpad ────────
  // The CQ line itself (`CQ KD9TAW EN52`) and the YOURCALL / ---- stand-ins it uses before
  // a callsign and grid are set, the quick-reply macros (RR73, 73 and whatever the operator
  // typed), the Winter Field Day chip, and every SNR / audio-frequency / dT / tier reading
  // under a bubble are tokens and stay in the components.
  'tempo.empty.heading': 'No conversation selected',
  'tempo.empty.body': 'Pick a station from the roster, or call CQ to be heard on the band.',
  // `<chip>` is the Field Day chip; the call site supplies the element and the event name.
  'tempo.empty.fdActive':
    '<chip>{{event}}</chip> active — call CQ, then send your exchange from the chat box.',
  'tempo.cq.button': '📣 Call CQ',
  'tempo.cq.onAir': 'Transmits the standard <b>{{cq}}</b> and arms TX.',
  'tempo.heartbeat.launch.on': '💓 Heartbeat on',
  'tempo.heartbeat.launch.off': '🤍 Heartbeat off',
  'tempo.heartbeat.launch.title':
    'Periodically beacon your presence so other Tempo stations can hear you and deliver queued messages — turn off to stay silent',
  'tempo.heartbeat.chip.on': '💓 Heartbeat',
  'tempo.heartbeat.chip.off': '🤍 Heartbeat',
  'tempo.heartbeat.chip.title':
    'Presence heartbeat — periodically beacon so other Tempo stations can hear you and deliver queued messages',
  'tempo.band.quickbar.aria': 'Band broadcasts',
  'tempo.header.band': 'Band — open calls',
  'tempo.header.broadcastAs': 'You broadcast as DE {{call}}',
  // Reads `1 messages` at a count of one today, and this phase changes no visible text: the
  // English stays one form. `{{count}}` still selects a form in any locale that supplies one.
  'tempo.header.messages': '{{count}} messages',
  'tempo.header.fd.title':
    'Winter Field Day is active — Tempo is a first-class Field Day contact surface',
  'tempo.header.fd.working': 'Working {{call}}',
  'tempo.header.fd.running': 'Running (calling CQ)',
  'tempo.header.fd.searchPounce': 'Search & pounce',
  'tempo.messages.empty': 'No messages yet — say hello.',
  'tempo.composer.quickReplies.aria': 'Quick replies',
  'tempo.composer.fdExchange.title': 'Send your Winter Field Day exchange (class + section)',
  'tempo.composer.placeholder.broadcast': 'Broadcast to all (DE {{call}}…)',
  'tempo.composer.placeholder.direct': 'Message {{peer}}…',
  'tempo.composer.aria.broadcast': 'Broadcast to all on frequency',
  'tempo.composer.aria.direct': 'Message {{peer}}',
  'tempo.composer.send': 'Send',
  // The capacity meter. `{{frames}}/{{max}}` is a count of T/R overs and `{{payload}}` the
  // per-over character budget — the numbers are invariant, the sentence is not.
  'tempo.meter.title': {
    one: '{{count}} character · {{frames}}/{{max}} overs. Each over carries up to {{payload}} characters; {{max}} overs max — longer text is trimmed before it sends.',
    other:
      '{{count}} characters · {{frames}}/{{max}} overs. Each over carries up to {{payload}} characters; {{max}} overs max — longer text is trimmed before it sends.',
  },
  'tempo.meter.unit': 'overs',
  'tempo.meter.full': 'full',
  // One bubble's delivery state. The ticks themselves (✓, ✓✓, ⚠, ⋯, ↻) are glyphs and stay
  // in the component; this is what a screen reader hears and the hover says.
  'tempo.bubble.abandoned': 'Not sent — abandoned on restart. Tap to send it again.',
  'tempo.bubble.noAck': 'Sent {{attempts}}× — no acknowledgement. Tap to send it again.',
  'tempo.bubble.held': 'Waiting to send',
  'tempo.bubble.held.peer': 'Waiting to send — {{call}} not heard yet',
  'tempo.bubble.sending': 'Sending — try {{attempt}}',
  'tempo.bubble.sent': 'Sent',
  'tempo.bubble.onAir': 'On air',
  'tempo.bubble.delivered': 'Delivered',
  'tempo.bubble.confirmed': 'Confirmed — they answered after this went out',
  'tempo.bubble.resend.title': 'Tap to re-send this message',
  'tempo.bubble.incomplete.title':
    'Only {{got}} of {{total}} parts of this message were received — the rest never arrived',
  'tempo.bubble.incomplete.badge': '{{got}} of {{total}} received',

  // ── APRS — the cockpit, its two health chips, and the station card ──────────────────
  // ⚠️ THE UNITS RULE OWNS THIS SURFACE. Callsign-SSIDs (`W9XYZ-9`), APRS symbol codes and
  // their table, digipeater paths (`WIDE1-1`), every dial frequency and channel, dBFS
  // levels, positions, grids, distances, bearings, speeds, altitudes, the packet kind and
  // the weather readings are DATA — they arrive as values and are never translated. So is
  // the rig-menu path in the level advice (`IC-9700: SET > Connectors > USB AF Output
  // Level`): it is what the operator reads on the radio's own screen.
  // ⚠️ THE CHANNEL IS INTERPOLATED, never written into a sentence — a literal `144.390`
  // in this file is a frequency a translator could reformat, and `i18n.invariant.test.ts`
  // refuses one.
  // ⚠️ NOT HERE, deliberately: the TX On/Off arm latch. Its label, its two tooltips and its
  // accessible name are a transmit-path control and move with that batch, not this one.
  'aprs.head.hint': 'AFSK-1200 packet — decode positions/messages, send a beacon',
  'aprs.head.packets': '{{count}} pkts',
  'aprs.channel.title':
    'APRS frequency by region — selecting one tunes the rig (2 m FM, AFSK-1200)',
  'aprs.retune.label': 'Re-tune',
  'aprs.retune.title.loading': 'Reading your APRS channel…',
  'aprs.retune.title':
    'Re-tune the rig to the selected APRS frequency (2 m FM simplex; switches to your 2 m radio)',
  'aprs.retune.title.noCoverage':
    "This radio doesn't cover {{freq}} MHz — RF APRS needs a VHF radio.",
  'aprs.dial.title': "The rig's current dial / band / mode (this view hides the top bar's readout)",
  // Monitor arms the DECODER on the receive audio. Three states, because "decoding" and
  // "may ack by itself" are different things — the ack still needs TX on.
  'aprs.monitor.label.auto': '● Monitoring (auto)',
  'aprs.monitor.label.explicit': '● Monitoring',
  'aprs.monitor.label.off': 'Monitor',
  'aprs.monitor.title.explicit':
    'You armed the decoder, so automatic acks are allowed — an incoming message addressed to you is acked when TX is on. Click to stop.',
  'aprs.monitor.title.auto':
    'Armed automatically when you opened APRS: RECEIVE ONLY. It will never send an automatic ack. To allow those, stop it and arm it yourself, then turn TX on. Click to stop.',
  'aprs.monitor.title.off':
    'Arm the APRS decoder on the RX audio. Arming it yourself also allows automatic acks once TX is on.',
  // The one-click fix beside the wrong-frequency chip: it moves the DIAL, and keys nothing.
  'aprs.tuneFix.label': 'Tune to {{freq}}',
  'aprs.tuneFix.title': 'Tune the radio to {{freq}} FM for APRS',
  'aprs.showInet.label.shown': 'Internet {{count}}',
  'aprs.showInet.label.hidden': 'Internet {{count}} hidden',
  'aprs.showInet.title.hide': {
    one: 'Hide the {{count}} station only the internet has reported, leaving what this radio actually hears',
    other:
      'Hide the {{count}} stations only the internet has reported, leaving what this radio actually hears',
  },
  'aprs.showInet.title.show': {
    one: 'Show the {{count}} station the internet feed reports',
    other: 'Show the {{count}} stations the internet feed reports',
  },

  // The internet feed's chip and its controls.
  'aprs.inet.chip.title': '{{detail}}\n\nClick for internet feed controls.',
  'aprs.inet.panel.aria': 'APRS-IS internet feed',
  'aprs.inet.enabled.label': 'Internet feed',
  'aprs.inet.radius.label': 'Radius (km)',
  'aprs.inet.watch.label': 'Watched calls',
  'aprs.inet.note':
    'Changing the radius or watched calls reconnects the feed — the server does the filtering, so a new subscription has to be sent. Server, port, traffic types and the iGate live in Settings ▸ APRS.',
  'aprs.inet.note.open': 'Open them',
  // The feed's four states. `{{gate}}` is the iGate sentence below — a WHOLE sentence with
  // its own leading space, empty when the iGate is off, interpolated so a translation can
  // place it rather than having it glued on after the fact.
  'aprs.inet.off.label': 'Internet off',
  'aprs.inet.off.detail': 'The APRS-IS feed is switched off.',
  'aprs.inet.connecting.label': 'Internet connecting',
  'aprs.inet.connecting.detail':
    'Not connected to APRS-IS yet — retrying with backoff.{{gate}}',
  'aprs.inet.quiet.label': 'Internet quiet',
  'aprs.inet.quiet.detail.recent':
    'Connected, but no packets recently — nothing matches your filter. Widen the radius or add watched calls.{{gate}}',
  'aprs.inet.quiet.detail.never':
    'Connected, but no packets yet — nothing matches your filter. Widen the radius or add watched calls.{{gate}}',
  'aprs.inet.live.label': 'Internet {{count}}',
  'aprs.inet.live.detail.verified': 'Connected and verified — {{count}} packets received.{{gate}}',
  'aprs.inet.live.detail.readOnly': 'Connected read-only — {{count}} packets received.{{gate}}',
  // `{{reason}}` is the server's own rejection text, printed as it arrived.
  'aprs.inet.gate': 'iGate on: {{uploaded}} contributed.',
  'aprs.inet.gate.held': 'iGate on: {{uploaded}} contributed, {{held}} held back.',
  'aprs.inet.gate.held.reason':
    'iGate on: {{uploaded}} contributed, {{held}} held back (last: {{reason}}).',

  // Which radio the decoder is listening to, named only when more than one could be.
  'aprs.radioNote.label': 'on {{name}}',
  'aprs.radioNote.detail':
    '{{count}} of your radios cover this band, so APRS had a choice to make. It follows the active radio, currently {{name}} — if that is not the rig your packet audio is wired to, this is why nothing is decoding. Routing rules decide which radio a band goes to: Settings → Radios.',

  // The decode health chip. `{{level}}` is the live input reading below, interpolated whole
  // so a translation places it; `{{db}}` is a dBFS number and never formatted for a locale.
  'aprs.health.level.silence': 'input over the most recent 0.1 s: digital silence (exactly zero)',
  'aprs.health.level.peak': 'input peak over the most recent 0.1 s: {{db}} dBFS',
  'aprs.health.dbfs.silence': 'silence',
  'aprs.health.norf.label': 'No 2 m radio',
  'aprs.health.norf.detail':
    "This radio doesn't cover {{freq}} MHz, so it can't receive RF APRS. RF APRS needs a VHF radio. The internet feed works without one — turn it on to see APRS traffic reported by other stations.",
  'aprs.health.off.label': 'Monitor off',
  'aprs.health.off.detail':
    'The APRS decoder is not running. Arm Monitor to decode the RX audio.',
  // Three whole messages, because each names a DIFFERENT thing as wrong — the mode, the
  // dial, or both — and the closing sentence belongs to all three.
  'aprs.health.wrongFreq.label': 'Wrong frequency',
  'aprs.health.wrongMode.label': 'Wrong mode',
  'aprs.health.wrongMode.detail':
    'The radio is on {{want}} but in {{mode}} — APRS needs FM. FM packet audio demodulated as SSB is garbled, so nothing will decode however strong the signal is. Tune to the APRS channel to start hearing it.',
  'aprs.health.wrongFreqMode.detail':
    'The radio is on {{dial}} {{mode}} — APRS needs {{want}} FM. Nothing on this channel can decode as APRS packet, whatever the audio level says. Tune to the APRS channel to start hearing it.',
  'aprs.health.wrongFreq.detail':
    'The radio is on {{dial}} — APRS needs {{want}}. Nothing on this channel can decode as APRS packet, whatever the audio level says. Tune to the APRS channel to start hearing it.',
  'aprs.health.noCapture.label': 'No input',
  'aprs.health.noCapture.detail':
    'Armed, but no audio samples are arriving at all — the capture device is not delivering anything. Check that Input Device (RX) in Settings ▸ Radio ▸ Audio is the radio (not a microphone or a disconnected device); what you hear on the speaker does not tell you what the app is capturing.',
  'aprs.health.decoding.label': '{{count}} decoded',
  'aprs.health.decoding.detail': '{{count}} packets decoded since arming. Live {{level}}.',
  'aprs.health.decoding.detail.aged':
    '{{count}} packets decoded since arming, last one {{age}} ago. Live {{level}}.',
  'aprs.health.unreadable.label': '{{count}} failed CRC',
  // `{{advice}}` is the headroom sentence below — empty when the burst sits in the healthy
  // band, and carrying its own leading space when it does not.
  'aprs.health.unreadable.detail':
    '{{count}} bursts heard since arming, last one {{age}} ago — none passed the checksum. Some of that is normal: when the squelch opens partway through a burst the start of the packet is lost, and a part-heard packet can never pass. It is only a fault if nothing ever decodes — in which case check the rig is on {{channel}} in FM.{{advice}} Last burst peaked {{burst}}; live {{level}}.',
  // Headroom, never a cause: measurement says level does not decide whether a frame decodes.
  // `sample(s)` reads the same at every count today, and this phase changes no visible text.
  'aprs.health.advice.clipping':
    "The burst is CLIPPING (peaked {{peak}}, {{samples}} sample(s) at the rails) — lower the rig's USB AF output level, or the Windows input level for that device. Packet survives a lot of clipping, so this costs headroom rather than decodes, but there is no reason to run into the rails.",
  'aprs.health.advice.quiet':
    "The burst peaked {{peak}}, well below the healthy {{min}} to {{max}} band — raise the rig's USB AF output level (IC-9700: SET > Connectors > USB AF Output Level) or the Windows input level for that device. That buys margin against noise; it is not by itself why a checksum fails.",
  'aprs.health.silent.label': 'Silent',
  'aprs.health.silent.detail':
    'The input is alive and delivering audio, but it is silent — normally that just means the squelch is closed between packets, which is what an idle FM channel looks like. To confirm the routing, open the squelch: hiss should show up here as a level. If it still reads silent with the squelch open, the wrong input device is selected. Live {{level}}.',
  'aprs.health.listening.label': 'Listening',
  'aprs.health.listening.detail':
    'Audio is reaching the decoder and no packets have been heard recently — a quiet channel. Live {{level}}. With the squelch open, hiss should sit around {{hiss}}; a packet burst should peak {{min}} to {{max}}.',

  // How long ago, compactly. The unit letter rides inside the message with its number so a
  // translation can never separate the two.
  'aprs.age.secs': '{{secs}}s',
  'aprs.age.mins': '{{mins}}m',
  'aprs.age.hours': '{{hours}}h',

  // The Via column: how this station reached us. The stored source kind (`rf` / `inet` /
  // `both`) is the token; these are its words. A locale keeps `RF` — it is the ham
  // abbreviation — and is free to shorten `net` its own way.
  'aprs.source.rf.label': 'RF',
  'aprs.source.inet.label': 'net',
  'aprs.source.both.label': 'RF+net',
  'aprs.source.rf.title': 'Your receiver decoded this station off the air',
  'aprs.source.inet.title':
    'Reported by APRS-IS — your receiver has not heard this station',
  'aprs.source.both.title': 'Heard off the air by your receiver AND reported by APRS-IS',

  // The beacon and message composers. The symbol names come from `aprsBeacon.ts` and the
  // channel list from `APRS_FREQS`; both are read as values.
  'aprs.beacon.title': 'Position beacon',
  'aprs.beacon.lat.label': 'Lat',
  'aprs.beacon.lon.label': 'Lon',
  'aprs.beacon.symbol.label': 'Symbol',
  'aprs.beacon.comment.label': 'Comment',
  'aprs.beacon.path.label': 'Path',
  'aprs.beacon.send': 'Send beacon',
  'aprs.msg.title': 'Message',
  'aprs.msg.to.label': 'To',
  'aprs.msg.to.placeholder': 'callsign',
  'aprs.msg.text.label': 'Text',
  'aprs.msg.text.placeholder': 'up to 67 chars',
  'aprs.msg.send': 'Send message',
  'aprs.messages.title': 'Messages',
  // What the board says back. `{{call}}` is a callsign and `{{freq}}` a dial reading.
  'aprs.status.badPosition': 'Enter a valid latitude and longitude first.',
  'aprs.status.beacon.sending': 'Sending beacon…',
  'aprs.status.beacon.queued': 'Beacon queued — keying now.',
  'aprs.status.msg.missing': 'Enter a callsign and a message first.',
  'aprs.status.msg.sending': 'Sending message…',
  'aprs.status.msg.queued': 'Message to {{call}} queued — keying now.',
  'aprs.status.tune.deferred':
    'Transmitting right now — the radio will move to {{freq}} when this over ends.',
  'aprs.status.tune.now': 'Tuning to {{freq}} FM…',

  'aprs.table.age': 'Age',
  'aprs.table.symbol': 'Symbol',
  'aprs.table.from': 'From',
  'aprs.table.via': 'Via',
  'aprs.table.type': 'Type',
  'aprs.table.position': 'Position',
  'aprs.table.dist': 'Dist',
  'aprs.table.info': 'Info',
  'aprs.row.title.highlight': 'Highlight {{call}} on the map',
  'aprs.row.title.noPosition': '{{call}} reported no position — nothing to highlight',
  'aprs.map.noPositions': 'No positions heard yet — status and message packets carry none.',

  // ── The station card ────────────────────────────────────────────────────────────────
  // ⭐ The per-source honesty line: "your receiver heard it" and "a server reported it" are
  // different claims, and they stay different in every language.
  'aprs.card.aria': 'Station {{call}}',
  'aprs.card.symbol.unknown': 'Unrecognised symbol',
  'aprs.card.close': 'Close',
  'aprs.card.source.rf.label': 'Heard on RF',
  'aprs.card.source.rf.detail': 'your receiver decoded this station {{age}} ago',
  'aprs.card.source.inet.label': 'Via APRS-IS',
  'aprs.card.source.inet.detail': 'the internet feed reported it {{age}} ago',
  'aprs.card.source.unknown.label': 'Source unknown',
  'aprs.card.source.unknown.detail': 'no reception recorded for this station',
  'aprs.card.position.label': 'Position',
  'aprs.card.position.none': 'none reported — heard, but nothing to plot',
  'aprs.card.fromYou.label': 'From you',
  'aprs.card.motion.label': 'Motion',
  'aprs.card.motion.stationary': 'stationary',
  'aprs.card.comment.label': 'Comment',
  'aprs.card.path.label': 'Path',
  // `{{path}}` is the digipeater list exactly as the packet carried it (`WIDE1-1,WIDE2-1`).
  'aprs.card.path.direct': 'direct — no digipeaters in the path',
  'aprs.card.path.requested': 'direct — requested {{path}}, none used',
  'aprs.card.path.digipeated': 'digipeated via {{path}}',
  'aprs.card.packets.label': 'Packets',
  'aprs.card.packets.value': '{{count}} since {{age}} ago',
  'aprs.card.wx.title': 'Weather',
  'aprs.card.wx.temperature': 'Temperature',
  'aprs.card.wx.wind': 'Wind',
  'aprs.card.wx.wind.dirUnknown': 'unknown',
  'aprs.card.wx.wind.atSpeed': '{{dir}} at {{speed}}',
  'aprs.card.wx.gust': 'Gust',
  'aprs.card.wx.humidity': 'Humidity',
  'aprs.card.wx.pressure': 'Pressure',
  'aprs.card.wx.rain1h': 'Rain, last hour',
  'aprs.card.wx.rain24h': 'Rain, 24 h',
  'aprs.card.raw.show': '▸ Raw packet',
  'aprs.card.raw.hide': '▾ Raw packet',
  'aprs.card.qrz.error': 'Could not open {{call}} on QRZ',
  'aprs.card.aprsfi.title':
    'Open this station on aprs.fi (third-party site) in your browser',
  'aprs.card.age.secs': '{{secs}} s',
  'aprs.card.age.mins': '{{mins}} min',
  'aprs.card.age.hours': '{{hours}} h',
  'aprs.card.age.days': '{{days}} d',

  // ══════════════════════════════════════════════════════════════════════════════════════
  // THE OPERATE COCKPIT — the FT8/FT4 operating surface.
  // ══════════════════════════════════════════════════════════════════════════════════════
  //
  // The cockpit header, the merged operating strip, the two decode panes (Band Activity and
  // Rx Frequency), the Call Roster and the WSJT-X Tx1–Tx6 message machine.
  //
  // ⚠️ THIS IS THE FIRST SURFACE OF THE TRANSMIT HALF, AND PART OF IT IS DELIBERATELY ABSENT.
  // The strip's Stop TX button, its Tune button, the ATU button (all three key or cut a
  // carrier) and the TX On/Off tooltip that states the abort semantics are NOT here: they
  // stay written in `OperateQsoStrip.tsx` until the transmit-path batch moves them with the
  // stop-line sweeps re-run. Operate's third stop, Esc, is keyboard-only and has no string.
  //
  // ⚠️ THE UNITS RULE OWNS EVERY READING ON THESE SCREENS, and none of them is here: every
  // callsign, grid, DXCC entity, US state, band and mode name, SNR and dB report, audio
  // offset in Hz, DT in seconds, dial frequency, bearing, distance and slot count arrives
  // from the snapshot and is interpolated as data. So do the TOKENS this cockpit is built
  // out of, which stay in the components as named constants: the mode chips' own names
  // (FT8/FT4/FT2 and their T/R slot lengths), `Native` and `Companion` (the backend's own
  // words for the signal source — `radio.sourceLabel` is interpolated beside them), `Rx`/`Tx`
  // on the two audio-offset spinners, `SPLIT ▲`, `AUTO-CQ`, `HOUND`, `CQ`, `B4`, `HARQ`,
  // `SNR`, `DT`, `Hz`, `QRZ`, and the `Tx` of the six message rows. The QRZ button reuses
  // `callbook.qrzPage.*` — looking a callsign up is one act with one wording, and five other
  // surfaces already share it.

  // ── Operate ▸ the cockpit header ────────────────────────────────────────────────────
  // The mode chips carry the tier name in the code and their EXPLANATION here; the T/R
  // period, the Hz threshold and the band names inside these sentences are written where
  // they are read, exactly as every other explanatory number in this file is.
  'operate.header.modes.aria': 'Operating mode',
  'operate.mode.ft8.title': 'Standard WSJT-X FT8 — 15 s T/R',
  'operate.mode.ft4.title': 'Standard WSJT-X FT4 — 7.5 s T/R',
  'operate.mode.ft2.title': 'FT2 (Decodium) — 3.75 s T/R, FT4 with a halved symbol time',
  'operate.header.msk144Period.aria': 'MSK144 T/R period (seconds)',
  'operate.header.msk144Period.title':
    'T/R period — 15 s is the 6 m workhorse; 30 s eases deep-search on 2 m',

  'operate.header.decodeDepth.aria': 'Decode depth',
  'operate.header.decodeDepth.title':
    'FT8/FT4 decode depth — Deep catches weaker signals but uses more CPU/battery (a field/POTA lever)',
  'operate.header.decodeDepth.fast': 'Fast',
  'operate.header.decodeDepth.norm': 'Norm',
  'operate.header.decodeDepth.deep': 'Deep',

  // A CONFIGURATION control on the transmit path is not a transmit control — the batch-13
  // ruling, where the drive slider moved and Prove TX did not.
  'operate.header.power.label': 'Pwr',
  'operate.header.power.title': "TX drive (Pwr) — trim down until your rig's ALC is just zero",

  // The DXpedition selector. `Hound` is WSJT-X's role name and stays in the code; only its
  // explanation and the Off row are words.
  'operate.header.dxped.label': 'DXped:',
  'operate.header.dxped.aria': 'DXpedition mode',
  'operate.dxped.off.label': 'Off',
  'operate.dxped.off.title': 'No DXpedition special mode',
  'operate.dxped.hound.title':
    "DXpedition hound: calls go out above 1000 Hz, your R+report auto-moves to the Fox's frequency, and the QSO ends on the Fox's RR73 with NO parting 73 — a 73 there is QRM in the Fox's own segment. Off again at every launch: turn it on for the DXpedition, not for the day.",

  // The signal source. `{{active}}` is the backend's own `sourceLabel` and `{{addr}}` the
  // configured companion UDP address — both data. Two whole sentences, because the
  // "listening …" clause is a statement about where we are listening, not a tail.
  'operate.header.source.aria': 'Signal source',
  'operate.header.source.title':
    'Where decodes come from — active: {{active}}. Native = Nexus decodes local audio; Companion = ride an upstream WSJT-X/JTDX/MSHV decode stream over UDP {{addr}}.',
  'operate.header.source.title.listening':
    'Where decodes come from — active: {{active}} · listening {{addr}}. Native = Nexus decodes local audio; Companion = ride an upstream WSJT-X/JTDX/MSHV decode stream over UDP {{addr}}.',
  'operate.header.source.native.title': 'Native engine — Nexus decodes local audio',
  'operate.header.source.companion.title':
    'Companion — ride an existing WSJT-X / JTDX / MSHV decode stream over UDP {{addr}}',

  // The two DF spinners. `{{label}}` is `Rx` or `Tx` — a direction token supplied by the
  // cockpit, so the sentence reads the same for both without being written twice.
  'operate.header.offsets.aria': 'Audio offsets (Hz)',
  'operate.header.df.title':
    '{{label}} audio offset (Hz) — Enter/blur commits, clamped 200–4000',
  'operate.header.df.aria': '{{label}} offset in Hz',

  // `{{hint}}` is FN_KEY_HINT (platform.ts) — appended on the Mac only, and migrated with
  // that module rather than here.
  'operate.header.decode.label': 'Decode',
  'operate.header.decode.title': 'Re-decode the last period (F6)',
  'operate.header.decode.title.mac': 'Re-decode the last period (F6)\n{{hint}}',

  'operate.header.record.start.aria': 'Record QSO audio',
  'operate.header.record.stop.aria': 'Stop recording this QSO',
  'operate.header.record.start.title':
    'Record the received audio to a WAV in the recordings folder',
  'operate.header.record.stop.title': 'Recording — click to stop recording this QSO',
  // One whole sentence per outcome, never "Could not " plus a verb.
  'operate.header.record.startFailed': 'Could not start recording',
  'operate.header.record.stopFailed': 'Could not stop recording',


  'operate.header.layout.aria': 'Operate layout',
  'operate.header.layout.classic.label': 'Classic',
  'operate.header.layout.classic.title':
    'Classic — WSJT-X layout (Band Activity + Rx Frequency pair, roster aside)',
  'operate.header.layout.roster.label': 'Roster',
  'operate.header.layout.roster.title': 'Roster — GridTracker layout (Call Roster dominant)',

  'operate.header.spot.aria': 'Spot a callsign to the DX cluster',
  'operate.header.spot.title':
    'Spot a callsign to the DX cluster (opens a popup — call, frequency, comment)',
  'operate.header.popOut.aria': 'Open Operate in its own window',
  'operate.header.popOut.title': 'Open Operate in its own window (for a second monitor)',

  // ── Operate ▸ the waterfall strip, the seams and the ⊞ panel names ──────────────────
  'operate.waterfall.redock.label': '⧉ Waterfall popped out — click to re-dock',
  'operate.waterfall.redock.title':
    'The waterfall is in its own window — click to bring it back here',
  'operate.waterfall.splitter.label': 'waterfall height',
  'operate.seam.bandActivityRxFreq.label': 'Band Activity / Rx Frequency',
  'operate.seam.qsocolStations.label': 'Rx Frequency column / Stations roster',

  // The ⊞ menu's entries — the panes' operator-facing names, resolved when the menu is
  // built rather than at import (the registry-by-getter rule, batch 3).
  'operate.panel.waterfall': 'Waterfall',
  'operate.panel.bandActivity': 'Band Activity',
  'operate.panel.callRoster': 'Call Roster',
  'operate.panel.rxfreq': 'Rx Frequency',
  'operate.panel.txmsgs': 'Tx Messages',
  'operate.panel.stations': 'Stations',
  'operate.panel.txmeters': 'TX Meters',

  // The rotor's two answers. `{{call}}` is a callsign, `{{deg}}` a bearing and `{{error}}`
  // the backend's own refusal — all three pass through verbatim.
  'operate.rotor.pointed': 'Rotator → {{call}}: {{deg}}°',
  'operate.rotor.failed': 'Rotator: {{error}}',

  // ── Operate ▸ the merged operating strip ────────────────────────────────────────────
  // ⚠️ Stop TX, Tune, ATU and the TX On/Off tooltip are ABSENT by design — see the block
  // header. What is here is the sequencer, the readouts and the QSO controls around them.
  //
  // Why a mode cannot run a QSO: one sentence each, shown on every control it disables.
  'operate.strip.rxOnly.why':
    'This mode is receive-only in Nexus — it decodes but does not transmit',
  'operate.strip.beacon.why':
    'This is a beacon mode — it transmits your callsign, grid and power on a schedule. There is no QSO sequence. Set the transmit % and power in Settings ▸ Beacons (WSPR & FST4W).',

  'operate.strip.roles.aria': 'Sequencer role',
  'operate.strip.callCq.label': 'Call CQ',
  'operate.strip.callCq.title':
    'Auto CQ — call CQ continuously, work each station that answers with the normal FT8/FT4 sequence, then return to CQ automatically',
  'operate.strip.sandp.label': 'S&P',
  'operate.strip.sandp.title': 'Monitor — search & pounce',
  'operate.strip.txControls.aria': 'Transmit controls',
  'operate.strip.holdTx.label': 'Hold Tx',
  'operate.strip.holdTx.title':
    'Hold Tx Freq: keep your TX offset where you put it when you double-click a station to work them. Off, your TX moves onto theirs (WSJT-X\'s behaviour). A plain waterfall click never moves TX either way.',

  // The state cap — a readout, not a control.
  'operate.strip.state.transmitting': '▲ TRANSMITTING',
  'operate.strip.state.receiving': '▼ Receiving',
  'operate.strip.state.txOff': '■ TX off',

  // The rig moved under us. `{{rigMode}}` is what the radio reports and `{{mode}}` what
  // Nexus commanded — both mode names, and both data.
  'operate.strip.rigDiverge.label': 'rig: {{mode}}',
  'operate.strip.rigDiverge.title':
    'Your rig is on {{rigMode}}, but Nexus has {{mode}}. Something moved it at the radio (SmartSDR, another program, or the mode knob). Transmit and logging use {{mode}} — set the radio to match, or re-pick the band here.',
  'operate.strip.narrowFilter.label': 'filter {{hz}} Hz',
  'operate.strip.narrowFilter.title':
    "The radio's receive filter is {{hz}} Hz — far narrower than an FT8/FT4 window. Signals outside it are not reaching the decoder at all. Widen the filter at the radio (2.4-3 kHz is normal).",

  'operate.strip.autoCq.title':
    'Auto CQ is running — calling CQ continuously, working each station that answers, then returning to CQ for the next one. Click S&P to stop.',
  'operate.strip.report.title': 'Report received about your signal',

  // "Now sending". The three non-message states are whole answers; `txNow` itself is the
  // message on the air and is never translated.
  'operate.strip.now.stalled': 'Stalled',
  'operate.strip.now.rxOnly': '— receive-only, not transmitting',
  'operate.strip.now.beacon': '— beacon: transmits on schedule',
  'operate.strip.now.listening': '— listening',
  // Only ever rendered above one, so English needs a single wording; `{{count}}` is passed
  // so a locale that inflects can answer with plural forms.
  'operate.strip.attempts.title': 'Sent {{count}} times — calling repeatedly',
  'operate.strip.resend.title': 'Re-arm and re-send this message',

  'operate.strip.freetext.placeholder': 'Free text (Tx5)',
  'operate.strip.freetext.aria': 'In-QSO free text',
  'operate.strip.send.label': 'Send',
  'operate.strip.send.title': 'Send on the next over',
  'operate.strip.log.label': 'Log',
  'operate.strip.log.title': 'Log this QSO now',

  // The transmit-cycle button: four WHOLE labels, never a stem plus a period token.
  'operate.strip.period.title':
    'Transmit cycle — click to cycle Auto → Tx 1st → Tx 2nd. Auto picks the opposite cycle of the station you answer; the station you work must be on the OPPOSITE period.',
  'operate.strip.period.auto.first': 'TX AUTO / 1st',
  'operate.strip.period.auto.second': 'TX AUTO / 2nd',
  'operate.strip.period.first': 'TX 1st / even',
  'operate.strip.period.second': 'TX 2nd / odd',
  'operate.strip.skipTx1.label': 'Skip Tx1',
  'operate.strip.skipTx1.title':
    'Skip Tx1 — when you answer a CQ, open with your signal report (Tx2) instead of your grid (Tx1), saving a cycle. Standard callsigns only (a compound call still sends its grid). Resets each launch, like WSJT-X.',
  'operate.strip.nextSlot.label': 'next {{secs}}s',
  'operate.strip.nextSlot.title': 'Time to the next slot',

  // ── Operate ▸ the decode panes (Band Activity / Rx Frequency) ───────────────────────
  'operate.decodes.title': 'Band Activity',
  // `{{hz}}` is the live RX audio offset, rounded by the cockpit and never formatted here.
  'operate.decodes.rxFreq.title': 'Rx Frequency · {{hz}} Hz',
  'operate.decodes.erase.label': 'Erase',
  'operate.decodes.erase.title': 'Erase this pane (WSJT-X Erase)',

  // The filter chips. `CQ`, `CQ+73` and `B4` are Q-code/log tokens and stay in the code;
  // these four are words. Every chip's EXPLANATION is a word.
  'operate.decodes.filters.aria': 'Filter decodes',
  'operate.decodes.filter.all': 'All',
  'operate.decodes.filter.me': 'To me',
  'operate.decodes.filter.rx': 'On RX',
  'operate.decodes.filter.new': 'New',
  'operate.decodes.filter.title.all': 'All decodes',
  'operate.decodes.filter.title.cq': 'CQ calls only',
  'operate.decodes.filter.title.cq73':
    '73 and RR73 signoffs included — a free frequency is about to appear',
  'operate.decodes.filter.title.me': 'Directed to my callsign',
  'operate.decodes.filter.title.rx':
    'On my RX frequency (±50 Hz), plus anything addressed to me — follow a QSO without clutter',
  'operate.decodes.filter.title.b4': 'Worked before',
  'operate.decodes.filter.title.new': 'New DXCC / new grid — the "new one" chase',

  'operate.decodes.hideBlocked.label': '−Blk',
  'operate.decodes.hideBlocked.title':
    'Hide blocked callsigns from this pane (they render dimmed when off). The auto-responder never answers blocked calls regardless — Alt-double-click a row to block or unblock.',
  'operate.decodes.hideConfirmed.label': '−Conf',
  'operate.decodes.hideConfirmed.title':
    "Hide stations whose ENTITY is already confirmed (LoTW/card) on this band — chase what you still need. It is the country that is confirmed, not necessarily this callsign. A station that's new on the band always shows.",
  'operate.decodes.hideB4.title':
    'Hide stations you have already worked (B4) from whichever filter is active — CQ-only minus B4, and friends',
  'operate.decodes.hideB4.title.idle':
    'The B4 chip shows worked stations — the hide switch is idle there',

  // The sort picker. Its `value`s are stored tokens; `SNR` and `DT` are column tokens and
  // stay in the code, so only these two labels are words.
  'operate.decodes.sort.label': 'sort',
  'operate.decodes.sort.time': 'Time',
  'operate.decodes.sort.freq': 'Freq',

  // `{{count}}` is how many rows the pane is showing.
  'operate.decodes.heard': '{{count}} heard',
  'operate.decodes.reviewing': '▲ reviewing — scroll to bottom to follow',
  // `decode(s)` is the shipped English and stays as written; a locale may answer with
  // plural forms, which is what `{{count}}` is for.
  'operate.decodes.harq.title': 'IR-HARQ recovered {{count}} decode(s) this session',

  'operate.decodes.list.aria':
    'Decoded stations — arrow to move, Enter to select, Shift+Enter to work',
  // TWO empty states, deliberately different: nothing decoded at all, versus a filter
  // hiding what the history holds.
  'operate.decodes.empty.title': 'No decodes yet',
  'operate.decodes.empty.detail':
    'Waiting for the next slot — decoded signals will appear here as they arrive.',
  'operate.decodes.emptyFiltered.title': 'Nothing matches “{{filter}}”',
  'operate.decodes.emptyFiltered.detail':
    '{{count}} decodes in history are hidden by the current filter — pick another chip to see them.',
  // `{{time}}` is the period's UTC start, formatted invariantly by decodeHistory.
  'operate.decodes.period.aria': 'Period {{time}} UTC',

  // A row, read aloud. Everything in it is data; the two optional clauses are interpolated
  // whole, each carrying its own separator, so no language is served four fragments.
  'operate.decodes.row.aria': '{{call}}, {{snr}} dB, {{hz}} hertz, {{message}}{{country}}{{azimuth}}',
  'operate.decodes.row.aria.country': ', {{country}}',
  'operate.decodes.row.aria.azimuth': ', {{deg}} degrees',
  'operate.decodes.row.aria.azimuth.approx': ', about {{deg}} degrees',
  'operate.decodes.row.title': 'Click to select {{call}} · double-click to work{{highlight}}',
  'operate.decodes.row.highlighted': ' · highlighted by your logger (UDP)',

  'operate.decodes.tier.title': 'Decoded by {{tier}}',
  'operate.decodes.utc.title': 'UTC heard',
  'operate.decodes.dt.title': 'DT — time offset (s); large = clock/sync skew',
  'operate.decodes.dt.title.msk144': 'T — when in the period the ping landed (s)',
  'operate.decodes.marker.lowConf.title': 'Low-confidence decode',
  'operate.decodes.marker.ap.title': 'AP-assisted decode',
  'operate.decodes.needs.aria': 'needs',
  'operate.decodes.harqRv.title': 'Recovered by IR-HARQ (RV0–RV{{rv}})',
  'operate.decodes.tag.you': 'YOU',
  'operate.decodes.lotw.title':
    'Uploads to LoTW — a QSO with {{call}} should confirm (ARRL activity list)',
  'operate.decodes.lotw.thisStation': 'this station',

  // ── Operate ▸ the Call Roster ───────────────────────────────────────────────────────
  'operate.roster.title': 'Call Roster',
  'operate.roster.filter.neededOnly': 'Needed only',
  'operate.roster.filter.hideWorked': 'Hide worked',
  'operate.roster.filter.hideWorked.title':
    'Hide stations you have already worked — EXCEPT the ones that still fill a need, which stay on the list. That is why a B4 chip can survive this filter: you worked that call on another band or mode, and it is still a new slot here. Turn on Needed only to see just those.',
  'operate.roster.filter.hideBlocked': 'Hide blocked',
  'operate.roster.filter.hideBlocked.title':
    'Drop blocked callsigns from the roster entirely (unchecked: they render dimmed). Alt-double-click a row to block or unblock; the auto-responder never answers blocked calls either way.',
  // Two whole labels: the button names the station when there is one to name.
  'operate.roster.spot.label': 'Spot',
  'operate.roster.spot.label.call': 'Spot {{call}}',
  'operate.roster.spot.title': 'Spot {{call}} to the DX cluster at the current dial',
  'operate.roster.spot.title.none': 'Select a station to spot it to the DX cluster',

  'operate.roster.grid.aria': 'Call roster — arrow to move, Enter to select, Shift+Enter to work',
  // `{{column}}` is the header's own word, so the sort hint is written once.
  'operate.roster.sort.title': 'Sort by {{column}}',
  'operate.roster.col.call': 'Call',
  'operate.roster.col.calling': 'Calling',
  'operate.roster.col.calling.title':
    'Sort by who each station is calling (CQ = calling nobody)',
  'operate.roster.col.need': 'Need',
  'operate.roster.col.country': 'Country',
  'operate.roster.col.state': 'State',
  'operate.roster.col.state.title':
    'Sort by state or province (from the callsign, or the heard grid)',
  'operate.roster.col.grid': 'Grid',
  'operate.roster.col.dist': 'Dist',
  'operate.roster.col.bearing': 'Brg',
  'operate.roster.col.age': 'Age',

  'operate.roster.empty': 'No stations heard yet — decoded stations appear here as they arrive.',
  // The row, read aloud — four optional clauses, each interpolated whole with its own
  // separator. `{{need}}` is a need TAG (NewMode, Confirm…), a token like a band name.
  'operate.roster.row.aria': '{{call}}{{grid}}{{need}}{{worked}}{{working}}',
  'operate.roster.row.aria.grid': ', grid {{grid}}',
  'operate.roster.row.aria.need': ', needed {{need}}',
  'operate.roster.row.aria.worked': ', worked',
  'operate.roster.row.aria.working': ', working now',
  'operate.roster.row.work.title': 'Double-click to work {{call}}',
  'operate.roster.lotw.title': 'Uploads to LoTW — this contact should confirm',
  'operate.roster.calling.title': 'Working {{call}}',
  'operate.roster.calling.cq.title': 'Calling CQ — not in a QSO',
  'operate.roster.calling.cqDir.title':
    'Calling CQ {{dir}} — a DIRECTED call. He is asking for {{dir}} only, so answering from anywhere else will usually be ignored.',
  'operate.roster.state.title': '{{call}} is in {{state}}',
  // The Age column. The unit letter rides inside the message with its number, so a
  // translation can never separate the two (the Now-Bar's rule, batch 15).
  'operate.roster.age.now': 'now',
  'operate.roster.age.slots': '{{count}} sl',
  'operate.roster.age.minutes': '{{minutes}}m',

  // ── Operate ▸ shared by both panes ──────────────────────────────────────────────────
  // One wording each, because a decode row and a roster row say the same thing about the
  // same station and reading two is how the pair drifts.
  'operate.row.ignored.title': 'Ignored this session (Alt-double-click to restore)',
  'operate.b4.sameBand': 'Worked before on this band',
  'operate.b4.otherBand': 'Worked before (another band)',

  // ── Operate ▸ the WSJT-X Tx1–Tx6 message machine ────────────────────────────────────
  // The six rows are named `Tx 1`…`Tx 6` in the code — WSJT-X's own slot names, and the
  // tokens the Alt+N hints and this panel's own label point at.
  'operate.tx.aria': 'Standard messages (Tx1–Tx6)',
  'operate.tx.dxCall.label': 'DX Call',
  'operate.tx.dxCall.aria': 'DX callsign',
  'operate.tx.dxGrid.label': 'DX Grid',
  'operate.tx.dxGrid.aria': 'DX grid locator',
  'operate.tx.generate.label': 'Generate Std Msgs',
  'operate.tx.generate.title':
    'Generate the six standard messages from DX Call / Grid / report (WSJT-X Generate Std Msgs)',
  'operate.tx.clear.label': 'Clear',
  'operate.tx.clear.title': 'Clear DX Call + Grid (F4)',
  'operate.tx.rows.aria': 'Tx message rows',
  'operate.tx.next.title': 'Queued as the next transmission',
  'operate.tx.tx5.placeholder': 'Free text',
  'operate.tx.tx5.aria': 'Tx5 free text',
  'operate.tx.tx6.placeholder': 'CQ call',
  'operate.tx.tx6.aria': 'Tx6 Call CQ (edit for a directed CQ)',
  // The directed-CQ tokens are what goes on the air; they stay as written.
  'operate.tx.tx6.hint': 'Edit for a directed CQ — CQ DX / CQ NA / CQ POTA / CQ TEST',
  'operate.tx.callCq.title': 'Call CQ (Alt+6)',
  'operate.tx.send.title': 'Send this as the next transmission (Alt+{{n}})',

  // ══════════════════════════════════════════════════════════════════════════════════════
  // THE KEYBOARD AND PICTURE COCKPITS — RTTY, PSK and SSTV.
  // ══════════════════════════════════════════════════════════════════════════════════════
  //
  // Three cockpits with one shape: a header, a band waterfall, one content pane and a TX
  // dock. RTTY and PSK are the two KEYBOARD modes and their surfaces are deliberately the
  // same surface (PSK was built from RTTY's, verbatim) — but each keeps its OWN keys, per
  // the rule at the foot of this file: a shared key that two surfaces later want to word
  // differently cannot be split without orphaning both translations, and "Arm RX" already
  // explains a different demodulator in each.
  //
  // ⚠️ THE UNITS RULE LANDS ON THE TONES AND THE RASTER here. Every baud rate, shift, mark
  // and space tone, AFC offset in Hz, sub-mode name (PSK31, QPSK31), SSTV mode name and its
  // raster and key-down seconds, VIS code, dial reading, callsign, FSK ID and megapixel
  // count on these screens is data and stays in the code — as do the vocabularies each file
  // gathers as named constants (RTTY's RTTY/RX ▼/TX ▲/CQ, PSK's Bd and its Rev/RX ▼/TX ▲,
  // SSTV's own name and its 'MYCALL'/'TEXT' placeholders), the F-key macro TEXTS (they go on
  // the air), the `value` of every <select>, and the sniffed image formats the picker
  // offers. What moved is the prose around them.
  //
  // ⚠️ AND WHAT IS ABSENT, DELIBERATELY: every control that STOPS a transmission, and the
  // continuous-TX latch beside it. See the PARTIAL block in `hardcoded-strings.test.ts` for
  // the per-file list — those move in the transmit-path batch, with the stop-line sweeps
  // re-run. A refusal TOAST is not a control, so the toasts those controls raise are here.

  // ── RTTY ▸ the cockpit header, its pills and the ⊞ panel names ──────────────────────
  'rtty.panel.waterfall': 'Waterfall',
  'rtty.panel.stream': 'Decoded Text',
  'rtty.header.mode.title':
    'RTTY — Baudot/ITA2 at the configured baud + shift (45.45 / 170 Hz is the HF standard; change it in Settings → RTTY)',
  'rtty.header.backend.fsk.title':
    'True FSK — data bits on the serial keyline, rig in RTTY mode (its narrow RTTY filters work). Change the backend in Settings → RTTY.',
  'rtty.header.backend.afsk.title':
    'AFSK — soundcard tones through the rig in LSB (soundcard-clocked, the robust default). Change the backend in Settings → RTTY.',
  'rtty.header.power.label': 'Power',
  'rtty.header.power.title':
    'RF output power — RTTY keys the carrier for the whole over, so most rigs want well under their SSB rating',
  'rtty.header.band.title': "Showing the rig's current band",
  'rtty.waterfall.hint': 'click nets the decoder',

  // ── RTTY ▸ the decoded-text pane ────────────────────────────────────────────────────
  // The pane's own name is lower-case where the frame prints it and title-case in the ⊞
  // menu, because that is what each surface shipped; two keys rather than one re-cased.
  'rtty.pane.stream.title': 'Decoded text',
  'rtty.pane.log.title': 'Log',
  'rtty.stream.title':
    "Decoded RTTY text — faint characters are low-confidence copy (the demodulator's soft metric)",
  'rtty.arm.on.label': 'RX armed',
  'rtty.arm.off.label': 'Arm RX',
  'rtty.arm.on.title':
    'RX armed — decoding the receive audio (RX only, never keys the rig). Click to disarm.',
  'rtty.arm.off.title':
    'Arm RX — start decoding RTTY from the receive audio (RX only, never keys the rig)',
  'rtty.arm.failed': 'Could not switch the RTTY decoder',
  // The auto-sequencer's own toggle is a stop control (its off-click aborts the QSO and
  // unkeys), so its label and tooltips are NOT here — only what it says when it refuses.
  'rtty.auto.failed': 'Could not switch the RTTY auto-sequencer',
  'rtty.afc.locked.title':
    'AFC locked — acquired the mark/space pair and frozen on it (offset from the nominal tones)',
  'rtty.afc.title':
    'AFC offset from the nominal mark/space tone pair — locks once a signal is acquired',
  'rtty.afcReset.label': 'Re-tune',
  'rtty.afcReset.title':
    'Re-acquire AFC — drop and rebuild the demodulator (use when it froze on the wrong signal)',
  'rtty.clear.label': 'Clear',
  'rtty.clear.title': 'Clear the decoded transcript',
  'rtty.stream.listening': 'listening…',
  'rtty.stream.idle': 'Arm RX to decode RTTY from the receive audio',

  // ── RTTY ▸ the auto-sequencer row ───────────────────────────────────────────────────
  // Its six states are words; `{{call}}` is the callsign the decoder surfaced.
  'rtty.seq.aria': 'RTTY auto-sequencer',
  'rtty.seq.callingCq': 'Calling CQ',
  'rtty.seq.answering': 'Answering',
  'rtty.seq.exchangeSent': 'Exchange sent',
  'rtty.seq.confirmed': 'Confirmed',
  'rtty.seq.done': 'Done',
  'rtty.seq.idle': 'Idle',
  'rtty.autoCq.label': 'Auto call',
  'rtty.autoCq.title':
    'Call CQ and auto-run the QSO — the engine keys only after you click, never on its own',
  'rtty.autoCq.failed': 'Auto CQ refused',
  'rtty.autoAnswer.label': 'Answer',
  'rtty.autoAnswer.title': 'Answer {{call}} and auto-run the exchange (search & pounce)',
  'rtty.autoAnswer.none.title':
    'No CQ heard yet — Answer lights up when the decoder surfaces one',
  'rtty.autoAnswer.failed': 'Auto answer refused',

  // ── RTTY ▸ the macro row and the compose bar ────────────────────────────────────────
  // `CQ` and `73` name themselves and stay in the code; these two are words. The macro
  // TEXTS are what goes on the air and are invariant, tooltip included.
  'rtty.macros.aria': 'RTTY macros',
  'rtty.macro.answer.label': 'Answer',
  'rtty.macro.exchange.label': 'Exchange',
  // ⚠️ `{CALL}` is a macro token the expander matches literally — safe here only because
  // interpolation is `{{double}}`.
  'rtty.hisCall.placeholder': 'Their call…',
  'rtty.hisCall.aria': 'Worked station callsign (the {CALL} macro token)',
  'rtty.compose.aria': 'RTTY compose',
  'rtty.compose.placeholder': 'Type RTTY to send… (Enter)',
  'rtty.compose.placeholder.latched': 'Typing on the air…',
  'rtty.compose.send.label': 'Send',
  'rtty.send.noCallsign': 'Set your callsign in Settings before transmitting',
  'rtty.send.noTheirCall': 'Enter their call first (the {CALL} field)',
  'rtty.send.txLocked': 'TX locked — this frequency is outside your license privileges',
  'rtty.send.failed': 'RTTY send failed',
  'rtty.latch.failed': 'Continuous TX refused',
  'rtty.type.failed': 'RTTY typing refused',

  // ── PSK ▸ the cockpit header, its pills and the ⊞ panel names ───────────────────────
  // The sub-mode NAMES and their one-line hints live in `pskModes.ts` and move with that
  // module; the cockpit interpolates them. `Rev` is the polarity control's own token, so
  // each of its two faces is one whole label rather than a stem plus on/off.
  'psk.panel.waterfall': 'Waterfall',
  'psk.panel.stream': 'Decoded Text',
  'psk.header.mode.aria': 'PSK sub-mode',
  'psk.header.power.label': 'Power',
  'psk.header.power.title':
    'RF output power — key Tune and wind it back until the ALC stops moving; overdriven PSK splatters',
  'psk.mode.failed': 'PSK mode switch refused',
  'psk.rev.on.label': 'Rev on',
  'psk.rev.off.label': 'Rev off',
  'psk.rev.on.title':
    'Reversed polarity (LSB) — decoding and transmitting with the ±90° phase shifts mirrored. Click for normal (USB, the standard).',
  'psk.rev.off.title':
    'Normal polarity (USB, the standard). Click if a QPSK31 station warbles but prints garbage — an LSB station’s phase shifts are mirrored.',
  'psk.header.band.title': "Showing the rig's current band",
  'psk.waterfall.hint': 'click nets the decoder',

  // ── PSK ▸ the decoded-text pane ─────────────────────────────────────────────────────
  'psk.pane.log.title': 'Log',
  'psk.pane.stream.title': 'Decoded text',
  'psk.stream.title':
    "Decoded PSK31 text — faint characters are low-confidence copy (the demodulator's phase-margin metric)",
  'psk.arm.on.label': 'RX armed',
  'psk.arm.off.label': 'Arm RX',
  'psk.arm.on.title':
    'RX armed — decoding the receive audio (RX only, never keys the rig). Click to stop; stopping is remembered for this session.',
  'psk.arm.off.title':
    'Arm RX — start decoding PSK31 from the receive audio (RX only, never keys the rig)',
  'psk.arm.failed': 'Could not switch the PSK decoder',
  'psk.carrier.on.title':
    'Carrier — the decoder reads a PSK signal at its cursor; the AFC offset from the netted frequency is shown (slew-limited, never more than ±25 Hz)',
  'psk.carrier.off.title':
    'No carrier at the cursor yet — click a trace on the waterfall to net the decoder onto it',
  'psk.afcReset.label': 'Re-acquire',
  'psk.afcReset.title':
    'Re-acquire — drop and rebuild the demodulator for a fresh AFC pull from the netted frequency (use when it pulled onto a neighbor)',
  'psk.clear.label': 'Clear',
  'psk.clear.title': 'Clear the decoded transcript',
  'psk.stream.listening': 'listening… click a PSK trace on the waterfall to net the decoder',
  'psk.stream.idle': 'Arm RX to decode PSK31 from the receive audio',

  // ── PSK ▸ the macro row, the compose bar and the drive hint ─────────────────────────
  'psk.macros.aria': 'PSK macros',
  'psk.macro.answer.label': 'Answer',
  'psk.macro.exchange.label': 'Exchange',
  'psk.hisCall.placeholder': 'Their call…',
  'psk.hisCall.aria': 'Worked station callsign (the {CALL} macro token)',
  'psk.compose.aria': 'PSK compose',
  'psk.compose.placeholder': 'Type PSK31 to send… (Enter)',
  'psk.compose.placeholder.latched': 'Typing on the air…',
  'psk.compose.send.label': 'Send',
  'psk.send.noCallsign': 'Set your callsign in Settings before transmitting',
  'psk.send.noTheirCall': 'Enter their call first (the {CALL} field)',
  'psk.send.txLocked': 'TX locked — this frequency is outside your license privileges',
  'psk.send.failed': 'PSK send failed',
  'psk.latch.failed': 'Continuous TX refused',
  'psk.type.failed': 'PSK typing refused',
  'psk.drive.text':
    "Keep the rig's ALC near zero — an overdriven PSK31 signal splatters (IMD). Lower TX audio until the ALC meter barely moves.",
  'psk.drive.title':
    "PSK31 is an amplitude-shaped mode: if the rig's ALC is compressing, the signal splatters into the neighbors (IMD). Nexus keys at a modest drive by default — set TX audio / power so the rig's ALC meter barely moves.",

  // ── SSTV ▸ what the file picker refuses, and why ────────────────────────────────────
  // Positive identification only: an unrecognised header falls through to the decoder, so
  // there is no "unknown format" entry here. The iPhone path names Apple's own menu items —
  // a locale should use the ones that OS actually shows.
  'sstv.refuse.heic':
    "iPhone HEIC photos can't be read here — Nexus has no HEVC decoder. On the iPhone: Settings → Camera → Formats → Most Compatible (new photos are JPEG), or Settings → Photos → Transfer to Mac or PC → Automatic (converts on send). Then re-send this picture.",
  'sstv.refuse.avif':
    'That is an AVIF file. SSTV sends JPEG, PNG, WebP, BMP or GIF — export or save-as one of those.',
  'sstv.refuse.tiff':
    'That is a TIFF file. SSTV sends JPEG, PNG, WebP, BMP or GIF — export or save-as one of those.',
  'sstv.refuse.raw':
    'That is a camera RAW file. SSTV sends JPEG, PNG, WebP, BMP or GIF — export a JPEG from it first.',
  'sstv.refuse.psd':
    'That is a Photoshop file. SSTV sends JPEG, PNG, WebP, BMP or GIF — export or save-as one of those.',
  'sstv.refuse.svg':
    'That is an SVG drawing, not a photo. SSTV sends JPEG, PNG, WebP, BMP or GIF — export it as one of those.',

  // ── SSTV ▸ what the receiver is hearing (the sstvDecodeStatus ladder) ───────────────
  // ⚠️ ONE WHOLE SENTENCE PER STATE, with the "where to tune" clause interpolated WHOLE and
  // carrying its own leading space — it is an appositive the caller either has or has not,
  // not a tail glued onto a stem. `{{freq}}` is a dial reading and `{{mode}}` a mode name,
  // both formatted invariantly by the view; `{{age}}` is a stamped age and `{{vis}}` the VIS
  // code clause, which is a technical token and is built in the view.
  'sstv.rx.where': ' Images on this band appear at {{freq}} {{mode}}.',
  'sstv.rx.off':
    'The receiver is stopped — nothing is being decoded. Press Arm to start it.{{where}}',
  'sstv.rx.nocapture':
    'Listening, but no audio is reaching the decoder at all — the capture device is not delivering anything. Check that Input Device (RX) is the radio; hearing the signal on the speaker does not mean the app is capturing it.',
  'sstv.rx.starting': 'Receiver started — no audio has reached the decoder yet.{{where}}',
  'sstv.rx.unsupported':
    'Heard an SSTV header {{age}} ago in a mode this build cannot decode{{vis}}. The signal and the audio path are fine — Scottie, Martin, Robot and PD images all decode.',
  'sstv.rx.decoded': {
    one: '{{count}} image decoded since arming, last one {{age}} ago. Listening for the next header.',
    other:
      '{{count}} images decoded since arming, last one {{age}} ago. Listening for the next header.',
  },
  'sstv.rx.silent':
    'Audio is arriving but it is silent. If you can hear the signal on the speaker, the app is on a different input — check Input Device (RX), and RX Gain if the level is just low.{{where}}',
  'sstv.rx.listening':
    'Hearing audio, no SSTV header yet — a picture decodes automatically when one starts.{{where}}',
  'sstv.rx.unreachable':
    'Cannot read the receiver state — the app is not answering. The decoder may still be running.',
  'sstv.rx.openAudio': 'Open audio settings',

  // ── SSTV ▸ the header and the RX stage ──────────────────────────────────────────────
  'sstv.panel.waterfall': 'Waterfall',
  // One key each for the two lower panes: the ⊞ menu entry and the frame's own title are
  // the same word for the same pane.
  'sstv.panel.txcompose': 'Transmit',
  'sstv.panel.gallery': 'Gallery',
  'sstv.arm.on.label': 'Armed',
  'sstv.arm.off.label': 'Arm',
  'sstv.arm.on.title':
    'Armed — any VIS header heard auto-decodes and auto-saves to the gallery (RX only). Click to disarm.',
  'sstv.arm.off.title':
    'Arm — auto-decode any VIS header heard on the receive audio (RX only, never transmits)',
  'sstv.arm.failed': 'Could not switch the SSTV receiver',
  // A CONFIGURATION control on the transmit path is not a transmit control (the batch-13
  // ruling): the drive slider moved, Stop did not.
  'sstv.header.power.label': 'Power',
  'sstv.header.power.title': 'RF output power — set it against a Tune carrier, below ALC',
  'sstv.header.mode.title':
    'Detected SSTV mode — fills in (Martin / Scottie / Robot / PD) when the receiver hears a VIS header',
  'sstv.header.band.title': "Showing the rig's current band — SSTV decodes wherever you're tuned",
  'sstv.header.slant.label': 'Slant',
  'sstv.header.slant.title':
    'Slant trim — fine sample-clock correction. Auto-corrected by the decoder; the manual trim comes in a later build.',
  'sstv.header.slant.aria': 'SSTV slant trim (disabled — decoder not wired yet)',
  'sstv.stage.aria': 'SSTV image',
  'sstv.waterfall.hint': 'the band — a picture takes this space when one arrives',
  'sstv.caption.lines': '{{mode}} — {{done}}/{{total}} lines',
  'sstv.caption.decoding': 'decoding {{mode}}…',
  'sstv.caption.decoding.airtime':
    'decoding {{mode}}… the picture lands when the transmission ends (≈{{clock}})',
  // Appended to the caption when the arriving header says the dial is off frequency;
  // `{{hz}}` is the signed offset, already stringified invariantly.
  'sstv.caption.tuneOff': ' · tuning {{hz}} Hz',

  // ── SSTV ▸ the composer ─────────────────────────────────────────────────────────────
  // The preview's accessible name is ONE whole sentence per framing state, with the overlay
  // clause interpolated whole — never a stem plus three tails. `{{w}}`/`{{h}}` are the
  // raster the picture is sent at.
  'sstv.tx.preview.aria.empty': 'No image chosen',
  'sstv.tx.preview.aria.fits':
    'Transmit preview, {{w}}×{{h}} — the picture already fits, no crop needed{{overlays}}',
  'sstv.tx.preview.aria.cropX':
    'Transmit preview, {{w}}×{{h}}. Drag or use the arrow keys to choose which part of the picture is sent (left and right); Home re-centres.{{overlays}}',
  'sstv.tx.preview.aria.cropY':
    'Transmit preview, {{w}}×{{h}}. Drag or use the arrow keys to choose which part of the picture is sent (up and down); Home re-centres.{{overlays}}',
  'sstv.tx.preview.aria.overlays':
    ' Click a text overlay to select it; arrows move it, Delete removes it.',
  'sstv.tx.drop.hint':
    'Drop an image here, or choose one below — any size, resized to the mode for you.',
  'sstv.tx.file.choose': 'Choose image…',
  'sstv.tx.file.change': 'Change image…',
  // The overlay presets. `CQ`, `73` and the `MYCALL` stand-in are what gets PAINTED INTO the
  // picture and stay in the view; only the Reply button and the free-text one are words.
  'sstv.tx.overlay.presets.label': 'Text:',
  'sstv.tx.overlay.reply.label': 'Reply',
  'sstv.tx.overlay.reply.title': 'Reply to {{call}} (the newest FSK ID in the gallery)',
  'sstv.tx.overlay.reply.none.title':
    'Enabled once a station has been received with an FSK ID',
  'sstv.tx.overlay.add.label': '+ Text',
  'sstv.tx.overlay.text.aria': 'Overlay text',
  'sstv.tx.overlay.style.aria': 'Text style',
  'sstv.tx.overlay.style.title':
    "Crisp: the ident's pixel font, proven through the decoder. Banner: big display text with an outline, MMSSTV-style.",
  'sstv.tx.overlay.style.crisp': 'Crisp',
  'sstv.tx.overlay.style.banner': 'Banner',
  'sstv.tx.overlay.size.aria': 'Text size',
  // The swatches: the palette id in `sstvOverlay.ts` is the STORED VALUE, and these are the
  // words a screen reader reads off it.
  'sstv.tx.overlay.color.aria': 'Text colour',
  'sstv.tx.overlay.color.white': 'white',
  'sstv.tx.overlay.color.black': 'black',
  'sstv.tx.overlay.color.yellow': 'yellow',
  'sstv.tx.overlay.color.orange': 'orange',
  'sstv.tx.overlay.color.red': 'red',
  'sstv.tx.overlay.color.green': 'green',
  'sstv.tx.overlay.color.cyan': 'cyan',
  'sstv.tx.overlay.color.blue': 'blue',
  'sstv.tx.overlay.treatment.aria': 'Contrast treatment',
  'sstv.tx.overlay.treatment.title':
    'What keeps the text readable on the far end: a solid plate behind it, or a thick outline around it',
  'sstv.tx.overlay.treatment.plate': 'Plate',
  'sstv.tx.overlay.treatment.outline': 'Outline',
  'sstv.tx.overlay.remove.aria': 'Remove overlay {{text}}',
  'sstv.tx.overlay.remove.title': 'Remove this text',
  // What actually goes out — one line, every figure interpolated: the file name, the source
  // size in its own parentheses, the mode's raster, its name and its key-down time.
  'sstv.tx.name': '{{name}}{{size}} → {{w}}×{{h}} · {{mode}} · {{clock}} key-down',
  // Where the identification is. `{{call}}` is the operator's own callsign.
  'sstv.tx.id.inPicture': "{{call}} — you've said it's already in the picture",
  'sstv.tx.id.inText': '{{call}} in your text · no plate burned in',
  'sstv.tx.id.plate': '{{call}} burned in · top left',
  'sstv.tx.id.missing':
    'No callsign set — SSTV identifies by burning your call into the picture, so it will not transmit without one.',
  'sstv.tx.id.missing.action': 'Set your callsign',
  'sstv.tx.id.missing.where': 'Set one in Settings ▸ Station.',
  'sstv.tx.idopt.label': 'My picture already shows my callsign',
  'sstv.tx.idopt.title':
    'Skip the burned-in callsign for this picture only — use when the image already shows your call, e.g. a pre-made QSO card',
  'sstv.tx.notice.upscale':
    "That picture is {{w}}×{{h}}, smaller than {{mode}}'s {{mw}}×{{mh}} — it will be enlarged and look soft.",
  'sstv.tx.notice.exactFit': 'Already {{w}}×{{h}} — sent pixel for pixel, no crop needed.',
  'sstv.tx.notice.gif': 'Sending the first frame — SSTV transmits one still picture.',
  // `{{magic}}` is the file's first four bytes as hex — naming what it really starts with.
  'sstv.tx.notice.notImage': "That file isn't an image Nexus can read (it starts with {{magic}}).",
  'sstv.tx.notice.damaged':
    "That image is damaged and only decoded partly — Nexus won't transmit half a picture. Try re-exporting it.",
  'sstv.tx.notice.tooLarge':
    "That's a {{w}}×{{h}} image ({{mp}} megapixels) — too large to work with. Export a smaller copy; anything over about 4000 px wide is already far more than SSTV can send.",
  'sstv.tx.pixels.failed': 'Could not read the image pixels',

  // ── SSTV ▸ the transmit dock and what it says ───────────────────────────────────────
  // ⚠️ Stop and the bar's own accessible name are ABSENT by design — see the block header.
  // `{{freq}}` in the ISS prompt is the downlink frequency, a constant in the view.
  'sstv.tx.mode.label': 'Mode',
  'sstv.tx.mode.aria': 'SSTV transmit mode',
  'sstv.tx.mode.title':
    'Transmit mode. VHF/2 m images use PD-120 (ARISS); HF uses Scottie 1 (NA) or Martin 1 (EU).',
  'sstv.tx.send.label': 'Send',
  'sstv.tx.send.noImage.title': 'Choose an image to transmit first',
  'sstv.tx.send.noCallsign.title':
    'Set your callsign in Settings → Station — SSTV identifies by burning it into the picture, and will not transmit without one',
  'sstv.tx.send.title':
    'Transmit this image with {{call}} burned in — switches to Phone (USB/LSB) and keys the rig',
  'sstv.tx.send.noCallsign': 'Set your callsign — SSTV identifies by burning it into the picture',
  'sstv.tx.send.noCallsign.action': 'Set callsign',
  'sstv.tx.iss.confirm':
    '{{freq}} MHz is the ISS SSTV downlink. Transmit only during a sanctioned ARISS uplink event. Send anyway?',
  'sstv.tx.send.failed': 'SSTV send refused',
  'sstv.tx.announce.sending': 'Transmitting SSTV {{mode}}',
  'sstv.tx.announce.finished': 'SSTV transmit finished',
  'sstv.tx.progress': 'TX — {{mode}} · {{clock}} remaining',

  // ── SSTV ▸ the gallery ──────────────────────────────────────────────────────────────
  // `{{mode}}` is an SSTV mode name and `{{when}}` a UTC stamp; both arrive formatted.
  'sstv.gallery.empty':
    'Received images collect here — auto-saved with callsign (FSK ID), mode, frequency, and time.',
  'sstv.gallery.thumb.alt': '{{mode}} image received {{when}}',
  'sstv.gallery.delete.aria': 'Delete the {{mode}} image received {{when}}',
  'sstv.gallery.delete.title': 'Delete this image',
  'sstv.gallery.delete.confirm.title': 'Delete the {{mode}} received {{when}}?',
  'sstv.gallery.delete.confirm.body': 'The image file is removed and cannot be recovered.',
  'sstv.gallery.delete.confirm.label': 'Delete image',
  'sstv.gallery.delete.failed': 'Could not delete the image',
  'sstv.gallery.edit.aria': 'Edit and resend the {{mode}} image received {{when}}',
  'sstv.gallery.edit.title': 'Load this image into the composer',
  'sstv.gallery.edit.loaded': 'Loaded {{mode}} image into the composer',
  'sstv.gallery.edit.failed': 'Could not load that image into the composer',

  // ══════════════════════════════════════════════════════════════════════════════════════
  // THE CW COCKPIT — the keyer, the zero-beat scope, the decode and the F-key dock.
  // ══════════════════════════════════════════════════════════════════════════════════════
  //
  // One cockpit, four surfaces: a header that carries the keyer (back-end, speed, pitch,
  // macro profile, filter width), a scope strip that is either the rig's own panadapter or
  // the CW-narrow audio view, a pane region (decode, sent echo, rig controls, band activity,
  // copilot, log) and a TX dock of F-key macros with a type-ahead send bar.
  //
  // ⚠️ THE UNITS RULE LANDS ON THE KEY AND THE DIAL. Every WPM figure, sidetone pitch in Hz,
  // filter width in Hz, scope span and reference level in dB/dBm, dial reading, callsign,
  // RST and bearing on this screen is data and stays in the code — as do the vocabulary the
  // file gathers as named constants (the CW mode badge, the WPM/dB/dBm units, the rig's own
  // DSP/NR/AGC/BW group names, the CAT and WinKeyer back-end names, the SPLIT ▲ and REC
  // plates), the F-key macro TEXTS with their {MYCALL}/{RST}/{NAME}/{EXCH} tokens, the ± zoom
  // presets, and the `value` of every <select>. What moved is the prose around them.
  //
  // Nothing that STOPS a transmission is here, and nothing was deferred either: CW's stop
  // line is Stop TX (→ stopCw + haltTx) and Tune, both drawn by `CockpitHeader`, plus Esc,
  // which is a window keydown with no string at all. The F-key macros and the send bar are
  // SENDERS, which move normally (the batch-18 ruling), and the keyer back-end, speed and
  // pitch controls are CONFIGURATION on the transmit path, which moves exactly as PTT Method
  // and the drive slider did (the batch-13 ruling).

  // ── CW ▸ the ⊞ panel names and the pane frames ──────────────────────────────────────
  // Two spellings on purpose, as RTTY has: the ⊞ menu names the pane in title case and says
  // which cockpit's Decode it is, while the frame head above the pane prints the short word.
  'cw.panel.scope': 'Scope',
  'cw.panel.scopeCtl': 'Scope Controls',
  'cw.panel.dsp': 'DSP Toggles',
  'cw.panel.txmeters': 'TX Meters',
  'cw.panel.rxdsp': 'RX DSP Levels',
  'cw.panel.bandActivity': 'Band Activity',
  'cw.panel.copilot': 'CW Copilot',
  'cw.panel.decode': 'CW Decode',
  'cw.panel.sent': 'Sent Echo',
  'cw.pane.decode.title': 'Decode',
  'cw.pane.sent.title': 'Sent',
  'cw.pane.rigctl.title': 'Rig controls',
  'cw.pane.bandActivity.title': 'Band activity',
  'cw.pane.copilot.title': 'Copilot',
  'cw.pane.log.title': 'Log',

  // ── CW ▸ the header: the mode badge, speed, keyer, pitch, macro profile, filter ──────
  'cw.header.mode.title': "The rig is set to CW while you're in this section",
  'cw.wpm.label': 'Speed',
  'cw.wpm.aria': 'CW keyer speed (WPM)',
  // Two whole sentences, not a stem plus a Mac tail: compact Mac keyboards have no
  // PgUp/PgDn and Fn+↑/Fn+↓ is what sends them, which is a second statement.
  'cw.wpm.title': 'Keyer speed — PgUp/PgDn to nudge (Shift = ±4)',
  'cw.wpm.title.mac': 'Keyer speed — PgUp/PgDn to nudge (Shift = ±4) · on a Mac: Fn+↑/Fn+↓',
  'cw.keyer.label': 'Keyer',
  'cw.keyer.aria': 'CW keyer back-end',
  // The four back-end LABELS: `CAT` is a protocol and `WinKeyer` a product, so both stay in
  // the code; these two are words. Every `value` is the stored token and never moves.
  'cw.keyer.serial.label': 'Serial',
  'cw.keyer.soundcard.label': 'Soundcard',
  // What each back-end IS and what it needs — carried on the select AND on each option. The
  // soundcard entry is the one that stops an operator keying a tone through SSB with nothing
  // routed and the drive over ALC, so it is operating information, not chrome.
  'cw.keyer.cat.title': 'CAT keyer — the rig generates CW (rig in CW). Zero extra hardware.',
  'cw.keyer.serial.title':
    "Serial keyline — Nexus toggles DTR/RTS into the rig's KEY jack (rig in CW, rig shapes the signal). The clean N1MM/fldigi method for rigs without CAT CW. Set the keyline port + line in Settings ▸ CW.",
  'cw.keyer.winkeyer.title':
    'K1EL WinKeyer — hardware keyer over serial (rig in CW). Set its port in Settings ▸ CW.',
  'cw.keyer.soundcard.title':
    "Soundcard keyer — a keyed audio tone (this TAKES THE RADIO OUT OF CW, into a data mode like FT8 uses; CW mode returns when you pick another keyer). A workaround: works ONLY if Nexus's audio output is routed to the rig (like FT8) AND PTT works, and you must keep drive below ALC. WinKeyer or the serial keyline are the clean options.",
  'cw.pitch.label': 'Pitch',
  'cw.pitch.aria': 'CW pitch (Hz)',
  'cw.pitch.title': "Sidetone / zero-beat pitch (Hz) — the scope's dashed marker",
  'cw.macroProfile.label': 'Macros',
  'cw.macroProfile.aria': 'CW macro profile',
  'cw.macroProfile.title':
    'CW macro profile — your active F-key set (edit sets in Settings ▸ CW)',
  // What an unnamed profile is called in the picker. `{{n}}` is its position, invariant.
  'cw.macroProfile.unnamed': 'Profile {{n}}',
  'cw.macroProfile.failed': 'Could not switch macro profile',
  'cw.filter.title': 'RX filter / passband width (CAT) — narrow to dig CW out of QRM',
  // `{{step}}` is the nudge in Hz — supplied by the call site, never written here.
  'cw.filter.narrower.title': 'Narrower (−{{step}} Hz)',
  'cw.filter.wider.title': 'Wider (+{{step}} Hz)',
  'cw.filter.failed': 'Could not set filter width',
  // `{{call}}` is a callsign, `{{bearing}}` a heading in degrees and `{{error}}` the engine's
  // own words — all three arrive invariant.
  'cw.rotator.pointed': 'Rotator → {{call}}: {{bearing}}°',
  'cw.rotator.failed': 'Rotator: {{error}}',
  'cw.record.start.aria': 'Record QSO audio',
  'cw.record.stop.aria': 'Stop recording this QSO',
  'cw.record.off.title': 'Record the received audio to a WAV in the recordings folder',
  'cw.record.on.title': 'Recording — click to stop recording this QSO',
  'cw.record.startFailed': 'Could not start recording',
  'cw.record.stopFailed': 'Could not stop recording',

  // ── CW ▸ the scope strip and its zoom ───────────────────────────────────────────────
  // The strip is the rig's real RF panadapter when one streams and the CW-narrow audio view
  // otherwise, so each state names itself. `{{lo}}` and `{{hi}}` are the audio window's edges
  // in Hz and `{{khz}}` a zoom preset — all three are supplied by the call site.
  'cw.scope.tuneHint': 'Scroll here to tune the VFO',
  'cw.scope.nativeRf.label': 'RF Panadapter',
  'cw.scope.nativeRf.title': 'Native RF panadapter — the real RF spectrum around your dial.',
  'cw.scope.audio.label': 'CW audio',
  'cw.zeroBeat.label': 'Zero beat',
  'cw.zeroBeat.aria': 'Zero-beat tuning indicator',
  'cw.zeroBeat.none': 'no signal',
  'cw.zeroBeat.locked': 'ON PITCH',
  'cw.zeroBeat.title': 'Where the received tone sits against your CW pitch. The light comes on within {{tol}} Hz. Tune until the needle centres — it runs the same way as the scope below it. Display only: it never moves your radio.',
  'cw.scope.audio.title':
    'Receiver AUDIO centered on your CW pitch ({{lo}}–{{hi}} Hz) — tune a signal onto the dashed hairline, mid-screen, to zero-beat it.',
  'cw.scope.audio.sub': 'zero-beat',
  'cw.scope.colors.label': 'Colors',
  'cw.scope.splitter.label': 'scope height',
  'cw.rfZoom.aria': 'Panadapter zoom',
  'cw.rfZoom.full.label': 'Full',
  'cw.rfZoom.full.title': "The rig's whole scope sweep (set the width on the radio)",
  'cw.rfZoom.span.title': '±{{khz}} kHz around your dial',

  // ── CW ▸ the rig-control strip (scope controls, DSP toggles, RX DSP levels) ──────────
  // ⚠️ `Rig\u00a0scope` and `Flex\u00a0pan` carry a NON-BREAKING SPACE, written as an
  // escape so it cannot be lost to a careless edit: each is one chip label whose two words
  // must not be split across a line, and a translation keeps it. `{{span}}` is a sweep width
  // the call site prints.
  'cw.rigScope.aria': 'Rig scope control',
  'cw.rigScope.label': 'Rig\u00a0scope',
  'cw.rigScope.title': "These command the radio's own scope, not just the on-screen zoom",
  'cw.rigScope.span.title': "Set the radio's scope span to {{span}}",
  'cw.rigScope.ref.title': 'Scope reference level — lower to lift weak signals out of the noise',
  'cw.rigScope.ref.aria': 'Scope reference level (dB)',
  'cw.flexPan.aria': 'Flex panadapter control',
  'cw.flexPan.label': 'Flex\u00a0pan',
  'cw.flexPan.title':
    "These command the FlexRadio's real SmartSDR panadapter, not just the on-screen zoom",
  'cw.flexPan.span.title': 'Set the Flex panadapter bandwidth to {{span}}',
  'cw.flexPan.ref.title':
    'Panadapter reference level (dBm) — lower to lift weak signals out of the noise',
  'cw.flexPan.ref.aria': 'Flex panadapter reference level (dBm)',
  // One word, one key: both reference sliders are labelled for the same thing.
  'cw.scope.ref.label': 'Ref',
  // The DSP function NAMES (NB, NR, Notch, AGC) are the rig's own and stay in the code;
  // `{{func}}` is the one the toggle failed on.
  'cw.dsp.aria': 'Rig DSP functions',
  'cw.dsp.nb.title': 'Noise Blanker — kills impulse/ignition noise (RX)',
  'cw.dsp.nr.title': 'Noise Reduction — pulls a tone out of broadband hiss (RX, DSP)',
  'cw.dsp.notch.title': 'Auto-Notch (ANF) — nulls a competing carrier (RX, DSP)',
  'cw.dsp.toggleFailed': 'Could not toggle {{func}}',
  'cw.rxDsp.aria': 'RX DSP levels',
  'cw.rxDsp.nr.title':
    'Noise-reduction depth — raise until the noise floor drops, back off if the tone gets watery',
  'cw.rxDsp.nr.aria': 'Noise-reduction level',
  'cw.rxDsp.agc.aria': 'AGC speed',
  'cw.rxDsp.agc.title': 'AGC time constant — Fast for CW/pileups, Slow for steady copy',
  // The five chips are words over stored tokens ('auto' / 'fast' / 'mid' / 'slow' /
  // 'off' — `Engine::AGC_SPEEDS`, and the order they render in).
  'cw.rxDsp.agc.auto': 'Auto',
  'cw.rxDsp.agc.fast': 'Fast',
  'cw.rxDsp.agc.mid': 'Mid',
  'cw.rxDsp.agc.slow': 'Slow',
  'cw.rxDsp.agc.off': 'Off',

  // ── CW ▸ the decode pane and the sent echo ──────────────────────────────────────────
  // `{{window}}` is the AI decoder's audio window in Hz, supplied by the call site.
  'cw.decode.ai.badge': 'AI',
  'cw.decode.ai.on.title': 'AI decoder on — click for the classic pitch decoder',
  'cw.decode.ai.off.title': 'AI decoder off (classic pitch decoder) — click to turn AI on',
  'cw.decode.clear.label': 'Clear',
  'cw.decode.clear.title': 'Clear the decoded + sent transcript',
  'cw.decode.title':
    'Live CW decode — the AI (neural-net) decoder reads the whole {{window}} Hz window, far better weak-signal copy than a pitch-tracking decoder. Turn AI off to fall back to the classic decoder.',
  'cw.decode.log.aria': 'Decoded CW',
  'cw.decode.listening': 'listening…',
  'cw.sent.title': "What you've transmitted (F-key macros expanded to the real text)",

  // ── CW ▸ the copilot chips ──────────────────────────────────────────────────────────
  'cw.copilot.working.label': 'Working',
  'cw.copilot.heard.label': 'Heard',
  'cw.copilot.empty': 'Decoded calls appear here…',
  'cw.copilot.worked.title': "The station you're working — the F-keys + log use this",
  'cw.copilot.work.title': 'Work {{call}} — set it for the F-keys + log',

  // ── CW ▸ the TX dock: the F-key macros and the send bar ─────────────────────────────
  // The macro TEXTS are what goes on the air and are invariant, every character of them; so
  // are the labels that are on-air shorthand (CQ, 73, AGN, TU, CQ FD, ?). These five are
  // words, and each names a MEANING both macro sets share — the casual and the Field Day set
  // both have a "Call".
  'cw.macros.aria': 'CW macros',
  'cw.macro.call.label': 'Call',
  'cw.macro.reply.label': 'Reply',
  'cw.macro.exch.label': 'Exch',
  'cw.macro.myCall.label': 'My Call',
  'cw.macro.hisCall.label': 'His Call',
  'cw.compose.placeholder': 'Type CW to send… (Enter)',
  'cw.compose.send.label': 'Send',
  'cw.send.txLocked': 'TX locked — this frequency is outside your license privileges',
  'cw.send.failed': 'CW send failed',

  // ══════════════════════════════════════════════════════════════════════════════════════
  // PHONE — the voice cockpit and the voice keyer pane inside it.
  // ══════════════════════════════════════════════════════════════════════════════════════
  //
  // ⚠️ THE DENSEST TRANSMIT FILE IN THE TREE, and what is NOT here says as much as what is:
  // the PTT row in the pinned dock — the button's four labels, its three-armed tooltip, the
  // Lock toggle beside it and the Field Day exchange chip that shares the row — stays written
  // in `components/PhoneCockpit.tsx`, and so do the voice keyer's ■ Stop and ■ Stop & save.
  // PTT is Phone's stop-line census (features/panelState.ts) and `stop-line.test.tsx` finds it
  // by ACCESSIBLE NAME, matching all four labels; those move in the transmit-path batch with
  // the sweeps re-run. What DID move is everything around them, the refusal TOASTS the
  // deferred controls raise included — a toast is not a control and no sweep can see one
  // (the batch-19 ruling).
  //
  // The units rule lands on the PASSBAND: every dial reading, split offset in kHz, filter and
  // scope width in Hz, reference level in dB/dBm, mic/power/NR percentage, sideband and mode
  // name, and the rig's own control-group names (DSP, NR, AGC, BW, REC, SPLIT) are invariant
  // and stay in the code.

  // ── Phone ▸ the ⊞ panel names and the pane frames ────────────────────────────────────
  // Two spellings on purpose, as CW and RTTY have: the ⊞ menu names the pane in title case
  // and the frame head above the pane prints the short word.
  'phone.panel.scope': 'Scope',
  'phone.panel.rigscope': 'Rig Scope Controls',
  'phone.panel.txmeters': 'TX Meters',
  'phone.panel.dsp': 'DSP Functions',
  'phone.panel.dspLevels': 'RX DSP Levels',
  'phone.panel.bandActivity': 'Band Activity',
  'phone.panel.voiceKeyer': 'Voice Keyer',
  'phone.pane.bandActivity.title': 'Band activity',
  'phone.pane.voiceKeyer.title': 'Voice keyer',
  'phone.pane.rigscope.title': 'Rig scope controls',
  'phone.pane.dsp.title': 'DSP functions',
  'phone.pane.dspLevels.title': 'RX DSP levels',
  'phone.pane.log.title': 'Log',

  // ── Phone ▸ the header: the mode picker, split, mic gain, filter and REC ─────────────
  // `AUTO`, `USB`, `LSB` and `FM` are mode names — the buttons print them from the code and
  // `{{mode}}`/`{{sideband}}` carry them into these sentences unchanged.
  'phone.mode.aria': 'Phone mode',
  'phone.mode.auto.title':
    'AUTO — sideband by band (now {{sideband}}); a band change re-asserts this',
  'phone.mode.force.title': 'Force {{mode}} until you change bands',
  'phone.mode.failed': 'Could not set mode',
  'phone.header.power.label': 'Power',
  'phone.header.power.title': 'RF output power',
  // The mic-is-dead warning: native Flex DAX transmit audio is a RADIO-WIDE setting, so this
  // screen is the only one that can say the physical microphone is disconnected.
  'phone.micDax.label': 'mic off (DAX)',
  'phone.micDax.title':
    'Flex native DAX audio is on, so the radio takes transmit audio from DAX and your microphone is disconnected — on every slice and in every program, SmartSDR included. Turn OFF Flex native DAX audio in Settings ▸ Radio ▸ Rig & CAT to use the mic.',
  // `{{rigMode}}` is what the rig reports over CAT and `{{mode}}` what Phone commands.
  'phone.rigMismatch.chip': 'rig: {{mode}}',
  'phone.rigMismatch.title':
    "Your rig is on {{rigMode}}, but Phone is set to {{mode}}. Logging and TX use {{mode}} — turn the rig's mode knob (or re-pick the band) to match.",
  // `{{freq}}` is the split TX dial and `{{step}}` the nudge in kHz — both supplied by the
  // call site, never written here.
  'phone.split.on.title': 'Split ON — TX {{freq}} MHz. Click for simplex.',
  'phone.split.off.title': 'Work split — TX off your RX frequency (e.g. up 5)',
  'phone.split.lower.title': 'TX {{step}} kHz lower',
  'phone.split.higher.title': 'TX {{step}} kHz higher',
  'phone.split.offset.title': 'TX offset from your RX dial (kHz)',
  'phone.split.setFailed': 'Could not set split',
  'phone.split.clearFailed': 'Could not clear split',
  'phone.noCat.label': '⚠ no rig control',
  'phone.noCat.title':
    'No CAT link — set a rigctld/CAT rig in Settings so the app can switch the mode and follow the dial. On VOX/RTS-DTR PTT the rig has no command channel.',
  'phone.mic.label': 'Mic',
  'phone.mic.aria': 'Mic gain',
  'phone.mic.title': 'Microphone gain — raise it until SSB peaks tickle the ALC zone',
  'phone.filter.title': 'RX filter / passband width (CAT)',
  'phone.filter.narrower.title': 'Narrower (−{{step}} Hz)',
  'phone.filter.wider.title': 'Wider (+{{step}} Hz)',
  'phone.filter.failed': 'Could not set filter width',
  'phone.record.start.aria': 'Record QSO audio',
  'phone.record.stop.aria': 'Stop recording this QSO',
  'phone.record.off.title': 'Record the received audio to a WAV in the recordings folder',
  'phone.record.on.title': 'Recording — click to stop recording this QSO',
  // Two whole sentences, not a verb spliced into a stem: the toast names the act that failed.
  'phone.record.startFailed': 'Could not start recording',
  'phone.record.stopFailed': 'Could not stop recording',
  // One string, two places: the meter's own accessible name and the label wrapping it.
  'phone.rxMeter.label': 'RX audio level',

  // ── Phone ▸ what the PTT handler says when it refuses, or puts the switch back up ────
  // The BUTTON stays written in the cockpit (it is the stop-line census); these are the
  // toasts it raises, and a toast is neither a control nor anything a sweep can see.
  'phone.tx.locked': 'TX locked — this frequency/mode is outside your license privileges',
  'phone.tx.turnedBackOn': 'TX was off — turned it back on. Press PTT again to talk.',

  // ── Phone ▸ the scope strip, its span chips and its zoom ─────────────────────────────
  // The strip is the rig's real RF panadapter when one streams and the receiver's audio
  // passband otherwise, so each state names itself. `{{khz}}` and `{{hz}}` are the preset
  // widths — figures, supplied by the call site rather than written into the sentence.
  'phone.scope.tuneHint': 'Scroll here to tune the VFO',
  'phone.scope.nativeRf.label': 'RF Panadapter',
  'phone.scope.nativeRf.title':
    'Native RF panadapter — the real RF spectrum around your dial, not the demodulated audio passband.',
  'phone.scope.audio.label': 'Passband',
  'phone.scope.audio.title':
    'Receiver AUDIO spectrum on your rig’s axis: the centre line is your dial, and the passband sits on the side your sideband is on (USB above, LSB below) — the other half is quiet because an SSB receiver only hears one side. Not a band-wide RF panadapter, so a voice fills the passband rather than sliding across it as you tune.',
  'phone.scope.audio.sub': 'RX audio',
  'phone.scope.splitter.label': 'scope height',
  'phone.scope.span.aria': 'Bandscope span',
  'phone.span.auto.label': 'Auto',
  'phone.span.auto.title': "Follows the radio's filter — the scope shows what the rig can pass",
  'phone.span.full.label': 'Full',
  'phone.span.full.title': 'The whole captured passband — {{khz}} kHz of sideband from your dial',
  'phone.span.voice.label': 'Voice',
  'phone.span.voice.title': 'Voice energy — {{khz}} kHz of sideband from your dial',
  'phone.span.zoom.title': 'Zoomed — {{khz}} kHz of sideband from your dial',
  'phone.span.tight.title': 'Tight — {{hz}} Hz of sideband from your dial, for fine tuning',
  'phone.rfZoom.aria': 'Panadapter zoom',
  'phone.rfZoom.full.label': 'Full',
  'phone.rfZoom.full.title': "The rig's whole scope sweep (set the width on the radio)",
  'phone.rfZoom.span.title': '±{{khz}} kHz around your dial',

  // ── Phone ▸ the rig-control strip (scope controls, DSP toggles, RX DSP levels) ───────
  // ⚠️ `Rig\u00a0scope` and `Flex\u00a0pan` carry a NON-BREAKING SPACE, written as an escape
  // so it cannot be lost to a careless edit: each is one chip label whose two words must not
  // be split across a line, and a translation keeps it. `{{span}}` is a sweep width the call
  // site prints.
  'phone.rigScope.aria': 'Rig scope control',
  'phone.rigScope.label': 'Rig\u00a0scope',
  'phone.rigScope.title': "These command the radio's own scope, not just the on-screen zoom",
  'phone.rigScope.span.title': "Set the radio's scope span to {{span}}",
  'phone.rigScope.ref.title':
    'Scope reference level — lower to lift weak signals out of the noise',
  'phone.rigScope.ref.aria': 'Scope reference level (dB)',
  'phone.flexPan.aria': 'Flex panadapter control',
  'phone.flexPan.label': 'Flex\u00a0pan',
  'phone.flexPan.title':
    "These command the FlexRadio's real SmartSDR panadapter, not just the on-screen zoom",
  'phone.flexPan.span.title': 'Set the Flex panadapter bandwidth to {{span}}',
  'phone.flexPan.ref.title':
    'Panadapter reference level (dBm) — lower to lift weak signals out of the noise',
  'phone.flexPan.ref.aria': 'Flex panadapter reference level (dBm)',
  // One word, one key: both reference sliders are labelled for the same thing.
  'phone.scope.ref.label': 'Ref',
  // The DSP function NAMES (NB, NR, Notch, COMP, VOX) are the rig's own and stay in the code;
  // `{{func}}` is the one the toggle failed on.
  'phone.dsp.aria': 'Rig DSP functions',
  'phone.dsp.nb.title': 'Noise Blanker — kills impulse/ignition noise (RX)',
  'phone.dsp.nr.title': 'Noise Reduction — pulls voice out of broadband hiss (RX, DSP)',
  'phone.dsp.notch.title': 'Auto-Notch (ANF) — nulls carriers/heterodynes (RX, DSP)',
  'phone.dsp.comp.title': 'Speech Compressor — more average talk power (TX)',
  'phone.dsp.manualNotch.title':
    'Manual notch — the one you place yourself on a whistle, using the notch frequency slider. Distinct from Notch, which hunts a carrier automatically.',
  'phone.dsp.vox.title': 'Voice-Operated Transmit — hands-free keying (TX)',
  'phone.dsp.toggleFailed': 'Could not toggle {{func}}',
  'phone.rxDsp.aria': 'RX DSP levels',
  'phone.rxDsp.nr.title':
    'Noise-reduction depth — raise until the noise floor drops, back off if audio gets watery',
  'phone.rxDsp.nr.aria': 'Noise-reduction level',
  'phone.rxDsp.comp.title':
    'Speech processor depth — how hard the compressor works. Raise it for punch on a weak path; too much sounds harsh and splatters.',
  'phone.rxDsp.comp.aria': 'Speech processor depth',
  'phone.rxDsp.notchFreq.title':
    'Manual notch frequency — slide it onto the whistle you want gone. This is the notch you place yourself, not the automatic one.',
  'phone.rxDsp.notchFreq.aria': 'Manual notch frequency in hertz',
  'phone.rxDsp.agc.aria': 'AGC speed',
  'phone.rxDsp.agc.title': 'AGC time constant',
  // The five chips are words over stored tokens ('auto' / 'fast' / 'mid' / 'slow' /
  // 'off' — `Engine::AGC_SPEEDS`, and the order they render in).
  'phone.rxDsp.agc.auto': 'Auto',
  'phone.rxDsp.agc.fast': 'Fast',
  'phone.rxDsp.agc.mid': 'Mid',
  'phone.rxDsp.agc.slow': 'Slow',
  'phone.rxDsp.agc.off': 'Off',

  // ── Phone ▸ the voice keyer pane ─────────────────────────────────────────────────────
  // F1–F6 are key names and `{{slot}}` is the number one of them carries; `{{label}}` is the
  // operator's own name for a slot and is never translated. `{{hint}}` is the Mac Fn-key note
  // (platform.ts). The pane's two ■ Stop buttons are NOT here — see the banner above.
  'phone.keyer.hint': 'click or press F1–F6 to send · Esc stops',
  'phone.keyer.hint.mac': 'click or press Fn+F1–F6 to send · Esc stops',
  // `{{exchange}}` is the Field Day class + section, a token; the element around it comes
  // from the call site.
  'phone.keyer.fd.hint':
    'Field Day: record a slot with your exchange <b>“{{exchange}}”</b> for one-key sends.',
  'phone.keyer.slot.play.title': 'Play F{{slot}} ({{label}})',
  // ONE sentence, not a stem plus a shared tail: recording the rig's RX audio into a slot puts
  // the WRONG AUDIO on the air the moment the slot is played, so the warning rides both
  // controls that start a recording — the ● tool and the empty slot button itself.
  'phone.keyer.slot.record.title':
    "Record F{{slot}}. Records from your INPUT DEVICE — often the rig's RX audio, not a mic. If it is, record the message elsewhere and use Import (⤓).",
  'phone.keyer.slot.unnamed': 'Slot {{slot}}',
  'phone.keyer.slot.state.record': 'record',
  'phone.keyer.import.title': 'Import a .wav file',
  'phone.keyer.clear.title': 'Clear this recording',
  'phone.keyer.empty': 'F{{slot}} has no recording yet — record or import one',
  'phone.keyer.busyRecording': 'Finish the recording first',
  'phone.keyer.releasePtt': 'Release PTT before sending a voice message',
  // NAME THE CONTROL THAT IS ON THIS SCREEN: Phone shows no Enable-Tx button, so PTT is the
  // switch when TX is off.
  'phone.keyer.txOff': 'TX is off — click PTT once to turn it back on, then play the message',
  'phone.keyer.playFailed': 'Could not play F{{slot}}',
  'phone.keyer.recordFailed': 'Could not start recording',
  'phone.keyer.saveFailed': 'Could not save recording',
  'phone.keyer.saved': 'Saved F{{slot}} ({{label}})',
  'phone.keyer.importFailed': 'Could not import the WAV',
  'phone.keyer.imported': 'Imported F{{slot}} ({{label}})',
  'phone.keyer.clearFailed': 'Could not clear the slot',
  // What the pane says AFTER its teardown acted, for the operator who walked off the Phone
  // screen and never opened the ⊞ menu. Both are conditional on what actually happened.
  'phone.keyer.hide.stoppedOver':
    'F{{slot}} was on the air — the voice keyer closed and stopped it',
  'phone.keyer.hide.discarded': 'Recording for F{{slot}} discarded — the voice keyer closed',
  'phone.keyer.hide.recorderStuck':
    'Could not stop the recorder for F{{slot}} — it may still be running. Reopen the voice keyer.',

  // ══════════════════════════════════════════════════════════════════════════════════════
  // THE SHELL — chrome, navigation, and the ⊞ panel menu.
  // ══════════════════════════════════════════════════════════════════════════════════════
  //
  // Everything an operator sees BETWEEN the cockpits: the rail he navigates with, the
  // Now-Bar above it, the app's own toasts and status lane, the ⊞ menu that removes a pane,
  // the frame every pane wears, and the two registries (features, goal profiles) whose words
  // reach Settings and the wizard.
  //
  // ⚠️ THE VOCABULARY HERE IS HALF TOKENS. A section named for a MODE keeps its name in the
  // code, never here: CW, Phone, RTTY, PSK, SSTV, APRS, FT, Tempo, TempoFast/TempoDeep. So do
  // the programme and event names (POTA, SOTA, Field Day) and the link report's field names
  // (RV, dT) and unit symbols (MHz, Hz, dB). What moved is the prose AROUND them — and mode,
  // band and programme names written INSIDE a sentence stay written there, exactly as they
  // are everywhere else in this file: they are this application's technical vocabulary, not
  // words a translator replaces.

  // ── The shell: loading, window title, the rails, the crash escape ───────────────────
  'shell.loading': 'Connecting to Nexus…',
  // `Nexus` is the program's name — the same word in every language.
  'shell.windowTitle': '{{section}} — Nexus',
  // Screen-reader announcement of the radio's transmit state. Not a control: the TX controls
  // themselves live in the cockpits and move with them.
  'shell.tx.announce.on': 'Transmitting',
  'shell.tx.announce.off': 'Receiving',
  'shell.rail.stations.aria': 'Resize stations panel (double-click to reset)',
  'shell.rail.waterfall.aria': 'Resize waterfall pane (double-click to reset)',
  'shell.bandActivity.title': 'Band Activity — heard on the band',
  'shell.roam.aria': 'Roam settings',
  'shell.roam.close.aria': 'Close Roam settings',
  // The crash panel's escape. `{{section}}` is a section name from the feature registry.
  'shell.crash.section': 'This section',
  'shell.crash.retry': 'Try again',
  'shell.crash.back': 'Back to {{section}}',

  // ── The status lane (status.ts → the Now-Bar) ───────────────────────────────────────
  // A lane item PERSISTS while its condition holds, so its wording is what the operator
  // stares at. `detail` is the backend's own message where one exists and is interpolated as
  // data, never translated.
  'shell.lane.audio.message': 'RADIO STOPPED',
  'shell.lane.radioConfig.message': 'RADIO CONFIG',
  'shell.lane.txPowerZero.message': 'NO RF POWER',
  'shell.lane.txPowerZero.detail':
    'The radio reports 0% power and transmit is armed — it will key and put nothing on the air. Check the Pwr slider, and the rig\u2019s own power for THIS mode: Yaesu rigs keep a separate level for SSB, DATA, CW and AM.',
  'shell.lane.recording.message': 'RECORDING',
  'shell.lane.prop.offline.message': 'Prop: no live data',
  'shell.lane.prop.offline.detail':
    'No live propagation data yet — set your callsign in Settings and check your internet connection.',
  'shell.lane.prop.cached.message': 'Prop: cached {{minutes}}m',
  'shell.lane.prop.cached.detail':
    'Live propagation refetch failed — showing the last-good snapshot.',

  // ── Deleting a conversation ─────────────────────────────────────────────────────────
  // Raised by BOTH hosts — the main window and a torn-off panel — deliberately in the same
  // words: it is one act, and the two mirrors drifting apart is what put the guard here.
  'shell.conversation.delete.title': 'Delete the conversation with {{peer}}?',
  'shell.conversation.delete.body':
    "Any messages still waiting to send will be cancelled. This can't be undone.",
  'shell.conversation.delete.action': 'Delete conversation',
  'shell.conversation.delete.failed': 'Could not delete conversation',

  // ── What the shell says when an action fails, and when it lands ─────────────────────
  // Every `{{call}}`, `{{band}}`, `{{mode}}`, `{{freq}}` and `{{source}}` below is a token
  // interpolated as data — a callsign, a band or mode name, a dial reading, the source label
  // the engine reports. None of them is translated, and none is locale-formatted.
  'shell.error.switchMode': 'Could not switch mode',
  'shell.error.selectStation': 'Could not select station',
  'shell.ownCall': '{{call}} is your own call',
  'shell.tempo.open.failed': 'Could not open {{call}}',
  'shell.tempo.opened': '▶ {{call}} — open in Tempo',
  'shell.work.failed': 'Could not work {{call}}',
  'shell.work.started': '▶ Working {{call}} — transmitting your call',
  'shell.work.failed.cat': 'Could not work {{call}} — check CAT',
  'shell.work.ready': '▶ {{call}} — {{mode}} {{band}}, ready to log',
  'shell.work.here': '▶ {{call}} — {{band}} {{freq}} MHz',
  'shell.pounce.qsy.failed': 'Could not QSY to {{call}}',
  'shell.log.failed': 'Could not log QSO',
  'shell.log.discard.failed': 'Could not discard QSO',
  'shell.toast.logged': 'Logged QSO',
  'shell.toast.nothingToLog':
    'Nothing to log — the QSO already closed or no report was exchanged',
  'shell.message.failed': 'Message could not be sent',
  'shell.bandFeed.failed': 'Could not open the band feed',
  'shell.resend.failed': 'Could not re-send to {{peer}}',
  'shell.resend.sending': '↻ Re-sending to {{peer}}',
  'shell.resend.qso.failed': 'Could not resend',
  'shell.freetext.failed': 'Could not send free text',
  'shell.broadcast.failed': 'Could not broadcast',
  'shell.cq.failed': 'Could not call CQ',
  'shell.cqRun.toggle.failed': 'Could not toggle the CQ run',
  'shell.cqRun.resume.failed': 'Could not resume the CQ run',
  'shell.beacon.failed': 'Could not toggle the heartbeat',
  'shell.frequency.failed': 'Could not set frequency',
  'shell.blocklist.failed': 'Could not update the blocklist',
  'shell.aprs.tune.failed': 'Could not tune to APRS',
  'shell.tx.enable.failed': 'Could not enable transmit',
  'shell.tx.mute.failed': 'Could not mute transmit',
  'shell.tx.level.failed': 'Could not set TX level',
  'shell.tune.failed': 'Could not toggle tune',
  'shell.proveTx.failed': 'Could not key the transmitter',
  'shell.halt.failed': 'Could not stop transmit',
  'shell.radio.switch.failed': 'Could not switch radios',
  'shell.pegLock.failed': 'Could not set peg-lock',
  'shell.overrideTx.failed': 'Could not queue TX to {{call}}',
  'shell.txPeriod.failed': 'Could not set transmit period',
  'shell.cycleMode.failed': 'Could not set the cycle mode',
  'shell.holdTx.failed': 'Could not toggle Hold Tx',
  'shell.offset.failed': 'Could not set offset',
  'shell.tier.failed': 'Could not change tier',
  'shell.digital.failed': 'Could not switch to Digital',
  'shell.source.switchFailed': 'Could not switch signal source',
  'shell.roam.toggle.failed': 'Could not toggle Roam',
  'shell.qsy.failed': 'Could not QSY to {{band}}',
  'shell.qsy.noChannel': 'No channel for {{band}} in the band plan',
  'shell.qsy.done': 'QSY {{dial}} MHz — listening',
  // `{{section}}` is CW or Phone — a mode name, so it is a token the sentence carries.
  'shell.recall.sectionOff': 'Enable the {{section}} section in Settings to recall this memory',
  'shell.recall.failed': 'Recall failed — check CAT',
  'shell.recall.error': 'Recall failed: {{error}}',
  'shell.recall.done': '{{name}} — {{freq}} MHz {{mode}}',
  // The phone policy commands SSB and FM only; anything else is tuned but not commanded, and
  // the operator is told to set it himself rather than sold a mode the app never sent.
  'shell.recall.done.setMode': '{{name}} — {{freq}} MHz · set {{mode}} on the rig',
  'shell.net.reminder': 'Net {{until}}: {{name}} — {{freq}} {{mode}}',
  'shell.net.tune': 'Tune',
  'shell.rotator.pointed': '↗ Pointing antenna to {{bearing}}° ({{call}})',
  'shell.rotator.failed': "Couldn't point the antenna at {{call}}",
  // `WSJT-X`, `JTDX` and `MSHV` are program names and `:2237` their agreed UDP port.
  'shell.source.companion': 'Source: {{source}} — listening for WSJT-X/JTDX/MSHV on :2237',
  'shell.source.set': 'Source: {{source}}',
  'shell.wizard.saveFailed': "Setup didn't fully save: {{error}} — check Settings",

  // ── A torn-off panel window ─────────────────────────────────────────────────────────
  'detached.connecting': 'Connecting to the radio…',
  'detached.fieldDay.inactive': 'Field Day isn’t active.',
  'detached.fdClub.away.head': 'Club board',
  'detached.fdClub.away.body':
    "The club board shows here while Field Day is the section you are working in. Step back into Field Day and it comes straight back — nothing has stopped, and the host is still collecting contacts.",
  'detached.fdClub.off.head': 'Club sync is off',
  'detached.fdClub.off.body': 'This station is not hosting a club event and has not joined one, so there are no other positions to show.',
  'detached.fdClub.off.route': 'Turn it on in Settings ▸ Contesting ▸ Field Day Club Sync ▸ Host a club event — or paste the host station’s address into Join event at to join one someone else is running.',
  'detached.fdClub.off.wait': 'Leave this window open. The board fills in by itself the moment sync starts.',
  'detached.unavailable': 'Panel “{{panel}}” isn’t available as a standalone window yet.',

  // ── The navigation rail ─────────────────────────────────────────────────────────────
  // Every `label` a MODE names — FT, Tempo, Phone, CW, RTTY, PSK, SSTV, APRS — and the two
  // programme/event names (Field Day, POTA/SOTA) stay in `components/ModeNav.tsx`. Their
  // TOOLTIPS are prose and live here.
  'nav.aria': 'Operating mode',
  'nav.digital.group.label': 'Digital',
  'nav.digital.group.aria': 'Digital modes',
  'nav.digital.ft.title': 'FT weak-signal cockpit — FT8 / FT4 (pick the tier in the top bar)',
  'nav.digital.tempo.title':
    'Tempo — two-way free-text calling (TempoFast / TempoDeep), with Roam (coordinated QSY) inside',
  'nav.digital.rtty.title': 'RTTY — Baudot teletype (45.45 baud): streaming decode + F-key macros',
  'nav.digital.psk.title':
    'PSK31 — narrow-band keyboard mode: click a trace on the waterfall, read the ragchew (receive)',
  'nav.digital.sstv.title': 'SSTV — slow-scan TV: received images decode into the gallery',
  'nav.digital.aprs.title':
    'APRS — AFSK-1200 packet: decode positions/messages, send a position beacon',
  'nav.phone.title': 'Phone (SSB) operating — PTT, sideband, RF power, panadapter (casual)',
  'nav.cw.title': 'CW operating — keyboard + F-key macros, WPM, spectrum (casual)',
  'nav.connect.label': 'Connect',
  'nav.connect.title':
    'Connect — THE map: grayline globe + live spots + openings + propagation, with click-to-work',
  'nav.needed.label': 'Needed',
  'nav.needed.title': "Needed — what you still need that's on the air now; single-click to QSY",
  'nav.spots.label': 'Spots',
  'nav.spots.title':
    'Spots — every cluster/RBN spot on the air (the raw firehose); filter by band/mode',
  'nav.dxped.label': 'DXped',
  'nav.dxped.title': 'DXpeditions — active now, the forward calendar, and what you need from each',
  'nav.sats.label': 'Satellites',
  'nav.sats.title':
    'Satellites — pass times over your grid, favorites, polar plots, and rotor tracking',
  'nav.logbook.label': 'Logbook',
  'nav.logbook.title': 'Logbook — your ADIF contacts',
  'nav.awards.label': 'Awards',
  'nav.awards.title':
    'Awards — your Journey (firsts, ladders, milestones) + official DXCC/WAS/WAZ progress',
  'nav.stats.label': 'Stats',
  'nav.stats.title':
    'Statistics — your logbook sliced: QSOs by band/mode/year/hour, top DXCC entities, states, confirmations',
  'nav.fieldDay.title': 'Field Day — contest rate workspace',
  'nav.fdClub.label': 'Club Board',
  'nav.fdClub.title': 'Club band board — who is on what band at every position on site, in its own window for a second monitor',
  'nav.pota.title': "POTA / SOTA — parks & summits: who's on now (hunt) + tag your activation",
  'nav.memories.label': 'Memories',
  'nav.memories.title':
    'Memories — saved channels: repeaters, nets, calling freqs; groups + ★ favorites; one click to tune',
  'nav.program.label': 'Program',
  'nav.program.title':
    'Program — build channel lists for your radios: local repeaters → CHIRP CSV, rig memories, or tune-now',
  'nav.order.reset.label': 'Reset order',
  'nav.order.reset.title': 'Reset the section order to default',
  'nav.mode.title': 'Active operating mode',
  // The operating-mode badge. `QSO` is a Q-code and `FIELD DAY` the event's name, so only the
  // conversational mode's word is prose.
  'nav.mode.chat': 'CHAT',
  // One word for the gear: its tooltip, its accessible name and its visible label are the
  // same claim about the same button.
  'nav.settings.label': 'Settings',

  // ── The Now-Bar ─────────────────────────────────────────────────────────────────────
  // Three chips answering the three questions an operator actually asks, plus the connector
  // pills. The feed NAMES (Cluster, Phone, PSKR) are the services' own and stay in the code.
  'nowbar.aria': 'Now: band, getting out, and top need',
  'nowbar.label': 'NOW',
  // Compact ages. The unit letter rides inside the message with its number so a translation
  // can never separate the two.
  'nowbar.age.secs': '{{secs}}s',
  'nowbar.age.mins': '{{mins}}m',
  'nowbar.age.hours': '{{hours}}h',
  'nowbar.band.label': 'Band',
  'nowbar.band.open': 'open',
  'nowbar.band.fair': 'fair',
  'nowbar.band.quiet': 'quiet',
  'nowbar.band.closed': 'closed',
  // The chip's own tooltip, used only when the advisory carries no reason of its own (that
  // reason is backend prose, interpolated as data — it moves in phase 3).
  'nowbar.band.title.connect': 'Open Connect — the map + nowcast',
  'nowbar.band.title.plain': 'Band activity',
  'nowbar.out.label': 'Out',
  // Reads the same at every value in English ("1 hear you" as much as "3 hear you"), so it is
  // one string: giving it plural forms would enshrine the English wording, and rewording it
  // is a text change this phase does not make.
  'nowbar.out.hearYou': '{{count}} hear you',
  'nowbar.out.none': 'no spots of you yet',
  // `PSK Reporter` is the service's name; `{{band}}` a band name.
  'nowbar.out.title': '{{hear}} station(s) hear you · you hear {{ihear}} (PSK Reporter, {{band}})',
  'nowbar.out.title.none': 'No propagation data yet',
  'nowbar.need.label': 'Need',
  'nowbar.need.value': '{{entity}} {{band}} · {{likelihood}}',
  'nowbar.need.none': 'nothing workable now',
  // Two WHOLE sentences rather than one with a tail: "live-confirmed" is a claim about the
  // expedition, and a language that puts it elsewhere in the sentence must be able to.
  // `{{where}}` is the entity with its bearing, both data.
  'nowbar.need.where': '{{entity}} {{azimuth}}',
  'nowbar.need.title': '{{call}} ({{where}}) — {{need}} on {{band}}, likelihood {{likelihood}}',
  'nowbar.need.title.confirmed':
    '{{call}} ({{where}}) — {{need}} on {{band}}, likelihood {{likelihood}} (live-confirmed)',
  'nowbar.need.title.none': 'No DXpedition needs workable right now',
  'nowbar.prop.live': 'PROP LIVE',
  'nowbar.prop.partial': 'PROP PARTIAL',
  'nowbar.prop.cached': 'PROP CACHED',
  'nowbar.prop.offline': 'NO LIVE DATA',
  'nowbar.prop.title':
    'Propagation nowcast data is {{source}} — separate from the Cluster/PSKR connection pills',
  // The connector pills. Each state is a WHOLE sentence: "connected but quiet" and "cannot
  // reach the server" are different claims, and they were one broken-looking "waiting" once.
  'nowbar.feed.live.value': 'live {{age}}',
  'nowbar.feed.live.value.noAge': 'live',
  'nowbar.feed.live.title': '{{name}}: receiving (last {{age}} ago)',
  'nowbar.feed.connected.value': 'connected',
  'nowbar.feed.connected.title':
    '{{name}}: connected — no reports yet (normal until you transmit or the band stirs)',
  'nowbar.feed.connecting.value': 'connecting…',
  'nowbar.feed.connecting.title': '{{name}}: trying to reach the server',
  'nowbar.feed.reconnecting.value': 'reconnecting…',
  'nowbar.feed.reconnecting.title': '{{name}}: connection dropped — retrying',
  'nowbar.feed.reconnecting.title.age':
    '{{name}}: connection dropped — retrying (last event {{age}} ago)',
  'nowbar.feed.idle.value': 'idle {{age}}',
  'nowbar.feed.idle.title': '{{name}}: connected, no data for {{age}} (a quiet band is normal)',
  // Defensive: an unknown future backend state renders visibly rather than as a fake idle.
  'nowbar.feed.unknown.title': '{{name}}: {{state}}',
  // The optional host/detail a pill carries. One entry owns the parenthesis and the space, so
  // no caller ever glues them on.
  'nowbar.feed.title.detail': '{{title}} ({{detail}})',

  // ── The Tempo cockpit header ────────────────────────────────────────────────────────
  // The tier NAMES (TempoFast/TempoDeep and their Fast/Deep slots) are the modes' own.
  'tempo.header.tier.aria': 'Tempo tier',
  'tempo.header.tier.fast.title': 'TempoFast — fast conversational tier',
  'tempo.header.tier.deep.title': 'TempoDeep — robust weak-signal tier (15 s)',
  // TX drive: a CONFIGURATION control on the transmit path, not a transmit control (the
  // batch-13 ruling — Tx Power moved, Prove TX did not).
  'tempo.header.power.label': 'Pwr',
  'tempo.header.power.title': "TX drive (Pwr) — trim down until your rig's ALC is just zero",
  'tempo.header.cqRun.aria': 'CQ run',
  'tempo.header.cqRun.off': '📢 Call CQ',
  'tempo.header.cqRun.off.title':
    'Start a CQ run — keep calling CQ every idle TX slot until someone answers',
  'tempo.header.cqRun.paused': 'CQ paused ✕',
  'tempo.header.cqRun.paused.title':
    'CQ run paused (you are in a conversation) — click to stop the run',
  'tempo.header.cqRun.on': '📢 Calling CQ… ✕',
  'tempo.header.cqRun.on.title': 'Calling CQ every idle TX slot — click to stop',
  'tempo.header.cqRun.resume': '▶ Resume',
  'tempo.header.cqRun.resume.title':
    'Resume calling CQ now (it auto-resumes after the conversation goes quiet)',

  // ── The ⊞ Panels menu ───────────────────────────────────────────────────────────────
  // The ENTRIES are not here: each cockpit names its own panels, and those words move with
  // the cockpit. What lives here is the menu itself — the button, the popover, and the two
  // controls that put a mis-tick right.
  'panels.button': '⊞ Panels',
  'panels.button.hidden': '⊞ Panels · {{count}} hidden',
  'panels.button.title':
    'Show or hide the panels on this screen — untick one and its neighbours expand into the space it leaves',
  'panels.popover.aria': 'Panels on this screen',
  'panels.tag.popped': 'popped out',
  'panels.undo': 'Undo last change',
  'panels.undo.title': 'Put the layout back the way it was before the last change',
  'panels.reset': 'Reset layout',
  'panels.reset.title': 'Show every panel again (the stock layout)',

  // ── The cockpit pane frame ──────────────────────────────────────────────────────────
  // `{{title}}` is the pane's own name, supplied by the cockpit.
  'pane.popOut.aria': 'Open {{title}} in its own window',
  'pane.popOut.title': 'Open this pane in its own window (for a second monitor)',
  'pane.hide.aria': 'Hide {{title}}',
  'pane.hide.title': 'Hide this pane (restore it from the ⊞ Panels menu)',

  // ── Drag handles ────────────────────────────────────────────────────────────────────
  // One entry for both handles (`Splitter` sizes a panel, `SplitterSeam` splits two): the
  // tooltip makes the same statement about the same gesture, and `{{label}}` — the
  // separator's accessible name — is what says which handle it is.
  'splitter.title': 'Drag to resize ({{label}})',

  // ── The theme chips ─────────────────────────────────────────────────────────────────
  'theme.aria': 'Theme',
  'theme.light.label': 'Light',
  'theme.light.title': 'Light (sunlight)',
  'theme.dark.label': 'Dark',
  'theme.dark.title': 'Dark (shack)',

  // ── The waterfall palette picker ────────────────────────────────────────────────────
  // Two wordings, and the difference is load-bearing: an UNSCOPED picker drives the master
  // value four cockpits share, a SCOPED one drives that mode's key alone. Naming the modes it
  // actually reaches is why this control stopped lying. (The palette NAMES come from
  // `waterfall.ts`, which this batch does not own.)
  'waterfall.palette.aria.scoped': 'Waterfall color palette (this mode)',
  'waterfall.palette.aria.shared':
    'Waterfall color palette (Phone, CW, RTTY and SSTV — FT has its own)',
  'waterfall.palette.title.scoped': 'Waterfall color palette — applies to this mode',
  'waterfall.palette.title.shared':
    'Waterfall color palette — shared by Phone, CW, RTTY and SSTV. The FT waterfall keeps its own.',

  // ── The FT wide graph (`Waterfall.tsx`) ─────────────────────────────────────────────
  // ⚠️ Every span in kHz, the `dBr` legend with its ticks, the frequency axis and scrollback
  // time tape drawn into the bitmap, the RX/TX marker plates and the zoom LABELS (they live in
  // `waterfall.ts`) are measurements and tokens, and stay in the code. `{{mod}}` is the
  // platform's own modifier key — `Ctrl`, or `⌘` on a Mac.
  'waterfall.title': 'Waterfall',
  'waterfall.hint': 'left = RX · right / Shift = TX · {{mod}} = both',
  'waterfall.zoom.aria': 'Waterfall zoom span',
  'waterfall.zoom.title':
    'Waterfall view — Std (0–3 kHz, WSJT-X-like), Full (0–4 kHz), or zoom in around the RX marker',
  'waterfall.gain.title': 'Gain — contrast (how punchy strong signals look). Center = auto.',
  'waterfall.gain.aria': 'Waterfall gain (contrast)',
  'waterfall.zero.title':
    'Zero — where the black point sits relative to the noise floor. Center = the default (background black); left shows more of the noise, right buries it deeper.',
  'waterfall.zero.aria': 'Waterfall zero (baseline)',
  // The scroll direction: the button says the CURRENT state, the tooltip says which end is
  // newest and which way history travels. Four WHOLE tooltips — what the 3D view does with the
  // direction is part of the statement, not a sentence glued to the end of it.
  'waterfall.flow.down.label': 'Scrolls down',
  'waterfall.flow.up.label': 'Scrolls up',
  'waterfall.flow.down.title':
    'Scrolls down — the newest row appears at the TOP and history travels downward. Click for the other way: newest at the bottom, history travelling up (the default).',
  'waterfall.flow.down.title.dss':
    'Scrolls down — the newest row appears at the TOP and history travels downward. Click for the other way: newest at the bottom, history travelling up (the default). The 3D view keeps its own front-to-back perspective either way.',
  'waterfall.flow.up.title':
    'Scrolls up — the newest row appears at the BOTTOM and history travels upward (the default). Click for the other way: newest at the top, history travelling down.',
  'waterfall.flow.up.title.dss':
    'Scrolls up — the newest row appears at the BOTTOM and history travels upward (the default). Click for the other way: newest at the top, history travelling down. The 3D view keeps its own front-to-back perspective either way.',
  'waterfall.dss.on.title': 'Switch to the flat 2D waterfall',
  'waterfall.dss.off.title':
    'Switch to the 3D stacked-spectrum view (a rolling perspective of the last ~96 rows)',
  'waterfall.pause.title':
    'Pause the waterfall — then scroll back through history with the mouse wheel',
  'waterfall.pause.resume.title':
    'Resume the live waterfall (history kept accumulating while paused)',
  'waterfall.popOut.title':
    'Pop the waterfall out into its own window (frees this space; drag to another monitor)',
  'waterfall.canvas.title':
    'Click sets RX (WSJT-X) · Shift+click sets TX · {{mod}}+click sets both',
  'waterfall.legend.title':
    'Color = signal strength (dB relative to the current strongest signal)',
  // Drawn on the canvas, but a STATE MESSAGE rather than a tick label. `{{age}}` is how far
  // back the scrollback stands, formatted by `ageLabel`.
  'waterfall.paused': '⏸ PAUSED',
  'waterfall.paused.back': '⏸ PAUSED · −{{age}}',
  'waterfall.paused.now': 'now',

  // ── The rig scope (`PhoneScope.tsx`, shared by the Phone and CW cockpits) ────────────
  // ⚠️ The three window WIDTHS (23/12/47 Hz), every S-unit and dB reading, the audio and RF
  // spans, the `FLEX RF` / `CI-V RF` feed names and the `DIAL` plate drawn into the bitmap are
  // measurements and product names, and stay in the component.
  'scope.window.balanced.title':
    'Resolution: Balanced — 2048-point window, 171 ms. The default. A 25 WPM dit is shorter than this window, so CW keying reads as a solid bar. Click for sharper.',
  'scope.window.sharp.title':
    'Resolution: Sharp — 4096-point window, 341 ms. Half the carrier width, at double the time smear. Best for picking a weak carrier out of a crowded passband. Click for faster.',
  'scope.window.fast.title':
    'Resolution: Fast — 1024-point window, 85 ms. Carriers read twice as wide, but keying and speech onsets actually resolve. Click to return to the default.',
  'scope.resolution.aria': 'Scope resolution {{width}} — click to change',
  // `{{reading}}` is the S-unit plate (S7, S9+20) and `{{db}}` the CAT reading it came from.
  'scope.smeter.title': 'S-meter {{reading}} ({{db}} dB rel S9, via CAT)',
  'scope.smeter.title.tx': 'S-meter paused during transmit',
  'scope.smeter.title.none': 'No CAT S-meter reported by this rig',
  'scope.source.flex.title':
    'Native FlexRadio panadapter (SmartSDR) — real RF spectrum, not the soundcard FFT',
  'scope.source.civ.title':
    'Native Icom CI-V scope — real RF spectrum, not the soundcard FFT',
  'scope.dynamic.title':
    "How far the strongest signal in this view stands above the noise floor. The scope's vertical scale is FIXED at 50 dB above the noise, so a louder signal really does draw a taller spike — use G to widen or tighten that window.",
  'scope.gain.title': 'Visual gain — stretch (right) or flatten (left) the color contrast',
  'scope.gain.aria': 'Scope visual gain',
  'scope.zero.title':
    'Visual zero — raise (right) to darken the noise floor, lower (left) to reveal weak texture',
  'scope.zero.aria': 'Scope visual zero (floor)',
  'scope.dss.on.title': 'Back to the flat trace + waterfall',
  'scope.dss.off.title':
    'Switch to the 3D stacked-spectrum view (fills the panel; hides the trace)',
  'scope.pause.title': 'Pause the waterfall — then scroll back through it with the mouse wheel',
  'scope.pause.resume.title': 'Resume the live scope (history kept filling while paused)',
  'scope.flow.down.label': 'Scrolls down',
  'scope.flow.up.label': 'Scrolls up',
  'scope.flow.down.title':
    'Scrolls down — the newest row appears at the TOP and history travels downward. Click for newest at the bottom.',
  'scope.flow.up.title':
    'Scrolls up — the newest row appears at the BOTTOM and history travels upward. Click for newest at the top.',
  'scope.canvas.title': 'Click a signal to tune it · press and drag to slide the passband',
  'scope.paused.badge': '⏸ paused · wheel to rewind',

  // ── The MSK144 Fast Graph ───────────────────────────────────────────────────────────
  // The callsigns and the second ticks drawn on it are data and a scale.
  'fastGraph.aria': 'Fast Graph — signal power across the T/R period; pings draw as spikes',

  // ── The meters ──────────────────────────────────────────────────────────────────────
  // ⚠️ Every reading is a measurement built in the component — the RX level in dB on WSJT-X's
  // scale, the SWR ratio, ALC and power percentages, watts and compression dB — and the four TX
  // meter names (SWR, ALC, PO, COMP) are the rig's own front-panel vocabulary. These are
  // READOUTS: nothing here keys, gates or stops a transmission.
  'meters.rx.label': 'RX level',
  // `{{label}}` is the meter's own name (the host may pass one) and `{{level}}` its reading.
  'meters.rx.title': '{{label}}: {{level}} (aim ~30 dB, like WSJT-X)',
  'meters.tx.aria': 'Transmit meters',
  // `{{when}}` is `TX_METERS_WHEN`, the one string the ⊞ Panels menu shows for this panel —
  // interpolated rather than repeated so the panel and the menu cannot drift. It is still
  // written in `TxMeters.tsx`; see the note there for why it has not moved yet.
  'meters.tx.idle': 'TX meters — {{when}}',
  'meters.tx.swr.title': 'Antenna match — keep it under 2:1',
  'meters.tx.alc.title': 'ALC — set mic gain so SSB peaks just tickle the zone, never peg it',
  'meters.tx.po.title': 'Actual output power',
  'meters.tx.comp.title': 'Speech compression',

  // ── The rotator strip and the Rotor pane ────────────────────────────────────────────
  // Both surfaces drive the same mast, so they share the words for the one act they share (the
  // stop) and keep their own for everything else.
  //
  // ⚠️ NOT HERE: bird names, the track's own state word (both arrive from the backend and are
  // interpolated verbatim), every azimuth and elevation in degrees, the `°T`/`°M` true and
  // magnetic marks, `WMM`, `AOS`/`LOS`, `az`, `rotctld` and the SAT/ROTOR plates. The ■ buttons
  // on both surfaces stop ROTATION and the satellite track — never a transmission.
  'rotor.stop.failed': 'Rotator stop: {{error}}',

  'rotor.strip.aria': 'Rotator',
  'rotor.strip.az.title': 'Rotator at {{deg}}° true',
  'rotor.strip.az.title.magnetic': 'Rotator at {{deg}}° true · {{mag}}° magnetic (WMM)',
  'rotor.strip.pointAt.title': 'Point the antenna at {{call}}',
  'rotor.strip.stop.title': 'Stop rotation NOW (mid-pass: stops the satellite track too)',

  // A transponder HELD with no armed track — the QO-100/park case.
  'rotor.strip.held.aria': 'A satellite transponder holds the dial',
  'rotor.strip.held.title':
    '{{bird}} holds the dial — picked in Satellites; the dial stays on the bird through section changes. ■ releases the hold and hands the dial back',
  'rotor.strip.held.title.here':
    "{{bird}} holds the dial — picked in Satellites; the dial stays on the bird through section changes. Release it there or with the strip's ■",
  'rotor.strip.held.chip': '⟳ {{bird}} · bird holds the dial',
  'rotor.strip.held.chip.dial': '⟳ {{bird}} +dial',
  'rotor.strip.release.aria': 'Release the transponder hold',
  'rotor.strip.release.title': 'Release the transponder NOW — the dial is yours again',
  'rotor.strip.release.failed': 'Release: {{error}}',

  // Doppler ownership. `{{what}}` is the clause below that says what it is doing — one
  // sentence per surface with the variable clause interpolated whole, the shape
  // `sat.badge.dopplerOnly` already uses.
  'rotor.strip.doppler.dial.aria': 'Satellite Doppler owns the dial',
  'rotor.strip.doppler.tx.aria': 'Satellite Doppler owns the TX VFO',
  'rotor.strip.doppler.dial.title':
    'Satellite Doppler is {{what}} for {{bird}} ({{state}}) — ■ stops the track and hands the dial back',
  'rotor.strip.doppler.tx.title':
    'Satellite Doppler is {{what}} for {{bird}} ({{state}}) — ■ stops the track and releases the split',
  'rotor.strip.doppler.dial.steering': 'steering the radio dial',
  'rotor.strip.doppler.dial.atAos': 'armed to take the radio dial at AOS',
  'rotor.strip.doppler.tx.steering': 'steering the TX (split) VFO — the dial stays yours',
  'rotor.strip.doppler.tx.atAos':
    'armed to take the TX (split) VFO at AOS — the dial stays yours',
  'rotor.strip.doppler.chip.dial': '⟳ {{bird}} · Doppler holds the dial',
  'rotor.strip.doppler.chip.dialAtAos': '⟳ {{bird}} · dial at AOS',
  'rotor.strip.doppler.chip.tx': '⟳ {{bird}} · Doppler holds the TX VFO',
  'rotor.strip.doppler.chip.txAtAos': '⟳ {{bird}} · TX VFO at AOS',
  'rotor.strip.trackStop.aria': 'Stop the satellite track',
  'rotor.strip.trackStop.title': 'Stop the satellite track NOW — Doppler releases the dial',
  'rotor.strip.trackStop.failed': 'Track stop: {{error}}',

  // The auto-track chip. `{{doppler}}` is the optional clause naming what ELSE the track drives,
  // carrying its own separator.
  'rotor.strip.track.title':
    'Auto-tracking {{bird}} ({{state}}) — the Satellites section owns the rotor until LOS{{doppler}}',
  'rotor.strip.track.title.dial': '; Doppler owns the radio dial too',
  'rotor.strip.track.title.tx': '; Doppler drives the TX (split) VFO too — the dial stays yours',
  'rotor.strip.track.chip': '⟳ {{bird}}',
  'rotor.strip.track.chip.dial': '⟳ {{bird}} +dial',
  'rotor.strip.track.chip.uplink': '⟳ {{bird}} +uplink',

  // Configured but silent — the chip that is the trip to the model/port/baud fields. `{{state}}`
  // and `{{detail}}` are the two sentences above, interpolated whole.
  'rotor.strip.lost.silent': 'Rotator not answering',
  'rotor.strip.lost.stopped': 'Rotator stopped answering',
  'rotor.strip.lost.silent.title':
    'A rotator is configured but not answering — check the model, port and baud in Settings ▸ Radio ▸ Rotator (the baud belongs to the model), or the external rotctld, and the Connections log',
  'rotor.strip.lost.stopped.title':
    'The rotator stopped answering mid-pass, so the track let it go — point the antenna yourself. Check the model, port and baud in Settings ▸ Radio ▸ Rotator (the baud belongs to the model), or the external rotctld, and the Connections log',
  'rotor.strip.lost.open.aria': '{{state}} — open the rotator settings',
  'rotor.strip.lost.open.title': '{{detail}}. Click to open it',

  // ── The Amplifier pane (Connect) ────────────────────────────────────────────────────
  // ⚠️ NOT HERE, and none of it may move: the unit symbols W, V, A, ° and the `:1` of an SWR
  // ratio, the meter names SWR / ATU / Vdc, and the amplifier's own raw model id. Those are
  // the instrument's vocabulary; a translated `SWR` names no meter on any amplifier's panel.
  // NO PLURAL ENTRIES — see the Settings ▸ Amplifier block for why.
  // The amplifier's cockpit strip. Same invariant-token rule as the pane above: W stays W.
  'amp.strip.aria': 'Amplifier',
  'amp.strip.toOperate.title': 'Put the amplifier into Operate.',
  'amp.strip.toStandby.title': 'Put the amplifier into Standby. This does NOT stop a transmission — the exciter keeps keying and the drive passes straight through.',
  'amp.strip.keyed.title': 'Not while you are transmitting. Changing band or mode on a keyed amplifier can damage it.',
  'amp.strip.bandDown.aria': 'Amplifier band down',
  'amp.strip.bandUp.aria': 'Amplifier band up',
  'amp.strip.refused': 'Not sent',
  'amp.link.up': 'Linked',
  'amp.operate': 'Operate',
  'amp.standby': 'Standby',
  'amp.k.power': 'Power out',
  'amp.k.temp': 'PA temp',
  'amp.k.current': 'Current',
  'amp.swr.title': 'Standing-wave ratio measured at the antenna.',
  'amp.swrAtu.title': "Standing-wave ratio measured before the amplifier's tuner.",
  // ⚠️ THE ONE PLACE THE MISSING UNIT IS EXPLAINED RATHER THAN GUESSED AT. The SPE protocol
  // does not carry the scale, so no letter is printed and this says why.
  'amp.temp.unknownScale':
    "The SPE protocol does not say whether this is °C or °F — the amplifier reports whatever its own display is set to. Check the amplifier's front panel.",

  // Why the link is down. Four states, and `wrongModel` is the one that earns its own sentence:
  // a working link on a protocol Nexus does not speak must never read as "no amplifier".
  'amp.reason.noAnswer': 'Not answering',
  'amp.reason.portBusy': 'Port in use by something else',
  'amp.reason.wrongModel': 'Answering, but in a protocol Nexus does not read yet',
  'amp.reason.malformed': 'Answering with something Nexus cannot read',

  // Alarms — the amplifier's own, rendered as faults. `unknown` is a fault too: a code a later
  // firmware ships must reach the operator, not go quiet in front of a kilowatt.
  'amp.alarm.swrExceedingLimits': 'Alarm: SWR beyond limits',
  'amp.alarm.amplifierProtection': 'Alarm: amplifier protection tripped',
  'amp.alarm.inputOverdriving': 'Alarm: input overdriven',
  'amp.alarm.excessOverheating': 'Alarm: overheating',
  'amp.alarm.combinerFault': 'Alarm: combiner fault',
  'amp.alarm.fault': 'Fault',
  'amp.alarm.unknown': 'Alarm the amplifier did not name',

  'amp.warning.alarmAmplifier': 'Warning: amplifier alarm',
  'amp.warning.noSelectedAntenna': 'Warning: no antenna selected',
  'amp.warning.swrAntenna': 'Warning: antenna SWR',
  'amp.warning.noValidBand': 'Warning: no valid band',
  'amp.warning.powerLimitExceeded': 'Warning: power limit exceeded',
  'amp.warning.overheating': 'Warning: overheating',
  'amp.warning.atuNotAvailable': 'Warning: tuner not available',
  'amp.warning.tuningWithNoPower': 'Warning: tuning with no drive',
  'amp.warning.atuBypassed': 'Warning: tuner bypassed',
  'amp.warning.powerSwitchHeldByRemote': 'Warning: power switch held by remote',
  'amp.warning.combinerOverheating': 'Warning: combiner overheating',
  'amp.warning.combinerFault': 'Warning: combiner fault',
  'amp.warning.unknown': 'Warning the amplifier did not name',

  'rotor.pane.rose.aria': 'Rotator at {{deg}} degrees — click to slew',
  'rotor.pane.rose.aria.unknown': 'Rotator — position not reported; click to slew',
  'rotor.pane.az.title': 'True bearing',
  'rotor.pane.az.title.magnetic': '{{deg}}° true · {{mag}}° magnetic (WMM)',
  'rotor.pane.az.title.unknown':
    'This rotator does not report its position — pointing still works',
  'rotor.pane.track.title':
    'Auto-tracking {{bird}} ({{state}}) — the Satellites section owns the rotor until LOS; a manual slew or STOP halts it',
  'rotor.pane.commanded.title': 'Commanded heading — the needle is on its way',
  'rotor.pane.entry.aria': 'Azimuth to slew to (degrees true)',
  'rotor.pane.stop.label': '■ STOP',
  'rotor.pane.stop.title': 'Stop rotation NOW',
  'rotor.pane.hint': 'click the rose or type a bearing · headings are TRUE',
  'rotor.pane.hint.noPosition': 'no position from this rotator — pointing and STOP still work',
  'rotor.pane.slew.failed': 'Rotator: {{error}}',

  // ── The shared cockpit header ───────────────────────────────────────────────────────
  // ⚠️ THE FOUR TRANSMIT CONTROLS THIS HEADER DRAWS ARE ABSENT BY DESIGN: the TX-enable latch,
  // Tune, ATU and Stop TX stay written in `CockpitHeader.tsx` until the transmit-path batch
  // moves them with the stop-line sweeps re-run — one header, six cockpits, and three of those
  // labels are what `stop-line.test.tsx` matches by accessible name. So are the TX/RX pill's
  // three plates, which are the passive rendering of that same latch.
  //
  // What is here is the rest: the wheel-tune tooltips, the runaway-guard toast, the power
  // slider (a CONFIGURATION control on the transmit path — batch 13) and the CAT pill.
  'cockpit.header.readout.wheelTune.title': 'Scroll to tune',
  'cockpit.header.readout.digitTune.title': 'Scroll a digit to tune it',
  // Two WHOLE sentences, one per direction — the band name and the edge frequency are invariant
  // tokens formatted by the header, never by a locale-aware formatter.
  'cockpit.header.bandEdge.up':
    'Stopped at the {{band}} band edge, {{edge}} MHz — scroll again to keep tuning up',
  'cockpit.header.bandEdge.down':
    'Stopped at the {{band}} band edge, {{edge}} MHz — scroll again to keep tuning down',
  // `{{label}}` is the slider's own name — `Power` below, or the cockpit's word for it.
  'cockpit.header.power.label': 'Power',
  'cockpit.header.power.title': "{{label}} — trim so your rig's ALC is just zero",
  'cockpit.header.cat.ok.title': 'CAT link OK',
  'cockpit.header.cat.bad.title': 'No CAT link',

  // ── The cockpit tuning strip ────────────────────────────────────────────────────────
  // ⚠️ Every step in Hz, the RIT/XIT offsets and the two VFO letters are invariant: `{{hz}}` and
  // `{{vfo}}` arrive as tokens the strip formats itself. `RIT`, `XIT` and `Shift` are the rig's
  // and the keyboard's own names and are written into the sentences.
  'cockpit.tuning.aria': 'Tuning',
  'cockpit.tuning.down.title': 'Down {{hz}} Hz',
  'cockpit.tuning.down.aria': 'Tune down {{hz}} Hz',
  'cockpit.tuning.up.title': 'Up {{hz}} Hz',
  'cockpit.tuning.up.aria': 'Tune up {{hz}} Hz',
  'cockpit.tuning.wheel.title': 'Scroll to tune ±{{hz}} Hz (Shift = ×10)',
  'cockpit.tuning.step.label': 'Tuning step',
  'cockpit.tuning.vfo.aria': 'Active VFO',
  'cockpit.tuning.vfo.title': 'Use VFO {{vfo}}',
  'cockpit.tuning.rit.title': 'RIT clarifier — click to clear',
  'cockpit.tuning.rit.down.aria': 'RIT down',
  'cockpit.tuning.rit.up.aria': 'RIT up',
  'cockpit.tuning.xit.title': 'XIT clarifier — click to clear',
  'cockpit.tuning.xit.down.aria': 'XIT down',
  'cockpit.tuning.xit.up.aria': 'XIT up',

  // ── The top bar ─────────────────────────────────────────────────────────────────────
  // ⚠️ THE TX CLUSTER IS ABSENT BY DESIGN — TX On/Off with the tooltip that states the abort
  // semantics, Tune, Stop TX and the watchdog chip stay written in `TopBar.tsx` until the
  // transmit-path batch. So do the bar's plates: the product name, the TX/RX indicator, the OP
  // prefix, the REC badge and the UTC label.
  //
  // ⚠️ ALSO NOT HERE: every tier and mode NAME on the pills (they are the modes' own), the DT
  // and clock readings, the slot countdown, and the callsign and grid.
  'topbar.tier.aria': 'Link tier',
  'topbar.tier.tempoFast.title': 'Fast conversational tier',
  'topbar.tier.tempoDeep.title': 'Robust non-coherent tier — fading-resilient (15 s)',
  'topbar.tier.ft4.title': 'Standard WSJT-X FT4 (7.5 s)',
  'topbar.tier.ft8.title': 'Standard WSJT-X FT8 (15 s)',
  'topbar.tier.ft2.title':
    'FT2 — Decodium’s fast slotted mode (3.75 s), FT4 with a halved symbol time. Transmit and receive. No settings: the period is fixed',
  'topbar.tier.wspr.title':
    'WSPR propagation beacons — 2 min intervals. Transmits on a schedule; set the transmit % and power in Settings ▸ Beacons (WSPR & FST4W)',
  'topbar.tier.q65.title':
    'Q65 — EME / VHF+ scatter. Transmit and receive. Period + submode in Settings ▸ Q65',
  'topbar.tier.msk144.title':
    'MSK144 — meteor scatter. Transmits for nearly the whole period (that is how the mode works). Period in Settings ▸ MSK144',
  'topbar.tier.jt65.title':
    'JT65 — classic EME, 60 s. Receive only in this build (transmit is disabled pending a fix). Submode in Settings ▸ JT65',
  'topbar.tier.fst4.title': 'FST4 — slow weak-signal QSO mode (LF/MF). Transmit and receive',
  'topbar.tier.fst4w.title':
    'FST4W — LF/MF beacons. Transmits on a schedule; set the transmit % and power in Settings ▸ Beacons (WSPR & FST4W). Hashed calls show as <...>',
  // The same statement the Operate strip makes about a receive-only tier — deliberately its own
  // entry, because neither surface owns the other's wording.
  'topbar.rxOnly.why': 'This mode is receive-only in Nexus — it decodes but does not transmit',

  'topbar.help.label': 'Help',
  'topbar.field.label': 'Field',
  'topbar.field.on.title': 'Field mode is on: larger type, maximum contrast. Click to turn off.',
  'topbar.field.off.title':
    'Field mode for operating outdoors: larger type, maximum contrast',
  'topbar.operator.title': 'Operating as {{call}} — click to change who is at the key',
  'topbar.operator.switch': 'Switch to {{call}}',
  'topbar.operator.single': 'Single operator (clear)',
  // The chip before anyone has been set — Field Day only. It has to read as an invitation
  // rather than as a callsign, because at that moment it is the only thing on screen that
  // says an operator can be named at all.
  'topbar.operator.set': 'Set operator',
  'topbar.operator.set.title': 'Nobody is set as the operator — click to say who is at the key',
  'topbar.operator.firstSet': 'No operators logged yet — set the first one on the Field Day dashboard',
  // `{{rig}}` and `{{believed}}` are mode names, straight through.
  'topbar.rigMode.chip': 'rig: {{mode}}',
  'topbar.rigMode.title':
    "Your rig is on {{rig}}, but Nexus has {{believed}}. Turn the rig's mode knob (or pick the band in an operating cockpit) to match.",
  'topbar.rxLevel.label': 'RX audio level',
  'topbar.rxLevel.title': 'RX audio level (aim ~30 dB, like WSJT-X)',
  'topbar.recording.title': 'Recording this QSO to a WAV — click to stop',
  'topbar.txControls.aria': 'Transmit controls',
  'topbar.holdTx.label': 'Hold Tx',
  'topbar.holdTx.title':
    'Hold Tx Freq: keep your TX offset where you put it when you double-click a station to work them. Off, your TX moves onto theirs (WSJT-X\'s behaviour). A plain waterfall click never moves TX either way.',
  'topbar.slotClock.title': 'Time to next slot',
  'topbar.slotClock.label': 'next slot',
  'topbar.utc.title': 'UTC time',
  // `{{offset}}` is the signed clock error, formatted invariantly by the bar.
  'topbar.clock.label': 'clock {{offset}}',
  'topbar.clock.title':
    'PC clock is {{offset}} vs UTC (NTP). TempoFast/TempoDeep need it within ~0.5 s — sync via NTP / time.is (off-grid: GPS).',
  'topbar.sync.ok.label': 'Sync',
  'topbar.sync.bad.label': 'No Sync',
  'topbar.sync.ok.title': 'Time sync OK (from decode timing)',
  'topbar.sync.bad.title':
    'Decodes land far off the slot boundary — sync your PC clock (NTP / time.is; off-grid: GPS).',
  'topbar.dt.title': 'Decode time offset (how far heard signals land from the slot boundary)',
  // The transmit-cycle group: three WHOLE labels for the Auto button and one each for the two
  // locks, never a stem plus a period token. `<s>` is the small type, supplied by the call site.
  'topbar.txCycle.aria': 'Transmit cycle',
  'topbar.txCycle.auto.title':
    'Auto-cycle (FT8-style): when you answer a station, transmit on the opposite T/R cycle to theirs',
  'topbar.txCycle.auto.idle': 'Auto <s>cycle</s>',
  'topbar.txCycle.auto.first': 'Auto <s>1st</s>',
  'topbar.txCycle.auto.second': 'Auto <s>2nd</s>',
  'topbar.txCycle.first.label': 'Tx 1st <s>even</s>',
  'topbar.txCycle.first.title':
    'Lock transmit to the even (1st) T/R slots — the station you work must be Tx 2nd',
  'topbar.txCycle.second.label': 'Tx 2nd <s>odd</s>',
  'topbar.txCycle.second.title':
    'Lock transmit to the odd (2nd) T/R slots — the station you work must be Tx 1st',

  // ── Toasts ──────────────────────────────────────────────────────────────────────────
  // The default action word, used when a toast offers an action but names none. Toast BODIES
  // arrive already translated from whoever raised them.
  'toast.action.default': 'Work',
  'toast.dismiss': 'Dismiss notification',

  // ── The link pill ───────────────────────────────────────────────────────────────────
  // `dB` is a unit and `RV` the report's own field name; the verdict words are prose.
  'link.quality.solid': 'Solid {{snr}} dB',
  'link.quality.marginal': 'Marginal RV{{rv}}',
  'link.quality.weak': 'Weak {{snr}} dB',
  'link.dial.label': 'Dial',
  'link.band.label': 'Band',
  'link.tier.label': 'Tier',
  'link.audioFreq.label': 'Audio f',

  // ── The feature registry (features/registry.ts) ─────────────────────────────────────
  // A feature is a SECTION or a CAPABILITY, and its label + one-liner reach the operator in
  // Settings ▸ Features and in the setup wizard. The six sections named for a MODE keep their
  // names in the registry; so do Field Day and POTA / SOTA, which name an event and two
  // programmes. Everything else is prose.
  'features.category.operate': 'Operate',
  'features.category.dxAwards': 'DX & Awards',
  'features.category.contesting': 'Contesting',
  'features.category.propagation': 'Propagation',
  'features.category.logging': 'Logging',
  'features.category.system': 'System',
  'features.operate.label': 'Operate',
  'features.operate.oneLine': 'The waterfall-first cockpit — decode, tune, and work stations.',
  'features.cw.oneLine': 'CW operating — keyboard + F-key macros, WPM, spectrum, casual ragchew.',
  'features.phone.oneLine':
    'Phone (SSB) operating — PTT, band-aware sideband, RF power, panadapter.',
  'features.rtty.oneLine':
    'RTTY operating — 45.45 baud Baudot: streaming decode, F-key macros, FSK/AFSK keying.',
  'features.psk.oneLine':
    'PSK31 — narrow-band keyboard mode: click a trace, read the ragchew (receive).',
  'features.sstv.oneLine':
    'SSTV — slow-scan images auto-decode into a gallery (Martin/Scottie/Robot/PD).',
  'features.aprs.oneLine':
    'APRS — AFSK-1200 packet: decode positions/messages, send a position beacon.',
  'features.logbook.label': 'Logbook',
  'features.logbook.oneLine': 'Your ADIF contacts — the system of record.',
  'features.settings.label': 'Settings',
  'features.settings.oneLine': 'Operator, rig, network, and feature configuration.',
  'features.nowBar.label': 'Now Bar',
  'features.nowBar.oneLine': 'The persistent at-a-glance status strip (UTC, band, state, alerts).',
  'features.chat.label': 'Chat',
  'features.chat.oneLine': 'Free-form QSO text (TempoFast/TempoDeep).',
  'features.fieldDay.oneLine': 'Contest rate workspace (exchange, dupes, scoring, Cabrillo).',
  'features.connect.label': 'Connect',
  'features.connect.oneLine':
    'Situational awareness — the grayline map + live propagation in one view.',
  'features.needed.label': 'Needed',
  'features.needed.oneLine': "What you still need that's on the air now — single-click to QSY.",
  'features.spots.label': 'Spots',
  'features.spots.oneLine':
    'Every cluster/RBN spot on the air — the raw firehose, filter by band/mode.',
  'features.dxped.label': 'DXpeditions',
  'features.dxped.oneLine':
    'DXpeditions — active now, the forward calendar, and your needed status.',
  'features.sats.label': 'Satellites',
  'features.sats.oneLine':
    'Satellite passes over YOUR grid — when to try which bird, favorites first.',
  'features.memories.label': 'Memories',
  'features.memories.oneLine':
    'Saved channels — repeaters, HF nets, calling freqs: groups, ★ favorites, one-click tune, CHIRP CSV, starter packs + opt-in net reminders.',
  'features.program.label': 'Program',
  'features.program.oneLine':
    'Program your radios — local repeaters to a channel list: CHIRP CSV, rig memories, or tune-now.',
  'features.awards.label': 'Awards',
  'features.awards.oneLine':
    'Journey + official awards — firsts, sub-award ladders, DXCC/WAZ/WAS progress.',
  'features.stats.label': 'Stats',
  'features.stats.oneLine':
    'Your logbook, sliced — QSOs by band, mode, year, hour, entity, and confirmations.',
  'features.pota.oneLine': "Parks/Summits on the air — who's on now (hunt) + tag your activation.",
  'features.gamification.label': 'Achievements',
  'features.gamification.oneLine': 'Celebrate milestone unlocks (toasts + badges).',

  // ── Goal profiles (features/profiles.ts) ────────────────────────────────────────────
  // A profile is a named bundle of features, chosen by GOAL. `POTA / SOTA` is the two
  // programmes' own names; `6m` and `VHF` are a band and a band group, written into the
  // label with the prose around them.
  'profiles.starter.label': 'Just getting started',
  'profiles.starter.blurb':
    'Make some FT8/FT4 contacts. A clean cockpit and a simple log — extras stay out of the way.',
  'profiles.dx.label': 'DX chasing & awards',
  'profiles.dx.blurb':
    'Chase new ones: awards (DXCC/Challenge/Honor Roll/WAZ), propagation, the map, and the DXpedition board.',
  'profiles.contest.label': 'Contesting',
  'profiles.contest.blurb':
    'Run rate: the contest workspace and field log, with awards and prop out of the way.',
  'profiles.pota.blurb':
    'Activate and hunt: the map and a field log for parks-and-peaks operating.',
  'profiles.vhf.label': '6m / VHF & openings',
  'profiles.vhf.blurb':
    'Catch the band coming alive: Connect (map + openings), satellite passes, and the DXpedition board.',
  'profiles.everything.label': 'Everything (expert)',
  'profiles.everything.blurb':
    'Turn the whole console on. Every section and capability enabled.',

  // ── Shared across surfaces ──────────────────────────────────────────────────────────
  // `common.*` is for words that are genuinely the same act everywhere. Resist it: a shared
  // key that two surfaces want to word differently cannot be split later without orphaning
  // both translations. When in doubt, give the surface its own key.
  'common.dismiss': 'Dismiss',
} satisfies Record<string, Message>
