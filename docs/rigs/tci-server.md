# Share by TCI: let other programs use the radio through Nexus

> **Field status: not verified with the programs named here.** Nexus's TCI
> server is tested against its own test clients only. The program menus named
> below are taken from each program's own source code or documentation, and
> nobody has run them against Nexus yet. Where your version differs, go by
> what your program shows.

TCI (Transceiver Control Interface) is Expert Electronics' protocol for SDR
programs. An SDR program runs a small TCI server, and other programs connect
to it to read and set the radio and to get its audio. **Share by TCI** makes
Nexus one of those servers. Programs on this computer that speak TCI, such as
WSJT-X, JTDX, MSHV and Log4OM, connect to Nexus and use the radio Nexus runs,
whatever that radio is: a serial rig, a FlexRadio, or an Icom on the network.

It sits beside **Share this radio with other programs**, which shares the
radio as Hamlib NET rigctl. Both can be on at once. TCI also carries the
receive audio of a radio on a sound card, so WSJT-X, JTDX and MSHV need no
virtual audio cable to hear it.

For the other direction, Nexus connecting to an SDR program's own TCI
server, see [TCI: connect to an SDR program's TCI server](tci.md).

---

## What works today

| Part | Today |
|---|---|
| The dial: programs read it and tune the radio | Works |
| The mode: programs read it and switch the sideband | Works |
| Power: programs read it and may lower it | Works. A program never raises it. |
| Receive audio to programs | Works. Programs hear what Nexus hears: a Flex's own DAX audio with **Flex native DAX audio** on, the SDR program's audio on a TCI connection with **Audio over TCI** on, and otherwise the sound card under **Audio**. |
| Tune from a program | Works with **Programs on TCI may transmit** on |
| A program keying the radio's own microphone, on the Phone screen | Works with **Programs on TCI may transmit** on |
| A program's own transmit audio, on the Phone screen | Works with **Programs on TCI may transmit** on, once the program asks for a voice mode (see below) |
| FT8, FT4 and the other slot-timed modes from a program | Refused for now (see below) |
| CW macros from a logger's TCI keyer | Works on the CW screen, with **Programs on TCI may transmit** on |
| Spots from programs, on Nexus's band map and band activity | Works with **Spots from programs** on |
| IQ for skimmers | Not offered |

---

## Turn it on

1. Open **Settings ▸ Radio ▸ Transmit limits & sharing**.
2. Turn on **Share by TCI**, below **Share this radio with other programs**.
3. **Save.**

The address programs connect to appears beside the switch, usually
`127.0.0.1:40001`, with a **Copy** button. A second Nexus window on the same
computer serves on the next port, `127.0.0.1:40002`, and its address says so.
If the port is taken, a sentence saying so takes the address's place. Give
**TCI server port** a number nothing else on this computer uses, and save.

### The settings

| Setting | What it does | As it comes |
|---|---|---|
| **Share by TCI** | Runs the TCI server. Turning it off closes every program's connection. | Off |
| **TCI server port** | The port it listens on. | 40001 |
| **Programs on TCI may transmit** | Lets programs key the transmitter at all. Turning it off ends whatever a program is transmitting. Shown and set only at the station: from a computer driving the station, it reads "Set at the station". | Off |
| **CW macros from programs** | Lets a logger's TCI keyer send CW through Nexus's own CW keyer, on the CW screen only, and only while programs may transmit. | On |
| **Spots from programs** | Shows programs' spots on Nexus's band map and in the Phone and CW screens' band activity, and passes your clicks on spots back to them. A click on a program's spot only tunes Nexus. A spot or a click never transmits. | On |

Every one of these belongs to the station and is set only there.

### Programs connected

While the server runs, each program's connection is listed with its number
(TCI carries no program name, so connections are numbered as they open), the
address it comes from, whether it is receiving audio or transmitting, and
the last thing Nexus refused it. **Disconnect** closes one program's
connection; anything it is transmitting stops first. Disconnect is offered at
the station only.

---

## Set up each program

Use the address Nexus shows. The examples here use `127.0.0.1:40001`.

### WSJT-X 3.0 and WSJT-X Improved

1. **Settings ▸ Radio**: set **Rig** to `TCI Client RX1`.
2. In **TCI Server**, type `127.0.0.1:40001`. Do not leave it blank: WSJT-X
   sets the rig back to None at its next start when this field is empty.
3. Set **PTT Method** to **CAT**.
4. **Settings ▸ Audio**: tick **Use TCI Audio (restart required)**, then
   restart WSJT-X.

WSJT-X then follows the radio and decodes Nexus's receive audio. Nexus
refuses its FT8 and FT4 transmissions for now, as
[Transmitting through Nexus](#transmitting-through-nexus) explains.

### JTDX

JTDX comes from the same code as WSJT-X: set its rig to `TCI Client RX1` and
give it `127.0.0.1:40001` as its TCI server, then turn on its TCI audio.

### MSHV

MSHV takes the host and the port in two separate fields: `127.0.0.1` and
`40001`. It sets up its TCI audio by itself. On connecting, MSHV may ask for
a drive of 25 when the power it reads is 0; Nexus refuses a raise, so the
power stays where you set it.

### Log4OM 2

Log4OM 2.16 and later talk TCI for CAT control, its CW keyer and spots. Point
its TCI connection at `127.0.0.1`, port `40001`.

### MacLoggerDX

Pick its SunSDR or ExpertSDR3 TCI driver and give it the full address
`ws://localhost:40001`.

### Amplifier and tuner controllers, Stream Deck tools

Programs that follow the band (RF2K-S and similar amplifier software), tuner
boxes that send Tune, and button panels that set the frequency connect to the
same address.

**Do not use a program's TCI transmit signal as an amplifier's T/R switch.**
Nexus tells programs it is transmitting when it keys, not ahead of it. Key the
amplifier from the radio's own PTT output or from RF sensing, as you do
today.

---

## Transmitting through Nexus

With **Programs on TCI may transmit** off, as it comes, programs tune and read
but never transmit. With it on, every safeguard in front of Nexus's own
transmissions still applies:

- TX must be on in Nexus. A program can never turn it on.
- **Stop TX** ends what a program is transmitting, and holds: no program can
  transmit again until you turn TX on yourself. A band or radio change holds
  programs the same way.
- Your licence privileges and your power limits apply to programs too. For a
  program's own audio Nexus cannot know where in the passband the program
  puts its signal, so it checks the whole passband against your privileges.
- One transmitter: while a program transmits, Nexus's own transmissions wait,
  and the other way round. Run one FT8 program at a time.
- A program that disconnects or stops answering stops transmitting at once.

A program's own audio, a logger's voice macro for one, goes out on the Phone
screen only, and only once the program has asked Nexus for a voice mode: USB,
LSB, AM or FM. A program that has asked for no mode, or for CW, is refused.
WSJT-X and JTDX ask for a mode only when it differs from the one Nexus is in,
so set to USB while Nexus is already in USB they ask for nothing and are
refused too. Nexus keys the radio when the program's audio starts, and when
the program lets go, the last of its audio plays out before the radio
unkeys. Stop TX, Esc and every other stop end it at once, and so do 0.3
seconds with no audio from the program and 10 minutes on the air. With
**Flex native DAX audio** on, a Flex takes its own microphone on the Phone
screen, so a program's audio is refused there.

A logger's CW macros (Log4OM's TCI keyer, 5MContest) go out on Nexus's own
CW keyer, whichever one you use, on the CW screen only, and only while **CW
macros from programs** is on as well. They go at the speed the program sets;
your own CW speed stays as it is. A macro holding a character the keyer
cannot send is refused whole. **Stop TX**, Esc and a CW stop from any program
end it.

**FT8, FT4 and the other slot-timed modes may not transmit through TCI yet.**
A program times its own transmissions, and Nexus adds a short delay of its
own on the way to the radio. Until that delay has been measured and the
transmissions shown to decode, Nexus refuses a program's digital
transmissions. A program whose last mode request was a data mode (DIGU or
DIGL) is held the same way, whatever screen Nexus is on. Receiving and
decoding work.

---

## Only this computer

Nexus's TCI server listens on `127.0.0.1` and `::1` only. Another computer
cannot reach it, and a web page in your browser cannot connect to it either,
because Nexus refuses any connection that comes from a web page. TCI has no
password and no encryption, which is why it stays on this computer.

---

## When something does not work

- **"Programs cannot connect by TCI: ports … are taken."** Another program
  holds the port, usually an SDR program's own TCI server or two other Nexus
  windows. Close the one you do not need, or give **TCI server port** a port
  nothing else uses.
- **A program connects but does not transmit.** Look under it in **Programs
  connected**: "Last refused" says why, for example "programs on TCI may not
  transmit" or "transmit is not enabled". "A program may transmit its own
  audio only after it asks for a voice mode" means the program has not asked
  for USB, LSB, AM or FM: see
  [Transmitting through Nexus](#transmitting-through-nexus).
- **WSJT-X says "TCI Audio could not be switched on".** Nexus has no receive
  audio to send at that moment, usually because the sound card under
  **Audio** did not open. Check Nexus's audio settings, or untick **Use TCI
  Audio** and give WSJT-X the radio's own audio device, as you would without
  Nexus.
- **Settings ▸ Logging & Connectors ▸ Connections** keeps a Connection log;
  the server's own lines there say which programs connected and what Nexus
  refused them.

---

## Related guides

- [TCI: connect to an SDR program's TCI server](tci.md): Nexus as the program
  connecting to an SDR's TCI server.
- [Hermes Lite 2 and SDR Program Setup](sdr-programs.md): Thetis and piHPSDR.
- [Settings reference ▸ Transmit limits & sharing](../guide/settings-reference.md#transmit-limits--sharing):
  every setting in that section.
