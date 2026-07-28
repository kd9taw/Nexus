/* Nexus: the C-side result record for the WSPR decode core.
 *
 * WSPR has no library-shaped decoder upstream — WSJT-X runs the `wsprd`
 * EXECUTABLE as a subprocess — so `main()` was converted into
 * wspr_decode_core() and needs a way to hand results back that is not printf.
 *
 * The fields mirror upstream's own `struct result` (wsprd.c), narrowed to what a
 * caller can use: date/time are dropped (the caller knows when it captured the
 * audio) as are the decoder-internal diagnostics (cycles, jitter, blocksize,
 * metric, nhardmin, ipass).
 *
 * Layout is deliberately NOT the 64-byte record the FT8-family modes share:
 * WSPR's message is 22 characters from its own 50-bit layer, it reports a
 * frequency in MHz as a double rather than an audio offset in Hz, and it has a
 * drift term none of the others carry. Forcing it into the shared shape would
 * have meant lying about at least two of those.
 */
#ifndef WSPRD_NEXUS_H
#define WSPRD_NEXUS_H

typedef struct {
    double freq;        /* absolute RF frequency, MHz (dial + audio offset)   */
    float  sync;        /* sync quality                                      */
    float  snr;         /* SNR estimate, dB in 2500 Hz                       */
    float  dt;          /* time offset, seconds                              */
    float  drift;       /* frequency drift, Hz/minute — WSPR-specific        */
    char   message[23]; /* NUL-terminated "CALL GRID DBM"; 22 chars + NUL     */
    int    decodetype;  /* 0 = type 1, 1 = type 2, 2 = type 3                */
} wspr_decode_t;

/* Decode every WSPR signal in one 2-minute reception interval.
 *
 *   iwave     : nsamples int16 samples @ 12 kHz. WSPR reads 114 s = 1368000;
 *               a short buffer is zero-padded rather than refused.
 *   dialfreq  : rig dial frequency in MHz, used to report absolute frequency.
 *   quickmode : 1 = do not dig deep for weak signals.
 *   npasses   : subtraction passes (upstream default 2; 1 = -B).
 *   subtraction / more_candidates / stackdecoder : upstream's -s / -d / -J.
 *   out, max_out : caller's array.
 *
 * Returns the number of decodes (>= 0), or -1 on error.
 *
 * NOT thread-safe. The hashed-callsign table is OFF (upstream's -H), so type-2
 * and type-3 messages report the <...> hash form rather than resolving it.
 */
int wspr_decode_core(const short *iwave, long nsamples, double dialfreq,
                     int quickmode, int npasses, int subtraction,
                     int more_candidates, int stackdecoder,
                     wspr_decode_t *out, int max_out);

#endif /* WSPRD_NEXUS_H */
