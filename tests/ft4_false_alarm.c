/*
 * Nexus: native FT4 FALSE-ALARM test — decode noise, expect silence.
 *
 * FT4 mirror of ft8_false_alarm.c. Same argument: ft4_acquire.c proves the
 * decoder still finds signals that are there; nothing proved it does not invent
 * signals that are not. A false decode in Nexus reaches log_qso and then the
 * connector upload funnel (LoTW, QRZ, ClubLog, eQSL, PSK Reporter), so it
 * fabricates a contact in the operator's log and in someone else's award record.
 *
 * Method
 * ------
 * N independent AWGN-only frames, no signal at any frequency, distinct RNG seed
 * per frame. The decoder runs at the settings most likely to manufacture a
 * decode: ndepth 3 (Deep), the full 200-2900 Hz search, and AP armed with real
 * mycall/hiscall plus an in-band nfqso.
 *
 * nqso_progress is CYCLED 0..5 across frames rather than pinned. It selects which
 * AP hypothesis schedule runs, so a fixed value would leave most of the AP surface
 * untested — and AP is precisely where a decode gets manufactured, because it
 * hands the LDPC decoder a candidate message and asks whether the noise could be
 * it. Cycling costs nothing and covers all six schedules.
 *
 * FT4 has no a7 cross-cycle replay table (that is FT8-only), so unlike the FT8
 * test there is no per-frame state to reset: frames are independent by
 * construction.
 *
 * PASS iff zero decodes across all N frames. Any decode is printed in full so a
 * regression names the message, frequency and AP type that produced it.
 *
 * Usage: ft4_false_alarm [N]   (default below; raise it when chasing a rate)
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>

#include "libtempo.h"

#ifndef M_PI
#define M_PI 3.14159265358979323846
#endif

/* FT4 frames are ~6 s against FT8's 15 s, so a frame costs proportionally less
 * to decode; the default is set higher than the FT8 test for the same wall time. */
#define DEFAULT_FRAMES 300

/* Box-Muller Gaussian, unit variance. Matches ft4_acquire.c's generator. */
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

    static int16_t iwave[FT4_NMAX];
    const int MAXOUT = 64;
    ft4_decode_t out[64];

    const char *mycall  = "KD9TAW";
    const char *hiscall = "W1AW";
    const int   nfqso   = 1500;

    printf("FT4 false-alarm test: %d AWGN-only frames, ndepth=3, "
           "AP armed (mycall=%s hiscall=%s nfqso=%d Hz, nqso_progress cycling 0-5)\n",
           nframes, mycall, hiscall, nfqso);

    int total_false = 0;
    int frames_with_decodes = 0;

    for (int f = 0; f < nframes; f++) {
        srand(0x4EED0000u + (unsigned)f);
        for (int i = 0; i < FT4_NMAX; i++) {
            float v = grandf() * 100.0f;
            if (v >  32767.0f) v =  32767.0f;
            if (v < -32768.0f) v = -32768.0f;
            iwave[i] = (int16_t)lrintf(v);
        }

        int nqso_progress = f % 6;
        int ndec = ft4_decode_frame(iwave, 200, 2900, 3, mycall, hiscall,
                                    nqso_progress, nfqso, out, MAXOUT);
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
