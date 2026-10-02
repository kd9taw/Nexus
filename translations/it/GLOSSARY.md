# Glossary — the words that come back over and over

There are <!-- make-kit:rows -->6,396<!-- /make-kit --> rows in the CSV and about sixty words that appear in hundreds of them. If one
of those words gets translated three different ways across the file, the program reads as though
three people wrote it. So this is the list to settle **once**, before you start, and then not
think about again.

The **rows** column is a real count: how many rows of `nexus-it-translation.csv` contain that
word. The counts are made from the CSV itself each time the kit is built, so nothing on this list
is theoretical.

---

## A. Keep in English

These are the technical vocabulary of amateur radio, plus the proper names of file formats,
services and programs. An operator says these on the air and reads them on the front panel of the
radio. Translating them makes the program harder to use, not easier.

The checker (`verify-it.py`) enforces this list per row, using the `do_not_translate` column.

| Term | Rows | Why it stays |
|---|---:|---|
| Nexus | 225 | The program's name. |
| QSO / QSOs | 108 / 58 | The contact itself. Universal on the air; pluralises fine as *QSOs*. |
| LoTW | 97 | ARRL's Logbook of The World — a service name. |
| CAT | 91 | Computer Aided Transceiver — the radio control protocol. |
| TX / RX | 115 / 72 | Transmit and receive. Printed on the radio itself. |
| CW | 76 | The mode, as it appears in a mode picker. |
| QRZ | 74 | Both the Q-code and the callsign lookup site. |
| CQ | 79 | The call. Translating it would be strange on the air and on screen. |
| MHz / kHz / Hz | 68 / 18 / 47 | SI units. Never translated, never re-punctuated. |
| Doppler | 60 | The effect, in satellite work. Same word, proper name. |
| DX | 67 | Distant station / distance working. |
| ADIF | 54 | The log file format. |
| WSJT-X | 47 | The program Nexus is compatible with. |
| DXCC | 46 | The award programme and the entity list. |
| SSTV | 46 | The mode. |
| APRS / APRS-IS | 32 / 12 | The protocol and its internet backbone. |
| Field Day | 39 | The ARRL/RAC event's official name. |
| USB / LSB / SSB / FM | 38 / 10 / 31 / 25 | Mode names, as marked on the radio. |
| ClubLog | 33 | Service name. |
| FT8 / FT4 | 41 / 24 | Mode names. |
| HF / VHF / UHF | 33 / 18 / 4 | Band ranges. |
| VFO | 34 | The tuning register, marked VFO on every rig. |
| RF | 37 | Radio frequency (as in RF power, RF gain). |
| eQSL | 30 | Service name. |
| RTTY | 35 | The mode. |
| POTA / SOTA | 38 / 11 | Parks and Summits On The Air — programme names. |
| CSV | 24 | File format. |
| CHIRP | 22 | The radio-programming program. |
| UTC | 51 | The time standard. |
| PSK / PSK31 | 28 / 8 | Mode names. |
| QSY | 22 | Q-code: change frequency. |
| SmartSDR / FlexRadio | 22 / 10 | Product names. |
| Hamlib / rigctld | 23 / 13 | The radio-control library and its daemon. |
| DXpedition | 20 | The activity. Widely used unchanged. |
| ARRL / RAC / IARU | 23 / 4 / 2 | Organisation names. |
| SatNOGS | 21 | The satellite database. |
| AOS / LOS | 19 / 12 | Acquisition and loss of signal, on a satellite pass. |
| PTT | 23 | Push to talk. Printed on the microphone. |
| RBN | 19 | Reverse Beacon Network. |
| QTH | 23 | Q-code: location. |
| QSL | 18 | The confirmation, the card and the act. |
| HRDLog / HRD | 16 / 9 | Product names. |
| DAX | 15 | FlexRadio's audio transport. |
| VUCC / WAS / WAZ | 15 / 12 / 7 | Award programme names. |
| ALC / AGC / SWR / ATU | 15 / 6 / 7 / 0 | Front-panel radio vocabulary. |
| CI-V | 15 | Icom's CAT protocol. |
| DSP | 8 | Digital signal processing. |
| TQSL | 14 | The LoTW upload program. |
| Cabrillo | 9 | The contest log format. |
| RST | 6 | The signal report system (and the 599 that comes with it). |
| WPM | 5 | Words per minute, CW speed. |
| RR73 | 4 | The FT8 message. Never expanded, never translated. |
| Fox / Hound | 2 / 4 | The two roles in FT8 DXpedition mode, as WSJT-X names them. |
| EN52, EN52XA, EN52XA25 | 4 | Example grid squares. Copy the characters exactly. |

Also on the list, appearing fewer times each but under the same rule: JS8Call, JTDX, GridTracker,
MMSSTV, fldigi, N1MM, N3FJP, DXKeeper, Log4OM, HamQTH, PSK Reporter, Telnet, Winlink, WinKeyer,
K1EL, RIGblaster, SignaLink, OmniRig, Icom, Yaesu, Kenwood, Elecraft, Xiegu, MUF, EME, WSPR, MSK144,
Q65, JT65, FST4, FST4W, IOTA, ATNO, B4, QRM, QRP, VOX, NOAA, ISS, TLE, Maidenhead, AD1C, and the rig
model numbers (FT-991A, IC-7300, IC-9700, FTDX10 …) and submode names (DATA-U, USB-D, PKTUSB,
RTTY-AFSK), and the jack labels printed on a radio (DATA, ACC).

---

## B. Translate — but pick one word and use it everywhere

These are ordinary words, and Italian has a perfectly good word for each. The risk is not
mistranslation, it is *inconsistency*: "band" rendered three ways over three hundred rows.

**The right-hand column is deliberately blank.** I am not an Italian speaker and I am not going to
guess at your language. That choice is yours to make once, here, and then apply. Write your word
in, and use only that word in the CSV.

| English | Rows | Where it turns up | Your Italian word |
|---|---:|---|---|
| band | 346 | Band pickers, band map, per-band settings. The band *names* (20m, 40m) stay as they are. | |
| radio | 293 | The rig itself, and the radio list in Settings. | |
| mode | 243 | The emission mode. The mode *names* (FT8, USB, CW) stay as they are. | |
| rig | 168 | Same object as "radio" — decide whether Italian keeps two words or one. | |
| log / logbook | 191 / 97 | Both the noun and the verb ("log this contact"). Watch which one each row is. | |
| settings | 155 | The Settings screen and every reference to it. | |
| grid | 128 | The Maidenhead locator. Many operators say "grid" — your call. | |
| audio | 124 | Sound cards, levels, routing. | |
| station | 308 | Both your own station and the one you are working. | |
| port | 100 | Serial and network ports. | |
| dial | 103 | The dial frequency. A radio term, but the word itself is prose. | |
| transmit / receive | 123 / 35 | The verbs. The abbreviations TX/RX stay English. | |
| pass | 87 | A satellite pass. | |
| callsign | 99 | Appears constantly. Whatever you choose, choose it once. | |
| worked | 92 | "Worked before", "stations you have worked". | |
| frequency | 92 | | |
| tune | 54 | Two senses: tuning the radio, and the Tune button that keys a carrier. | |
| power | 64 | RF power, in watts. | |
| confirmed | 65 | A QSO confirmed by LoTW/eQSL/card. | |
| decode | 60 | Both noun and verb. | |
| spot / spots | 47 / 60 | A cluster or RBN spot. Both noun and verb. | |
| needed | 49 | "Needed" is also a screen name in the navigation — keep the screen name and the word matching. | |
| import / export | 47 / 51 | ADIF import and export. | |
| upload | 54 | Sending the log to LoTW, QRZ, eQSL, ClubLog. | |
| satellite | 48 | | |
| entity | 37 | A DXCC entity. Not the same thing as a country — the distinction matters to the award. | |
| section | 37 | An ARRL/RAC section in Field Day. The section *codes* (WI, ENY) stay as they are. | |
| cluster | 48 | The DX cluster. | |
| waterfall | 42 | The scrolling spectrum display. | |
| contact | 54 | The plain-English word for a QSO. Where the row says QSO, keep QSO. | |
| park / summit | 44 / 12 | POTA parks and SOTA summits. The reference codes stay as they are. | |
| beacon | 31 | | |
| antenna | 37 | | |
| memories | 31 | Saved channels — the Memories screen. | |
| cockpit | 32 | Nexus's word for an operating screen. Decide whether to translate it or keep it as a product term. | |
| pane / panel | 24 / 18 | The movable boxes inside a cockpit. | |
| operator | 28 | The person at the key. | |
| keyer | 21 | The CW keyer. | |
| exchange | 27 | The contest exchange. | |

---

## If I have this wrong

Both lists are my best guess as an American operator reading an Italian radio room from a long
way away. Where a term in list A is genuinely said in Italian on the air in Italy, move it to list
B and translate it. Where something in list B turns out to be a term of art that Italian operators
keep in English, move it to A and tell me. I will change the `do_not_translate` column in the CSV
so the checker agrees with you.

73, KD9TAW
