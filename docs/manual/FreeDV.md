# FreeDV beside Nexus

[FreeDV](https://freedv.org) is free digital voice for HF. Nexus does not encode or decode FreeDV
itself: you run FreeDV's own app (freedv-gui) beside Nexus. Nexus owns the radio and shares it with
FreeDV; FreeDV does the digital voice. When you log a contact in FreeDV, it lands in the Nexus log
by itself, and the Phone screen can list the calls FreeDV hears.

This page says what to set in FreeDV, and what Nexus does with what FreeDV sends it.

## 1. Rig control: FreeDV talks to the radio through Nexus

Only one program can hold the radio's CAT port, so FreeDV does not open it. It talks to Nexus's
CAT broker instead, which is **on by default**.

In FreeDV's rig control settings:

- **Rig control: Hamlib**, model **Hamlib NET rigctl**.
- **Address:** the one shown in Nexus under **Settings ▸ Radio ▸ Transmit limits & sharing**
  (`127.0.0.1:4532` unless you changed the sharing port). Leave FreeDV's serial port blank.
- **PTT: via CAT (Hamlib).** Then FreeDV's key goes through Nexus, and every Nexus transmit guard
  still applies: Enable TX, your licence class and Stop. Nexus's **Other programs may key
  transmit** switch (same section, on by default) must stay on.

**Do not use VOX or a serial PTT line (RTS/DTR) for FreeDV.** Both key the radio behind Nexus's
back, so none of Nexus's guards can stop them, and a serial PTT line fights Nexus for the COM port.

**Keep FreeDV's own Time-Out Timer on** (FreeDV 2.4 turns it on at 180 seconds). Nexus does not
yet put a time limit on a key held by a sharing program.

## 2. Which section, and which sideband

Run FreeDV with Nexus in the **Phone** section, not Digital:

- FreeDV is digital *voice*, so it is phone under the rules. In Digital, Nexus's licence gate
  judges a data emission, and with a US licence class set it refuses 14.236 MHz, which is
  outside the 20 m data segment.
- In Phone, Nexus keeps the radio in plain USB or LSB, and it refuses a sharing program's request
  to switch the radio to its data mode (DIGU/DIGL, PKTUSB/PKTLSB). FreeDV asks for the data mode
  by default, so **tick FreeDV's "Use USB/LSB instead of DIGU/DIGL"**.
- With the radio in plain USB/LSB, set the radio's **SSB transmit audio source to USB (or rear)**
  while you run FreeDV, and back to the microphone for SSB. The menu for this is the radio's own.

FreeDV uses the SSB sideband convention: LSB below 10 MHz, USB above, and Nexus's Phone section
commands the same. **60 m is the exception:** FreeDV, and the rules in the US, use USB there, while
Nexus's Phone section commands LSB below 10 MHz on 60 m too. On 60 m, set USB on the radio by hand.

## 3. Hearing only FreeDV

Turn off Nexus's own receive audio while you run FreeDV (**Settings ▸ Radio ▸ Receive audio on
this computer ▸ Play receive audio here**), so you hear FreeDV's decoded speech and not the modem
signal underneath it.

## 4. Sound devices, by computer

FreeDV needs two sound devices: the radio's (input and output) and yours (microphone and
speakers or a headset).

- **Windows and macOS** let Nexus and FreeDV use the radio's sound card at the same time.
- **Linux and the Raspberry Pi:** FreeDV needs PipeWire or PulseAudio. When Nexus's audio device
  is a card picked by name (an entry beginning `plughw:`), Nexus opens the card directly and FreeDV
  may not be able to open it too. The `default`, `pulse` and `pipewire` entries go through the
  desktop's sound server and can be shared. Nexus and FreeDV on one sound card on Linux have not
  yet been tested together.
- **Raspberry Pi:** FreeDV's own aarch64 build is for a Raspberry Pi 5 or later.
- **FlexRadio:** use FreeDV's own Flex waveform (the FDVU/FDVL modes). Nexus leaves those modes
  alone.

## 5. FreeDV's contacts land in the Nexus log

FreeDV 2.2 and later can hand each contact you log to a logging program ("QSO Logging", in
WSJT-X's network format). Nexus listens for them on **`127.0.0.1:2239`**, on by default.

In FreeDV's QSO Logging settings, turn logging on and set the address to **`127.0.0.1`** and the
port to **`2239`**. FreeDV's default port is 2237, but on most stations 2237 already belongs to
GridTracker or JTAlert, and it is where Nexus sends its own WSJT-X messages.

What happens when you press **Log QSO** in FreeDV:

- The contact goes into the Nexus log as mode **FreeDV**, with the call, grid, frequency and band,
  the reports, the name and the comment FreeDV sent. It is written to ADIF, LoTW and the other
  services as `MODE=DIGITALVOICE SUBMODE=FREEDV`, the form LoTW counts as phone.
- The same contact sent twice is one contact.
- **Only under your own call.** FreeDV sends the station call it is set to. If that is not your
  station call in Nexus (Settings ▸ Station), the contact is **refused**, never written under the
  wrong call. The reason shows under the switch in Settings, in the Phone screen's FreeDV pane and
  in the connection log. Fix the call in FreeDV and log the contact again.
- **The start time is the end time.** FreeDV sends one time, the moment you pressed Log QSO, and
  Nexus records exactly that for both. Nexus does not invent a start time.
- Contacts from FreeDV count for the Needed board's **New mode** as FreeDV, a mode of its own: a
  FreeDV station is a new mode for a country until you log a FreeDV contact with it.

The switch, the port and the listener's state are under **Settings ▸ Radio ▸ Transmit limits &
sharing ▸ Accept contacts other programs log**. The port is bound to this computer only, unless
you turn on **Accept from devices on my network** there. If another program already holds the
port, the address there is replaced by the reason; pick another port in Nexus and the same one in
FreeDV.

FreeDV's contacts go to QRZ, Club Log, eQSL and your other services like the contacts you log in
Nexus (held while contest mode is on), and one you also logged in Nexus within five minutes is
not written twice.

### Contacts from other programs

The same port takes the contacts other programs log, so Nexus can stay your log while another
program runs the radio:

- **WSJT-X, JTDX, MSHV:** add `127.0.0.1:2239` as a second UDP server (or forward to it from
  JTAlert or GridTracker). Nexus reads their QSO Logged and Logged ADIF messages; a program that
  sends both for one contact is logged once.
- **N1MM:** add `127.0.0.1:2239` to its contact broadcast. Edits and deletes in N1MM are not read
  yet: a contact changed there stays as it was first logged in Nexus.
- **Anything that sends ADIF over UDP**, Ham Radio Deluxe or Log4OM style.
- **The S.A.T. satellite tracker** (it sits on your network, not on this computer): on the S.A.T.,
  set **QSO LOG TYPE** to **Log4OM** or **HRD**, **QSO LOG IP** to this computer's address, and the
  port to **2239**; in Nexus, turn on **Accept from devices on my network**. *(The fields the
  S.A.T. sends have not yet been checked against a real capture.)*

The same rules apply to every program: only under your own station call, a toast and a line in
the connection log for each contact, **Logged via** the program on the contact's card, and nothing
sent back to WSJT-X UDP, HRD, N1MM or DXKeeper.

Nothing FreeDV sends to this port can key, tune or change the radio.

You can also log a FreeDV contact by hand: in the Phone screen's log strip, open the
other-radio entry and pick **FREEDV** as the mode.

## 6. The calls FreeDV hears: the Phone FreeDV pane

FreeDV 2.4 and later broadcast each callsign they decode. Nexus can list them in a **FreeDV pane**
on the Phone screen: add it from **⊞ Panels**. The pane shows whether FreeDV is talking to Nexus
(its logging messages arrive), the last contact it put in the log, any refusal, and the calls it
heard. A click on a heard call puts it in the log strip's call field. Nothing in the pane logs,
keys or tunes by itself.

Listening for heard calls is **off by default**. Turn it on with the pane's **Listen for them**,
or under **Settings ▸ Radio ▸ Transmit limits & sharing ▸ Calls FreeDV hears**. Nexus then listens
on FreeDV's default broadcast (`224.0.0.1`, port `7177`) and takes calls only from the FreeDV on
this computer. Windows may ask once whether Nexus may use the network. If you set FreeDV's
broadcast address to `127.0.0.1` with port `7177`, Nexus hears it there too.

The list is this session's only; Nexus does not keep it.

## 7. Where FreeDV is: the channels

These are the frequencies FreeDV's own app lists as where FreeDV stations are, each on its
sideband:

| Band | MHz | Side | Note |
|---|---|---|---|
| 160 m | 1.870 | LSB | |
| 80 m | 3.625, 3.643, 3.693, 3.697 | LSB | US: Extra class only |
| 80 m | 3.803 | LSB | |
| 60 m | 5.4035 | USB | the US channel centred on 5405.0 kHz |
| 60 m | 5.3685 | USB | not a US channel; 60 m differs by country |
| 40 m | 7.177 | LSB | US: not General (its passband reaches below 7.175) |
| 40 m | 7.197 | LSB | |
| 20 m | **14.236**, 14.240 | USB | 14.236 is FreeDV's 20 m centre |
| 17 m | 18.118 | USB | |
| 15 m | 21.313 | USB | |
| 12 m | 24.933 | USB | |
| 10 m | 28.330, 28.720 | USB | |

14.236 MHz is also the second SSTV overflow channel in Nexus's SSTV list, whose note now says it is
FreeDV's 20 m centre. Check before sending an image there.

## Related

- [Integrations](Integrations.md): the CAT broker and the other programs that share the radio.
- [Phone](Phone.md): the Phone cockpit.
