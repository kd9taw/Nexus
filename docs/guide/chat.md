# Chat (Tempo)

Chat is Nexus's own weak-signal conversation mode — the thing the program was originally built
to be, and still the only part of it you cannot get anywhere else. It is a text messenger that
runs over HF: a roster of who is on frequency, a threaded conversation per station, and free
text that goes on the air in slot-synchronous overs like FT8, but carries sentences instead of
a fixed exchange.

Two waveforms sit under it. **TempoFast** is a 4-second cycle, and it is what you will use for
an actual conversation — short overs, quick back-and-forth. **TempoDeep** is a 15-second cycle
that trades speed for depth, and it is the one that keeps working when a path is fading badly.
Both carry the same 77-bit payload the FT8 message set uses, and both decode the whole
200–2900 Hz passband every slot, so you do not have to be tuned onto anybody.

**Be honest with yourself about what this is.** Both tiers have closed real links on the air,
and every sensitivity figure quoted for them is a bench number from simulation — none are
proven on the air yet. Nobody else is running Tempo unless you have arranged it. This is a mode
for a sked with somebody who also has Nexus, for a group net, or for finding out whether the
thing works. If you want contacts today, the [Operate](operate-digital.md) cockpit is where
the population is.

## The tour

**The Stations roster**, down the left, is who Nexus has heard on Tempo — not the FT8 roster,
which is a different list. Each card shows the callsign, entity, grid, distance and bearing,
how long ago it was heard, and its SNR. The chips above it filter to **Heard now**,
**Beaconing** (heard three or more times, so probably sitting there) or **Needed**, and the
search box takes wildcards — `PA*` for every PA prefix, several terms for "any of these". A
station stays on this roster far longer than on the FT cockpit's, because a queued message
needs to know you heard them recently.

Above the roster sits **★ Band — open calls**, the broadcast feed: everything heard that was
not addressed to anybody in particular. That is where CQs land, and where your own CQ goes.

Below it, **Recent chats** keeps your threads, so a conversation stays reachable after its
station has dropped off the live roster. The ✕ on a row archives that thread.

**The conversation pane** is the middle of the screen and behaves like any messenger: your
messages on one side, theirs on the other, oldest at the top. Each of your bubbles carries its
own delivery state, and this is the part worth learning, because on HF "sent" is not a simple
idea:

| What you see | What it means |
|---|---|
| Waiting to send | Queued. If it names a station, they have not been heard yet — see store-and-forward below. |
| Sending — try *n* | On the air now, attempt *n*. |
| Sent | It went out. |
| Delivered | They acknowledged it. |
| Confirmed | They answered *after* it went out, so you know they got it. |
| Sent *n*× — no acknowledgement | It went out repeatedly and nothing came back. **Tap it to send again.** |
| Not sent — abandoned on restart | It was still queued when Nexus closed. **Tap it to send again.** |

A partly-received message shows how much arrived — "3 of 5 received" — rather than pretending
it is whole or throwing it away.

**The composer** is the box at the bottom. Type and press Enter. The quick-reply chips beside
it are your own macros from Settings, so they say what you told them to say.

**The capacity meter** beside the composer is the thing that has no equivalent in a normal
messenger, and it matters: it counts your text in **overs**, not characters. Each over carries
a fixed number of characters, and a message can span only so many. The meter tells you how many
overs what you have typed will take, and says **full** when you have reached the limit —
anything past it is trimmed before it sends. On TempoFast an over is four seconds, so a
five-over message is twenty seconds of transmission. Short sentences get through; paragraphs do
not.

**Call CQ** transmits the standard `CQ <YOURCALL> <YOURGRID>` on the band feed and arms
transmit. **Heartbeat** is a presence beacon: leave it on and Nexus periodically says you are
here, so other Tempo stations can hear you — which is what lets them deliver anything they have
queued for you. Turn it off to sit silent.

## Core workflows

### Have a conversation

Pick a station from the roster, or click a thread in Recent chats. Type, press Enter, watch the
bubble's state. Keep overs short — this is a mode where a sentence is a transmission.

### Call CQ and be found

Press **📣 Call CQ**. It goes out on the band feed and arms transmit. Anyone running Tempo who
hears it sees you on their roster and can open a thread with you. Leaving **Heartbeat** on
between calls means they can find you even if they missed the CQ.

### Send to a station who is not there yet

This is the feature that makes Tempo different from a chat window, and it is worth
understanding before you need it. **A directed message queues until its recipient is actually
heard, and then delivers.** The bubble sits at "Waiting to send — *call* not heard yet" for as
long as that takes — minutes, or until the band opens.

This is why the roster keeps stations long after they have gone quiet, and why the heartbeat
matters: presence is what turns a queued message into a delivered one. Nothing is broadcast
blindly into an empty band on your behalf.

### When a message will not go

Marginal paths are the normal case here, so failures are visible rather than silent. A bubble
reading "Sent 4× — no acknowledgement" is telling you the truth: it went out four times and
nothing came back. Tap it to re-queue the same text. Do not retype it — tapping re-sends the
identical message, which is what the error-correction machinery below wants.

Underneath, failed overs are not simply thrown away. Nexus combines a failed frame with its
retransmissions rather than starting over, so the third attempt at a marginal message is
working with everything the first two brought in as well. It is on by default and there is
nothing to configure; you will see it as messages completing that felt like they should not
have.

### Winter Field Day

When Winter Field Day is active, Tempo is a first-class contact surface for it: the header says
so, the empty pane tells you to call CQ and send your exchange, and the first quick-reply chip
becomes your class and section. That is Winter Field Day only — the summer event's chrome does
not appear here.

## Honest limits

- **There is no population.** Tempo is not a mode you tune to and find people on. Arrange a
  sked, or run it alongside FT8 and see who turns up.
- **Every sensitivity number is from the bench.** Simulation figures, not on-air measurement.
  On-air decode-rate-versus-SNR reports are the single most useful thing a tester can send.
- **Text is charged by the over.** The capacity meter is not a suggestion; text past the limit
  is trimmed before transmission.
- **This screen has no stop control of its own.** Call CQ and the heartbeat put a signal on the
  air, but neither is a transmit latch. Transmit is stopped from the top bar, as it is
  everywhere outside the mode cockpits.
- **No screenshot in this chapter yet** — it is owed, like PSK's.

## Related guides

- [Operate (digital)](operate-digital.md) — FT8 and FT4, where the population is.
- [Connect](connect.md) — the situational-awareness screen, including who is on the band.
- [Settings reference](settings-reference.md) — macros, which are what the quick-reply chips
  say.
- [Logbook and QSL](logbook-qsl.md) — where a Tempo contact goes once it is worked.
