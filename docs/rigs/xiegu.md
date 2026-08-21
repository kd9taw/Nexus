# Xiegu Setup

> **Field status:** Xiegu radios are supported in the model picker and use
> Icom's CI-V backend, but they are **not yet field-verified** on the author's
> bench. The setup below follows from the code and each radio's known USB
> behavior — if you run a Xiegu, your field report is genuinely valuable.

Xiegu rigs connect over **Serial (USB / COM)** and speak Icom-style CI-V, so
Hamlib drives them through its Icom-family backend. They are Icom-*style*, not
Icoms, and Nexus stopped treating them as Icoms where it matters: a failed
**Test CAT** no longer quotes an Icom rig menu at you, and no longer offers
Icom's USB driver for a Silicon Labs or WCH bridge chip. It names the cure it
can be sure of — the **Baud** field here in Settings — and leaves the rest to
your radio's manual.

---

## No Xiegu names its model over USB — you always pick it

Every Xiegu sits behind a generic USB-serial bridge chip, and a bridge chip
reports **its own name, not the radio's**. Detect fills in the **serial port**
(and, on Windows, the driver link the chip needs) and leaves **Rig Model
empty** — for all five radios, not just the older ones. That is the honest,
documented result, not an error; set **Rig Model** by hand from the table below.

What splits the family in two is the **cabling**, not self-identification.

### X6100 / X6200 — one cable, two serial ports

The **X6100** and **X6200** carry a **built-in sound card**, so a single USB-C
cable carries both CAT and audio. That cable enumerates as a **WCH CH342**,
which presents **two serial ports**: `USB-Enhanced-SERIAL-A` and
`USB-Enhanced-SERIAL-B`. **CAT answers on the -B one only** — the A port opens
cleanly and then returns nothing forever, which looks exactly like a dead radio.
Detect badges the right one *"CI-V port — use this one"*, and **Auto-test** tries
it first.

1. Plug in over USB-C and power on.
2. **Settings ▸ Radio ▸ Rig & CAT ▸ Detect my radio** — it fills the port and the
   radio's built-in audio device. Take the row badged *CI-V port*; the other row
   is the same radio's dead port.
3. Set **Rig Model** to your radio from the table below.
4. Set **Baud** to **19200** (see *Baud* below).
5. **Save**, then **Test CAT**.

<!-- TODO: capture screenshot — Xiegu X6100 detected over USB-C with built-in audio -->

### G90 / X5105 / X108G — one serial port, audio is separate

The **G90** and **X5105** sit behind a bare **Silicon Labs CP210x** USB-serial
bridge: one port, no audio on it.

1. Connect the rig's CAT cable; on Windows install the driver Detect names if it
   flags one missing (for these two that is the **Silicon Labs CP210x**).
2. Run **Detect my radio** to fill the serial port (or pick it manually / use
   **Auto-test**).
3. Set **Rig Model** to your radio from the table below.
4. **Audio is separate** — the G90 and X5105 have no USB audio codec, so you
   need an external interface (a Xiegu **CE-19** data adapter or a
   **Digirig**-class interface). Select that interface as your input/output
   device in **Settings ▸ Radio ▸ Audio**.
5. Set **Baud** to **19200**, **Save**, then **Test CAT**.

The **X108G** is in the model list too, behind **Show all models**. It follows
the same steps: pick the port, pick the model, wire audio through whatever
interface your cable gives you.

<!-- TODO: capture screenshot — Xiegu G90 with CP210x port detected and model picked manually -->

---

## Baud

The Xiegu CI-V family runs at **19200**. Nexus's default for a new radio is
38400, and at 38400 the rig never answers — `rigctld` connects and the radio
stays silent. Picking the **G90** sets 19200 for you; on the other models
**check the Baud field after you pick the model** and set it to 19200 if it
still reads 38400.

If CAT is already silent, **Test CAT** finds the rate for you: it walks the
common CI-V rates on the port you picked and tells you which one the radio
answered on.

---

## What Nexus does automatically per section

Xiegu uses the Icom CI-V command set, so mode handling matches Icom:

- **Digital (FT8/FT4/TempoFast/TempoDeep)** → data submode (Hamlib `PKTUSB`) where the rig
  supports it, opened to a wide passband.
- **Phone (SSB)** → **USB** above 10 MHz, **LSB** below.
- **CW** → **CW** on the CAT keyer; **USB/LSB** on the soundcard keyer.

If a particular Xiegu firmware rejects the data submode, Nexus falls back rather
than looping — plain USB still passes FT8 audio, though the rig's own DSP (NR/NB)
may interfere with decodes if left on.

---

## Xiegu models in the picker

| Model | Hamlib # | Cabling | Detect result |
|---|---|---|---|
| X6100 | 3087 | One USB-C (built-in audio), **two** serial ports — CAT on SERIAL-B | Port + audio — pick model |
| X6200 | 3091 | One USB-C (built-in audio), **two** serial ports — CAT on SERIAL-B | Port + audio — pick model |
| G90 | 3088 | CP210x CAT + external audio | Port only — pick model |
| X5105 | 3089 | CP210x CAT + external audio | Port only — pick model |
| X108G | 3076 | Serial CAT + external audio (**Show all models**) | Port only — pick model |

---

## PTT

Choose **CAT (via rigctld)** where the radio supports CI-V PTT, or drive PTT
from your data interface with **VOX** (or **Serial RTS/DTR** on a cable that
keys off a control line). CAT frequency/mode control works independently of the
PTT method.

---

*Port detected but CAT won't answer, or audio device confusion with a CE-19 /
Digirig? See [Troubleshooting](../troubleshooting.md).*
