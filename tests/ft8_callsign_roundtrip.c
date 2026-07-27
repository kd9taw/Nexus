/*
 * Nexus: FT8 standard-callsign round-trip test — no legitimate call may be
 * rejected by the 28-bit field's structural validation.
 *
 * Why
 * ---
 * unpack28 gained a callok() structural check (WSJT-X 3.0.2) that rewrites
 * anything not shaped like a callsign to 'QU1RK' and clears the success flag.
 * That is a false-decode reducer and it moves in the safe direction, but it is
 * still a REJECTION added to the decode path: if it rejects a call some operator
 * actually holds, that station silently stops decoding. A yield ladder using one
 * canonical message would never notice, because it only ever transmits K1ABC.
 *
 * So this test drives real callsign shapes through encode -> waveform -> decode
 * at high SNR, where any failure is attributable to the message layer rather
 * than to sensitivity. Every message must come back exactly.
 *
 * Coverage aims at the shapes callok reasons about:
 *   - call area at position 2 (K1A, W1AW) and position 3 (9A1AA, 2E1ABC)
 *   - all-digit-looking prefixes that are legal (9A, 4X, 8P, 3D2, 1A)
 *   - the longest standard forms (KH6ABC, VP8ABC)
 *   - a suffix-less short call (K1A) against the n<3 rule
 * Rare prefixes are the point: 2E1, 3D2, 4X4, 8P9, KH6, 9A1, 1A0 all begin with
 * a digit or sit at an unusual call-area offset, which is exactly where a
 * position-based structural rule is most likely to be wrong.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>

#include "libtempo.h"

#ifndef M_PI
#define M_PI 3.14159265358979323846
#endif

static float grandf(void) {
    double u1 = (rand() + 1.0) / (RAND_MAX + 2.0);
    double u2 = (rand() + 1.0) / (RAND_MAX + 2.0);
    return (float)(sqrt(-2.0 * log(u1)) * cos(2.0 * M_PI * u2));
}

static void trim(char *s) {
    for (int j = (int)strlen(s) - 1; j >= 0 && s[j] == ' '; j--) s[j] = '\0';
}

int main(void) {
    /* Messages exercising the 28-bit standard-callsign field in both slots. */
    const char *msgs[] = {
        "CQ K1A EN52",
        "CQ W1AW FN31",
        "CQ 9A1AA JN75",
        "CQ 2E1ABC IO91",
        "CQ 3D2AB RH91",
        "CQ 4X4AA KM72",
        "CQ 8P9AA GK03",
        "CQ KH6ABC BL11",
        "CQ VP8ABC GD18",
        "CQ ZL1ABC RF73",
        "CQ JA1ABC PM95",
        "CQ G4ABC IO91",
        "9A1AA KD9TAW EN52",
        "2E1ABC KD9TAW -12",
        "KH6ABC 4X4AA R-08",
        "KD9TAW 8P9AA RR73",
    };
    const int NMSG = (int)(sizeof(msgs) / sizeof(msgs[0]));

    const float fs = 12000.0f;
    const int   noff = 6000;                 /* 0.5 s FT8 TX start @ 12 kHz */
    const float snr = -5.0f;                 /* well above threshold: message layer under test, not sensitivity */
    const float bw_ratio = 2500.0f / (fs / 2.0f);

    static float   dd[FT8_NMAX];
    static float   wave[FT8_NZ];
    static int16_t iwave[FT8_NMAX];
    const int MAXOUT = 64;
    ft8_decode_t out[64];

    int failed = 0;

    printf("FT8 standard-callsign round-trip: %d messages at %.0f dB\n", NMSG, snr);

    for (int m = 0; m < NMSG; m++) {
        int itone[FT8_NN];
        int nsym = 0;
        ft8_encode(msgs[m], (int)strlen(msgs[m]), itone, &nsym);
        if (nsym <= 0) {
            printf("  FAIL '%s' — ft8_encode rejected it\n", msgs[m]);
            failed++;
            continue;
        }

        int nwave = FT8_NZ;
        ft8_gen_wave(itone, nsym, fs, 1500.0f, wave, &nwave);
        if (nwave <= 0) {
            printf("  FAIL '%s' — ft8_gen_wave failed\n", msgs[m]);
            failed++;
            continue;
        }

        memset(dd, 0, sizeof(dd));
        float sig = sqrtf(2.0f * bw_ratio) * powf(10.0f, 0.05f * snr);
        for (int i = 0; i < nwave; i++) {
            int k = i + noff;
            if (k >= 0 && k < FT8_NMAX) dd[k] += sig * wave[i];
        }
        srand(20260727 + m);
        for (int i = 0; i < FT8_NMAX; i++) {
            float v = (dd[i] + grandf()) * 100.0f;
            if (v >  32767.0f) v =  32767.0f;
            if (v < -32768.0f) v = -32768.0f;
            iwave[i] = (int16_t)lrintf(v);
        }

        ft8_a7_reset();
        int ndec = ft8_decode_frame(iwave, 200, 2900, 3, "", "", 0, 0,
                                    /*nutc*/0, /*la7final*/1, out, MAXOUT);
        if (ndec < 0) {
            printf("  FAIL '%s' — decoder error %d\n", msgs[m], ndec);
            failed++;
            continue;
        }

        int found = 0;
        int sawquirk = 0;
        for (int i = 0; i < ndec && i < MAXOUT; i++) {
            char t[40];
            strncpy(t, out[i].message, sizeof(t) - 1);
            t[sizeof(t) - 1] = '\0';
            trim(t);
            if (strcmp(t, msgs[m]) == 0) found = 1;
            if (strstr(t, "QU1RK")) sawquirk = 1;
        }

        if (found) {
            printf("  ok   '%s'\n", msgs[m]);
        } else {
            printf("  FAIL '%s' — not recovered (%d decode(s)%s)\n",
                   msgs[m], ndec, sawquirk ? ", saw QU1RK: callok rejected a real call" : "");
            for (int i = 0; i < ndec && i < MAXOUT; i++) {
                char t[40];
                strncpy(t, out[i].message, sizeof(t) - 1);
                t[sizeof(t) - 1] = '\0';
                trim(t);
                printf("         got: '%s'\n", t);
            }
            failed++;
        }
    }

    if (failed == 0) {
        printf("RESULT: PASS (all %d messages round-tripped)\n", NMSG);
        return 0;
    }
    printf("RESULT: FAIL (%d of %d messages did not round-trip)\n", failed, NMSG);
    return 1;
}
