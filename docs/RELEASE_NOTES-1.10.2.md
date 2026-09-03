*Released 3 September 2026 — everything new since 1.10.1*

**When someone answers your CQ, you hear about it — every time.** Running a CQ with
auto-sequencing on, the only alert you reliably got was the "73" after a contact was
already finished. Now every station that calls you raises the alert, once: the one that
answers your CQ, and any others that call in while you are working someone. A station you
called yourself never "calls you" — its replies are just the QSO — and the sign-off at
the end is never mistaken for a new call. It works the way GridTracker does. Reported from
the field.

**The transmit meters read during Tune again.** Since late August the SWR, ALC and power
bars stood still while Tune was keyed. That was a deliberate change to keep a slow meter
read from interrupting the tune carrier — made on the assumption that nobody watches the
meters during a tune-up. You do: it is how you see the antenna tuner take, and on a remote
station it is the only way. The meters run during Tune again, with the interruption
prevented a different way, so a slow radio costs you a reading, never a gap in the carrier.

**CAT is more honest about what your radio is doing.** The connection indicator used to be
able to show green over a radio that was switched off, and a band change made while the
radio was off could get recorded as "the radio refused that frequency" and then never sent
again for the rest of the session. Green now means the radio actually answered, a radio
that simply is not responding is reported as exactly that, and no band gets blacklisted
over a link that was only down for a moment. Where the radio's USB connection comes and
goes, Nexus now watches for it and reopens the port when it returns — even if it comes
back under a different COM number. A few radios still need a restart to reconnect after
being left off; that case is still being worked.

**Your logged connectors remember they are working.** The connection health shown in
Settings ▸ Logging & Connectors lived only in memory, so every launch reset every service
to "stored — not verified yet" until the next contact went up — and for HRDLog.net, World
Radio League and Cloudlog, which return no confirmation, that line was the only sign the
credentials worked. It now survives a restart, and pushing a single contact from the
Logbook updates the row on the spot with the service's real answer.

**The band list hides a station you just worked.** With "hide worked-before" on, a station
you had just finished a contact with stayed on the Band Activity list, with its B4 badge,
until you clicked somewhere else. It now clears the moment the contact is done — while the
station you are still working stays put.
