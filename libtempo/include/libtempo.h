/*
 * Tempo: C ABI for the standalone FT1 4-CPM turbo modem (libtempo).
 *
 * Decoupled from Qt / WSJT-X. Links FFTW3 single precision. No GUI.
 *
 * Frame / array constants (from ft1/ft1_params.f90):
 *   FT1_NN    = 99     total channel symbols
 *   FT1_NMAX  = 48000  raw audio samples (4.0 s @ 12 kHz)
 *   FT1_NDOWN = 54     downsample factor
 *   FT1_NDMAX = 888    downsampled complex samples
 */
#ifndef LIBFT1_H
#define LIBFT1_H

#include <stddef.h>   /* size_t (tempo_ctx_size) */
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define FT1_NN        99      /* total channel symbols                 */
#define FT1_NMAX      48000   /* raw audio samples @ 12 kHz (4.0 s)    */
#define FT1_NDOWN     54      /* downsample factor                     */
#define FT1_NDMAX     888     /* downsampled complex samples           */
#define FT1_NSPS_NUM  3000    /* samples-per-symbol numerator          */
#define FT1_NSPS_DEN  7       /* samples-per-symbol denominator        */
#define FT1_MSG91     91      /* decoded message bits (77 msg + 14 CRC)*/

/*
 * Encode an FT1 message into 99 quaternary channel symbols {0,1,2,3}.
 *   msg       : NUL- or space-terminated message string (<= 37 chars)
 *   msg_len   : number of valid chars in msg
 *   itone_out : caller buffer of FT1_NN (99) ints
 *   nsym_out  : number of symbols written (99 on success)
 */
void ft1_encode(const char *msg, int msg_len,
                int *itone_out /*[99]*/, int *nsym_out);

/*
 * Encode an FT1 message for a specific IR-HARQ redundancy version.
 *   irv = 0 : byte-identical to ft1_encode (initial transmission).
 *   irv = 1 : first retransmission  (87 new LDPC(348,91) parity + 87 systematic,
 *             RV1 Costas sync). Combine with RV0 at the receiver for coding gain.
 *   irv = 2 : second retransmission (RV2 parity + systematic, RV2 Costas sync).
 * Out-of-range irv clamps to 0. Other args as ft1_encode.
 */
void ft1_encode_rv(const char *msg, int msg_len, int irv,
                   int *itone_out /*[99]*/, int *nsym_out);

/*
 * Generate the real-valued 4-CPM audio waveform from channel symbols.
 *   itone     : nsym channel symbols
 *   nsym      : number of symbols (99)
 *   nsps_num  : samples-per-symbol numerator   (FT1_NSPS_NUM = 3000)
 *   nsps_den  : samples-per-symbol denominator (FT1_NSPS_DEN = 7)
 *   fsample   : output sample rate (Hz), e.g. 12000.0f
 *   f0        : audio carrier frequency (Hz), e.g. 1500.0f
 *   wave_out  : caller buffer (length *nwave_out on input)
 *   nwave_out : in = buffer capacity; out = samples produced
 */
void ft1_gen_wave(const int *itone, int nsym, int nsps_num, int nsps_den,
                  float fsample, float f0,
                  float *wave_out, int *nwave_out);

/*
 * Decode a received FT1 frame (real-time / single-candidate path).
 * Mirrors ft1_test: ft1_downsample -> normalize -> turbo_decode_ft1.
 *   wave           : FT1_NMAX (48000) raw audio samples @ 12 kHz
 *   f0             : candidate carrier frequency (Hz)
 *   snr_est        : SNR estimate (dB in 2500 Hz BW)
 *   message91_out  : caller buffer of FT1_MSG91 (91) int8 bits (0/1)
 *   ntype_out      : 1=turbo, 2=OSD, -1=failed
 *   nharderror_out : hard error count, -1 if failed
 */
void ft1_decode_rt(const float *wave /*[FT1_NMAX]*/, float f0, float snr_est,
                   int8_t *message91_out /*[91]*/,
                   int *ntype_out, int *nharderror_out);

/*
 * Unpack the 77 message bits (message91_out[0..76]) back to readable text.
 *   bits77   : 77 int8 bits (0/1)
 *   msg_out  : caller buffer (>= 38 bytes recommended)
 *   msg_cap  : capacity of msg_out in bytes (incl. NUL)
 *   success  : 1 if unpack succeeded, 0 otherwise
 */
void ft1_unpack(const int8_t *bits77 /*[77]*/,
                char *msg_out, int msg_cap, int *success);

/*
 * One decode result from the full RX acquisition pipeline.
 *
 * Layout (matches the Fortran bind(C) type ft1_decode_t; LP64, no padding
 * needed since all fields are naturally 4-byte aligned and message[38] is
 * followed by two 4-byte ints):
 *   offset  size  field
 *      0      4    float sync
 *      4      4    int   snr
 *      8      4    float dt
 *     12      4    float freq
 *     16     38    char  message[38]   (NUL-terminated)
 *     54      2    (padding to 4-byte boundary)
 *     56      4    int   nap
 *     60      4    float qual
 *     64      4    int   rv
 *   total: 68 bytes, 4-byte aligned.
 */
typedef struct {
    float sync;          /* sync metric                                  */
    int   snr;           /* SNR estimate, dB (rounded)                   */
    float dt;            /* time offset, seconds                         */
    float freq;          /* audio frequency, Hz                          */
    char  message[38];   /* NUL-terminated decoded message text          */
    int   nap;           /* AP type used (0 = none)                      */
    float qual;          /* decode quality metric                        */
    int   rv;            /* redundancy version 0/1/2 (rv>0 = recovered by */
                         /* joint-turbo combining that many RVs), or -1   */
} ft1_decode_t;

/*
 * Run the FULL FT1 receive acquisition pipeline on a 4-second frame:
 * Costas sync candidate search (time + frequency) -> downconvert ->
 * fine sync -> turbo decode -> OSD/AP fallback -> signal subtraction (SIC)
 * -> IR-HARQ combining. Finds signals WITHOUT a known time offset.
 *
 *   iwave         : FT1_NMAX (48000) int16 audio samples @ 12 kHz
 *   nfa, nfb      : frequency search band edges (Hz), e.g. 200 .. 2900
 *   ndepth        : decode depth (3 = full turbo+OSD+SIC; <=0 defaults to 3)
 *   mycall        : NUL/space-terminated callsign for AP (may be "")
 *   hiscall       : NUL/space-terminated callsign for AP (may be "")
 *   nqso_progress : QSO progress index (selects the AP pass schedule)
 *   frame_time_ms : monotonic millisecond timestamp for THIS frame (need not be
 *                   wall-clock; only monotonic + consistent across frames). Keys
 *                   cross-frame IR-HARQ: a failed RV0 frame is buffered and a
 *                   later RV1/RV2 at the same freq (+-10 Hz, within 30 s) is
 *                   joint-turbo-combined. Call ft1_harq_reset() on band/QSO
 *                   change. Only the low 32 bits / differences <= 30 s matter.
 *   out           : caller array of ft1_decode_t (capacity max_out)
 *   max_out       : capacity of out
 *
 * Returns the number of decodes found (>= 0), or -1 on error. Up to
 * min(found, max_out) entries are written to out.
 *
 * NOTE: not thread-safe / not reentrant (the FT1 pipeline keeps process-
 * global SAVE state and this call uses a module-level results buffer).
 */
int ft1_decode_frame(const int16_t *iwave /*[FT1_NMAX]*/,
                     int nfa, int nfb, int ndepth,
                     const char *mycall, const char *hiscall,
                     int nqso_progress, int frame_time_ms,
                     ft1_decode_t *out, int max_out);

/*
 * Clear all IR-HARQ soft-combining buffers. Call on band change, QSO change,
 * or an intentional QSY so a new exchange does not joint-combine with stale RV
 * frames from a previous one. (Buffers otherwise persist across decode calls
 * and self-expire 30 s after their last update.)
 */
void ft1_harq_reset(void);

/*===========================================================================
 * FT8: native decode/encode of the standard WSJT-X FT8 mode (15 s T/R),
 * built on the vendored WSJT-X GPL sources (lib/ft8). Full-frame decode via
 * the core primitives (ft8apset -> sync8 -> ft8b); no nzhsym/shmem. The a7
 * cross-cycle AP table (WSJT-X iaptype=7) IS wired in — see ft8_decode_frame's
 * nutc/la7final args and ft8_a7_reset().
 *===========================================================================*/

#define FT8_NN     79       /* total channel symbols                        */
#define FT8_NSPS   1920     /* samples per symbol @ 12 kHz                   */
#define FT8_NMAX   180000   /* raw audio samples (15.0 s @ 12 kHz)          */
#define FT8_NZ     151680   /* samples in the full 12.64 s waveform (NSPS*NN)*/

/*
 * Encode an FT8 message into 79 channel tones {0..7}.
 *   msg/msg_len : message text (<= 37 chars) and its valid length
 *   itone_out   : caller buffer of FT8_NN (79) ints
 *   nsym_out    : symbols written (79), or -1 on bad message
 */
void ft8_encode(const char *msg, int msg_len,
                int *itone_out /*[79]*/, int *nsym_out);

/*
 * Generate the real FT8 audio waveform (Gaussian BT=2.0) from channel tones.
 *   itone     : nsym tones
 *   nsym      : number of tones (79)
 *   fsample   : sample rate (Hz), e.g. 12000.0f
 *   f0        : audio carrier (Hz), e.g. 1500.0f
 *   wave_out  : caller buffer (capacity *nwave_out on input)
 *   nwave_out : in = capacity; out = samples produced (nsym*FT8_NSPS), or -1
 */
void ft8_gen_wave(const int *itone, int nsym, float fsample, float f0,
                  float *wave_out, int *nwave_out);

/*
 * One decode result from the FT8 full-frame acquisition.
 *
 * Layout (matches the Fortran bind(C) type ft8_decode_t):
 *   offset  size  field
 *      0      4    float sync
 *      4      4    int   snr
 *      8      4    float dt    (xdt - 0.5, seconds)
 *     12      4    float freq
 *     16     38    char  message[38]  (NUL-terminated)
 *     54      2    (padding to 4-byte boundary)
 *     56      4    int   nap   (iaptype; 0 = none)
 *     60      4    float qual
 *   total: 64 bytes, 4-byte aligned.
 */
typedef struct {
    float sync;          /* sync metric                                  */
    int   snr;           /* SNR estimate, dB (rounded)                   */
    float dt;            /* time offset, seconds (xdt - 0.5)             */
    float freq;          /* audio frequency, Hz                          */
    char  message[38];   /* NUL-terminated decoded message text          */
    int   nap;           /* AP type used (iaptype; 0 = none)             */
    float qual;          /* decode quality metric [0,1]                  */
} ft8_decode_t;

/*
 * Decode EVERY FT8 signal in a complete 15 s frame.
 *   iwave         : FT8_NMAX (180000) int16 audio samples @ 12 kHz
 *   nfa, nfb      : frequency search band edges (Hz), e.g. 200 .. 2900
 *   ndepth        : 1..3 (3 = full bp+osd, 3 passes; <=0 defaults to 3)
 *   mycall/hiscall: NUL/space-terminated callsigns for AP (may be "")
 *   nqso_progress : QSO progress index (AP pass schedule)
 *   nfqso         : QSO/RX audio freq (Hz) being worked (WSJT-X nfqso); the deep
 *                   AP passes + sync center on it. 0 / out of [nfa,nfb] = band mid
 *   nutc          : slot key for the a7 cross-cycle AP table = slot UTC
 *                   seconds-of-day (slot*15 for FT8; 0..86399). A new nutc rolls
 *                   the per-parity prior-slot table; parity = mod(nutc/5,2).
 *                   A nutc BEHIND the last seen slot (redecode of an older
 *                   capture) leaves all a7 state untouched. Constant nutc =>
 *                   the prior-slot table never populates => a7 is inert.
 *   la7final      : 1 = authoritative full-audio (boundary) pass: direct decodes
 *                   are saved into the a7 table and the cross-cycle replay runs
 *                   (recovered decodes report nap = 7). 0 = early partial pass:
 *                   slot bookkeeping only, no save/replay.
 *   out           : caller array of ft8_decode_t (capacity max_out)
 *   max_out       : capacity of out
 *
 * Returns the number of decodes found (>= 0), or -1 on error. Up to
 * min(found, max_out) entries are written. NOT thread-safe / not reentrant.
 */
int ft8_decode_frame(const int16_t *iwave /*[FT8_NMAX]*/,
                     int nfa, int nfb, int ndepth,
                     const char *mycall, const char *hiscall,
                     int nqso_progress, int nfqso,
                     int nutc, int la7final,
                     ft8_decode_t *out, int max_out);

/*
 * Clear the a7 cross-cycle decode table (prior-slot call pairs + slot tracker).
 * Call on band change / QSO change so stale prior-cycle pairs are not replayed
 * as AP hypotheses against the new band's audio. Mirrors ft1_harq_reset.
 */
void ft8_a7_reset(void);

/*===========================================================================
 * FT4: native decode/encode of the standard WSJT-X FT4 mode (7.5 s T/R,
 * 4-GFSK), built on the vendored WSJT-X GPL sources (lib/ft4 + ft4_decode.f90).
 * Driven via the OO ft4_decoder + a collector callback (no nzhsym/a7/shmem).
 *===========================================================================*/

#define FT4_NN     103      /* sync + data channel symbols (16 + 87)        */
#define FT4_NSPS   576      /* samples per symbol @ 12 kHz                  */
#define FT4_NMAX   72576    /* samples in iwave (21*3456, ~6.05 s window)   */

/*
 * Encode an FT4 message into 103 channel tones {0..3}.
 *   msg/msg_len : message text (<= 37 chars) and its valid length
 *   itone_out   : caller buffer of FT4_NN (103) ints
 *   nsym_out    : symbols written (103), or -1 on bad message
 */
void ft4_encode(const char *msg, int msg_len,
                int *itone_out /*[103]*/, int *nsym_out);

/*
 * Generate the full-length real FT4 audio frame (FT4_NMAX samples) from tones,
 * exactly as ft4sim does (gen_ft4wave positions the shaped/ramped signal).
 *   itone     : nsym tones (103)
 *   nsym      : number of tones (103)
 *   fsample   : sample rate (Hz), e.g. 12000.0f
 *   f0        : audio carrier (Hz)
 *   wave_out  : caller buffer (capacity *nwave_out on input, >= FT4_NMAX)
 *   nwave_out : in = capacity; out = samples produced (FT4_NMAX), or -1
 */
void ft4_gen_wave(const int *itone, int nsym, float fsample, float f0,
                  float *wave_out, int *nwave_out);

/* One decode result from FT4 full-frame acquisition (same layout as
 * ft8_decode_t: 64 bytes). */
typedef struct {
    float sync;          /* sync metric                                  */
    int   snr;           /* SNR estimate, dB (rounded)                   */
    float dt;            /* time offset, seconds                         */
    float freq;          /* audio frequency, Hz                          */
    char  message[38];   /* NUL-terminated decoded message text          */
    int   nap;           /* AP type used (iaptype; 0 = none)             */
    float qual;          /* decode quality metric [0,1]                  */
} ft4_decode_t;

/*
 * Decode EVERY FT4 signal in a complete frame.
 *   iwave         : FT4_NMAX (72576) int16 audio samples @ 12 kHz
 *   nfa, nfb      : frequency search band edges (Hz)
 *   ndepth        : 1..3 (3 = full bp+osd; <=0 defaults to 3)
 *   mycall/hiscall: NUL/space-terminated callsigns for AP (may be "")
 *   nqso_progress : QSO progress index (AP pass schedule)
 *   nfqso         : QSO/RX audio freq (Hz) being worked (WSJT-X nfqso); the deep
 *                   AP passes center on it. 0 / out of [nfa,nfb] = band mid
 *   out           : caller array of ft4_decode_t (capacity max_out)
 *   max_out       : capacity of out
 *
 * Returns the number of decodes found (>= 0), or -1 on error. Up to
 * min(found, max_out) entries are written. NOT thread-safe / not reentrant.
 */
int ft4_decode_frame(const int16_t *iwave /*[FT4_NMAX]*/,
                     int nfa, int nfb, int ndepth,
                     const char *mycall, const char *hiscall,
                     int nqso_progress, int nfqso,
                     ft4_decode_t *out, int max_out);

/*===========================================================================
 * FST4: WSJT-X slow weak-signal mode. DECODE ONLY.
 *===========================================================================*/

/* ⭐ ALL 7 T/R periods, and BOTH modes (FST4 + FST4W). ntrperiod and iwspr are
 * ARGUMENTS. fst4_decode sizes everything from ntrperiod:
 *     period (s)     15      30      60     120      300       900      1800
 *     samples    180000  360000  720000 1440000  3600000  10800000  21600000
 * The caller supplies ntrperiod*12000 samples; the routine reads nfft1, which the
 * table keeps <= that at every period. FST4_NMAX_MAX is the ceiling, NOT the
 * contract.
 *
 * iwspr=0 is FST4 (QSO mode, 77-bit messages); iwspr=1 is FST4W, the WSPR-like
 * BEACON mode (50-bit messages, no AP decoding). FST4W is why the period had to
 * become an argument: its standard beacon intervals are 120/300/900/1800 s, so a
 * wrapper pinned to 15 s could not do FST4W in any useful form.
 *
 * ⚠️ FST4W HASHED CALLSIGNS DO NOT RESOLVE. The k50 lookup table is populated
 * upstream from fst4w_calls.txt, a GUI-side file the headless build removed. With
 * an empty table the lookup reports the `<...>` hash form — the same result an
 * empty file produced upstream. Beacon reception, SNR and grid all work; only
 * resolving a previously-heard hashed call is missing.
 *
 * An unsupported period or iwspr returns -1 rather than being clamped. */
#define FST4_NMAX_MAX 21600000  /* ceiling: 1800 s @ 12 kHz                     */
#define FST4_NPERIODS 7         /* {15, 30, 60, 120, 300, 900, 1800}            */

/* NO fst4_encode / fst4_gen_wave, deliberately. FST4 ships receive-only: the
 * Rust ModeKind reports Capabilities{tx:false} and modes::tx_mode() refuses to
 * hand it to the transmit path. Adding TX means adding those two entry points,
 * flipping that flag, AND passing the FT-mode TX approval gate. */

/* One decode result from FST4 acquisition (same 64-byte layout as
 * ft8_decode_t / ft4_decode_t). */
typedef struct {
    float sync;          /* sync metric                                  */
    int   snr;           /* SNR estimate, dB (rounded)                   */
    float dt;            /* time offset, seconds                         */
    float freq;          /* audio frequency, Hz                          */
    char  message[38];   /* NUL-terminated decoded message text          */
    int   nap;           /* AP type used (iaptype; 0 = none)             */
    float qual;          /* decode quality metric [0,1]                  */
} fst4_decode_t;

/*
 * Decode EVERY FST4/FST4W signal in one complete T/R period.
 *   iwave         : ntrperiod*12000 int16 audio samples @ 12 kHz
 *   ntrperiod     : 15|30|60|120|300|900|1800 (s). Anything else returns -1.
 *   iwspr         : 0 = FST4 (QSO), 1 = FST4W (beacon). Anything else returns -1.
 *   nfa, nfb      : frequency search band edges (Hz)
 *   ndepth        : 1..3 (3 = full bp+osd; <=0 defaults to 3)
 *   mycall/hiscall: NUL/space-terminated callsigns for AP (may be "")
 *   nqso_progress : QSO progress index (AP pass schedule)
 *   nfqso         : QSO/RX audio freq (Hz) being worked; the deep AP passes
 *                   center on it. 0 / out of [nfa,nfb] = band mid
 *   out           : caller array of fst4_decode_t (capacity max_out)
 *   max_out       : capacity of out
 *
 * Returns the number of decodes found (>= 0), or -1 on error. Up to
 * min(found, max_out) entries are written. NOT thread-safe / not reentrant.
 */
int fst4_decode_frame(const int16_t *iwave /*[ntrperiod*12000]*/,
                      int ntrperiod, int iwspr,
                      int nfa, int nfb, int ndepth,
                      const char *mycall, const char *hiscall,
                      int nqso_progress, int nfqso,
                      fst4_decode_t *out, int max_out);

/*===========================================================================
 * JT65: the classic WSJT weak-signal / EME mode. DECODE ONLY.
 *===========================================================================*/

/* 65-tone MFSK, one tone per 372 ms, carrying a 72-bit message through a
 * (63,12) Reed-Solomon code. It predates the 77-bit era, which shows in two
 * places a caller must know about:
 *
 *   * MESSAGES ARE 22 CHARACTERS, not 37 — the legacy packjt layer, not
 *     packjt77. The shared 38-byte message field is simply never filled past 22.
 *   * SUBMODES ARE A/B/C, passed as 0/1/2. The decoder squares them
 *     (mode65 = 2**nsubmode), giving 1x/2x/4x tone spacing; wider survives more
 *     Doppler spread, which is why EME operators move up the letters as the path
 *     degrades.
 *
 * ⭐ THE FRAME IS 60 s BUT ONLY 52 s ARE DECODED. dd0 is an explicit-shape dummy
 * at 60*12000 = 720000, so the caller must supply all of it — but upstream reads
 * only npts = 52*12000 = 624000, and this ABI does the same. The tail is buffer
 * the decoder never touches. This is the one case here where "supply more than is
 * read" is the correct contract rather than waste.
 *
 * ⭐ EVERY CALL IS INDEPENDENT. JT65 supports multi-period message averaging
 * (avg65); the ABI pins clearave so frame N is never influenced by frames
 * 1..N-1, for the same reason Q65 does.
 *
 * ⭐ NOT KVASD. JT65's historical Reed-Solomon decoder was the non-free KVASD
 * binary, invoked as a subprocess. This build uses ftrsd — the Franke-Taylor
 * soft-decision decoder written to replace it — and neither ships nor invokes
 * KVASD. The ftrsd RS codec is Phil Karn's under a separate GPL grant; see
 * NOTICE. */
#define JT65_NMAX      720000  /* buffer contract: 60 s @ 12 kHz               */
#define JT65_NPTS      624000  /* what the decoder actually reads (52 s)       */
#define JT65_NSUBMODES 3       /* A, B, C as 0, 1, 2                           */

/* NO jt65 encode / gen_wave, deliberately. Receive-only: the Rust ModeKind
 * reports Capabilities{tx:false} and modes::tx_mode() refuses the transmit path. */

/* One decode from JT65 acquisition. Byte-compatible with the other modes'
 * records (64 bytes, 4-byte aligned). */
typedef struct {
    float sync;          /* sync metric                                  */
    int   snr;           /* SNR estimate, dB (rounded)                   */
    float dt;            /* time offset, seconds                         */
    float freq;          /* audio frequency, Hz                          */
    char  message[38];   /* NUL-terminated; only 22 are ever used        */
    int   ft;            /* decode type: 1 = Reed-Solomon, 2 = deep search */
    int   qual;          /* deep-search confidence; 0 for an RS decode   */
} jt65_decode_t;

/*
 * Decode EVERY JT65 signal in one 60 s T/R period.
 *   iwave      : JT65_NMAX (720000) int16 audio samples @ 12 kHz — the full 60 s
 *   nsubmode   : 0, 1 or 2 for JT65A/B/C. Anything else returns -1.
 *   nfa, nfb   : frequency search band edges (Hz)
 *   ndepth     : 1..3; also scales the Reed-Solomon trial budget
 *   mycall/hiscall : NUL/space-terminated callsigns for AP (may be "")
 *   hisgrid    : NUL/space-terminated 6-char grid for AP (may be "")
 *   nfqso      : QSO/RX audio freq (Hz). 0 / out of [nfa,nfb] = band mid
 *   out        : caller array of jt65_decode_t (capacity max_out)
 *   max_out    : capacity of out
 *
 * Returns the number of decodes found (>= 0), or -1 on error. NOT thread-safe.
 */
int jt65_decode_frame(const int16_t *iwave /*[JT65_NMAX]*/,
                      int nsubmode,
                      int nfa, int nfb, int ndepth,
                      const char *mycall, const char *hiscall,
                      const char *hisgrid,
                      int nfqso,
                      jt65_decode_t *out, int max_out);

/*===========================================================================
 * MSK144: WSJT-X METEOR-SCATTER mode. DECODE ONLY.
 *===========================================================================*/

/* MSK144 sends 72 ms frames (864 samples @ 12 kHz) continuously through the T/R
 * period, so a single ionised meteor trail lasting a tenth of a second can carry
 * a whole message. Its decoder is shaped unlike the others here.
 *
 * ⭐ SLIDING WINDOW, NOT ONE-SHOT. `mskrtd` analyses one 7168-sample block per
 * call and is driven at half-block (3584-sample, ~0.3 s) steps across the period
 * — about 50 calls per 15 s. This ABI owns that slide internally, so the caller
 * still hands over one period and gets every decode back.
 *
 * ⭐ nutc IS THE PERIOD LABEL AND MUST DIFFER BETWEEN PERIODS. mskrtd dupe-checks
 * against the previous message and resets on `nutc00 != nutc0 || tsec < tsec0`.
 * Both disjuncts are live: tsec0 advances every call (a LABELLED assignment at
 * mskrtd.f90:238 that every exit path reaches), so the check also self-clears at
 * a period boundary when tsec restarts. nutc is still required — it is the UTC
 * field of the output line and the other half of the reset. Pass the period's
 * UTC, or any per-period-distinct value.
 *
 * T/R periods: 5, 10, 15 or 30 s. 15 s is the 6 m workhorse. The frame is
 * ntrperiod*12000 samples; an unsupported period returns -1 rather than being
 * clamped. */
#define MSK144_NMAX_MAX 360000  /* ceiling: 30 s @ 12 kHz                       */
#define MSK144_NPERIODS 4       /* {5, 10, 15, 30}                              */

/* NO msk144 encode / gen_wave, deliberately. Receive-only: the Rust ModeKind
 * reports Capabilities{tx:false} and modes::tx_mode() refuses the transmit path. */

/* One decode from MSK144 acquisition. Byte-compatible with the ft8/ft4/fst4/q65
 * records (64 bytes, 4-byte aligned). */
typedef struct {
    float sync;          /* ALWAYS 0.0 — mskrtd reports no sync metric   */
    int   snr;           /* SNR estimate, dB (rounded)                   */
    float dt;            /* time offset within the period, seconds       */
    float freq;          /* audio frequency, Hz                          */
    char  message[38];   /* NUL-terminated decoded message text          */
    int   dtype;         /* 0 = frame-averaged, 1 = '&' single-ping      */
                         /* (mskspd), 2 = '^' long average               */
    int   reserved;      /* unused, always 0                             */
} msk144_decode_t;

/*
 * Decode EVERY MSK144 signal in one complete T/R period.
 *   iwave      : ntrperiod*12000 int16 audio samples @ 12 kHz
 *   ntrperiod  : 5, 10, 15 or 30 (s). Anything else returns -1.
 *   nutc       : per-period label; MUST DIFFER between periods (see above)
 *   nfa, nfb   : search band edges (Hz). MSK144 searches a centre +/- tolerance
 *                rather than a range, so both are derived from what is asked for.
 *   ndepth     : 1..3 (3 = deepest; <=0 defaults to 3)
 *   mycall/hiscall : NUL/space-terminated callsigns (may be "")
 *   nfqso      : QSO/RX audio freq (Hz). 0 / out of [nfa,nfb] = band mid
 *   out        : caller array of msk144_decode_t (capacity max_out)
 *   max_out    : capacity of out
 *
 * Returns the number of decodes found (>= 0), or -1 on error. NOT thread-safe.
 *
 * Shorthand (MSK40) messages and SWL mode are OFF, matching WSJT-X's defaults;
 * phase-equalizer training is off and its .pcoeff dump was removed from the
 * vendored source.
 */
int msk144_decode_frame(const int16_t *iwave /*[ntrperiod*12000]*/,
                        int ntrperiod, int nutc,
                        int nfa, int nfb, int ndepth,
                        const char *mycall, const char *hiscall,
                        int nfqso,
                        msk144_decode_t *out, int max_out);

/*===========================================================================
 * Q65: WSJT-X weak-signal mode for EME/VHF+ and ionoscatter. DECODE ONLY.
 *===========================================================================*/

/* ⭐ ALL 5 T/R periods x ALL 5 submodes. ntrperiod and nsubmode are ARGUMENTS.
 *
 * THE FRAME LENGTH IS A FUNCTION OF THE PERIOD:
 *     period (s)     15      30      60      120       300
 *     samples    180000  360000  720000  1440000   3600000
 * The caller supplies ntrperiod*12000 samples for the period it asks for.
 * Q65_NMAX_MAX is the ceiling (300 s), NOT the contract — sizing every buffer at
 * the max wastes 20x on a 15 s decode.
 *
 * Q65-30A is not the mode's main use: EME on VHF/UHF runs Q65-60A/B/C, 6 m
 * meteor/ionoscatter is where 30 belongs, and 15 is troposcatter.
 *
 * An unsupported period or submode returns -1 rather than being clamped: the
 * modem would otherwise read a different span of iwave than the caller sized,
 * and a decode off the wrong window is a plausible wrong answer, not a crash. */
#define Q65_NMAX_MAX  3600000  /* ceiling: 300 s @ 12 kHz                      */
#define Q65_NPERIODS  5        /* {15, 30, 60, 120, 300}                       */
#define Q65_NSUBMODES 5        /* A..E, passed as 0..4                         */

/* NO q65_encode / gen_q65wave, deliberately. Q65 ships receive-only: the Rust
 * ModeKind reports Capabilities{tx:false} and modes::tx_mode() refuses to hand
 * it to the transmit path. Adding TX means adding those entry points, flipping
 * that flag, AND passing the FT-mode TX approval gate. */

/* One decode result from Q65 acquisition. Byte-compatible with ft8/ft4/
 * fst4_decode_t (64 bytes, 4-byte aligned), but the last two fields carry what
 * Q65 actually reports rather than FT8's nap/qual. */
typedef struct {
    float sync;          /* snr1: sync-curve correlation metric          */
    int   snr;           /* SNR estimate, dB in 2500 Hz (rounded)        */
    float dt;            /* time offset, seconds                         */
    float freq;          /* audio frequency, Hz                          */
    char  message[38];   /* NUL-terminated decoded message text          */
    int   idec;          /* decode type: 0=q0, 1=q1, 2=q2, 3=q3 list     */
    int   nused;         /* T/R periods averaged (always 1 — see below)  */
} q65_decode_t;

/*
 * Decode EVERY Q65 signal in one complete T/R period.
 *   iwave         : ntrperiod*12000 int16 audio samples @ 12 kHz
 *   ntrperiod     : 15, 30, 60, 120 or 300 (s). Anything else returns -1.
 *   nsubmode      : 0..4 for submodes A..E. Anything else returns -1.
 *   nfa, nfb      : frequency search band edges (Hz)
 *   ndepth        : 1..3 (3 = deepest; <=0 defaults to 3)
 *   mycall/hiscall: NUL/space-terminated callsigns for AP (may be "")
 *   hisgrid       : NUL/space-terminated 6-char grid for AP (may be "")
 *   nqso_progress : QSO progress index (AP pass schedule)
 *   nfqso         : QSO/RX audio freq (Hz) being worked; the deep AP passes
 *                   center on it. 0 / out of [nfa,nfb] = band mid
 *   out           : caller array of q65_decode_t (capacity max_out)
 *   max_out       : capacity of out
 *
 * Returns the number of decodes found (>= 0), or -1 on error. Up to
 * min(found, max_out) entries are written. NOT thread-safe / not reentrant.
 *
 * EVERY CALL IS INDEPENDENT. Q65 supports multi-period message averaging, but
 * this ABI pins lclearave=.true. so the averaging arrays are cleared at the top
 * of every decode and frame N is never influenced by frames 1..N-1. `nused` is
 * therefore always 1. Real averaging needs a stateful session API plus the
 * per-chain state swap the manifest specifies, not a flag flip.
 */
int q65_decode_frame(const int16_t *iwave /*[ntrperiod*12000]*/,
                     int ntrperiod, int nsubmode,
                     int nfa, int nfb, int ndepth,
                     const char *mycall, const char *hiscall,
                     const char *hisgrid,
                     int nqso_progress, int nfqso,
                     q65_decode_t *out, int max_out);

/*===========================================================================
 * DX1-S: non-coherent M-FSK + soft-LDPC robust tier (fading-resilient).
 *===========================================================================*/

/* DX1 transmit-waveform length (samples @ 12 kHz): chirp sync + 58 symbols. */
int dx1_frame_len(void);

/* DX1 receive capture-window length (samples): a full 15 s T/R slot. */
int dx1_capture_len(void);

/*
 * Encode text -> DX1 audio (chirp sync preamble + 8-FSK data).
 *   msg/msg_len : message text (<= 37 chars) and its length
 *   f0          : audio carrier (lower comb edge), Hz, e.g. 1500.0
 *   fsample     : sample rate, Hz, e.g. 12000.0
 *   wave_out    : caller buffer, capacity max_out >= dx1_frame_len()
 *   returns     : samples written (> 0), or -1 on pack failure / small buffer
 */
int dx1_encode_wave(const char *msg, int msg_len, float f0, float fsample,
                    float *wave_out, int max_out);

/*
 * Decode ONE known carrier (single-offset). The sync chirp is searched in time
 * over [idt_lo, idt_hi] and in frequency over only +-6.25 Hz of f0.
 *   returns : nharderr (< 0 => decode/CRC failed); msg_out NUL-filled on fail.
 */
int dx1_decode_buf(const float *wave, int nwave, float f0, float fsample,
                   int idt_lo, int idt_hi, char *msg_out, int msg_cap,
                   float *snr_out, float *sync_out);

/*
 * One decode from the DX1 full-passband scan.
 *
 * Layout (matches the Fortran bind(C) type dx1_decode_t; all fields naturally
 * 4-byte aligned, a 2-byte tail pad after message[38]):
 *   offset  size  field
 *      0      4    float freq      (resolved carrier, Hz)
 *      4      4    float sync      (chirp sync metric)
 *      8      4    int   snr       (SNR estimate, dB, rounded)
 *     12     38    char  message[38] (NUL-terminated)
 *     50      2    (padding to 4-byte boundary)
 *   total: 52 bytes, 4-byte aligned.
 */
typedef struct {
    float freq;          /* resolved carrier, Hz                         */
    float sync;          /* chirp sync metric                            */
    int   snr;           /* SNR estimate, dB (rounded)                   */
    char  message[38];   /* NUL-terminated decoded message text          */
} dx1_decode_t;

/*
 * Decode EVERY DX1 signal in the audio passband in one slot (full-band
 * acquisition, like ft1_decode_frame for FT1), vs dx1_decode_buf's single
 * carrier. Three stages: coarse chirp-correlation carrier scan on a 12.5 Hz
 * grid -> median-threshold peak-pick -> full decode per survivor (the CRC-14
 * inside the LDPC decoder rejects false peaks).
 *
 *   wave/nwave : audio samples @ fsample (one capture window)
 *   f_lo, f_hi : carrier (lower-comb-edge) scan range, Hz, e.g. 200 .. 2900
 *   fsample    : sample rate, Hz
 *   out        : caller array of dx1_decode_t (capacity max_out)
 *   max_out    : capacity of out (also caps decodes/slot)
 *
 * Returns the number of decodes found (>= 0); up to min(found, max_out) are
 * written to out.  NOT thread-safe / not reentrant.
 */
int dx1_decode_band(const float *wave, int nwave, float f_lo, float f_hi,
                    float fsample, dx1_decode_t *out, int max_out);

/*===========================================================================
 * PER-CHAIN DECODER CONTEXT (ft8_cabi.f90)
 *
 * The modem's decode state is process-global Fortran SAVE storage: the a7
 * cross-cycle replay table, the packjt77 callsign hash tables, the IR-HARQ
 * slot pool, the cached wideband spectra. Two radio chains decoding two bands
 * in ONE process share every byte of it, and the result is not a crash - it is
 * a CRC-valid, syntactically perfect, WRONG decode, logged and uploaded and
 * indistinguishable afterwards from a real QSO.
 *
 * A context is one chain's private copy of that state. Give each chain one and
 * wrap its decode:
 *
 *     void *ctx = malloc(tempo_ctx_size());   // 8-byte aligned or better
 *     tempo_ctx_reset(ctx);
 *     ...
 *     tempo_ctx_restore(ctx);                 // install this chain's state
 *     ft8_decode_frame(...);                  // (or ft4_/ft1_/dx1_)
 *     tempo_ctx_save(ctx);                    // capture it back
 *
 * The buffer is opaque: sized by tempo_ctx_size(), never inspected, only ever
 * handed back to these calls. Which symbols it carries is decided by
 * libtempo/modem-state-manifest.toml (the class-1 rows).
 *
 * NOT thread-safe. restore -> decode -> save must run under the SAME lock that
 * serializes every other modem call; another decode landing between the
 * restore and the save is exactly the corruption a context exists to prevent.
 *===========================================================================*/

/* Bytes one per-chain context needs. Allocate from THIS, never from a
 * hard-coded size: it is computed from the library's own declarations, so a
 * modem-source refresh that resizes a table cannot silently desync callers. */
size_t tempo_ctx_size(void);

/* Write the modem's load-time state into ctx (tempo_ctx_size() bytes). A fresh
 * context is NOT a zeroed one - the callsign tables are blank-filled and the
 * 22-bit hash index starts at -1 - so this, not memset, is how one is made.
 * Touches only the caller's buffer: reads and writes no modem state, no lock. */
void tempo_ctx_reset(void *ctx);

/* Copy the live modem state OUT into ctx. Caller holds the modem lock. */
void tempo_ctx_save(void *ctx);

/* Copy ctx IN over the live modem state. Caller holds the modem lock. */
void tempo_ctx_restore(void *ctx);

#ifdef __cplusplus
}
#endif

#endif /* LIBFT1_H */
