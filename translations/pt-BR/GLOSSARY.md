# Glossary — the words that come back over and over

There are 5,044 rows in the CSV and about sixty words that appear in hundreds of them. If one of
those words gets translated three different ways across the file, the program reads as though
three people wrote it. So this is the list to settle **once**, before you start, and then not
think about again.

The **rows** column is a real count: how many rows of `nexus-ptbr-translation.csv` contain that
word. Everything here was counted from the catalog itself, so nothing on this list is theoretical.

---

## A. Keep in English

These are the technical vocabulary of amateur radio, plus the proper names of file formats,
services and programs. A Brazilian operator says these on the air and reads them on the front
panel of the radio. Translating them makes the program harder to use, not easier.

The checker (`verify-ptbr.py`) enforces this list per row, using the `do_not_translate` column.

| Term | Rows | Why it stays |
|---|---:|---|
| Nexus | 104 | The program's name. |
| QSO / QSOs | 96 / 54 | The contact itself. Universal on the air; pluralises fine as *QSOs*. |
| LoTW | 96 | ARRL's Logbook of The World — a service name. |
| CAT | 82 | Computer Aided Transceiver — the radio control protocol. |
| TX / RX | 79 / 71 | Transmit and receive. Printed on the radio itself. |
| CW | 67 | The mode. Never "telegrafia" in a mode picker. |
| QRZ | 67 | Both the Q-code and the callsign lookup site. |
| CQ | 63 | The call. Translating it would be strange on the air and on screen. |
| MHz / kHz / Hz | 62 / 16 / 44 | SI units. Never translated, never re-punctuated. |
| Doppler | 62 | The effect, in satellite work. Same word, proper name. |
| DX | 52 | Distant station / distance working. |
| ADIF | 49 | The log file format. |
| WSJT-X | 47 | The program Nexus is compatible with. |
| DXCC | 43 | The award programme and the entity list. |
| SSTV | 40 | The mode. |
| APRS / APRS-IS | 40 / 12 | The protocol and its internet backbone. |
| Field Day | 34 | The ARRL/RAC event's official name. |
| USB / LSB / SSB / FM | 33 / 10 / 27 / 21 | Mode names, as marked on the radio. |
| ClubLog | 33 | Service name. |
| FT8 / FT4 | 32 / 17 | Mode names. |
| HF / VHF / UHF | 32 / 18 / 4 | Band ranges. |
| VFO | 31 | The tuning register, marked VFO on every rig. |
| RF | 30 | Radio frequency (as in RF power, RF gain). |
| eQSL | 30 | Service name. |
| RTTY | 27 | The mode. |
| POTA / SOTA | 24 / 9 | Parks and Summits On The Air — programme names. |
| CSV | 24 | File format. |
| CHIRP | 23 | The radio-programming program. |
| UTC | 23 | The time standard. Never "TUC". |
| PSK / PSK31 | 23 / 8 | Mode names. |
| QSY | 22 | Q-code: change frequency. |
| SmartSDR / FlexRadio | 22 / 9 | Product names. |
| Hamlib / rigctld | 22 / 13 | The radio-control library and its daemon. |
| DXpedition | 21 | The activity. Widely used unchanged. |
| ARRL / RAC / IARU | 20 / 3 / 1 | Organisation names. |
| SatNOGS | 19 | The satellite database. |
| AOS / LOS | 19 / 12 | Acquisition and loss of signal, on a satellite pass. |
| PTT | 18 | Push to talk. Printed on the microphone. |
| RBN | 18 | Reverse Beacon Network. |
| QTH | 17 | Q-code: location. |
| QSL | 16 | The confirmation, the card and the act. |
| HRDLog / HRD | 16 / 9 | Product names. |
| DAX | 15 | FlexRadio's audio transport. |
| VUCC / WAS / WAZ | 15 / 13 / 7 | Award programme names. |
| ALC / AGC / SWR / ATU | 14 / 4 / 3 / 6 | Front-panel radio vocabulary. |
| CI-V | 14 | Icom's CAT protocol. |
| DSP | 14 | Digital signal processing. |
| TQSL | 12 | The LoTW upload program. |
| Cabrillo | 6 | The contest log format. |
| RST | 6 | The signal report system (and the 599 that comes with it). |
| WPM | 5 | Words per minute, CW speed. |
| RR73 | 4 | The FT8 message. Never expanded, never translated. |
| Fox / Hound | 2 / 2 | The two roles in FT8 DXpedition mode, as WSJT-X names them. |
| EN52, EN52XA, EN52XA25 | 4 | Example grid squares. Copy the characters exactly. |

Also on the list, appearing fewer times each but under the same rule: JS8Call, JTDX, GridTracker,
MMSSTV, fldigi, N1MM, N3FJP, DXKeeper, Log4OM, HamQTH, PSK Reporter, Telnet, Winlink, WinKeyer,
K1EL, RIGblaster, OmniRig, Icom, Yaesu, Kenwood, Elecraft, Xiegu, MUF, EME, WSPR, MSK144, Q65,
JT65, FST4, FST4W, IOTA, ATNO, B4, QRM, QRP, VOX, NOAA, ISS, TLE, Maidenhead, AD1C, and the rig
model numbers (FT-991A, IC-7300, IC-9700, FTDX10 …) and submode names (DATA-U, USB-D, PKTUSB,
RTTY-AFSK).

---

## B. Translate — but pick one word and use it everywhere

These are ordinary words, and Portuguese has a perfectly good word for each. The risk is not
mistranslation, it is *inconsistency*: "band" rendered three ways over three hundred rows.

**The right-hand column is deliberately blank.** I am not a Portuguese speaker and I am not going
to guess at your language — that choice is yours to make once, here, and then apply. Write your
word in, and use only that word in the CSV.

| English | Rows | Where it turns up | Your pt-BR word |
|---|---:|---|---|
| band | 309 | Band pickers, band map, per-band settings. The band *names* (20m, 40m) stay as they are. | |
| radio | 227 | The rig itself, and the radio list in Settings. | |
| mode | 208 | The emission mode. The mode *names* (FT8, USB, CW) stay as they are. | |
| rig | 151 | Same object as "radio" — decide whether Portuguese keeps two words or one. | |
| log / logbook | 130 / 60 | Both the noun and the verb ("log this contact"). Watch which one each row is. | |
| settings | 117 | The Settings screen and every reference to it. | |
| grid | 111 | The Maidenhead locator. Many Brazilian operators say "grid" — your call. | |
| audio | 106 | Sound cards, levels, routing. | |
| station | 103 | Both your own station and the one you are working. | |
| port | 97 | Serial and network ports. | |
| dial | 95 | The dial frequency. A radio term, but the word itself is prose. | |
| transmit / receive | 94 / 30 | The verbs. The abbreviations TX/RX stay English. | |
| pass | 85 | A satellite pass. | |
| callsign | 78 | Appears constantly. Whatever you choose, choose it once. | |
| worked | 78 | "Worked before", "stations you have worked". | |
| frequency | 73 | | |
| tune | 55 | Two senses: tuning the radio, and the Tune button that keys a carrier. | |
| power | 52 | RF power, in watts. | |
| confirmed | 50 | A QSO confirmed by LoTW/eQSL/card. | |
| decode | 49 | Both noun and verb. | |
| spot / spots | 31 / 46 | A cluster or RBN spot. Both noun and verb. | |
| needed | 46 | "Needed" is also a screen name in the navigation — keep the screen name and the word matching. | |
| import / export | 45 / 42 | ADIF import and export. | |
| upload | 61 | Sending the log to LoTW, QRZ, eQSL, ClubLog. | |
| satellite | 41 | | |
| entity | 37 | A DXCC entity. Not the same thing as a country — the distinction matters to the award. | |
| section | 36 | An ARRL/RAC section in Field Day. The section *codes* (WI, ENY) stay as they are. | |
| cluster | 34 | The DX cluster. | |
| waterfall | 34 | The scrolling spectrum display. | |
| contact | 32 | The plain-English word for a QSO. Where the row says QSO, keep QSO. | |
| park / summit | 31 / 8 | POTA parks and SOTA summits. The reference codes stay as they are. | |
| beacon | 30 | | |
| antenna | 30 | | |
| memories | 28 | Saved channels — the Memories screen. | |
| cockpit | 26 | Nexus's word for an operating screen. Decide whether to translate it or keep it as a product term. | |
| pane / panel | 19 / 8 | The movable boxes inside a cockpit. | |
| operator | 22 | The person at the key. | |
| keyer | 21 | The CW keyer. | |
| exchange | 15 | The contest exchange. | |

---

## If I have this wrong

Both lists are my best guess as an American operator reading a Brazilian radio room from a long
way away. Where a term in list A is genuinely said in Portuguese on the air in Brazil, move it to
list B and translate it. Where something in list B turns out to be a term of art that Brazilian
operators keep in English, move it to A and tell me — I will fix the `do_not_translate` column in
the CSV so the checker agrees with you.

73, KD9TAW
