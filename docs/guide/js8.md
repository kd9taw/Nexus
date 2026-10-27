# JS8

JS8 is keyboard-to-keyboard chat on FT8's waveform, on the air compatible with
[JS8Call](https://js8call.com): the same four speeds, the same heartbeats, directed
messages, relays and store-and-forward inbox, so a JS8Call station and a Nexus station
work each other without either knowing the difference. It is a ragchew mode, not a
contest mode — a short exchange takes a few periods, a paragraph takes a minute — and it
is the mode of choice on 40 and 20 m when the band is too poor for voice and FT8's
canned exchange is not a conversation.

**Before you start.** JS8 is on out of the box — look in the Digital group of the rail,
after APRS. If you don't operate it, switch it off in
[Settings ▸ Appearance ▸ Features](settings-reference.md#features). Entering the screen
tunes the rig to the JS8 watering hole for the current band (7.078, 14.078, 3.578 … —
JS8Call 3.0.3's own list, USB, or your own; see [Frequencies](#frequencies-the-decode-filter-and-the-band-schedule))
and starts decoding every speed at once. Nothing transmits
until you enable TX in the header, every session. The mode's own switches — transmit
speed, which speeds to decode, heartbeat interval, heartbeat networking and its rules, the
three automatic-reply switches, the idle watchdog and your INFO/STATUS/group texts — are in
[Settings ▸ Digital ▸ JS8](settings-reference.md#js8).

## The tour

**The header.** A **JS8** badge, then the five speed chips — **Slow · Normal · Fast ·
JS8 40 · JS8 60** (30 / 15 / 10 / 6 / 4 second periods), named as JS8Call 3.0 names them:
JS8 40 is the speed called Turbo before 3.0, and the chip's tooltip says so; JS8 60 is 3.0's
newest speed (its tooltip calls it Ultra, its name in JS8Call's code). The lit one is the speed
*you* transmit at and the period the TX clock follows. Beside them an **RX ▼ n/5** plate says
how many of the five speeds the receiver is decoding right now (all five by default). Then the band
picker with the JS8 watering holes, license-filtered per band; a **Drive** slider (TX
audio level — set it so the rig's ALC barely moves); **Tune**; an **ATU** button when your
rig reports a tuner; the **TX On / TX Off** latch; **Stop TX**; and **⊞ Panels**.

**The waterfall.** Click sets your RX offset, right-click your TX offset, Shift-click
both. JS8 stations sit anywhere from 500 to 2500 Hz; heartbeats cluster in the
500–1000 Hz sub-band by convention.

**The panes** (each removable from ⊞ Panels). The three lists and the Inbox are JS8Call's own;
each list's **⋯** in its title bar holds what JS8Call's View menu and right-click menus hold, and
each row has a **⋯** of its own (a right-click on the row opens it too).

- **RX** — JS8Call's RX pane: what is sent on your RX offset, and what is addressed to you or
  a group you joined wherever it is in the passband, each message on one line as its frames
  arrive ("time · offset · text"), closed by **♢** after its last frame. **……** stands where a
  frame did not come within one and a half periods. Your own transmissions print here too,
  tinted, and lines addressed to you carry a bar. Double-click a line to write to the station
  that opens it. **⋯** ▸ **Clear** empties it and the compose box with it, as JS8Call's does;
  **Clear all lists…** empties the three lists and the compose box after asking. The text and the
  command go, the call in **To** stays; while a message of the compose box is going out, the box
  keeps what is still to go and the message goes on.
- **Band activity** — one row per audio offset holding every frame heard there (the last ten),
  joined, with ♢ after each message. Columns: offset, age, SNR, and from **⋯ ▸ Show columns**
  time delta and speed too, as JS8Call's; **⋯ ▸ Show columns ▸ Minimal column labels** (off, one
  switch for both lists, as JS8Call's) shortens the headings to JS8Call's (Off, SN, TD, Sp, Call,
  Dist) and drops Hz and ms from the cells; **⋯ ▸ Sort by** offers JS8Call's orders; **⋯ ▸ Show
  band heartbeats and ACKs** (on) hides heartbeats and their acknowledgements. CQ rows are
  tinted green, traffic to you carries a bar, and the two highlight lists in Settings tint their
  words. Double-click a row to move RX there. A row's **⋯**: **Jump to … Hz** (both your RX and
  TX offsets, as JS8Call's one offset; while a message is queued or going out, only with **Allow
  TX offset changes while sending** on, as in JS8Call), **Centre filter at … Hz** (while the
  decode filter is on), **Jump to … speed**, **Remove activity**.
- **Stations** — JS8Call's call list: `@ALLCALL` and your groups on top, then everyone heard,
  sorted by callsign (or any of JS8Call's orders from **⋯ ▸ Sort by**), a station with a
  message for you first. A station is kept as it was heard, as in JS8Call: W1AW and W1AW/P are
  two rows, each with its own report, and a message from W1AW/P comes from W1AW/P in the Inbox.
  One mark per station: **✕** blocked, **⚑** a message for you, **★**
  hearing you (it sent something to your call), **☎** called CQ in the last five minutes,
  **⚟** heard only through a relay. Columns as JS8Call's: callsign, age, SNR, offset, worked
  before ✓, logged name and comment, and from **⋯ ▸ Show columns** time delta, speed, grid,
  distance and azimuth. Click a callsign to address it; the five quick buttons send **SNR? ·
  GRID? · INFO? · HEARING? · QUERY MSGS** to that station, which answers by itself if its
  auto-reply is on; **HB** and **✉ n** chips show a heartbeat and messages you hold for it. A
  row's **⋯**: jump to its offset or speed (and, while the decode filter is on, centre the filter
  there), **Show message inbox**, **Pin to the top**, **Add new
  station or group…** (a call, or a `@GROUP` to join), **Remove this station** (until it is
  heard again) or **Remove this group** (leaves it; on `@ALLCALL` it turns on Settings' *Do not
  take part in @ALLCALL* and the row goes, and adding `@ALLCALL` turns it off and
  brings the row back, as in JS8Call), **Block this station** (Settings' RX block list).
  **Store…** at the end of a row leaves a message at your station for that station, as
  JS8Call's Store Message does: it waits in the Inbox, from your call, and goes out only when
  the station asks for it (QUERY MSGS, then QUERY MSG n). Storing it sends nothing.
- **Inbox** — JS8Call's message panel: messages addressed to you, and **MSG TO:** messages you
  are holding for someone else until they collect them (store and forward). Newest first;
  click a heading to sort by it. **Messages:** shows one station's. Clicking a message reads
  it; tick several and **Delete** them together (or press Delete). **Reply** puts
  `<path> MSG [MESSAGE]` in the compose box, the path the message came by, with `[MESSAGE]`
  picked for you to type over; nothing is sent until you send it, and while a message of yours
  is going out it puts nothing in, as JS8Call adds no text then. A new message to you puts a
  notice up for five minutes, as JS8Call's box stays, with **Show** to open it. A message held
  for one station stays until you delete it, as in JS8Call. One left for a group
  (`MSG TO:@GROUP`, a group you joined) is offered for 48 hours to every station that sends
  QUERY MSGS to the group, once to each, and handed over whenever a station asks for it with
  QUERY MSG n, again too, as in JS8Call. A long one keeps its `FROM <call>`: what does not fit
  one reply is cut from the end of the message, never from the `FROM`. The inbox keeps at most 100 messages
  (and 64 KB) of yours and as many held for others; when it is full, a message already handed
  over goes first, then the oldest, and a note above the messages says which message went and
  why until you press **Clear notes**. A held message is handed over in one transmission, which
  holds at most 19 frames at Slow, 39 at Normal, 59 at Fast, 99 at JS8 40 and 149 at JS8 60; one
  that needs more is not offered or handed over until you change to a speed it fits, and its row
  says so and names that speed.
- **Log** — the log strip, prefilled from the station you last addressed, as JS8Call's log
  dialog is: their SNR at your station in **RST sent**, the start time when you selected
  them (put their call in the To box), and the frequency the dial plus your TX offset. A JS8
  QSO logs as `MFSK` / `JS8` in ADIF.
- **Boxes** — up to six panes of [Conditions](connect.md) among these, from ⊞ Panels ▸
  **Arrange** ▸ **+ Add a box** at the foot of a column, each closed by its own **✕** (see
  [Phone](phone.md)). Nothing in a box transmits.
- **Moving them** — ⊞ Panels ▸ **Arrange**'s arrows, or drag a pane or a box by its title
  bar onto a column or between two panes (see [Phone](phone.md)). The Log moves only up and
  down in its column, and nothing in the TX dock can be dragged.

**The TX dock**, pinned under the panes. The **To** box (blank = everyone, a callsign,
`@ALLCALL`, or a `@GROUP` you belong to), the composer, a frame estimate ("3 frames ·
45 s") and **Send**; a command palette with JS8Call's 32 directed commands; JS8Call's
**REPLY · SNR · INFO · STATUS** buttons and **Saved** menu (see [Composing](#composing));
**CQ**; **HB NET** and, while it is on, **HB** with its menu (**▾**, or
right-click **HB**); the **AUTOREPLY · RELAY · HB ACK** chips; an automatic
reply's question, **Yes** or **No**; the queue with **Drop queue**; and the
idle-watchdog readout. **CQ**, **HB**, **REPLY**, **SNR**, **INFO**, **STATUS** and **Saved** are
greyed out while a message is waiting to go or going out, as JS8Call's are.

## Receiving

Every speed decodes at once, exactly as JS8Call 3.0's multi-decode does: a Slow station at
−26 dB and a JS8 60 station rattling off a paragraph on the same band both print. Untick
speeds in Settings if a small machine struggles. Settings saved before JS8 60 arrived keep the
four speeds they had; tick **JS8 60** under *Decode these speeds* to add it. A long message
arrives one frame per period and is stitched together by audio offset — you see the frames
arrive in Band activity (and in RX when they are on your offset or for you), and the finished
message in Inbox if it was for you. Settings ▸ Digital ▸ JS8 holds JS8Call's lists and marks:
the callsigns to hide in the RX pane, two highlight lists, the ♢ and …… marks, and a sound for
each of a CQ, a heartbeat, an ACK for you, a message to you, a message in your Inbox, a new
callsign and one you have worked (all off, as in JS8Call).

**JS8 60** is JS8Call 3.0's newest speed, and JS8Call calls it experimental: 4-second
periods, a signal 250 Hz wide, each frame about 2.6 s on the air, so the band carries a
paragraph in a few periods — when signals are strong. It needs the most signal of the five
(JS8Call puts its floor at −18 dB on its own scale, against Normal's −24). Pick it only
when the station you are working copies you well, and do not expect JS8Call 2.x stations to
see it: only JS8Call 3.0 and later decode it.

## Composing

The compose box works as JS8Call 3.0's does. Its switches and texts are in
[Settings ▸ Digital ▸ JS8](settings-reference.md#js8), under **Composing**.

- **Pills.** As you type, the parts of a message JS8Call recognises are drawn as coloured
  pills: who it goes to (a call, a relay route such as `W1AW>K1ABC`, or a `@GROUP`), who it is
  from, and the command (`SNR?`, `MSG`, `QUERY CALL K1ABC?` and the rest). Point at a pill to
  read what it does. A command typed with no address goes to the call in the **To** box, and
  the pill says so.
- **Cheaper words.** A word the JS8 dictionary sends in pieces is underlined in red.
  Right-click it for up to five words that take less airtime, cheapest first, and pick one to
  put it in place.
- **One-click messages.** **REPLY** puts the call in the **To** box and your reply message
  (`HW CPY?` unless you change it) in the compose box, `W1AW HW CPY?`. **SNR** puts in that call
  and the report you hear from it, `W1AW SNR -07`, once it has been heard. **INFO** and
  **STATUS** put in yours, for the **To** box call or for everyone. Each goes in where the cursor
  is, and then the whole box is sent, with anything you had typed, as JS8Call's buttons send it:
  with "GM" typed, **REPLY** sends `W1AW GM W1AW HW CPY?`, as JS8Call does. **CQ** sends your CQ
  message (`CQ CQ CQ` and your square unless you change it), and clears the **To** box as JS8Call
  does.
- **Saved messages.** The **Saved** menu lists the messages you keep (`TNX 73 GL` to start
  with). Pick one to put it in the compose box and send the box, to the **To** box call or to
  everyone. **Save current message**
  adds what is in the compose box, **Edit saved messages…** opens Settings, and **Restore
  previous message** puts the last message you sent back in the box.
- **Macros.** A saved message, the CQ and reply messages, INFO and STATUS may carry JS8Call's
  macros, replaced when the message is used: `<MYCALL>`, `<MYGRID4>`, `<MYGRID12>`, `<MYINFO>`,
  `<MYCQ>`, `<MYHB>`, `<MYREPLY>`, `<MYSTATUS>`, `<MYVERSION>`, `<MYIDLE>`, and for the call in
  the **To** box once it has been heard, `<CALL>`, `<SNR>` and `<TDELTA>`. A macro typed into
  the compose box is replaced when you press **Send**.
- **Fill only.** Turn off **Send CQ, REPLY, SNR, INFO, STATUS, saved messages and a station's
  asks at once** (JS8Call's "Immediately transmit…") and those buttons, and the quick asks on a
  station row, put their message in the compose box instead; **Send** sends it. The repeating
  CQ always sends.

## Frequencies, the decode filter and the band schedule

**Frequencies.** The band picker holds JS8Call 3.0.3's suggested frequencies: 1.8435,
3.578, 5.363, 7.078, 10.130, 14.078, 18.104, 21.078, 24.922, 28.078, 50.318 and
144.178 MHz, all USB. JS8Call 3 moved 160 m up to 1.8435 and added 60 m at 5.363; 60 m is
channelised and differs by country, so check your own band plan there. Your own list is
in [Settings ▸ Digital ▸ JS8](settings-reference.md#js8) under **Frequencies**: change, add
or remove a row and the list becomes yours, each row with a description if you like.
**Reset** puts JS8Call's list back, **Export…** saves yours to Downloads, and **Import…**
reads such a file back. Opening JS8 tunes the first frequency of the list on the current
band. **Return to the last used frequency** (off by default, as in JS8Call) tunes back to
the frequency you last used in JS8 instead.

**The decode filter.** The **Filter** plate on the waterfall says what the decoder listens to,
**Filter 500–2500 Hz** out of the box as JS8Call ships it, and opens the waterfall
controls. Switch the filter off to decode the whole passband, or set its centre and width;
**Centre on my RX offset** puts it on the station you are listening to. While it is on, a row's
**⋯** in Band activity or the call list has **Centre filter at … Hz**, which centres it on that
row, as JS8Call's row menus do.

**QSY to centre**, in the same controls, moves the rig's dial so your TX offset sits on the
passband centre (1500 Hz unless you change it): for narrow filters, and rigs whose passband
is uneven. Your RX offset moves with it, so you stay on the same station, and the stations
heard move to their new offsets. It never moves the rig while you are transmitting or while
a message is going out.

**The band schedule** (Settings ▸ Digital ▸ JS8 ▸ **Change frequency by schedule**, off by
default, as in JS8Call) changes the rig's frequency **by itself**, with nobody at the radio.
Give each row a frequency and a UTC window. When a window opens, the JS8 screen shows a
notice, and 30 seconds later the frequency changes unless you press **Cancel**. It never
changes while you are transmitting or while a message is going out: it waits until both are
clear. If you move off the scheduled frequency yourself, that row leaves you there; the
next row with another frequency moves you again. The frequency does not change back when a
row ends; add a row for that.

**DIRECTED.TXT.** With **Write ALL.TXT decode log** on
([Settings ▸ Logging & Connectors](settings-reference.md#integrations--feeds)), JS8 also writes
JS8Call's DIRECTED.TXT beside ALL.TXT: one line per directed message heard, with the date and
time (UTC), the dial, the offset, the SNR and the message, ♢ where it ended.

**ALL.TXT.** Each JS8 line in ALL.TXT names the frame's speed with JS8Call's letter, after the
offset: A Normal, B Fast, C JS8 40, E Slow, I JS8 60. JS8Call writes it on the lines it hears;
Nexus writes it on the lines it sends as well.

## Transmitting

JS8 keys **one frame per period, every period**, with no even/odd parity — a 60-character
message at Normal is five frames, 75 seconds. **Send** queues the frames; the next period
boundary sends the first. Plain text (no addressee) goes out prefixed with your callsign,
so every frame identifies you.

Everything automatic is gated twice. The first act is the **TX On** latch in the header —
never remembered across launches. The second is the switch for that kind of transmission:
**AUTOREPLY** (answers SNR?, GRID?, INFO?, STATUS?, HEARING?, QUERY MSGS addressed to you,
QUERY CALL — "YES -12 (5M)", the SNR and how long ago you last heard the station asked about,
on @ALLCALL too but once per station in 15 minutes — and AGN? with your last transmission again,
and acknowledges with an ACK a MSG to you or to a group you joined, or a MSG TO: you hold for
another station, as JS8Call does; with it off, the answer, the ACK or a relay is put in the
message box for you to send instead, and nothing keys by itself; QUERY CALL and QUERY MSGS are
not answered at all with it off, as in JS8Call),
**RELAY** (passes on messages routed through your callsign and holds MSG TO: traffic),
**HB ACK** (answers heartbeats with your report, but only while **HB NET** and **AUTOREPLY** are
on as well, as in JS8Call) — those three persist in Settings, at
JS8Call's defaults (on, on, off) — and **HB**, the heartbeat schedule, which is
session-only. A chip lights only when what it switches can go out now. One that is on but
cannot says why when you point at it (TX is off, the idle watchdog holds it, or it needs
another switch), and nothing keys.
Replies about held messages read as JS8Call 3.0's: `YES MSG ID 4 +2` says two more wait after
message 4, a delivery ends `NEXT MSG ID 5` when another waits, and a heartbeat acknowledgement
carries `MSG ID 4 +2)`, as JS8Call 3.0.3 sends it.

**Heartbeats, as JS8Call 3.0 has them.** **HB NET** is JS8Call's *Enable Heartbeat
Networking*: off by default and remembered, it sends nothing by itself, and without it there
is no **HB** button and no heartbeat acknowledgement. **HB** starts the repeat (Settings sets
the interval; the first heartbeat goes one interval after you switch it on), or at interval 0
sends one heartbeat. Its menu has **Send heartbeat now** and, with an interval set, **Start**
or **Stop heartbeat timer**. Heartbeats and acknowledgements go out at Slow, Normal and Fast
only: at JS8 40 and JS8 60 **HB** disappears, **HB NET** is greyed out, and a running repeat stops. As in
JS8Call, a heartbeat counts as @ALLCALL traffic: a station's heartbeat is acknowledged at most once
in 15 minutes, and a station just acknowledged, or just answered on @ALLCALL, gets neither again
for 15 minutes; the log says when a heartbeat goes unacknowledged for this. With the
**QSO pause** (on by default), a call in the **To** box pauses the heartbeat repeat until you
clear the box, holds back every acknowledgement, and stops a repeating CQ; switching **HB** on
clears the **To** box. When **HB ACK** is on but held back for one of these reasons (the speed,
the QSO pause, or a message still arriving) it shows as not armed and says why. Settings also
has JS8Call's *Allow heartbeats outside 500–1000 Hz* (your own TX offset instead of a free
spot), *Don't acknowledge stations that heartbeat more often than every 55 minutes* (such a
station is added to *Never acknowledge heartbeats from these callsigns*, as JS8Call adds it to
its list, and gets no acknowledgement again; **HB ACK** names the ones the rule added; take a
station off that list to acknowledge it again, and turning the option off leaves the list as it
is; once the list holds 500 calls the rule adds no more, but a station it stops is still not
acknowledged until you turn the option off) and *Do not take part in @ALLCALL* (nothing to @ALLCALL is answered or marked as yours), all
off by default. An update from a version without **HB NET** turns it on if you had **HB ACK**
on or a heartbeat repeat interval set, so the **HB** button stays; otherwise it starts off.
Every automatic reply first asks in the dock, as JS8Call asks: "A transmission is queued for
autoreply … would you like to send this transmission?" **Yes** sends it in the next period; **No**
sends nothing, and so does no answer, which is No after 89 seconds. A message you hold for
someone is handed over only on Yes. Turn **Ask for confirmation before sending automatic replies**
off in Settings and a reply goes by itself in the period after the query, as JS8Call answers:
it shows in the queue meanwhile, and **Drop queue** takes it back. As in JS8Call, no reply is made while your message box holds text (you are typing, a reply
waits there, or a message of yours is going out) or while a message to you is still arriving.
After an hour with nothing typed (the idle watchdog; adjustable), heartbeats,
auto-reply and relay switch themselves off, nothing more is filed or held, and the dock says so.

**Stop TX** (header, or **Esc** anywhere in the screen) cuts the frame on the air, empties
the queue, drops a reply still asking and stops the heartbeat schedule. **Drop queue** only
empties the queue — a frame already on the air finishes. Turning **TX Off** lets the
frame in flight complete, as in the FT8 screen; what is still queued, the rest of that message
included, is dropped, not sent when TX comes back on. So is what waits while the dial is outside
your license privileges, and the screen says so.

**Your license privileges count the whole signal.** With a US license class set, 🔒 TX LOCKED
judges each frame from your TX offset up the width its speed takes: 25 Hz at Slow, 50 at Normal,
80 at Fast, 160 at JS8 40 and 250 at JS8 60. Near the edge of your privileges the top of the frame
counts too. A General on 5.363 MHz, where the 60 m segment ends at 5366.5 kHz, keys a Normal frame
at a TX offset up to 3449 Hz and sees TX LOCKED from 3450 Hz, where the frame would reach the edge.
A heartbeat or acknowledgement on its own spot in 500–1000 Hz is judged on that spot: on a dial just
below the bottom of a segment, with your TX offset inside it, a heartbeat whose spot falls below the
segment is not sent, and the screen says why.

**Typeahead.** After **Send** the message stays in the message box while it goes out, as
JS8Call's does: the box shows it as it goes on the air (with your callsign and the addressee in
front), and as each frame is sent its text moves out of the box to the line beside it. Keep typing
and the frames still to go carry what you typed; the change is taken just before the next frame.
Text typed after the last frame has gone goes straight after it, as a message of its own. Delete
everything still to go and the message ends after the frame on the air. **Enter** and **Send** do
not queue a new message while yours is going out. A MSG or relay message cannot be changed while it
goes out (the box is read-only), as in JS8Call. While it goes out, **REPLY**, **SNR**, **INFO**,
**STATUS** and **Saved** are greyed out and a button that fills the box adds nothing, as JS8Call
adds no text then. A macro typed into the box meanwhile is replaced as **Send** replaces it, except
`<CALL>`, `<SNR>` and `<TDELTA>`, which stay as typed. The box empties when the message has gone,
or when you press **Stop TX**.

**When a frame keys.** As in JS8Call, each frame keys the radio a moment before its period
begins, the **TX delay** (0.2 seconds unless you change it in Settings ▸ Digital ▸ JS8, from 0.1 to
0.5), and sends silence up to the period's start delay, so its first tone goes out on time however
long the radio takes to switch to transmit, and a station hearing you reads your timing (DT) near
0.0. A radio slow to key plays less of that silence; one that keys after the start delay leaves out
what it missed and sends the rest of the frame on time, however late it keys, as JS8Call does
(the more it missed, the less likely the station hearing you decodes that frame). Only if the radio
keys after the frame's last tone would have ended (at Normal, about 13.1 seconds into the period)
does Nexus send nothing of that frame: it unkeys at once, as JS8Call stops there, and the screen
says why. TX stays on and the rest of the message goes out in the next
periods, but the station you are sending to misses that frame. A frame Nexus has not keyed by the
time JS8Call stops starting frames (at Normal, about 2.2 seconds into the period) waits for the
next period instead. **Stop TX** or **TX Off** during the delay unkeys at once, and nothing of that
frame goes out.

**Between frames.** The radio drops PTT after each frame and keys again for the next, as JS8Call
does by default: at Normal it is unkeyed for about 1.4 seconds between two frames (Slow about 3.8,
Fast and JS8 40 about 1.5, JS8 60 about 0.9). Turn on **Hold PTT between frames** in Settings ▸
Digital ▸ JS8 and it stays keyed from the end of one frame of a message to the start of the next
instead, and each frame after the first starts on its period, the radio being keyed already. It
still drops after the message's last frame, never holds from one message into the next, and drops
at once on **Stop TX** or **TX Off**.

**Moving the TX offset while sending.** While a message is waiting to go or going out, the TX
offset stays where it is, as in JS8Call: a right-click on the waterfall is refused and the screen
says why, and a list row's **Jump to … Hz** is greyed out. Turn on **Allow TX offset changes while
sending** and the move (or the jump) is taken at once, as in
JS8Call: the frame on the air moves to the new offset a tenth of a second later, from where it has
got to (receivers lose that frame, as they lose JS8Call's), and the frames still to go are sent
there. At Slow, a move in the first few seconds of a frame lands near its end instead: the sound
card already holds more than 20 seconds of it. A heartbeat or acknowledgement on its own spot in
500–1000 Hz stays on it, and so does a frame when the move would leave your licence's privileges;
on a Flex radio's own audio (DAX) the frame on the air finishes where it started. Moving RX is
never held.

## Spotting and APRS-IS

With the **PSK Reporter** switch on
([Settings ▸ Logging & Connectors ▸ Integrations & Feeds](settings-reference.md#integrations--feeds))
and a real callsign set, JS8 spots the way JS8Call's SPOT button does:

- Every station you hear — a heartbeat, a CQ, a directed message, the first frame of a plain
  message — goes to PSK Reporter as a JS8 spot, with the grid it sent or the one it sent before.
  The same station heard again on the same band within the hour updates its spot rather than
  adding one.
- The same stations, and every command sent to a @group (heartbeats, CQs, @JS8NET and @APRSIS
  messages), go to JS8NET, JS8Call's spotting network, once a minute. Your callsign, grid and INFO
  go with them, the INFO with its macros replaced as an INFO? reply has them.
- **Spot @APRSIS messages to APRS-IS** (off by default; JS8Call ships it on): a station heard
  sending `@APRSIS GRID` is put on the APRS map at that locator, and the text after `@APRSIS CMD`
  goes to APRS-IS as a raw packet. They go up under your callsign: Nexus logs in to APRS-IS with
  the passcode worked out from it, as the iGate does, only while JS8 is open and something is
  waiting to go, and stores nothing.
- **Never spot these callsigns** keeps a station off all three networks.

A contest that allows spots only over RF (Winter Field Day) stops all of it while it runs, as it
stops the PSK Reporter uploads.

**Relay APRS-IS messages to heard stations** (JS8Call 3.0; off by default) **transmits with
nobody at the radio.** While it is on and JS8 is open, Nexus listens to APRS-IS for messages; one
addressed to a station you have heard on JS8 is acknowledged on APRS-IS in that station's name and
keyed to it as `@APRSIS MSG TO:<call> <text> DE <sender>`. It goes out only while TX is on, one
message at a time, ahead of any automatic reply waiting, and through every transmit guard: the TX
latch, your license privileges, the idle watchdog and the TX watchdog. A message that could not
be sent now is not acknowledged either, and none is relayed to a station on the never-spot
list. A repeat of the same message within two minutes is acknowledged again but not sent again.
When another station relays an APRS message to you this way, it lands in your Inbox from
**APRS**.

## Programs written for JS8Call

Loggers, maps and message tools made for JS8Call's API can read from Nexus as they read from
JS8Call. Turn on **TCP API** or **UDP API** in
[Settings ▸ Digital ▸ JS8](settings-reference.md#js8). Both are off until you do, and they use
JS8Call's ports: 2442 for TCP, 2242 for UDP.

- **What a program gets:** what Nexus hears, as it hears it (every frame, and every directed
  message, heartbeats and CQs included), each station spotted while PSK Reporter spotting is on,
  when the transmitter keys (a tune carrier too, as in JS8Call) and the tones of each frame it
  sends, a band change, and each JS8 contact you log. The messages
  are JS8Call's own, so a program reads them as it reads JS8Call's.
- **What a program can ask for**, once you turn on **Accept TCP requests** or **Accept UDP
  requests** for its side (both off until you do, as in JS8Call): the callsign, grid, INFO and
  STATUS, the stations heard, the band activity, the free offsets, the decode filter, the
  switches (heartbeat networking among them), the call in your **To** box, the inbox, the dial and
  offset, the speed, whether the transmitter is keyed, the message going out, how many messages
  wait behind it, and the Nexus version. While that side's Accept is off, a program's
  requests get no answer, as in JS8Call.
- **Read-only.** A program cannot send a message, tune, change the frequency, the speed or a
  setting, or store a message through Nexus. Those requests get no answer, as JS8Call answers
  none it does not accept. A TCP line that is not a JSON request is answered `API.ERROR` with the
  reason, as JS8Call answers it.
- **This computer only.** Both listen on 127.0.0.1 whatever the port, so a program on another
  computer cannot reach them. These two settings are changed only here, at the station: the
  Remote page cannot see or change them.
- **One TCP program at a time**, as in JS8Call. A second one is told "Connections Full".

Not yet: sending through the API, or changing the decode filter through it (a program asking to
gets no answer). The call a program reads as selected is the one in your **To** box, and an inbox
message filed before Nexus started reads dial 0. The message going out is the one Nexus shows in its queue; what you are
still typing stays in the cockpit and is not sent to a program.

## Compatibility

On the air: JS8Call 2.2 and later, and JS8Call-improved, at Slow, Normal, Fast and JS8 40
(Turbo); JS8Call 3.0 and later at JS8 60 as well, a speed JS8Call 2.x does not decode.
For other programs: JS8Call 3.0's API, read-only, as above.
On Nexus Remote: the JS8 screen shows what the station's shows, HB NET, the call list's marks,
the Inbox's notes, the filter plate and the RX pane's lines among them, and it follows the station's
groups, RX block list and @ALLCALL choice; the decode filter, QSY to centre, the band schedule, the
groups and the block list are changed at the station only. A station on 1.18.0 or earlier shows its
JS8 screen there without them, as before.
The full protocol reference is
[JS8Call's source](https://github.com/js8call/js8call), read as the specification; see
NOTICE.
