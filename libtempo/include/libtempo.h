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
 *   nftx          : TX audio freq (Hz) — WSJT-X nftx (mainwindow.cpp:3722). The
 *                   SECOND deep-AP window: ft8b.f90:305 skips a candidate only
 *                   when it is outside BOTH nfqso+-napwid AND nftx+-napwid, so
 *                   a station answering your CQ on YOUR tx frequency still gets
 *                   the deep AP masks. 0 / out of [nfa,nfb] = same as nfqso,
 *                   which is right whenever RX and TX are not split.
 *   nzhsym        : half-symbol count for this pass — WSJT-X nzhsym
 *                   (mainwindow.cpp:1877-1879). 50 = full 14.4 s frame; 41 =
 *                   the 11.808 s EARLY partial pass, which upstream runs
 *                   deliberately cheap: sync threshold 2.0 instead of 1.3
 *                   (ft8_decode.f90:178) and the AP passes 5-8 OFF
 *                   (ft8b.f90:275). It does NOT change how much audio is read —
 *                   always FT8_NMAX. Any value other than 41 is treated as 50.
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
 *   lft8apon      : nonzero = a-priori decoding ON (WSJT-X "Enable AP"): ft8b's
 *                   AP passes 5-8 run and the a7 replay is allowed. 0 = AP off:
 *                   regular passes only AND the a7 replay is skipped (both
 *                   consumers of ft8b's lft8apon; a7 slot bookkeeping still
 *                   runs so re-enabling AP next slot has a live table).
 *   lapcqonly     : nonzero = restrict AP to the CQ hypothesis (ft8b lapcqonly:
 *                   one AP pass, iaptype=1). WSJT-X derives this automatically
 *                   (>300 s without TX); here the caller decides. Ignored when
 *                   lft8apon = 0.
 *   out           : caller array of ft8_decode_t (capacity max_out)
 *   max_out       : capacity of out
 *
 * Returns the number of decodes found (>= 0), or -1 on error. Up to
 * min(found, max_out) entries are written. NOT thread-safe / not reentrant.
 */
int ft8_decode_frame(const int16_t *iwave /*[FT8_NMAX]*/,
                     int nfa, int nfb, int ndepth,
                     const char *mycall, const char *hiscall,
                     int nqso_progress, int nfqso, int nftx, int nzhsym,
                     int nutc, int la7final,
                     int lft8apon, int lapcqonly,
                     ft8_decode_t *out, int max_out);

/*
 * What ft8_decode_frame_cb / ft4_decode_frame_cb call while a pass runs. `rec` points at a copy
 * of a decode the pass has just accepted (the same record `out` holds), or is NULL after each FT8
 * sync candidate is tried. A nonzero answer to a NULL `rec` stops the pass there; the answer to a
 * decode is ignored. Runs on the decoding thread with the modem busy: it must not call back into
 * the modem.
 */
typedef int (*tempo_decode_cb)(void *user, const ft8_decode_t *rec);

/*
 * ft8_decode_frame, handing each decode to `cb` the moment the pass accepts it, as WSJT-X's
 * decoder hands each to its callback (ft8_decode.f90:233-238) for jt9 to write and flush at once.
 * The decodes come in the pass's own order: the sync candidates within 10 Hz of nfqso first
 * (sync8.f90:155-163), then by sync; the a7 replay's after the passes. After every sync candidate
 * of the direct passes `cb` is asked (with NULL) whether to stop, where WSJT-X asks its early
 * pass's give-up (ft8_decode.f90:240-242); stopping skips the remaining candidates, passes and the
 * a7 replay, and `out` holds what was found. `cb` NULL is exactly ft8_decode_frame.
 */
int ft8_decode_frame_cb(const int16_t *iwave /*[FT8_NMAX]*/,
                        int nfa, int nfb, int ndepth,
                        const char *mycall, const char *hiscall,
                        int nqso_progress, int nfqso, int nftx, int nzhsym,
                        int nutc, int la7final,
                        int lft8apon, int lapcqonly,
                        ft8_decode_t *out, int max_out,
                        tempo_decode_cb cb, void *user);

/*
 * What WSJT-X's FT8 decoder keeps of each early decode to subtract it from the full pass's audio
 * (ft8_decode.f90 itone_save/f1_save/xdt_save/lsubtracted). Matches the Fortran bind(C) type
 * ft8_sub_t: 332 bytes.
 */
typedef struct {
    int   itone[79]; /* the decode's channel tones */
    float f1;        /* carrier, Hz */
    float xdt0;      /* ft8b's start time, s (the record's dt + 0.5) */
    int   ok;        /* 1 = a direct decode, tones known; 0 = not subtractable (a7) */
    int   done;      /* 1 = already subtracted from the residual */
} ft8_sub_t;

/*
 * ft8_decode_frame_cb plus WSJT-X's carry between the passes of one period. `subs` (NULL, or
 * max_out records) receives each decode's ft8_sub_t. With 1..200 `nearly` early decodes, `early`,
 * `early_subs` and `resid` non-NULL, nzhsym 50 and depth >= 2, the pass starts from `resid` (the
 * 47 step's residual) over the first 47*3456 samples, subtracts the early decodes not yet
 * subtracted, and dedupes against them without handing them to `cb` (ft8_decode.f90:121,
 * :152-162); the early records open `out`. Anything else is ft8_decode_frame_cb. The caller must
 * only carry a residual made from the same first 47*3456 samples as `iwave`.
 *
 * la7final 2 is the rescue pass after it: the raw audio again, `early` holding the 1..200
 * (<= max_out) records already reported and `early_subs`/`resid` NULL. They open `out`, are
 * deduped against, never handed to `cb` and never saved into the a7 table again (one found again
 * gives its entry this pass's dt and frequency); new decodes are saved once and the a7 replay
 * runs. Without such records, 2 is 1.
 */
int ft8_decode_frame_dp(const int16_t *iwave /*[FT8_NMAX]*/,
                        int nfa, int nfb, int ndepth,
                        const char *mycall, const char *hiscall,
                        int nqso_progress, int nfqso, int nftx, int nzhsym,
                        int nutc, int la7final,
                        int lft8apon, int lapcqonly,
                        ft8_decode_t *out, int max_out,
                        tempo_decode_cb cb, void *user,
                        ft8_sub_t *subs, int nearly,
                        const ft8_decode_t *early, const ft8_sub_t *early_subs,
                        const float *resid /*[FT8_NMAX]*/);

/*
 * WSJT-X's FT8 47 step (ft8_decode.f90:124-149): from the first 47*3456 samples of `iwave` (the
 * rest taken as zero), subtract each early decode with dt < 0.396 s (DT-refined at depth 3), set
 * its `done`, and write the residual to `resid`. `cb` (may be NULL) is asked with NULL after each
 * early decode; nonzero stops there (the 14.3 s bail-out). Returns the decodes reached, or -1 when
 * `nearly` is not 1..200.
 */
int ft8_subtract_early(const int16_t *iwave /*[FT8_NMAX]*/, int ndepth, int nearly,
                       ft8_sub_t *early_subs, float *resid /*[FT8_NMAX]*/,
                       tempo_decode_cb cb, void *user);

/*
 * Clear the a7 cross-cycle decode table (prior-slot call pairs + slot tracker).
 * Call on band change / QSO change so stale prior-cycle pairs are not replayed
 * as AP hypotheses against the new band's audio. Mirrors ft1_harq_reset.
 */
void ft8_a7_reset(void);

/* How many decodes the slot decoded last has saved into the a7 table. Read only. */
int ft8_a7_saved(void);

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
 *   lapcqonly     : nonzero = restrict AP to the CQ hypothesis (the vendored
 *                   ft4_decoder's lapcqonly: iaptype forced 1, one AP pass).
 *                   NOTE: the vendored FT4 decoder has NO AP on/off flag — AP
 *                   runs whenever ndepth > 1; CQ-only is FT4's only
 *                   decoder-honest AP restriction.
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
                     int lapcqonly,
                     ft4_decode_t *out, int max_out);

/*
 * ft4_decode_frame, handing each decode to `cb` the moment the decoder finds it, as WSJT-X's FT4
 * decoder hands each to its callback (ft4_decode.f90:456). The candidates within 20 Hz of nfqso
 * are tried first (getcandidates4.f90:70-81). `cb` is never called with NULL (FT4 has no
 * give-up) and its answer is ignored; ft4_decode_t has ft8_decode_t's layout. `cb` NULL is
 * exactly ft4_decode_frame.
 */
int ft4_decode_frame_cb(const int16_t *iwave /*[FT4_NMAX]*/,
                        int nfa, int nfb, int ndepth,
                        const char *mycall, const char *hiscall,
                        int nqso_progress, int nfqso,
                        int lapcqonly,
                        ft4_decode_t *out, int max_out,
                        tempo_decode_cb cb, void *user);

/*===========================================================================
 * FST4: WSJT-X slow weak-signal mode. Decode, and transmit (see below).
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

/* FST4 and FST4W transmit: the Rust ModeKind reports Capabilities{tx:true}. The
 * two TX entry points, fst4_encode_msg and fst4_gen_wave (fst4_cabi.f90), are
 * declared in crates/tempo-fast-sys/src/lib.rs, not prototyped here.
 * fst4_gen_wave takes fsample 12000 only and writes exactly 160*nsps samples. */

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
 * WSPR: the propagation-beacon mode. DECODE ONLY.
 *===========================================================================*/

/* 50-bit messages ("CALL GRID DBM"), K=32 r=1/2 convolutional coding, a 110.6 s
 * transmission inside a 2-minute window at ~1.4 baud. Beacons, not QSOs — the
 * point is to hear who is propagating where.
 *
 * ⭐ THIS DECODER WAS A PROGRAM. WSJT-X has no library-shaped WSPR decoder; it
 * runs the `wsprd` EXECUTABLE as a subprocess. Nexus converted main() into
 * wspr_decode_core() — see the MODIFIED FOR NEXUS blocks in wsprd.c for every
 * piece of the program shell that was removed.
 *
 * ⭐ SERIALIZE. fftwf_plan_dft_* is the FFTW PLANNER, which is not thread-safe,
 * and this decoder plans three transforms per call against freshly-allocated
 * buffers. Two concurrent decodes corrupt FFTW's internal plan list. The Rust
 * wrapper holds MODEM_LOCK for exactly this reason; a caller reaching this
 * symbol by another route must serialize it itself.
 *
 * ⭐ THE HASHED-CALLSIGN TABLE IS OFF (upstream's own -H). Type-2 and type-3
 * messages therefore report the <...> hash form rather than resolving a
 * previously-heard callsign — the same limitation FST4W has here, and for the
 * same reason: the table persisted to a file on disk, which is a path for one
 * radio chain's callsigns to reach another's decoder.
 *
 * The frame is 114 s of the 120 s window: WSPR_NMAX = 114*12000 = 1368000. A
 * short buffer is zero-padded rather than refused. */
#define WSPR_NMAX     1368000  /* samples the decoder reads (114 s @ 12 kHz)   */
#define WSPR_PERIOD_S 120      /* the reception interval is 2 minutes          */

/* NO wspr encode / gen_wave. Receive-only: the Rust ModeKind reports
 * Capabilities{tx:false} and modes::tx_mode() refuses the transmit path. */

/* One decode from WSPR acquisition.
 *
 * Deliberately NOT the 64-byte record the FT8-family modes share: WSPR reports
 * an ABSOLUTE frequency in MHz as a double (not an audio offset in Hz), carries
 * a drift term no other mode here has, and its message is 22 characters from its
 * own 50-bit layer. Forcing it into the shared shape would have meant
 * misrepresenting at least two of those fields. */
typedef struct {
    double freq;        /* absolute RF frequency, MHz (dial + audio offset)   */
    float  sync;        /* sync quality                                      */
    float  snr;         /* SNR estimate, dB in 2500 Hz                       */
    float  dt;          /* time offset, seconds                              */
    float  drift;       /* frequency drift, Hz/minute — WSPR-specific        */
    char   message[23]; /* NUL-terminated "CALL GRID DBM"; 22 chars + NUL     */
    int    decodetype;  /* 0 = type 1, 1 = type 2, 2 = type 3                */
} wspr_decode_t;

/*
 * Decode every WSPR signal in one 2-minute reception interval.
 *   iwave       : WSPR_NMAX int16 samples @ 12 kHz (short = zero-padded)
 *   nsamples    : how many of them are real
 *   dialfreq    : rig dial frequency in MHz — reported freq is dial + offset
 *   quickmode   : 1 = do not dig deep for weak signals (upstream -q)
 *   npasses     : subtraction passes; upstream default 2, 1 = -B
 *   subtraction : 0 disables signal subtraction between passes (-s)
 *   more_candidates : upstream -d, a deeper candidate list
 *   stackdecoder    : upstream -J, the Jelinek stack decoder instead of Fano
 *   out, max_out    : caller's array
 *
 * Returns the number of decodes written (>= 0), or -1 on error. NOT thread-safe;
 * see the serialization note above.
 */
int wspr_decode_core(const short *iwave, long nsamples, double dialfreq,
                     int quickmode, int npasses, int subtraction,
                     int more_candidates, int stackdecoder,
                     wspr_decode_t *out, int max_out);

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
 * FT2: Decodium's fast slotted mode. TRANSMIT AND RECEIVE.
 *===========================================================================*/

/* ⭐ DECODIUM's FT2 (IU8LMC, decodium3-build), NOT upstream WSJT-X's abandoned
 * experiment of the same name — the two are incompatible. FT4 with a halved
 * symbol time: LDPC(174,91)+CRC14, 4-GFSK at 41.67 baud (~167 Hz occupied),
 * 105 channel symbols = 2.52 s of audio in a 3.75 s T/R period, -10.8 to
 * -12 dB. The on-air FT2 population runs Decodium, so Decodium is the
 * compatibility baseline. See NOTICE for provenance.
 *
 * The decode entry point wraps ft2_triggered_decode — the flat text-lines
 * subroutine Decodium's own runtime calls — over one fixed 45000-sample
 * (3.75 s @ 12 kHz) window. One call per period; no slide, no period label. */
#define FT2_NMAX  45000   /* decode window: 3.75 s @ 12 kHz                    */
#define FT2_NN    103     /* sync+data channel symbols (encode output)         */
#define FT2_NN2   105     /* + ramp-up/ramp-down symbols, what hits the air    */
#define FT2_NWAVE 30240   /* FT2_NN2 * 288 samples = 2.52 s @ 12 kHz           */

/* One FT2 decode. Byte-compatible with the ft8/ft4/msk144/q65 records
 * (64 bytes, 4-byte aligned). */
typedef struct {
    float sync;          /* ALWAYS 0.0 — the FT2 line has no sync metric */
    int   snr;           /* SNR estimate, dB                             */
    float dt;            /* time offset, seconds (about -1.0 .. +1.0)    */
    float freq;          /* audio frequency, Hz                          */
    char  message[38];   /* NUL-terminated decoded message text          */
    int   dtype;         /* 0 = ordinary decode, 1..4 = AP decode type n */
    int   reserved;      /* unused, always 0                             */
} ft2_decode_t;

/*
 * Decode every FT2 signal in one 3.75 s window.
 *   iwave      : FT2_NMAX int16 samples @ 12 kHz — always the full window
 *   nfa, nfb   : candidate search band edges (Hz)
 *   ndepth     : 1..3 (3 = deepest, loosens sync/OSD thresholds; <=0 -> 3)
 *   mycall/hiscall : NUL/space-terminated callsigns (may be ""); empty
 *                    hiscall disables the AP types that need the DX call
 *   nfqso      : QSO audio frequency (Hz) — candidate ordering follows it
 *   out        : caller array of ft2_decode_t (capacity max_out)
 *   max_out    : capacity of out
 * Returns the number of decodes found (>= 0). NOT thread-safe.
 */
int ft2_decode_frame(const int16_t *iwave /*[FT2_NMAX]*/,
                     int nfa, int nfb, int ndepth,
                     const char *mycall, const char *hiscall,
                     int nfqso,
                     ft2_decode_t *out, int max_out);

/*
 * Encode a message: text -> the FT2_NN channel tones (0..3).
 * Returns FT2_NN on success, -1 if the message will not pack.
 */
int ft2_encode_msg(const char *msg, int msg_len, int *itone_out /*[FT2_NN]*/);

/*
 * Channel tones -> real audio at 12 kHz. Writes FT2_NWAVE samples — the
 * generator appends the raised-cosine ramp-up/down symbols itself, which is
 * the FT2_NN vs FT2_NN2 distinction. nsym must be FT2_NN; nwave_cap must be
 * >= FT2_NWAVE. Returns FT2_NWAVE, or -1 on refusal.
 */
int ft2_gen_wave(const int *itone, int nsym, float f0,
                 float *wave_out, int nwave_cap);

/*===========================================================================
 * JTTY: WSJT-X's untimed keyboard mode. ENCODE, WAVEFORM AND RECEIVE ONLY:
 * nothing here keys a transmitter. Not offered in any release: the modem is
 * from a WSJT-X pre-release (see NOTICE).
 *===========================================================================*/

/* 4-GFSK at 31.25 baud, 59-symbol frames (13 sync + 46 code, 1.888 s), each one
 * 34-bit atom under a CRC-12 and a K=10 tail-biting convolutional code; 1..16
 * frames per message, no slot clock. jtty_cabi.f90 is Nexus's marshalling over
 * upstream's genjtty/gen_jttywave; the jtty_rx_* functions further down are
 * upstream's own C ABI (jtty_receive_context.f90), used as they are. */
#define JTTY_NSPS          384    /* samples/symbol at 12 kHz: 31.25 baud     */
#define JTTY_FRAME_SYMBOLS 59     /* 13 sync + 46 code symbols                */
#define JTTY_MAX_FRAMES    16
#define JTTY_MAX_TONES     944    /* JTTY_FRAME_SYMBOLS * JTTY_MAX_FRAMES     */
#define JTTY_TEXT_LEN      80     /* longest message text, bytes              */
#define JTTY_ERR_ARGS      (-1)   /* a length, count, tone or capacity refused */
#define JTTY_ERR_ENCODE    (-2)   /* the modem cannot encode the text         */

/* One receive update. 120 bytes, 8-byte aligned, no padding. Times are seconds
 * on the caller's sample timeline (sample index / 12000). */
typedef struct {
    int64_t message_id;  /* stable across a message's growing updates       */
    double  start_s;     /* the message's first frame                        */
    double  latest_s;    /* its latest frame                                 */
    float   freq;        /* audio frequency of tone 0, Hz                    */
    int     terminal;    /* 0 growing, 1 complete, 2 expired, 3 rx ended     */
    int     snr;         /* dB, the first frame                              */
    char    text[84];    /* the text so far, NUL-filled                      */
} jtty_update_t;

/* Upstream's structured-atom descriptor (jtty_source_codec.f90), 20 bytes. */
typedef struct {
    int8_t  kind;        /* 0 call, 1 exch number, 2 exch location,
                            3 class+section, 5 control, 6 grid              */
    int8_t  subtype;
    int8_t  role;
    int8_t  reserved;    /* must be 0                                        */
    int32_t value;
    char    text[9];     /* up to 8 bytes + NUL                              */
} jtty_source_atom_c;

/*
 * Text -> channel tones (0..3). profile: 0 unknown, 1 Field Day, 2 RTTY
 * Roundup. is_final: 1 sets end-of-message on the last frame, 0 leaves it
 * clear (a segment still being typed); anything else is refused. sent
 * (capacity >= JTTY_TEXT_LEN+1) receives the normalised text. Returns the tone
 * count (whole frames), 0 for an empty message, JTTY_ERR_ARGS (nothing
 * written) or JTTY_ERR_ENCODE. NOT thread-safe.
 */
int jtty_encode_text(const char *msg, int msg_len, int profile, int is_final,
                     int *itone_out, int max_tones, char *sent, int sent_cap);

/*
 * 1..16 atoms -> channel tones, one frame each. status: 0 ok, 1 invalid
 * descriptor, 2 unknown ARRL/RAC section. Returns the tone count, 0 when the
 * atoms are refused, JTTY_ERR_ARGS when max_tones is too small.
 */
int jtty_encode_atoms(const jtty_source_atom_c *atoms, int natoms,
                      int *itone_out, int max_tones, int *status);

/*
 * Text -> one 34-bit source word per frame: what jtty_encode_text sends,
 * before the CRC and the code. Source bit 1 is the most significant of the 34,
 * bit 33 reserved (0), bit 34 end-of-message, set on the last frame only when
 * is_final is 1 (0 leaves it clear; anything else is refused). Returns the
 * frame count, 0 for an empty message, JTTY_ERR_ARGS (nothing written) or
 * JTTY_ERR_ENCODE, as jtty_encode_text.
 */
int jtty_pack_text(const char *msg, int msg_len, int profile, int is_final,
                   int64_t *frames_out, int max_frames, char *sent, int sent_cap);

/*
 * 1..16 source words -> the text as the receiver shows it (text_cap >=
 * JTTY_TEXT_LEN+1, NUL-terminated). A word the receiver would discard
 * (reserved bit, unassigned form, field out of range) is left out. eom: 1 when
 * every word is valid and the last carries end-of-message. Returns 0 when every
 * word is valid, 1 when one or more are not, JTTY_ERR_ARGS for a refused count,
 * word (2**34 or more) or capacity (nothing written).
 */
int jtty_unpack_frames(const int64_t *frames, int nframes, char *text_out,
                       int text_cap, int *eom);

/*
 * Tones -> real audio at 12 kHz, amplitude 1: nsym*JTTY_NSPS samples. nsym
 * must be whole frames up to JTTY_MAX_TONES, every tone 0..3, nwave_cap at
 * least nsym*JTTY_NSPS. Returns the sample count or JTTY_ERR_ARGS.
 */
int jtty_gen_wave(const int *itone, int nsym, float f0,
                  float *wave_out, int nwave_cap);

/* Drain up to max_out pending updates of a receive handle; returns how many. */
int jtty_take_updates(int handle, jtty_update_t *out, int max_out);

/* Upstream's receive API. Eight contexts; each keeps its own reception state
 * and upstream swaps it in around every call, but the DSP scratch is shared:
 * call every one of these under the same modem lock. */
int32_t jtty_rx_create(void);           /* 1..8, or 0 when all are taken     */
void    jtty_rx_destroy(int32_t handle);
void    jtty_rx_begin(int32_t handle, int64_t session_id, int64_t grid_origin,
                      int64_t first_search, int32_t nsps /* 240/320/384/480 */);
/* Up to max_steps quarter-frame searches over pcm[0..count), whose first
 * element is sample first_sample, never at or past stop_sample. Searches
 * center +- tolerance always, and 1200-1500 / 1500-1800 Hz where they fall
 * inside low..high. Returns the steps run (0: more audio needed), -1 for an
 * invalid or stopped handle, -2 when pcm does not reach back far enough. */
int32_t jtty_rx_process(int32_t handle, const int16_t *pcm, int32_t count,
                        int64_t first_sample, int64_t stop_sample,
                        int32_t max_steps, int32_t low, int32_t high,
                        float center, float tolerance);
int64_t jtty_rx_next_required_sample(int32_t handle);
int64_t jtty_rx_next_search_sample(int32_t handle);
void    jtty_rx_end(int32_t handle, int32_t reason /* 2 expired, 3 ended */);

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
