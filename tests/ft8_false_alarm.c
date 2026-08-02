/*
 * Nexus: native FT8 FALSE-ALARM test — decode noise, expect silence.
 *
 * The counterpart to ft8_acquire.c. That test proves the decoder still finds
 * signals that are there; this one proves it does not invent signals that are
 * not. Both directions matter, and only the first was covered until now.
 *
 * Why this is worth a gate of its own
 * -----------------------------------
 * A lost decode costs one QSO. A FALSE decode in Nexus reaches log_qso and then
 * the connector upload funnel — LoTW, QRZ, ClubLog, eQSL, PSK Reporter — so it
 * fabricates a contact in the operator's log and in someone else's award record.
 * That asymmetry is why any change touching syncmin, candidate selection, the
 * OSD/AP surface, or the number of decode passes has to be measured here before
 * it ships, not just against the yield ladder.
 *
 * Method
 * ------
 * N independent AWGN-only frames, no signal at any frequency. Each frame gets a
 * distinct RNG seed. The decoder runs at the settings most likely to manufacture
 * a decode:
 *   - ndepth 3 (Deep), the shipping default
 *   - AP fully armed: real mycall/hiscall and an in-band nfqso, so the ft8apset
 *     hypotheses (iaptype 1-6) are live. AP is where a false decode is most
 *     likely to come from, because it hands the LDPC decoder a candidate message
 *     and asks whether the noise could be it.
 *   - nqso_progress CYCLED 0..5 across frames. It selects which AP hypothesis
 *     schedule runs, so pinning it would leave most of the AP surface untested.
 *   - the full 200-2900 Hz search range
 *
 * a7 cross-cycle state is reset before every frame and nutc is held constant, so
 * frames are genuinely independent and one false decode cannot seed more through
 * the replay table. Testing the a7 replay path against noise is a separate
 * experiment and deliberately not conflated with this one.
 *
 * PASS iff zero decodes across all N frames. Any decode is printed in full so a
 * regression names the message, frequency and AP type that produced it.
 *
 * Usage: ft8_false_alarm [N]   (default below; raise it when chasing a rate)
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>

#include "libtempo.h"

#ifndef M_PI
#define M_PI 3.14159265358979323846
#endif

/* Frames to test when no argument is given. Enough to catch a gross regression
 * in CI without dominating the suite; pass a larger N by hand when measuring an
 * actual rate rather than gating on zero. */
#define DEFAULT_FRAMES 200

/* Box-Muller Gaussian, unit variance. Matches ft8_acquire.c's generator so both
 * tests exercise the decoder against noise of the same character. */
static float grandf(void) {
    double u1 = (rand() + 1.0) / (RAND_MAX + 2.0);
    double u2 = (rand() + 1.0) / (RAND_MAX + 2.0);
    return (float)(sqrt(-2.0 * log(u1)) * cos(2.0 * M_PI * u2));
}

static void trim(char *s) {
    for (int j = (int)strlen(s) - 1; j >= 0 && s[j] == ' '; j--) s[j] = '\0';
}

int main(int argc, char **argv) {
    int nframes = DEFAULT_FRAMES;
    if (argc > 1) {
        nframes = atoi(argv[1]);
        if (nframes <= 0) {
            printf("RESULT: FAIL (bad frame count '%s')\n", argv[1]);
            return 1;
        }
    }

    static int16_t iwave[FT8_NMAX];
    const int MAXOUT = 64;
    ft8_decode_t out[64];

    /* AP armed with real callsigns: this is the configuration in which the
     * decoder is most willing to accept a marginal candidate. */
    const char *mycall  = "KD9TAW";
    const char *hiscall = "W1AW";
    const int   nfqso   = 1500;

    printf("FT8 false-alarm test: %d AWGN-only frames, ndepth=3, "
           "AP armed (mycall=%s hiscall=%s nfqso=%d Hz, nqso_progress cycling 0-5)\n",
           nframes, mycall, hiscall, nfqso);

    int total_false = 0;
    int frames_with_decodes = 0;

    for (int f = 0; f < nframes; f++) {
        /* Independent noise per frame. */
        srand(0x5EED0000u + (unsigned)f);
        for (int i = 0; i < FT8_NMAX; i++) {
            float v = grandf() * 100.0f;
            if (v >  32767.0f) v =  32767.0f;
            if (v < -32768.0f) v = -32768.0f;
            iwave[i] = (int16_t)lrintf(v);
        }

        /* Independence: clear the cross-cycle table so a false decode in frame
         * f cannot be replayed into frame f+1 and inflate the count. */
        ft8_a7_reset();

        int nqso_progress = f % 6;
        /* lft8apon=1 / lapcqonly=0: AP fully armed, every hypothesis live —
         * the worst case this gate exists to measure. Do not "fix" a
         * regression here by turning AP off; that hides the false decode
         * instead of removing it. */
        int ndec = ft8_decode_frame(iwave, 200, 2900, 3, mycall, hiscall,
                                    nqso_progress, nfqso, /*nutc*/0, /*la7final*/1,
                                    /*lft8apon*/1, /*lapcqonly*/0,
                                    out, MAXOUT);
        if (ndec < 0) {
            printf("RESULT: FAIL (decoder error on frame %d, ndec=%d)\n", f, ndec);
            return 1;
        }
        if (ndec == 0) continue;

        frames_with_decodes++;
        total_false += ndec;
        for (int i = 0; i < ndec && i < MAXOUT; i++) {
            char m[40];
            strncpy(m, out[i].message, sizeof(m) - 1);
            m[sizeof(m) - 1] = '\0';
            trim(m);
            printf("  FALSE frame=%d nqso_progress=%d sync=%.2f snr=%d dt=%.2f s "
                   "freq=%.1f Hz nap=%d qual=%.2f msg='%s'\n",
                   f, nqso_progress, out[i].sync, out[i].snr, out[i].dt,
                   out[i].freq, out[i].nap, out[i].qual, m);
        }
    }

    printf("%d false decode(s) across %d frames (%d frame(s) affected)\n",
           total_false, nframes, frames_with_decodes);

    if (total_false == 0) {
        printf("RESULT: PASS (no decodes from noise)\n");
        return 0;
    }
    printf("RESULT: FAIL (decoder produced %d decode(s) from pure noise — "
           "a false decode here reaches log_qso and the upload funnel)\n",
           total_false);
    return 1;
}
