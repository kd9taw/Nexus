# Nexus in Italian — a translation kit

Thank you for offering to do this.

Nexus is a ham radio operating program: one window that runs the radio over CAT, decodes and
transmits FT8, CW, SSB, RTTY, PSK, SSTV and APRS, keeps the log, and shows the map, the spots and
the satellite passes. It already speaks English, German, Spanish, French and Japanese. You would
be adding Italian, and there are a lot of Italian operators who would rather not work in English.

## What I am asking you to do

Open **`nexus-it-translation.csv`** in a spreadsheet program or any text editor. Fill in the
**`italian`** column. That is all.

Leave the other columns exactly as they are. The program matches rows by the `key` column, so a
changed or deleted key is a lost string. When you save, keep the file as CSV in UTF-8, or the
accents will not survive.

| Column | What it is |
|---|---|
| `priority` | 1 is the most valuable work, 6 the least. See below. |
| `key` | The program's internal name for the string. **Do not change it.** |
| `english` | The English text. **Do not change it.** |
| `italian` | **Yours.** Empty for you to fill. |
| `do_not_translate` | Everything in that row that must be copied through *unchanged*. |
| `notes` | Context, but only where the English is ambiguous on its own. Most rows have none. |

Anything you leave blank simply shows in English. There is no broken state, no missing-text
placeholder, no crash. So a half-finished file is genuinely useful.

## The one rule that can actually break something

Some things inside a string are not words. They are slots the program fills in, or marks that
make a word bold. **Copy them through exactly, character for character.** You may move them
anywhere in the sentence (Italian word order is yours), but you may not rename them, translate
them, drop them, or add new ones.

**Example 1 — a slot the program fills in. `{{double braces}}`.**

```
English:  Imported {{count}} QSOs
Italian:  … {{count}} …            ← good: the slot can sit anywhere, and its name did not change
```

**Example 2 — this is the one that breaks. Same string, done wrong:**

```
English:  Imported {{count}} QSOs
Italian:  … {{conteggio}} …        ← WRONG
```

The program has no value called `conteggio`, so it cannot fill the slot, and the operator sees
the characters `{{conteggio}}` on screen, braces and all. Renaming a slot, deleting one, or
writing single braces `{count}` instead of double all fail the same way. (And `QSO` stays `QSO`:
it is what everyone says on the air.)

**Example 3 — bold marks. `<b>` … `</b>`.**

```
English:  <b>{{achievement}}</b> — turn on <b>{{feature}}</b>?
Italian:  <b>{{achievement}}</b> … <b>{{feature}}</b>?      ← good
```

These are not HTML and you cannot invent new ones. If you write `<grassetto>`, nothing goes bold
and the operator sees the characters `<grassetto>` printed in the middle of the sentence. Keep
each one paired: an opening `<b>` needs its `</b>`.

**And numbers keep their dot.** `45.45 baud`, `14.074`, `2.4 kHz`: a decimal comma in a
frequency, a rate or a shift is not a style choice, it is a number an operator reads off the
screen and dials into a radio. Italian prose gets a comma; a technical number never does.

There is one more, and it is rare. A handful of rows about CW macros contain tokens in **single**
braces and capitals: `{MYCALL}`, `{NAME}`, `{RST}`. Those are typed by the operator into their
own macros and matched literally. Copy them; the words around them are yours. Those rows are
flagged in `notes`.

The `do_not_translate` column already lists, for every row, exactly what has to survive. If a row
is blank there, the whole string is ordinary prose and you have a free hand.

## Ham words stay in English, but tell me where I am wrong

QSO, CW, RTTY, FT8, ADIF, Cabrillo, LoTW, QRZ, POTA, 20m, 599, DX, QSY: these stay as they are,
because that is what an operator says on the air and reads on the front of the radio, in Italy as
everywhere else. Translating them would make the program *harder* to use, not easier.
`GLOSSARY.md` lists the recurring ones with a count of how often each appears.

But you are the Italian operator here and I am not. Where Italian usage actually differs, where a
word I marked "keep in English" is genuinely said in Italian on the air, or where a word I marked
as ordinary prose is really a term of art, say so. That judgement is yours, and it is the part of
this I cannot do. Put a note back to me and I will fix the glossary, not argue.

## Plurals — a few rows come in pairs

<!-- make-kit:plurals -->103<!-- /make-kit --> strings change with the count, so they appear as two rows, with `::one` and `::other` on the
end of the key:

```
logbook.import.imported::one     Imported {{count}} QSO
logbook.import.imported::other   Imported {{count}} QSOs
```

`::one` is used for exactly 1, `::other` for everything else, 0 included. Fill both, separately.
Do not merge them into one row. That mistake shipped in French and Spanish once and put *"3 QSO
importé3 QSO importés"* on screen for three releases.

## The splash page

A few rows near the end of tier 2 have keys that start with `splashscreen.`. They are the words of
the small page that appears while Nexus moves the logbook into its new database, before the
program itself has opened. Fill all of them, or none of them; the page needs the whole set.

## The priority tiers

Work down the file in order and you are always doing the most valuable rows left. Stop wherever
you like.

<!-- make-kit:tiers -->
| Tier | Rows | What it covers |
|---:|---:|---|
| **1** | **770** | The frame the operator never stops looking at: the navigation bar, the top bar, panes and pop-outs, connection status, errors and toasts, band and frequency controls, the log-entry form, and Settings ▸ Station. |
| **2** | 320 | First run: the setup wizard, the Getting Started guide and the splash page. The first thing a new operator meets. |
| **3** | 1,118 | The daily operating surfaces: the FT8/FT4 cockpit, the logbook, the station roster, spots, the Needed panel, the waterfall and band map. |
| **4** | 479 | The settings people actually open: audio, radios, connections, alerts, transmit limits, integrations, backup, colours. |
| **5** | 3,044 | The other cockpits and features: Phone, CW, Tempo, RTTY, PSK, SSTV, APRS, JS8, satellites, the map, awards, memories. |
| **6** | 1,512 | The deep end: rig-control detail, confirmation-service setup, rotator and routing, the pages of the remote browser station, and the long tail. |
<!-- /make-kit:tiers -->

Tier 1 alone is <!-- make-kit:tier1 -->770<!-- /make-kit --> rows, about <!-- make-kit:tier1chars -->33,000<!-- /make-kit --> characters, roughly <!-- make-kit:tier1pct -->9<!-- /make-kit -->% of the text, and it covers
the menus, buttons, status messages and log form an operator sees all day. Tiers 1 and 2 together
(<!-- make-kit:tier12 -->1,090<!-- /make-kit --> rows) add what a new operator meets in the first hour: the setup wizard and the
Getting Started guide.

## Check your work before you send it

There is a checker in the kit. It needs Python 3 and nothing else:

```
python3 verify-it.py nexus-it-translation.csv
```

It tells you, in plain language, whether any row went missing, whether a slot or a bold mark got
renamed or dropped, whether a decimal comma crept into a number, and how far through you are. It
does not check your Italian, only that nothing mechanical broke. Run it before you send the file
and again whenever you have done a session's work.

## If you see English that is not in this file

A few controls are still drawn in English by the program itself rather than from this file. If you
spot one, tell me. That one is my job, not yours.

## Sending it back

Email the filled CSV to **kd9taw@protonmail.com**. Any amount is welcome, a hundred rows or all of
them. If it is easier to send in pieces as you go, send in pieces. If something in the English
does not make sense, or a string is impossible to translate without seeing the screen, put a
question in the `notes` cell of that row and I will answer it rather than guess.

You will be credited by callsign in the release notes and in the program's credits, as the
translator of the Italian catalog, unless you would rather not be named.

73,

KD9TAW

---

## For the maintainer

*Not part of the translator's job. This is how the kit is made and how a filled file comes back.*

**The kit is built, never edited.** `python3 translations/it/make-kit.py` (Python 3 and Node 22.6
or later) writes `nexus-it-translation.csv` fresh from `ui/src/i18n/en.ts` and the splash page,
rewrites the pin block in `verify-it.py`, and fills the counts in this README and the glossary.
Its docstring says where each column comes from: the tiers, the do-not-translate lists and most
notes are reused from the Portuguese kit wherever the key and its English still match, and are
computed otherwise. Run it right before handing the kit to the translator. Between then and the
return, no string change needs an Italian row, and a stale CSV in the tree is harmless.
`make-kit.py --check` writes nothing and exits 1 when the kit is stale; it is deliberately **not** a
CI gate, because that would make every string change an Italian chore again.

**When the file comes back:**

1. Rebuild with their work carried across:
   `python3 translations/it/make-kit.py --carry their-file.csv`. A filled cell is kept where its key
   still exists and its English has not changed; a plural pair travels whole. It reports how many
   cells were carried, how many were left empty because the English changed under them, and how
   many belonged to keys that are gone.
2. Check it: `python3 translations/it/verify-it.py translations/it/nexus-it-translation.csv`, which
   must exit 0.
3. Convert it:
   `python3 translations/it/csv-to-catalog.py translations/it/nexus-it-translation.csv -o ui/src/i18n/it.ts --splash-out splash-it.json`.
   An empty cell becomes an absent key, never `""`. A half-filled plural pair, or a half-filled set
   of splash strings, is refused and nothing is written.
4. Apply the touch list. The tag is `it`.

| # | File | Change |
|---|---|---|
| 1 | `ui/src/i18n/it.ts` | the converter's output |
| 2 | `ui/src/main.tsx` | `import { IT } from './i18n/it'`; `installCatalog('it', IT)` beside the others, **before** `initLocale()` |
| 3 | `ui/src/i18n/useLocale.ts` | `it: 'Italiano',` in `LOCALE_NATIVE_NAME` |
| 4 | `ui/src/i18n/placeholders.test.ts` | import `IT`, and add `['it', IT as Record<string, unknown>]` to `OTHER` |
| 5 | `ui/public/splashscreen.html` | the `--splash-out` object, under `"it"` in the `splash-strings` block |

`placeholders.test.ts` fails until row 4 is done (it checks that every catalog `main.tsx` installs
is on its list), and `splash.test.ts` fails until row 5 is. Then run, in `ui/`, `npm test -- src/i18n`,
the full `npm test` and `tsc -b`, and `scripts/browser-probe`, since this is a `ui/` change.

**⚠️ A partial catalog fails the suite.** `placeholders.test.ts` requires every English key to be
present, with no allowlist by design. Runtime is happy with a partial catalog (a missing key
falls back to English), but CI is not. Decide before the translator finishes whether an incomplete
file is held until it is complete, has its gaps filled from English, or gets an allowlist. "Stop
wherever you like" is only true at runtime until then.
