# Nexus in Portuguese — a translation kit

Thank you for taking this on.

Nexus is a ham radio operating program: one window that runs the radio over CAT, decodes and
transmits FT8, CW, SSB, RTTY, PSK, SSTV and APRS, keeps the log, and shows the map, the spots and
the satellite passes. It already speaks English, German, Spanish, French and Japanese — you would
be adding Portuguese, and there are a lot of Brazilian operators who would rather not work in
English.

## What I am asking you to do

Open **`nexus-ptbr-translation.csv`** in Excel, LibreOffice, Google Sheets, or any text editor.
Fill in the **`portuguese`** column. That is all.

Leave the other columns exactly as they are — the program matches rows by the `key` column, so a
changed or deleted key is a lost string.

| Column | What it is |
|---|---|
| `priority` | 1 is the most valuable work, 6 the least. See below. |
| `key` | The program's internal name for the string. **Do not change it.** |
| `english` | The English text. **Do not change it.** |
| `portuguese` | **Yours.** Empty for you to fill. |
| `do_not_translate` | Everything in that row that must be copied through *unchanged*. |
| `notes` | Context, but only where the English is ambiguous on its own. Most rows have none. |

Anything you leave blank simply shows in English. There is no broken state, no missing-text
placeholder, no crash. So a half-finished file is genuinely useful and genuinely shippable.

## The one rule that can actually break something

Some things inside a string are not words. They are slots the program fills in, or marks that
make a word bold. **Copy them through exactly, character for character.** You may move them
anywhere in the sentence — Portuguese word order is yours — but you may not rename them,
translate them, drop them, or add new ones.

**Example 1 — a slot the program fills in. `{{double braces}}`.**

```
English:    Imported {{count}} QSOs
Portuguese: {{count}} QSOs importados          ← good: the slot moved, the name did not change
```

**Example 2 — this is the one that breaks. Same string, done wrong:**

```
English:    Imported {{count}} QSOs
Portuguese: {{contagem}} contatos importados   ← WRONG
```

Two things just went wrong. The program has no value called `contagem`, so it cannot fill the
slot — the operator literally sees the characters `{{contagem}} contatos importados` on screen,
braces and all. And `QSO` became a word nobody says on the air. Renaming a slot, deleting one, or
writing single braces `{count}` instead of double all fail the same way.

**Example 3 — bold marks. `<b>` … `</b>`.**

```
English:    <b>{{achievement}}</b> — turn on <b>{{feature}}</b>?
Portuguese: <b>{{achievement}}</b> — ativar <b>{{feature}}</b>?    ← good
```

These are not HTML and you cannot invent new ones. If you write `<negrito>`, nothing goes bold
and the operator sees the characters `<negrito>` printed in the middle of the sentence. Keep
each one paired: an opening `<b>` needs its `</b>`.

**And numbers keep their dot.** `45.45 baud`, `14.074`, `2.4 kHz` — a decimal comma in a
frequency, a rate or a shift is not a style choice, it is a number an operator reads off the
screen and dials into a radio. Portuguese prose gets a comma; a technical number never does.

There is one more, and it is rare — a handful of rows about CW macros contain tokens in
**single** braces and capitals: `{MYCALL}`, `{NAME}`, `{RST}`. Those are typed by the operator
into their own macros and matched literally. Copy them; the words around them are yours. Those
rows are flagged in `notes`.

The `do_not_translate` column already lists, for every row, exactly what has to survive. If a row
is blank there, the whole string is ordinary prose and you have a free hand.

## Ham words stay in English — but tell me where I am wrong

QSO, CW, RTTY, FT8, ADIF, Cabrillo, LoTW, QRZ, POTA, 20m, 599, DX, QSY — these stay as they are,
because that is what an operator says on the air and reads on the front of the radio, in Brazil
as everywhere else. Translating them would make the program *harder* to use, not easier.
`GLOSSARY.md` lists the recurring ones with a count of how often each appears.

But you are the Brazilian operator here and I am not. Where Brazilian usage actually differs —
where a word I marked "keep in English" is genuinely said in Portuguese on the air, or where a
word I marked as ordinary prose is really a term of art — say so and change it. That judgement
is yours, and it is the part of this I cannot do. Put a note back to me and I will fix the
glossary, not argue.

## Plurals — a few rows come in pairs

61 strings change with the count, so they appear as two rows, with `::one` and `::other` on the
end of the key:

```
logbook.import.imported::one     Imported {{count}} QSO
logbook.import.imported::other   Imported {{count}} QSOs
```

`::one` is used for exactly 1, `::other` for everything else. Fill both, separately. Do not merge
them into one row — that mistake shipped in French and Spanish once and put *"3 QSO importé3 QSO
importés"* on screen for three releases.

## The priority tiers

Work down the file in order and you are always doing the most valuable rows left. Stop wherever
you like.

| Tier | Rows | What it covers |
|---:|---:|---|
| **1** | **542** | The frame the operator never stops looking at: the navigation bar, the top bar, panes and pop-outs, connection status, errors and toasts, band and frequency controls, the log-entry form, and Settings ▸ Station. |
| **2** | 313 | First run — the setup wizard and the Getting Started guide. The first thing a new operator meets. |
| **3** | 834 | The daily operating surfaces: the FT8/FT4 cockpit, the logbook, the station roster, spots, the Needed panel, the waterfall and band map. |
| **4** | 333 | The settings people actually open: audio, radios, connections, alerts, transmit limits, integrations, backup. |
| **5** | 2269 | The other cockpits and features: Phone, CW, Tempo, RTTY, PSK, SSTV, APRS, satellites, the map, awards, memories. |
| **6** | 753 | The deep end: rig-control detail, confirmation-service setup, rotator and routing, and the long tail. |

**Tier 1 on its own is a real release.** 542 rows, about 19,000 characters — roughly 8% of the
text — and it buys a program whose menus, buttons, status messages and log form are all in
Portuguese, with the rest quietly falling back to English. Tiers 1 and 2 together (855 rows) is
the point where a new Brazilian operator can install Nexus and never meet an English screen until
they go looking for one.

## Check your work before you send it

There is a checker in the kit. It needs Python 3 and nothing else:

```
python3 verify-ptbr.py nexus-ptbr-translation.csv
```

It tells you, in plain language, whether any row went missing, whether a slot or a bold mark got
renamed or dropped, whether a decimal comma crept into a number, and how far through you are. It
does not check your Portuguese — only that nothing mechanical broke. Run it before you send the
file and again whenever you have done a session's work.

## A few things that are English on purpose

Don't report these as bugs — they are not translated yet, by decision, and it is not your
oversight: the transmit controls (**Stop TX**, **Tune**, the TX-enable switch, **ATU**, the TX/RX
indicator) are drawn the same in every cockpit and are still hardcoded English, as are some
Settings section names. If you think one of them should be translated, that is a fair opinion —
tell me, it just is not in this file.

## Sending it back

Email the filled CSV to **kd9taw@protonmail.com**. Any amount is welcome — a hundred rows or all
of them. If it is easier to send in pieces as you go, send in pieces. If something in the English
does not make sense, or a string is impossible to translate without seeing the screen, put a
question in the `notes` cell of that row and I will answer it rather than guess.

You will be credited by callsign in the release notes and in the program's credits, as the
translator of the Portuguese catalog, unless you would rather not be named.

73,

KD9TAW

---

## For the maintainer

*Not part of the translator's job — this is the return path, for whoever receives the filled CSV.
Proof that it works end to end, with the numbers, is in `ROUNDTRIP.md`.*

**1. Check the file.** `python3 verify-ptbr.py their-file.csv` — must exit 0. Warnings are for a
human to read; errors are not negotiable.

**2. Convert it.**

```
python3 csv-to-catalog.py their-file.csv -o ui/src/i18n/pt.ts
```

Pure stdlib, deterministic (same CSV in, byte-identical file out). An empty cell becomes an
**absent key**, never `""` — that is the difference between a key that falls back to English and
one that renders blank. It refuses, and writes nothing, on a half-filled plural pair in either
direction: a `pt` entry holding only `other` renders the plural sentence for a count of 1, which is
worse than no entry at all.

**3. Apply the touch list** — four edits, and the tag is **`pt`**, never `pt-BR` (the guards read
`main.tsx` with `/installCatalog\(\s*'([a-z-]+)'/g`, which an uppercase `B` does not match, so a
`pt-BR` catalog ships unguarded — that is how es/fr shipped):

| # | File | Change |
|---|---|---|
| 1 | `ui/src/i18n/pt.ts` | the converter's output |
| 2 | `ui/src/main.tsx` | `import { PT } from './i18n/pt'`; `installCatalog('pt', PT)` **before** `initLocale()` |
| 3 | `ui/src/i18n/useLocale.ts` | `pt: 'Português',` in `LOCALE_NATIVE_NAME` |
| 4 | `ui/src/i18n/placeholders.test.ts` | import `PT`, and add `['pt', PT as Record<string, unknown>]` to `OTHER` |

Step 4 is the one that is easy to miss and impossible to notice: without it the suite stays green
and the catalog is checked by nothing. That is exactly how Spanish and French shipped.

**4. Run the suite.** In `ui/`: `npx vitest run src/i18n`, then `npm test` and `tsc -b`. The i18n
**test count must rise by two** against the same run before `pt.ts` existed — the new cases are
`pt carries English's holes…` and `pt translates every key English defines`. If it did not rise,
row 4 of the table did not take, and the catalog is unguarded.

**⚠️ A partial catalog fails the suite.** `placeholders.test.ts` requires every English key to be
present, with no allowlist by design. Runtime is happy with a partial catalog — a missing key falls
back to English — but CI is not. A tier-1-only fill (540 entries) fails with 4443 missing keys.
Decide before the translator starts whether an incomplete file gets held, gets its gaps filled from
English, or gets an allowlist; "stop wherever you like" is only true at runtime until then. The
options and their costs are in `ROUNDTRIP.md`.
