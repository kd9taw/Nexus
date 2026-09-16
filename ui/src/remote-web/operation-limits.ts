// Shared admission limits, not station authority or permission lifetimes.
export const OPERATION_RATE_WINDOW_MS = 1000
export const OPERATION_RATE_LIMIT = 4

/** THE COMMAND LANE'S ENTITLEMENT BUDGET. Two numbers, and they answer different questions.
 *
 *  The problem both exist for: admission is not enough. A session opened while the account was
 *  entitled keeps its observer lease (OBSERVER_LEASE_MS, 60 s) whatever happens to the
 *  entitlement afterwards, because the lease is the only thing that ends a session and nothing
 *  re-read the database while one ran. Measured, not reasoned: after a revocation, twelve
 *  stationControl commands reached a real station over the following minute, and the socket
 *  closed at t+60 s when the lease finally ran out.
 *
 *  The two lanes want different answers to that. A minute of continuing video across a network
 *  wobble is a feature; a minute of continuing to move somebody else's transmitter after their
 *  access ended is the thing a paid product has to be able to say cannot happen.
 *
 *  CHECK_MS is how stale the service's reading of the entitlement may be when it forwards a
 *  command that changes the station. Past it, the room reads the trials row again before the
 *  relay sees the command, so the worst case after a revocation is this long and not a lease.
 *  It is not zero because these commands are also gestures - a tuning wheel or a gain slider
 *  spends the relay's whole four-a-second budget - and a read per command would buy nothing for
 *  the reads it costs. Two seconds collapses a drag to one read and still bounds the exposure
 *  at about one turn of the wheel.
 *
 *  MS is how long any single reading stays usable, and it is the answer to a database that
 *  cannot be read at all: an outage falls back to the last good reading rather than to either
 *  extreme - not "refuse everyone", which would break paying operators on a D1 blip, and not
 *  "allow everyone", which is the bug. Its floor is the browser's renewal cadence, which
 *  re-reads the entitlement every 30 s (client.ts), so below that control would blink off
 *  between two healthy confirmations; 45 s leaves fifteen seconds of slack for a slow one. */
export const OPERATION_ENTITLEMENT_CHECK_MS = 2000
export const OPERATION_ENTITLEMENT_MS = 45000
