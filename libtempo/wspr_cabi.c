/* Nexus: C ABI wrapper for native WSPR ENCODE.
 *
 * The decode half lives in the vendored wsprd.c (wspr_decode_core, converted from
 * upstream's main()). This file is the transmit half, and it is a thin shim: the
 * encoder itself is upstream's get_wspr_channel_symbols() in wsprsim_utils.c,
 * which is ALREADY compiled into libtempo because the decoder calls it (wsprd.c
 * :1433) to regenerate and subtract a decoded signal. Encode and decode therefore
 * share one symbol generator and cannot drift apart — the same arrangement Q65 and
 * FST4 ended up with.
 *
 * Nexus-owned, kept OUT of the vendored tree so the WSJT-X sources stay pristine.
 *
 * ⭐ WSPR IS NOT A QSO MODE. The 50-bit payload carries exactly "CALL GRID DBM"
 * and nothing else — no exchange, no free text, no addressing. Transmitting it is
 * a scheduling decision (a percentage of intervals, unattended), not a sequencer
 * decision, which is why the operating layer treats these tiers separately.
 *
 * ⭐ NO WAVEFORM HERE, deliberately. Upstream has no library-shaped WSPR waveform
 * generator: wsprsim's add_signal_vector() builds an I/Q pair for the simulator's
 * own noise model, not slot-positioned audio. The waveform is plain
 * continuous-phase 4-FSK (162 symbols, 8192 samples each at 12 kHz, spacing =
 * 12000/8192 Hz) and is synthesised on the Rust side, where it can be tested.
 */

#include <stdlib.h>
#include <string.h>

#include "vendor/wsjtx/lib/wsprd/wsprsim_utils.h"

/* WSPR channel symbols per transmission. */
#define WSPR_NSYM 162

/* Hash-table sizes, matching wsprd.c:801-803 exactly. These exist for COMPOUND
 * callsigns (K1ABC/P, <PJ4/K1ABC>), whose full call cannot fit in 50 bits and is
 * carried as a hash the receiver resolves from calls it has already heard. A
 * beacon sending a standard callsign never touches them, but get_wspr_channel_symbols
 * writes through them while verifying the message round-trips, so they must be real
 * allocations rather than NULL. */
#define WSPR_HASHTAB_BYTES (32768 * 13)
#define WSPR_LOCTAB_BYTES (32768 * 5)

/*
 * wspr_encode_msg : "CALL GRID DBM" -> the 162 WSPR channel symbols (values 0..3).
 *
 *   msg          : message text, e.g. "KD9TAW EN52 30" (NUL-terminated)
 *   symbols_out  : caller buffer, WSPR_NSYM entries
 *   returns      : WSPR_NSYM on success, -1 if the message will not encode
 *
 * The symbols are sync (the 162-entry pr3 vector) interleaved with the convolutionally
 * encoded payload, exactly as upstream builds them.
 */
int wspr_encode_msg(const char *msg, unsigned char *symbols_out)
{
    char message[23];
    unsigned char symbols[WSPR_NSYM];
    char *hashtab = NULL;
    char *loctab = NULL;
    int rc = -1;
    size_t i;

    if (msg == NULL || symbols_out == NULL) {
        return -1;
    }

    /* Upstream's encoder takes a MUTABLE char* and copies at most 23 bytes; give it
     * its own NUL-padded buffer rather than the caller's string. */
    memset(message, 0, sizeof(message));
    for (i = 0; i < sizeof(message) - 1 && msg[i] != '\0'; i++) {
        message[i] = msg[i];
    }

    hashtab = calloc(WSPR_HASHTAB_BYTES, sizeof(char));
    loctab = calloc(WSPR_LOCTAB_BYTES, sizeof(char));
    if (hashtab == NULL || loctab == NULL) {
        free(hashtab);
        free(loctab);
        return -1;
    }

    memset(symbols, 0, sizeof(symbols));
    if (get_wspr_channel_symbols(message, hashtab, loctab, symbols) != 0) {
        /* Upstream returns non-zero on success here — it is the "message was
         * packed" flag, not an errno-style status. Copy out. */
        for (i = 0; i < WSPR_NSYM; i++) {
            symbols_out[i] = symbols[i];
        }
        rc = WSPR_NSYM;
    }

    free(hashtab);
    free(loctab);
    return rc;
}
