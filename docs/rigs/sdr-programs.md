# Hermes Lite 2 and SDR Program Setup

> **Field status: not verified on hardware.** Nobody has run the setup on this
> page against a real Hermes Lite 2 here. The Nexus settings it names are read
> off the shipped build. The Thetis and piHPSDR menus are taken from those
> programs' own documentation and have not been checked against a running
> copy, so where your version differs, go by what your program shows.

A Hermes Lite 2 has no CAT port of its own. The program that runs it on your
computer answers CAT for it: Thetis on Windows, or piHPSDR on Linux, the
Raspberry Pi and macOS. Nexus connects to that program, not to the radio. The
same goes for an ANAN, and for other SDRs that a program on the PC runs.

In Nexus that means **Rig Model** names the program you launched, not the
board on your desk, and **Connection** is **Network**, pointed at the
program's CAT server. The receive audio is made inside the SDR program, so it
reaches Nexus through a **virtual audio cable**.

---

## The rig model entries

Each program has its own entry in **Rig Model**:

| Program | Rig Model entry | Hamlib # | CAT server port, as the program comes |
|---|---|---|---|
| Thetis | *Thetis (Hermes Lite 2 / ANAN / HPSDR)* | 2054 | 13013 |
| piHPSDR | *piHPSDR / OpenHPSDR (Hermes Lite 2 / ANAN)* | 2040 | 19090 |
| PowerSDR / mRX PS | *PowerSDR / mRX PS (Apache ANAN / legacy FLEX)* | 2048 | Read it from the program |
| SDR Console | *SDR Console (SDR-Radio.com)* | 2056 | Read it from the program |

The numbers are the models in the copy of Hamlib that ships inside Nexus. All
four are in the list you see without ticking **Show all models**, and the
search box above **Rig Model** finds them by name or by number.

Don't pick a FlexRadio entry for an SDR program. On Thetis it does connect,
but Hamlib then drives Thetis with the FLEX-6000 commands: the S-meter reads
nothing, and keying is sent without the read-back.

---

## Before you start

- **The SDR program running**, with the radio receiving. Nexus talks to the
  program, so the program has to be up first.
- **A virtual audio cable program.** Nexus does not include one. You need two
  cables: one carries receive audio from the program to Nexus, the other
  carries transmit audio from Nexus to the program.
- **Nexus on the same computer as the SDR program**, if the audio goes
  through virtual cables. A virtual audio cable only joins programs on one
  computer.

---

## Thetis on Windows

The Hermes Lite 2 project points owners at a build of Thetis modified for the
HL2. Its CAT and audio menus carry the same names as the ones below.

### Turn on Thetis's CAT server

1. In Thetis, open **Setup ▸ Serial/Network/Midi CAT ▸ Network**.
2. Find the **TCP/IP CAT Server** box and read the address and port in it. As
   Thetis comes, it is `127.0.0.1:13013`.
3. Tick **TCP/IP CAT Server Running**.

Use the TCP/IP CAT Server box, not the **TCI Server** box on the same tab
(`127.0.0.1:50001` as Thetis comes). TCI is a different protocol, and the
Hamlib inside Nexus has no driver for it.

### Send Thetis's audio through the cables

1. In Thetis, open **Setup ▸ Audio ▸ VAC 1**.
2. Tick **Enable VAC 1**.
3. Set **Output** to the receive cable. Thetis plays its receive audio into
   it.
4. Set **Input** to the transmit cable. Thetis takes its transmit audio from
   it.

### Connect Nexus to Thetis

1. In Nexus, open **Settings ▸ Radio ▸ Rig & CAT**.
2. Type `Thetis` in the search box above **Rig Model**, then pick
   **Thetis (Hermes Lite 2 / ANAN / HPSDR) (2054)**.
3. Set **Connection** to **Network**. A **Network Address** field appears.
4. In **Network Address**, type the address and port from Thetis's TCP/IP CAT
   Server box, such as `127.0.0.1:13013`.
5. Set **PTT Method** to **CAT (via rigctld)**, so Nexus keys the radio
   through Thetis.
6. Click **Test CAT**. It saves your settings, starts Nexus's own copy of
   rigctld and reads the dial frequency back. When the link works it answers
   **Connected** and the frequency Thetis is tuned to.

**Expected result:** the frequency in Nexus follows Thetis. For FT8 Nexus
asks for the radio's data mode, which Thetis shows as **DIGU**.

Leave **rigctld TCP Port**, under **Advanced**, as it is. It is the port
Nexus's own rigctld listens on, and it has to be a different number from
Thetis's.

### Pick the audio devices in Nexus

1. Open **Settings ▸ Radio ▸ Audio**.
2. Set **Input Device (RX)** to the receive cable.
3. Set **Output Device (TX)** to the transmit cable.
4. Watch **Live input spectrum**. With Thetis's audio arriving, band noise
   shows as a moving floor. A flat line means nothing is arriving on that
   input.

Then set the levels as you would for any radio. The fields are described in
[Settings reference ▸ Audio](../guide/settings-reference.md#audio).

### Using a virtual COM pair

Thetis can also serve CAT on a serial port, which on one computer means a
virtual COM port pair. Nexus works with that too: set **Connection** to
**Serial (USB / COM port)**, **Serial Port** to Nexus's end of the pair,
**Rig Model** to the same Thetis entry, and **Baud** to the speed Thetis's CAT
port is set to. The TCP/IP CAT server needs no extra driver, which makes it
the simpler of the two.

---

## piHPSDR on Linux, the Raspberry Pi and macOS

### Turn on piHPSDR's CAT server

1. In piHPSDR, click **Menu**, then **CatTci**. The CAT/TCI menu opens.
2. On the **TCP** row, read the port number. As piHPSDR comes, it is `19090`.
3. Tick **Enable** on that row.

The **TCI** row is a different protocol, and Nexus cannot use it.

### Connect Nexus to piHPSDR

1. In Nexus, open **Settings ▸ Radio ▸ Rig & CAT**.
2. In **Rig Model**, pick
   **piHPSDR / OpenHPSDR (Hermes Lite 2 / ANAN) (2040)**.
3. Set **Connection** to **Network**.
4. In **Network Address**, type `127.0.0.1:19090` if piHPSDR runs on the same
   computer as Nexus. If it runs on another computer, type that computer's
   address and the port, such as `192.168.1.30:19090`.
5. Set **PTT Method** to **CAT (via rigctld)**.
6. Click **Test CAT**. When the link works it answers **Connected** and the
   frequency piHPSDR is tuned to.

### Audio with piHPSDR

piHPSDR picks its sound devices in two places: **RX Audio Out** in its RX
menu, and **TX Audio In** in its TX menu. Its **Only Audio to Radio** choice
gives a Hermes Lite 2 on its own no audio at all, so pick sound devices on the
computer for both.

- **Nexus on the same computer:** set RX Audio Out to the receive cable and
  TX Audio In to the transmit cable. In Nexus, set **Input Device (RX)** to
  the receive cable and **Output Device (TX)** to the transmit cable.
- **Nexus on another computer:** wire a sound card on the piHPSDR computer to
  a sound card on the Nexus computer, in both directions, and pick those cards
  on each side.

---

## Other SDR programs

PowerSDR / mRX PS and SDR Console work the same way. Pick the entry named for
the program, then set Connection and the address or port from that program's
own CAT settings (Network for a TCP server, Serial for a virtual COM pair),
and send the audio through two cables. Their menus have not been checked here,
so this page does not name them.

For a program that is not in the list, tick **Show all models** for the full
Hamlib catalog or type its model number. Or run rigctld yourself and point
Nexus at it as **NET rigctl**, as described under
[My rig isn't listed](index.md#my-rig-isnt-listed).

---

## When it does not connect

- **Test CAT says the address "is Thetis's CAT server, not a rigctld".**
  Thetis's port is in **rigctld TCP Port**, where Nexus's own rigctld listens.
  Put Thetis's address and port in **Network Address** and give rigctld TCP
  Port a different, free number. The message names the entry to pick.
- **It says the port "answered, but not as a rigctld".** Something other than
  a rigctld, such as another program's CAT server, is listening on rigctld TCP
  Port. The fix is the same.
- **Nothing answers.** Check that the program's CAT server is switched on
  (**TCP/IP CAT Server Running** in Thetis, **Enable** on the **TCP** row in
  piHPSDR) and that the port in Network Address is the one the program shows.
  More in [Test CAT fails or times out](../troubleshooting.md#test-cat-fails-or-times-out).
- **CAT works but nothing decodes.** Input Device (RX) is not the receive
  cable, or VAC 1 is not enabled in Thetis. Live input spectrum tells you
  which side to look at: flat means no audio is reaching Nexus. See also
  [Nothing is decoding](../guide/scenarios.md#nothing-is-decoding).
- **The radio keys but sends no audio.** Output Device (TX) is not the
  transmit cable, or the program's transmit input is not set to it (VAC 1
  **Input** in Thetis, **TX Audio In** in piHPSDR).

---

## Honest limits

- **No panadapter from the SDR.** Nexus draws its scope and waterfall from the
  receive audio, so they show only the audio passband. The program's own
  panadapter stays in the program.
- **Detect my radio never offers these entries.** An SDR program is not a USB
  device, so Detect and the serial port Auto-test never suggest one. You pick
  the entry yourself.
- **TCI is not an option.** The Hamlib that ships inside Nexus has no TCI
  driver, so a TCI server port does not work as a Network Address.
- **Linux sound devices.** On Linux the audio pickers list the computer's ALSA
  devices. A loopback cable made with the ALSA loopback module shows up there
  as a sound card. A PulseAudio or PipeWire virtual sink has no entry of its
  own and is reachable only through the `pulse` or `pipewire` entry. Neither
  arrangement has been tried with piHPSDR here.

---

## Related guides

- [FlexRadio Setup](flexradio.md): the same idea for a Flex, with SmartSDR CAT
  serving CAT and DAX carrying the audio.
- [Rig Setup Guides](index.md): the three connection types, and Test CAT.
- [Settings reference ▸ Rig & CAT](../guide/settings-reference.md#rig--cat):
  every field in that section.
- [Troubleshooting](../troubleshooting.md): CAT and audio problems on any rig.
