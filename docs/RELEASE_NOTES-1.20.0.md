# Nexus 1.20.0

FT8 and FT4 now change the over already on the air when a late reply arrives, as WSJT-X does. When your
partner's R-report or RR73 is decoded after your over has started, the rest of that over becomes the right
message. The over keeps its start and its end, where Nexus used to send the old message to the end and the right
one an over later. Nexus also decodes at WSJT-X's moments: FT8's full decode runs 14.4 seconds into the period
and FT4's one decode at 6.05 seconds, so a weak reply that only the full decode finds is known before your next
over starts. Each decode is handed on the moment it is found, as in WSJT-X, so that reply counts on a slower
computer too. An RR73 that answers your report or your RRR gets its 73, and Band Activity, Rx Frequency and the
WSJT-X UDP feed give each decode the start of the period it was heard in. Your RR73, RRR and 73 to a station
signing a call such as W9XYZ/M are WSJT-X's, and a caller shown as `<...>` is not answered until its call shows.

Nexus now supports TCI in full, as a Beta. It can drive an SDR program over TCI (ExpertSDR3 for the SunSDR,
Thetis 2.10.3.14 and later, and piHPSDR) with no rigctld, virtual COM port or virtual audio cable: the dial, the
mode, the receive audio, transmitting, the program's own panadapter in Nexus's scope, and Nexus's needed spots
on the program's panadapter. The other way round, **Share by TCI** makes Nexus a TCI server on this computer, so
WSJT-X, JTDX, MSHV, Log4OM and other TCI programs can use whichever radio Nexus runs, with every Nexus transmit
safeguard still in charge.

Every contest log numbers its contacts, the scoreboard shows on and off time, **Spot me**
posts one spot of your call, frequency markers sit on the band map as N1MM's Mark does, and **Match WPM** answers
a CW station at its own speed. The band map marks dupes and new multipliers, and predicts them from a call-history
file. The county box fills itself from your log, a dupe gets a red **DUPE** badge, and the Contest screen has a big
clock and statistics per operator. Every save of the contest log keeps backup copies beside it. In Field Day and
the other contests, a station signing a call such as W9XYZ/M is logged without angle brackets and gets the RR73
and 73 WSJT-X sends, and only your caller's own RR73 closes a Field Day contact.

CW has a turn cue that says whose turn it is when you copy with the decoder, the AI decoder keeps up with the
signal, the classic decoder copies fast stations from the first few characters, and Esc stops a CW message
sooner. RTTY has a tuning scope, callsign tags on its waterfall, Auto-tune and AFSK on USB. Nexus Remote can
operate the shack from a second Windows PC with Nexus on it, on your own network and over the internet. It is a
Beta in the regular Windows installer, and the settings that guard transmitting stay at the station.

On Windows, FT8, FT4 and the other WSJT-X modes finish decoding sooner. A KPA1500 can be reached over the
network, PstRotatorAz is driven directly, the FT-1000 and FT-1000D have their own entries in the rig list, and the
scope controls work on the IC-7300, IC-705, IC-905 and IC-7300MK2. The rest are fixes: a logger can no longer turn
TX back on after you stop it, FT8 and FT4 wait while another program keys your radio through Nexus, free text
FT8 cannot send is refused instead of keying a steady tone, CW holds the transmitter until its last word ends,
and Native Icom CI-V always tells the radio to stop transmitting as it closes.

## The short version

- **FT8 and FT4 switch the over on the air when a late reply arrives**, as WSJT-X does, on a sound card, on a
  Flex's DAX (Beta) and over TCI. ALL.TXT gets a second Tx line for the switched over.
- **FT8's full decode runs at 14.4 s and FT4's decode at 6.05 s**, as in WSJT-X, so a weak reply counts for your
  next over.
- **An RR73 to your report or to your RRR gets a 73**, and a repeated report carries the new signal report.
- **Each decode carries the start of its period** in Band Activity, Rx Frequency and the WSJT-X UDP feed, and
  programs on that feed see what you are transmitting.
- **Each decode counts the moment it is found**, as in WSJT-X, in the decode list, ALL.TXT, the UDP feed and the
  sequencer. FT8's early decode stops at 13.4 s, and the diagnostic log says when a period's decode was dropped.
- **Your RR73, RRR and 73 to a station signing W9XYZ/M are WSJT-X's**, a typed 73 too, a clicked Tx3 goes out as it
  shows, and a caller shown as `<...>` is not answered.
- **Free text FT8 cannot send is refused with the reason** instead of keying a steady tone, and a logger's Replay
  no longer marks old decodes as new.
- **FT8 decodes nothing from a period with almost no receive audio**, as WSJT-X does, so a muted or unplugged sound
  card cannot hand the sequencer a phantom RR73 or R-report from a station heard earlier.
- **TCI (Beta):** Nexus drives an SDR program over TCI, transmits through it, draws its panadapter in the scope and
  puts Nexus's needed spots on it.
- **Share by TCI:** programs on this computer use the radio through Nexus, may transmit voice, Tune and CW when you
  allow it, and can put their spots on Nexus's band map.
- **Contest logs number every contact**, the N1MM broadcast carries the number, and the scoreboard shows on and off
  time.
- **Contest entry:** Shift+Space goes back a box, the county box fills itself, a station you worked turns the Call
  box amber, a dupe shows a red DUPE badge, and an optional RST sent box.
- **Contest band map:** dupes struck through, new multipliers tagged MULT, predicted ones MULT?, and markers you set
  with Alt+M.
- **Spot me** on the Contest screen and the Spots board, asking twice where self-spotting may be against the rules.
- **CW speed match**, a **Resume** line when the contest has stopped, buttons to work the contest in CW or Phone, a
  big clock, statistics per operator, and resizable log panes.
- **The contest logger window** drags down to 420×320, stays in front if you ask, and can show the entry alone.
- **The contest log keeps backup copies** on every save, and contacts held back from the logging services survive a
  restart.
- **Field Day and contest FT with suffixed calls work as in WSJT-X**, and a call shown as `<...>` is not answered.
- **An unassisted entry turns off all CW decoding**, the classic decoder too.
- **CW:** a turn cue, an AI decoder that keeps up, fast stations copied from the start, a sooner stop, meters that
  move during CW, and CW that keeps the transmitter to the end of its last word.
- **RTTY:** a tuning scope, callsign tags and span and speed buttons on the waterfall, Auto-tune, AFSK on USB, and
  channel picks that put your signal on the channel.
- **SSTV and 2 m FM:** the FM channels transmit simplex with no tone, the right radio gets an FM pick, Phone keeps FM
  there, and the ISS auto-arm sets FM and leaves a satellite you are working alone.
- **Nexus Remote from a second Windows PC (Beta), in the regular Windows installer:** operate the shack on your
  network or over the internet, add the station by signing in, take control from another PC, and save the shack's
  files without replacing a file already on that PC.
- **Remote keeps the station's own settings at the station:** transmit limits, keying lines, the TQSL path,
  logging-service keys and the addresses they go to.
- **Logbook:** export the contacts you pick, open a contact's editor from a cockpit or Awards, a Recent contacts box
  under the log entry, and Fetch LoTW that works on a large account.
- **Radios:** the KPA1500 over the network and at its fast baud rates, PstRotatorAz direct, the FT-1000 and FT-1000D,
  the IC-7300 family's scope, the Icom network connection's SWR, and the IC-7300's sound card on a Mac.
- **Windows:** the FFT library uses SSE2 and AVX, so the WSJT-X modes finish decoding sooner.
- **Security:** the certificate libraries drop the distrusted emSign certificates, the updater no longer carries the
  `zip` library, the local sharing ports cap the length of a command, the PstRotatorAz link's rotator port hangs up
  on a web page, and Remote refuses a saved-profile list from afar that the station cannot read in full, and never
  sends out a list of its own that it cannot read.

## Upgrading from 1.19

- **FT8 and FT4 decode at new moments.** FT8's full decode runs 14.4 s into the period instead of after the period
  ends. FT4 decodes once, at 6.05 s, instead of at 5.5 s and again at 7.5 s. FT8's early decode still starts at
  11.8 s, and now stops at 13.4 s if it is still running.
- **Decodes show as they are found.** The decode list fills during each pass instead of in one block, and loggers
  and PSK Reporter get each decode as it is found. Each still appears once.
- **A caller shown as `<...>` is passed by, and free text FT8 cannot send is refused.** Nexus works the station once
  its call shows in full, as WSJT-X does. A refused free text says why and your queued message stays queued: after
  two calls only a report, R-report, RRR, RR73 or 73 fits, and free text on its own, as in Tx 5, holds 13
  characters.
- **A late reply changes the over on the air.** You will see a second Tx line in ALL.TXT for an over that switched,
  and GridTracker and JTAlert are told the new message as it switches. A Tx button clicked during an over switches
  it the same way.
- **Times in Band Activity and Rx Frequency are the start of the period.** A line the full decode found at :34 now
  reads :15. Hold the pointer over the time to see when it arrived. Your own transmissions still show when they were
  sent. JTAlert, GridTracker and loggers get the same period start over UDP.
- **A logger's Reply no longer turns TX back on after you stop it.** After Stop TX, Esc or a TX watchdog trip, a
  Reply from GridTracker or JTAlert is ignored until you turn TX on yourself. A logger's free text with "send" goes
  out only if TX is already on, as in WSJT-X.
- **FT8, FT4, JS8 and the other timed modes wait while another program keys your radio** through Nexus's CAT
  broker. Your CQ or QSO stays armed and goes out in your first period after the program lets go.
- **RTTY channel picks on AFSK tune a mark tone higher.** On 20 m the dial reads 14.0851 MHz, so the signal lands
  on the channel's frequency. FSK is unchanged.
- **Reverse in RTTY is only for a station sending upside down.** For USB, use the new **AFSK sideband** choice in
  Settings ▸ Digital ▸ RTTY.
- **SSTV's 145.800 and 144.500 MHz FM channels, the ISS park and Phone kept on them are simplex with no tone.** A
  repeater you tune keeps its own offset and tone.
- **An unassisted contest entry stops the classic CW decoder as well as the AI one.** Ending the entry brings the
  decoder back as you had it.
- **The assistance record and the contest-category note name the CW decoder**, not the AI CW decoder. Rows recorded
  before keep the name they were written with.
- **Your sent exchange and the {EXCH} macro carry a serial only in a contest whose rules ask for one.** Each contest
  contact's number goes into the exported ADIF as STX, and into its copy in your Logbook.
- **Contacts logged while Contest mode is on wait to upload** to QRZ, Club Log and the other services until you
  merge the contest log or turn Contest mode off. LoTW's automatic upload waits too.
- **The AI CW decoder uses more of the processor** for its faster copy: about half of one core where it used a
  sixth, never more than two thirds of a core on a slow computer. On Windows it runs at a lower priority than the
  radio and audio.
- **CW on a Flex over SmartSDR CAT, and on Yaesu radios keyed over CAT with no CW stop** (FT-991A, FTDX10 and
  others), goes to the radio a word at a time, and Nexus's CW speed wins over a speed turned on the radio.
- **The CAT keyer refuses a message holding a `;`**, a control character or a character with no Morse (an
  apostrophe, `"`, an accented letter). The CW screen says which character, and nothing keys. The WinKeyer, the
  serial key line and the soundcard keyer key as before.
- **FT-1000 and FT-1000D owners:** choose FT-1000 or FT-1000D in your radio settings instead of FT-1000MP.
- **PstRotatorAz** is driven by Nexus itself now, and a station already set up with it keeps its settings. Nexus
  listens for its replies on its UDP Control Port + 1 (12001 by default).
- **The first Fetch LoTW on a large account asks a year at a time**, newest first. The Connections log shows "piece
  n of m".
- **Nexus Remote from a second PC is a Beta, in the regular Windows installer.** During the beta its internet road
  runs on a test service. When the regular service starts, the beta accounts and their stations are deleted, and
  each shack then signs in or pairs again. Nothing is billed during the beta, and using it does not start your trial.
- **Nexus Remote from a second PC: both PCs must run the same version.** Update the shack first, or both together.
- **A PC added by signing in waits for Approve at the shack** before it uses the internet road. Check the key it
  shows against the one at the shack.
- **Settings driven from afar:** the transmit limits and safety stops, the PTT and keying lines, the licence class,
  the TQSL path, the logging-service keys and the addresses they are sent to are greyed out there and say "Set at
  the station". Change them at the station.
- **The logging-service key in Settings shows as dots** until you press Reveal.
- **Signing in to Nexus Remote no longer asks for your name, nickname or picture**, and the shack's own sign-in does
  not ask for your email address.
- **The old Remote browser workspace is gone.** It was hidden in 1.17.0.

## FT8 and FT4

- **A late reply changes the over already on the air, as in WSJT-X.** A weak R-report or RR73 that only the full
  decode finds used to leave the old message going out to the end of the over. Now the rest of the over becomes the
  new message, the over keeps its start and end, ALL.TXT gets a second Tx line, and programs on the WSJT-X UDP feed
  are told as it switches. A Tx button clicked during an over switches it too.
- **Which late replies change the over follows WSJT-X.** Once an over carries your RR73 or 73, no reply changes it.
  A CQ run that logs the contact and goes back to CQ, or gives up on a caller who went quiet, lets the over on the air
  finish. If you close with RRR and your partner's 73 arrives during the RRR, the rest of that over is your CQ
  (WSJT-X's Tx6), and TX turns off after it with "Disable Tx after sending 73".
- **Decodes at WSJT-X's moments.** FT8's full decode runs 14.4 s into the period and FT4's decode at 6.05 s, so a
  weak RR73 or report is known before your next over and gets its answer. A decode still running when your over
  starts still counts as soon as it is done.
- **An RR73 answering your report is answered with 73.** When the station you sent your report to rogers it with
  RR73 or RRR without sending its own R-report, Nexus sends 73 and the QSO is complete, where it kept sending the
  report.
- **An RR73 answering your RRR is answered with 73**, once, and the contact is logged once. In a Call CQ run with
  "Roger with RRR (not RR73)" on, the caller's RR73 gets one 73 and the run then calls CQ again. A Hound never sends
  the 73: by Fox/Hound's own rules the Fox's RR73 ends a Hound's contact, as in WSJT-X.
- **A repeated report carries the new signal report**, as WSJT-X 3.0 does, in FT8, FT4 and FST4. Heard at +20 and
  then +27, you send R+27 the second time and log +27.
- **Flex native audio (Beta): the late-reply switch works on DAX.** Nexus sends the radio only the audio that is
  due, so the part not yet sent is replaced, and Stop TX still cuts the over at once.
- **Each decode carries the start of its period.** Band Activity and Rx Frequency show the period's start, with the
  arrival time when you hold the pointer over it, and the period separators are right for MSK144, FST4, Q65 and JS8
  periods other than the usual one. Over UDP, each decode and a logger's Replay carry the period start, as WSJT-X
  sends it.
- **Programs that listen as they do to WSJT-X see what you are transmitting:** the message on the air, whether TX is
  enabled, the report you are sending, the station's grid and a tripped TX watchdog, the moment one changes.
- **A logger can no longer turn TX back on after you stop it**, and a logger's free text goes out only if TX is
  already on.
- **FT8 and FT4 no longer transmit on top of another program keying your radio through Nexus.** With "Other programs
  may key transmit" on, a program on the CAT broker (WSJT-X, VarAC) could key while your CQ or QSO was armed, and
  Nexus sent its own over under that key. Nexus now waits. Stop TX still drops the other program's key.
- **Windows: FT8, FT4 and the other WSJT-X modes finish decoding sooner.** The FFT library in the Windows build uses
  the processor's SSE2 and AVX instructions, as WSJT-X's own Windows build does. On one Windows PC the early FT8
  decode took 17-36 % less time and the full decode 1-21 % less, with exactly the same decodes. The library picks
  what the processor has, and a processor without AVX works as before.
- **The FT screen's transmit meters keep their places** from the moment the radio keys, instead of stretching and
  shrinking as each reading arrived.

- **Each decode counts the moment it is found, as in WSJT-X.** Nexus handed a pass's decodes to the QSO sequencer
  and the decode list only when the whole pass had finished. Now each one goes on as soon as it is found, and your
  RX frequency is looked at first, so a weak reply on your frequency is in the decision for your next over even on
  a slower computer, where it used to arrive after the over had started. The decodes are unchanged and each still
  appears once, in the list, in ALL.TXT and to loggers and PSK Reporter, which now get each one as it is found. A CQ
  run that picks the strongest, farthest or CQ callers first still compares all the answers a pass brings.
- **FT8's early decode at 11.8 s stops at 13.4 s** if it is still running, as WSJT-X's does, so a slow computer no
  longer loses that period's full decode to it.
- **The diagnostic log says when a period's decode was dropped.** When the decoder is still busy as a period ends,
  Nexus skips that period's decode. It said so only on a console that Windows never shows; the line, with how many
  periods have been dropped so far, is now in the log you send with a report.
- **Your RR73, RRR and 73 to a station signing W9XYZ/M are the ones WSJT-X sends.** Outside a contest, closing a
  contact with a station whose call has a suffix such as /M or /QRP, or a prefix such as PJ4/, sent
  `<W9XYZ/M> K1ABC RR73`: the right two calls, but not WSJT-X's message. Nexus now sends `W9XYZ/M <K1ABC> RR73` (or
  RRR) and `W9XYZ/M <K1ABC> 73`, the same message bit for bit, also when your own call ends in /P or /R, and the Tx
  panel's Tx4 and Tx5 show them that way.
- **Typing 73 to a station signing W9XYZ/M no longer sends it to W9XYZ.** The free text you type for the next
  message (`73`, `RR73`, `RRR` or a report) went out as `W9XYZ/M K1ABC 73`, which goes on the air as another
  station's call, `W9XYZ K1ABC 73`. Nexus now sends the message WSJT-X sends for the two calls, such as
  `W9XYZ/M <K1ABC> 73`.
- **A clicked Tx3 row goes out as it shows.** With your call ending in /P or /R and a station signing W9XYZ/M, the
  Tx3 row `<W9XYZ/M> K1ABC/P RRR` went out as the Tx4 message. It now goes as shown, as WSJT-X sends what its Tx3
  box holds.
- **A caller shown as `<...>` is not answered.** A station whose call has a suffix or a prefix can show as `<...>`
  until Nexus has decoded its call in full. A CQ run answered such a caller, sending your report or roger to a call
  no one could read, and a double-click on its CQ or another message, a logger's Reply, the Remote page or a click
  on `<...>` started a contact with it. Nexus now passes it by and answers the next station that called you, or
  calls CQ again, and a click says why. Once the call shows in full, the station is worked as usual. WSJT-X answers
  no `<...>`.
- **Free text FT8 cannot send no longer keys a steady tone.** Nexus puts both calls in front of the free text you
  type, so `TNX 73` becomes `W9XYZ K1ABC TNX 73`, a message FT8's encoder cannot build. On FT8 it went out as one
  steady tone for 13 seconds; on FT4 nothing went out, though Band Activity and ALL.TXT showed it sent. Now such a
  message is never transmitted, from the free-text box, a Tx row, a click during an over, the contest screen or
  Remote, and Nexus says why. The message you had queued stays queued, and text FT8 can send goes out as before.
- **A logger's Replay no longer marks old decodes as new.** When JTAlert or GridTracker asks Nexus to replay the
  decodes, they now go out marked not new, as WSJT-X sends them, so the program does not alert on them again.
- **A muted or unplugged sound card no longer finishes a QSO for you.** When a whole FT8 period's receive audio is
  under half a bit, as with the rig's audio muted, the input turned right down or the cable pulled, WSJT-X does not
  decode it at all. Nexus did, and after a busy period it could "hear" that period's stations again in a few stray
  bits of noise; in a QSO such a line could read as your partner's RR73 or R-report, and Nexus sent the next message
  and logged a contact that was never finished. Such a period now gives no line anywhere, and the QSO repeats its
  last message as WSJT-X does.

### Contest FT8 and FT4

These are on the Contest screen's Running and S&P. Field Day's messages are ARRL Field Day, Winter Field Day, and
FT1, DX1 or FT2 in any contest.

- **A late reply changes the over on the air inside a contact.** Your exchange becomes the RR73 when the runner's
  rogered exchange arrives, and the rogered exchange becomes the 73 when the caller's RR73 arrives. It never starts a
  contact: a CQ on the air is never switched to an answer.
- **Field Day: only your caller's own RR73 closes the contact.** An RR73, RRR or 73 sent to you by another station
  is ignored, as in WSJT-X, and your caller keeps getting your R and exchange until its own RR73 logs the contact.
- **Field Day S&P: only the station you called completes the contact.** A rogered exchange from a different station
  is ignored, and your exchange keeps going to the station you called.
- **Field Day: a station signing W9XYZ/M is logged as W9XYZ/M**, not `<W9XYZ/M>`, in the ADIF and Cabrillo files and
  the dupe check, so its next CQ is a dupe like any other.
- **Field Day: your RR73 and 73 to a station signing W9XYZ/M are WSJT-X's**, `W9XYZ/M <K1ABC> RR73` and
  `W9XYZ/M <K1ABC> 73`, the same message bit for bit.
- **Field Day: when you run, a station signing W9XYZ/M can close the contact.** Its `<K1ABC> W9XYZ/M RR73` is taken
  as sent to you, the contact is logged, and in ARRL Field Day on FT8 and FT4 it gets WSJT-X's 73.
- **Field Day: a call shown as `<...>` is not answered or logged.** Once the station has sent its call in full, in a
  CQ for example, its next message is worked as usual.
- **ARRL VHF contests and the standard messages: your 73 to a station signing W9XYZ/M carries its call.** It read
  `W9XYZ/M K1ABC 73`, which goes on the air as another station's call, `W9XYZ K1ABC 73`. Nexus now sends WSJT-X's
  `W9XYZ/M <K1ABC> 73`, and the RR73 in the standard messages is WSJT-X's too.
- **A double-click on a CQ from `<...>` answers nothing** with the contest switch on, and says why, as WSJT-X answers
  no `<...>`. Once the call shows in full, a double-click works it as usual.
- **ARRL VHF contests and the standard messages: S&P passes a CQ from `<...>` by**, as Field Day's S&P already did,
  and answers the station's next CQ once its call shows.
- **Contests with the standard messages: when you run, a station signing W9XYZ/M that runs WSJT-X gets an
  answer.** It calls with `<K1ABC> W9XYZ/M` and no grid, and your run kept calling CQ unless that station had Skip
  Tx1 on. Nexus now answers with your report, `<W9XYZ/M> K1ABC -12`, as WSJT-X does, and logs W9XYZ/M with no
  grid. The ARRL VHF contests and Field Day are unchanged.

## TCI (Beta): Nexus drives an SDR program

- **A TCI connection.** In Settings ▸ Radio ▸ Rig & CAT, Connection has **TCI (SDR program) — Beta**, for ExpertSDR3
  (SunSDR), Thetis 2.10.3.14 and later, and piHPSDR. You set the program's address and TCI port. **Find TCI server**
  looks for the program and says what answered, such as its version and the radio it drives; it only reads. The
  setup wizard looks for a TCI server on this computer and has an **SDR program (TCI)** choice.
- **Transmitting through the SDR program.** PTT on the Phone screen keys the program's own microphone, and Tune keys
  its tune carrier at its own tune drive. With **Audio over TCI** on, as it comes, Nexus's own audio goes to the
  program with no virtual audio cable: FT8, FT4, JS8 and the other digital modes, RTTY, PSK, SSTV, APRS, the voice
  keyer and the Remote microphone. FT8 and FT4 keep WSJT-X's mid-over switch. Nexus keys one of these overs only once
  the program has taken Nexus's audio, an over whose audio the program stops taking is ended with the reason on
  screen, and Stop TX ends an over at once. With Audio over TCI off, a TCI radio keys like any CAT radio and you route
  Nexus's audio with a virtual audio cable. CW goes out with the Soundcard keyer. Every Nexus transmit safeguard
  applies.
- **The SDR program's panadapter in Nexus's scope.** **SDR panadapter in the scope** (off as it comes) draws the
  program's own spectrum, 48 kHz as the programs come, in the RF scope and the Phone and CW scopes, each station at
  its own frequency. If the program stops sending it, the scope says so.
- **Nexus's needed spots on the SDR program's panadapter.** **Spots on the SDR panadapter** (off as it comes) puts the
  Needed board's stations that have a frequency on the program's panadapter, in the board's colours, the most wanted
  first and at most 100 at once. A spot goes once its station has not been heard for 15 minutes. A click on one in
  the program tunes Nexus there and does nothing else; while Nexus transmits it is ignored. On AetherSDR, Nexus takes
  off only its own spots and keeps each one up past AetherSDR's 30 minutes. An unassisted contest entry puts none up.
- **A clean close.** When Nexus closes the connection, it reads on for up to 0.2 s until the program answers, so the
  program has read Nexus's last commands, the transmitter's stop among them.
- **A slow program can no longer hold up a stop.** Each read gives up after 20 ms and carries on next time, each
  message to the program has 2 s in all, and connecting gives up after 3 s. A program that takes longer is treated as
  a lost connection; if Nexus was transmitting, it says the radio may still be transmitting, connects again and sends
  its stop first.
- **No more PTT warning after a CW message that went out whole** on the Soundcard keyer over TCI.
- **Programs sharing your radio are no longer hung up on for asking what the radio cannot tell them**, and neither is
  Nexus's own TCI connection, which was dropped and reopened over and over while it read the radio's levels.

## Share by TCI: programs on this computer use the radio through Nexus

- **Share by TCI** is a new switch in Settings ▸ Radio ▸ Transmit limits & sharing, off as it comes. Turned on and
  saved, Nexus is a TCI server at 127.0.0.1:40001 (with a Copy button). WSJT-X, JTDX, MSHV, Log4OM and other programs
  that speak TCI connect there and use whichever radio Nexus runs: they follow and set the dial, switch the sideband,
  may lower the power but never raise it, and get the receive audio Nexus hears. Only programs on this computer can
  connect, never another computer or a web page.
- **Programs on TCI may transmit** is a separate switch, off as it comes and set at the station only. With it on, a
  program can Tune, key the radio's microphone on the Phone screen and send a logger's CW macros on the CW screen,
  which Nexus's own CW keyer sends at the program's speed. No program can turn TX on.
- **A program's own transmit audio (Beta).** With that switch on and Nexus on the Phone screen, a program can put its
  own audio on the air, a logger's voice macro for one. Nexus keys the radio when the audio arrives, plays it the way
  it plays its own, and plays out the tail before it unkeys. It ends at once on Stop TX, Esc, TX Off, a logger's UDP
  Halt Tx, the Remote's Stop, the high-SWR cutoff, a band or radio change, the program's own stop or lost connection,
  0.3 s with no audio, and 10 minutes on the air. A program must ask for a voice mode (USB, LSB, AM or FM) first, and
  an over from a program whose last mode request was a data mode is held like FT8.
- **The programs connected** are listed with what each is doing and the last thing Nexus refused it, each with a
  **Disconnect**. Programs on a Flex's native connection with **Flex native DAX audio** on hear the radio's own DAX
  audio.
- **Spots from programs on Nexus's band map (Beta).** With **Spots from programs** on, as it comes, the spots a
  program sends over TCI (a logger's cluster spots, or a skimmer's) show on the band map and on the Phone and CW
  screens' band activity, marked with the program they came from. Nexus keeps at most 200 from one program and 500 in
  all, each for 30 minutes. A click on one tunes Nexus there and does nothing else, and every click on a spot in Nexus
  is passed back to the programs, so a logger can pick up the station. A spot or a click never transmits.
- **A new guide, Share by TCI**, covers turning it on and setting up each program.
- **A program's mode request no longer outlives its connection**, so a program listed later under the same Connection
  number never counts as having asked for a voice mode.
- **A program that reads only a few bytes at a time is disconnected after 5 s**, and whatever it was transmitting
  ends.

## Contesting

### The log and the score

- **Every contest log numbers its contacts 1, 2, 3…** in a first column, in the contest screen's log and the logger
  window. In a serial contest the number is the serial you sent. A removed contact's number is never reused, an edit
  keeps it, Clear test contacts starts again at 1, and the count carries on after a restart.
- **The N1MM broadcast carries each contest contact's number** in N1MM's sent-number field.
- **Contest operating time.** The scoreboard shows On time and Off time beside the rate meter, and the Score Summary
  states both. A break of 30 minutes or more between two contacts is off time, as the sponsors count it.
- **Statistics per operator.** In a multi-operator contest the score area has an Operators section: each operator's
  QSOs, points, the multipliers they worked first, their rates, their best clock hour and a band × mode table. The
  Summary export carries it, and a new **Operators** box shows who is at the key and the leaderboard.
- **Every save of the contest log keeps backup copies** in a `fieldday_history` folder beside it: the last 10 saves
  and one copy per hour for the last 48 hours, a few MB at most. Nexus never reads them back on its own. To use one,
  quit Nexus, copy it over `fieldday_backup_<n>.adi` (keep the damaged file first) and start Nexus again, or import it
  into the Logbook like any ADIF file.
- **Contacts held back during a contest survive a restart.** While Contest mode is on, a contact logged in the
  Logbook waits to go to QRZ, Club Log and the other services. That list now survives closing Nexus, and each contact
  goes once when you turn Contest mode off.
- **Contest contacts no longer upload while the contest runs.** A contact logged anywhere but the contest entry strip
  went to every logging service at once. The contest's contacts go out when you merge the contest log into your
  Logbook, and any other contact logged while Contest mode was on goes out when you turn it off.

### The entry

- **Shift+Space goes back a box**, as Space moves to the next one.
- **The county or state box fills itself** from your last contact with the station in this contest, else your
  call-history file, else your most recent contact with them anywhere in your log. A small word over the box says
  where it came from, and nothing you typed is written over. Mobiles and rovers are never filled.
- **An early "Worked" warning at QSO parties.** As soon as the call is in, a station you already logged on this band
  and mode turns the Call box amber and lists the counties or states you worked it from. It turns red only when the
  county you type is one already logged.
- **A dupe is plain to see:** a red **DUPE** badge on the Call box with when and where you worked them, for example
  "DUPE 14:32 20m CW", announced to a screen reader. Enter still logs it as a dupe that scores nothing, and with Enter
  Sends Message on a dupe never gets your exchange.
- **An optional RST sent box.** "Show the RST sent box" in Settings ▸ Contesting ▸ Contest adds it before the
  received report, as N1MM shows Snt and Rcv. Space and Tab skip it, and a report typed there goes into the Cabrillo
  and ADIF files for that contact only.
- **CW speed match.** With **Match WPM** on, when you answer a station Nexus sends at the speed it measured for that
  station, between the slowest and fastest you set in Settings ▸ CW. Your CQ, and everything after the answer, goes
  at your own speed. Off until you turn it on.
- **The entry's boxes are full size** in the Phone, CW, RTTY, PSK and JS8 screens, and the grey sample text in empty
  boxes is quieter so it no longer reads like something typed.

### The Contest screen and its windows

- **"Work this contest in CW" and "Work this contest in Phone"** take the main window to the CW or Phone screen, where
  the contest's log line has Enter Sends Message, Running / S&P and the F-key messages. The Contest screen and the
  logger window log and keep score but never transmit.
- **A Resume line when the contest is not running.** If Contest mode is on but the contest stopped, the CW and Phone
  log line says so and offers **Resume**, which puts the contest back in S&P and never starts calling CQ. Opening the
  CW or Phone screen enters the contest again in S&P too.
- **A big clock** on the Contest screen's top line: UTC first, your local time beside it.
- **Drag to resize the log and the last contacts**, with the mouse or the arrow keys. Each window remembers its own
  sizes.
- **The contest logger window drags down to 420×320** with its entry still on screen, **Keep in front** keeps it over
  your other programs, and it reopens where you left it, on a monitor that is still attached. **⧉ Pop out entry**
  opens it with the entry alone, sharing the contact with your entry.
- **Esc stops transmitting in the contest's pop-out windows**, as it does in the main window.

### Spots and the band map

- **Spot me.** A **Spot me** button on the Contest screen posts ONE spot of your call on your current frequency to the
  DX cluster, with a short comment you can edit. The Spots board has its own, with "CQ CW", "CQ SSB" and so on. Both
  share a one-minute wait, and nothing is ever spotted without your press.
- **Spot me asks twice where self-spotting may be against the rules** (CQ WW, CQ WPX, CQ WW RTTY, the California QSO
  Party, and any contest whose rules Nexus has not checked on this). Press **Spot anyway** within 10 seconds to post.
- **Frequency markers, as N1MM's Mark.** Press **Alt+M** in the contest entry, or right-click the band map, to mark
  the frequency you are on with the call in the entry. Click a marker to tune straight back (never while
  transmitting). It goes by itself once you log that call on that band. Nothing is posted or spotted.
- **Dupes and new multipliers on the band map.** While a contest runs, the Band activity strip, the pop-out band map
  and the Spots list in CW and Phone strike through a station you already worked and tag a new multiplier **MULT**,
  naming it in the tooltip. A multiplier from the exchange is known only for a station you worked before.
- **Multipliers predicted from your call-history file.** A station you have never worked shows **MULT?** when the
  county, state, section or zone the file gives it would be a new multiplier.

### Unassisted entries

- **An unassisted entry stops all CW decoding.** The classic decoder used to keep copying CW, offering call chips and
  filling His Call. CQ WW counts a CW decoder as assistance, so now neither decoder runs, and the Decode pane and the
  CW Copilot pane say why.
- **Declaring NON-ASSISTED offers the Unassisted entry switch.** Picking it writes it into your Cabrillo log but turns
  nothing off; a line beside the picker now says so, with a button that turns the switch on.
- **The contest-category note no longer calls your radio's own decodes exempt.** Both CQ WW's and ARRL's rules count
  any technology that identifies a callsign or multiplier.

## CW

- **A turn cue for when you copy with the decoder.** A small lamp at the start of the CW send row says whose turn it
  is (`≋ W1ABC sending`, `▶ Your turn · F3`, `⏸ Hold · W2XYZ`, `? Not sure · F7`), and the F-key that fits glows. It
  never transmits. It shows with the AI decoder on and the Decode pane shown, never in an unassisted entry, and
  unticking Turn Cue in ⊞ Panels hides it, after a restart too, until you tick it again. A screen reader hears
  "Your turn" and "Hold", and a chirp is there if you turn on CW turn sound.
- **The AI CW decoder keeps up with the signal.** Copy of a new station starts about a second after it does, letters
  land about a second after they are sent, and the last few letters show at once in a dimmer colour until the decoder
  confirms them. Only confirmed letters are used for the call chips, the log and the Remote copy.
- **The classic decoder copies fast stations from the first few characters**, anyone sending 15 to 50 WPM within
  about six characters, also through noise and uneven hand keying.
- **The decoder's speed is only from keying heard in the last 10 seconds**, and clicking a spot or tuning away starts
  it afresh, so a chip click never sends at the previous station's speed.
- **Esc ends a CW message sooner.** The WinKeyer, the serial keyline and the soundcard keyer stop before Nexus asks the
  radio to stop its own CW. On an FT-710 or FTX-1 keyed over CAT, the stop also sends Yaesu's playback stop.
- **On a radio that cannot stop mid-word, Esc leaves at most the word being sent.** On a Flex over SmartSDR CAT, and
  on Yaesu radios keyed over CAT with no CW stop, Nexus hands the radio one word at a time and sends its own speed at
  the start of every message.
- **The CAT keyer no longer passes a `;` to Kenwood, Yaesu and Elecraft radios**, where it would be read as a radio
  command. A message holding one, or a character the keyer cannot send, is refused before anything keys. A callbook
  name that `{HISNAME}` keys keeps only the characters the keyer can send.
- **The power, ALC and SWR meters track during CW**, as they do in SSB, on every keyer, and the high-SWR cutoff sees
  the last word.
- **CW holds the transmitter until its last word ends.** Another program's PTT over the CAT broker, an ATU tune-up,
  an RTTY, PSK, SSTV or APRS over, or a Remote start waits or is refused with "CW is sending". Tune, Phone PTT, the
  voice keyer and FT overs no longer key on top of CW either; an FT slot that starts while CW is sending is skipped
  and the next free slot sends the same message.
- **"Colors" in the CW scope's head sits with its colour picker**, apart from the zero-beat reading.

## RTTY

- **A tuning scope beside the decoded text**, the classic crossed ellipses, with the same reading in words ("Tuned",
  "Signal 20 Hz low") and ◀ ▶ to move the decoder 5 Hz at a time. It is the ⊞ Panels entry **Tuning**, shown by
  default, and works from Nexus's own Remote client. (Idea: the Diddle project by WW2DX.)
- **Callsign tags on the waterfall.** A station the decoder copies sending its own call gets a small tag at its
  frequency; click it to net onto that station. Tags fade after 3 minutes without being heard. In a running contest a
  station already in the log is struck through.
- **Span and speed buttons and a brighter picture.** The header has span buttons (1k to 4k) and speed buttons (Fast,
  Med, Slow), both remembered, and a contrast curve makes a mark and space pair stand out about twice as strongly.
  The RTTY and SSTV waterfalls scroll at an even pace.
- **The waterfall shades the mark and space tones** where the decoder listens, following the AFC, and turns red while
  you transmit. **Auto-tune** nets onto the nearest signal at your shift.
- **AFSK on USB.** Settings ▸ Digital ▸ RTTY has an **AFSK sideband** choice, LSB (the default) or USB. On USB Nexus
  uses the rig's upper data mode and swaps the tones itself, so mark stays high on the air.
- **RTTY's band channels put your signal on the RTTY frequency with AFSK.** A channel pick tuned the dial to the
  channel's frequency, so the AFSK signal went out a mark tone below it, among the FT4 signals on 20 m. With AFSK
  these picks now tune a mark tone higher. FSK is unchanged.
- **Netting is refused while you transmit**, from the waterfall, the tags and the tuning scope, so a click can no
  longer move the AFSK tones of an over on the air.

## SSTV, 2 m FM and the ISS

- **SSTV's FM channels and the ISS auto-arm transmit on the channel, with no repeater offset or tone.** After you had
  tuned a repeater, 145.800 and 144.500 MHz FM used that repeater's offset and tone, so on 144.500 the radio was set
  to transmit on 143.900 MHz, outside the 2 m band. These channels are simplex with no tone now, as APRS is.
- **Moving from SSTV to Phone no longer retunes the radio.** Phone stays on the SSTV frequency until you move it,
  unless your licence may not use it for phone.
- **On a 2 m FM SSTV channel, Phone keeps the radio in FM**, and its AUTO mode button reads FM, until you move off the
  channel or pick a mode.
- **With two radios, an FM channel picked on the SSTV screen goes to your FM radio**, and a frequency you pick right
  after an FM channel goes to the radio meant for its mode.
- **The ISS SSTV auto-arm puts the radio in FM** on 145.800 MHz, with two radios on the one set up for 2 m FM, and
  gives back your own frequency and mode at the end of the pass.
- **The ISS auto-arm leaves a satellite you are working alone.** While a satellite holds the radio, it only tells you
  the ISS is up, and arms if you release the satellite while the ISS is still up.

## Nexus Remote

### Operating from a second PC (Beta)

- **Operate the shack from a second Windows PC with Nexus on it (Beta).** On your own network, choose
  **Operate from this computer** beside your shack in **Stations on this network**. The second PC shows the full Nexus
  screens for the shack's radio, plays its receive audio and sends your microphone. Phone PTT, CW from the keyboard
  and its macros, FT8 and FT4 (timed by the shack) and JS8 work as at the shack. Stop has its own road, and the shack
  unkeys within about 5 s if the network or the second PC drops. There is no paddle from afar. It is in the regular
  Windows installer, a Beta like the rest of Nexus Remote, and needs no special build.
- **Over the internet too (Beta).** For a shack paired with a code, press **Sign in…** on its row in **Stations on
  this network** and sign in to Nexus Remote; a shack added by signing in has the internet road from the start.
  Either way, approve the PC at the shack: until then the road waits. **Operate from this computer** then reaches the
  shack over the internet when it is not on your network, and moves back by itself. The internet road is part of
  Remote's paid service. Nothing transmits unless Stop's own road is working. After 15 minutes with no click, key or
  PTT, Nexus asks "Still there?".
- **Add your station by signing in, with no code (Beta).** Press **Sign in…** in **Stations on this
  network** and sign in with the account your station is linked to, or one its owner invited to operate it. The
  station is then on that PC on its own network in one step. Over the internet the PC waits until you approve it at
  the shack, where Nexus asks with the PC's key: a sign-in alone never lets a PC or a browser operate the station over
  the internet. The PC shows its own key so you can check it.
- **Add your station from a PC that has never been on its network (Beta)**, such as a laptop away from
  home or a friend you invited: press **Not on this network** and the station is added over the internet, waiting for
  your approval at the shack.
- **A signed-in PC keeps its place on the station's network by signing in again every 30 days.** In the last week its
  row says by what day, and **Operate from this computer** signs in again by itself. A PC paired again with a code
  stays until you remove it at the shack (Beta).
- **Tune, ATU, RTTY and PSK (Beta), APRS, SSTV and Chat, and every JS8 switch** work from the second PC.
  Stop on either PC ends each, and losing the second PC for about 5 s ends them too. A satellite track started from
  the second PC stops when it drops.
- **Take control.** When another PC is operating the shack, the banner offers **Take control**: anything that PC is
  transmitting stops, and this PC starts with TX off.
- **The shack's recordings, ALL.TXT and SSTV pictures reach the second PC**, with **Save to this computer**, which
  never replaces a file already in that PC's Downloads folder: ALL.TXT saved a second time goes in as
  `ALL (1).TXT`, the way a browser names a download, and the message after the save names the file.
- **Stations on this network leads with Operate from this computer**, and **Operate at this address** works at an
  address or a computer name you type (`shack-pc.local`), which Nexus remembers with that station.
- **Updating the second PC no longer locks you out without warning.** While it operates a shack, its update notice
  names both versions and offers **Install anyway** only after you tick the warning.
- **Phone's Lock (hands-free PTT) works from a computer driving your station**, with a 3-minute limit from afar and a
  "Still talking?" question 20 seconds before it.

### Accounts and the browser stream

- **On the shack's own network, Remote stays free.** Stream and Listen work from a browser already approved for the
  station when the account's trial is over or its paid plan has lapsed, held to that network at both ends.
- **One trial per callsign and per shack computer.** Approving a station sends the station callsign and one-way
  hashes of this computer's hardware and installation IDs; the IDs never leave the computer.
- **Signing in shares less about you:** no name, nickname or picture, and the shack's own sign-in does not ask for
  your email address.
- **Inviting someone to operate says what that gives them:** all you can do from another computer, transmitting under
  your callsign and handling your log and your saved logins included. **Listen** gives none of that.
- **The shack's screen fits the browser window.** Browser zoom stays in the browser, the controls move to the side
  rail when that shows the shack's screen larger, and the shack's window takes the shape of the browser's picture
  while it streams. Stop TX stays in view at every size.
- **Pairing during a trial under another sign-in** now says so at the shack instead of saying access was denied.

### The station's settings stay at the station

- **Settings driven from afar show which settings only the station can change**, greyed out with "Set at the station":
  the transmit limits and safety stops, the switches that let other programs or a browser transmit, the PTT method,
  port and lines, the CW and RTTY keying lines, the Flex client's Beta switch, the licence class, the TQSL path, the
  logging-service keys and the addresses they are sent to.
- **A config profile saved from afar no longer carries those settings** into the station.
- **A save of Settings or of a profile from afar keeps the station's own value of every setting only the station
  can change:** what lets the station transmit with nobody at a key or lets another program or a browser transmit,
  the guards on a transmission, the lines that key the radio, the TQSL path, where your keys and passwords are sent,
  and the Club Log and Cloudlog keys. The CHANGELOG lists each one.
- **Logging-service keys stay at the station.** A computer driving it sees only whether one is saved, and the connector
  Test buttons run at the station and answer only whether it worked. A key a service echoes back is hidden from the
  message.
- **Working a spot from the second PC no longer turns TX on at the station**, and a voice-keyer message stops when
  that PC's permission to transmit is withdrawn.
- **Turning off Remote over this network takes effect at once**, however slow the disk.
- **A Linux station keeps Stop's road** after Nexus is paused and continued.
- **The built-in guide's Remote access section matches Remote as it is now.**

## Radios, amplifiers and rotators

- **Elecraft KPA1500 over the network.** Pick **Network (KPA1500)** under Connection and type the amplifier's address.
  Nexus uses its UDP server, so Elecraft's KPA1500 Remote keeps working alongside. A KPA1500 at 57600, 115200 or
  230400 baud on USB now connects too.
- **PstRotatorAz: Nexus talks to it directly**, with the commands its own manual documents, so pointing at a call or a
  bearing, STOP and the compass all work. "localhost" as its address works like 127.0.0.1.
- **Yaesu FT-1000 and FT-1000D in the rig list**, each with its own Hamlib driver, so clicking a spot tunes the radio.
- **IC-7300, IC-705, IC-905 and IC-7300MK2: scope span, reference level and Center/Fixed work** with Native Icom CI-V.
- **Icom network connection: the SWR reading is a trusted number**, so the high-SWR cutoff can be turned on.
- **Native Icom CI-V always tells the radio to stop transmitting as it closes**, even when the radio had stopped
  answering.
- **Mac: an IC-7300's sound card opens again.** Nexus opens a sound card by the name exactly as saved, ending spaces
  included.
- **A rotator set to a serial port that is not there says so, once**, naming the ports that are there, and starts by
  itself when the port appears.
- **A frequency the radio cannot be on is ignored.** With the wrong rig model or a garbled reply, Nexus could follow a
  reading such as 115.2 MHz and send it back to the radio. The CAT status now says what the radio reported and
  suggests checking the rig model.
- **Test CAT reports its own test**, not a dial message that arrived while it waited.
- **Phone: a refused ATT, PRE or monitor-level change says what failed.**

## Logging and confirmations

- **Export just the contacts you pick.** Ctrl-click (⌘-click on a Mac), Shift-click or Space picks rows in the
  Logbook, and **Export selected** saves them as an ADIF file in your Downloads folder. An export with no contacts
  says so and writes no file.
- **Click a contact to edit it.** A previous contact in a cockpit's lookup card, and a ✎ in Awards' confirmation
  checklist, open the Logbook's editor on that contact. A contact changed elsewhere since is not overwritten.
- **A Recent contacts box under the log entry** in Phone, CW, JS8, RTTY and PSK shows your last five contacts; click
  one to fix it. Turn it on in ⊞ Panels ("Recent contacts").
- **Fetch LoTW works on a large account the first time**, asking a year at a time and carrying on where it stopped.
- **Nexus names itself to QRZ** in callbook lookups and Logbook uploads, as QRZ asks. Your callsign is not sent in it.

## Your screen

- **The Log button stays on screen** in the Phone, CW, JS8, RTTY and PSK screens: the Log and Spot buttons stay at the
  bottom of the log pane while the fields above them scroll.
- **Spots pops out into its own window**, the whole board with its own filters, for a second monitor.
- **The voice keyer's Record, Import and Delete sit on the left of each slot**, away from Play, and **Delete** can be
  undone for about 8 seconds.

## JS8

- **JS8 sends to your own groups**, such as @MAGNET, and a compound call such as KD4JRX/WFD can go in the To box.
- **JS8 sends to a portable call**, W1AW/P, as JS8Call sends it, the same frame byte for byte.

## Also in this release

- **Security: the certificate checks behind Nexus's downloads no longer trust the distrusted emSign certificates**
  (RUSTSEC-2026-0381, 0382 and 0383, published 20 October 2026). Nothing to do on your side.
- **Security: the in-app updater no longer carries the `zip` library**, which three advisories cover
  (RUSTSEC-2026-0367, -0368 and -0369). Nexus's updates are installers, so that code was never used.
- **Security: the radio-sharing ports and the PstRotatorAz link's rotator port cap how much Nexus reads for one
  command.** A local connection sending an endless command is ended. These ports are reachable only from this
  computer.
- **Security: the PstRotatorAz link's rotator port hangs up on a web page.** A page open in a browser on this
  computer can send a request to that port. Nothing in such a request moved the antenna, but only because of where
  a browser's request puts a blank line. The port now ends the connection at the request's first line and answers
  it nothing, as the radio-sharing port does. Rotator programs are unaffected.
- **Security: Nexus Remote refuses a saved-profile list from afar that the station cannot read in full.** Nothing
  of that save is written, so a profile saved from another computer can never change the settings set at the
  station only. If the station's own saved profiles cannot be read in full, they are not sent to that computer, so
  no logging-service key saved in a profile leaves the station.
- **The old Remote browser workspace is removed.** Remote is the shack's own screen streamed to your browser.

## Known issues

- CW over TCI goes out with the Soundcard keyer; through the SDR program's own keyer it is not ready yet.
- Programs on Share by TCI may not send FT8, FT4 or the other slot-timed modes yet, until the delay Nexus adds to a
  program's timing has been measured.
- A program's own voice audio through Share by TCI needs a voice mode request: WSJT-X, JTDX and MSHV with their Radio
  mode at None are refused, and WSJT-X and JTDX ask for a mode only when it differs from Nexus's. On a Flex with native
  audio on the Phone screen, a program's audio is refused.
- piHPSDR's TCI has no panadapter and no spots. When Nexus clears its spots from an SDR program's panadapter, some
  programs also clear spots another program put there.
- The Icom network connection carries no audio, so programs on Share by TCI hear the sound card there.
- An IC-7300MK2 on USB still shows "SWR?".
- Cabrillo has no field for operating time, so the file is unchanged.
- From a second PC there is no paddle, and on a network that blocks Stop's own road (some hotels and offices)
  transmitting is off.
- Nexus Remote's internet road from a second PC runs on a test service during the beta (see Upgrading).

**180 changes since 1.19.0.** The full list is in the CHANGELOG.
