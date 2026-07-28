cmake_minimum_required(VERSION 3.20)

# Tempo: standalone FT1 modem library (decoupled from Qt / WSJT-X).
# Compiles ONLY the FT1 4-CPM turbo modem + minimal shared infra into
# static + shared libtempo, linking FFTW3 single precision. NO Qt, NO GUI.
project(libtempo LANGUAGES Fortran C CXX)

# ---------------------------------------------------------------------------
# WSJT-X-derived modem source. The complete corresponding source (GPL-3.0 §6)
# is vendored in-tree at libtempo/vendor/wsjtx/ and used by DEFAULT, so a fresh
# clone builds the modem with no external checkout. See vendor/wsjtx/README.md
# for provenance and license.
#
# To build against a different WSJT-X tree instead, override at configure time:
#     -DWX=/path/to/wsjtx-source        (its lib/ must hold the sources)
# ---------------------------------------------------------------------------
set(WX "${CMAKE_CURRENT_SOURCE_DIR}/vendor/wsjtx" CACHE PATH "WSJT-X-derived modem source tree (contains lib/)")
if(NOT IS_DIRECTORY "${WX}/lib")
    message(FATAL_ERROR
        "WSJT-X modem source not found at ${WX}/lib. The vendored copy lives at "
        "libtempo/vendor/wsjtx/; override with -DWX=/path/to/wsjtx-source if needed.")
endif()
set(LIB "${WX}/lib")

# ---------------------------------------------------------------------------
# FFTW3 single precision (used by four2a.f90 via the fftw3 module / fftw3.f03).
#
# Native (host == target): locate fftw3f via pkg-config, Boost via find_package.
# Cross-compiling to Windows from a non-Windows host (CMAKE_CROSSCOMPILING set by
# the MinGW toolchain file): pkg-config / find_package would pick up HOST libs,
# so instead point at the statically cross-built FFTW3f at FFTW_MINGW_PREFIX and
# use the host's header-only Boost (crc.hpp is header-only, arch-independent).
# ---------------------------------------------------------------------------
if(CMAKE_CROSSCOMPILING AND CMAKE_SYSTEM_NAME STREQUAL "Windows")
    # Cross build: use the MinGW static FFTW3f from the toolchain file.
    if(NOT DEFINED FFTW_MINGW_PREFIX)
        set(FFTW_MINGW_PREFIX "/tmp/fftw-mingw")
    endif()
    if(NOT EXISTS "${FFTW_MINGW_PREFIX}/lib/libfftw3f.a")
        message(FATAL_ERROR
            "Cross build needs MinGW FFTW3f at ${FFTW_MINGW_PREFIX}/lib/libfftw3f.a "
            "(build it with ./configure --host=x86_64-w64-mingw32 --enable-float "
            "--enable-static --disable-shared --prefix=${FFTW_MINGW_PREFIX}).")
    endif()
    set(FFTW3F_INCLUDE_DIRS "${FFTW_MINGW_PREFIX}/include")
    set(FFTW3F_LIBRARY_DIRS "${FFTW_MINGW_PREFIX}/lib")
    set(FFTW3F_LIBRARIES    "${FFTW_MINGW_PREFIX}/lib/libfftw3f.a")
    # Header-only Boost CRC: use the host include tree (no compiled Boost needed).
    add_library(boost_crc_headeronly INTERFACE)
    set(BOOST_CRC_TARGET boost_crc_headeronly)
else()
    find_package(PkgConfig REQUIRED)
    pkg_check_modules(FFTW3F REQUIRED fftw3f)
    # Boost provides <boost/crc.hpp> for crc14.cpp (header-only).
    find_package(Boost REQUIRED)
    set(BOOST_CRC_TARGET Boost::boost)
endif()

# ---------------------------------------------------------------------------
# Fortran build flags.
#  -fallow-argument-mismatch : legacy WSJT-X code passes mismatched ranks/types
#                              (e.g. four2a equivalence). gfortran 10+ requires
#                              this to downgrade to a warning.
# ---------------------------------------------------------------------------
set(CMAKE_Fortran_FLAGS "${CMAKE_Fortran_FLAGS} -fallow-argument-mismatch -ffree-line-length-none")

# Generated .mod files go in a per-target private dir (set below). CMake's
# built-in Fortran module dependency scanner resolves module `use` ordering
# automatically. Per-target dirs avoid a parallel-build race between the
# static and shared library targets writing the same .mod files.

# ---------------------------------------------------------------------------
# Minimal source list. Add a source only when the compiler/linker demands it.
# ---------------------------------------------------------------------------
set(LIBTEMPO_SOURCES
    # --- C ABI wrapper (this project) ---
    ${CMAKE_CURRENT_SOURCE_DIR}/tempofast_cabi.f90

    # --- DX1-S non-coherent M-FSK mode (this project) ---
    #     params first (defines module dx1_params used by the rest).
    ${CMAKE_CURRENT_SOURCE_DIR}/tempodeep/tempodeep_params.f90    # module dx1_params
    ${CMAKE_CURRENT_SOURCE_DIR}/tempodeep/gen_tempodeepwave.f90   # dx1_encode_msg, gen_dx1wave
    ${CMAKE_CURRENT_SOURCE_DIR}/tempodeep/tempodeep_detect.f90    # dx1_detect (1920-pt FFT)
    ${CMAKE_CURRENT_SOURCE_DIR}/tempodeep/tempodeep_llr.f90       # dx1_llr (log-sum-exp)
    ${CMAKE_CURRENT_SOURCE_DIR}/tempodeep/tempodeep_sync.f90      # dx1_sync (chirp correlate)
    ${CMAKE_CURRENT_SOURCE_DIR}/tempodeep/tempodeep_decode.f90    # dx1_decode (single carrier)
    ${CMAKE_CURRENT_SOURCE_DIR}/tempodeep/tempodeep_decode_all.f90 # dx1_decode_all (full-band scan)

    # --- FT1 modem (lib/ft1) ---
    ${LIB}/tempofast/cpm_trellis.f90            # module cpm_trellis_mod
    ${LIB}/tempofast/matched_filter_bank.f90    # module matched_filter_bank_mod
    ${LIB}/tempofast/bcjr_cpm.f90
    ${LIB}/tempofast/tempofast_interleave.f90
    ${LIB}/tempofast/gen_tempofast.f90
    ${LIB}/tempofast/gen_tempofastwave.f90
    ${LIB}/tempofast/tempofast_downsample.f90
    ${LIB}/tempofast/turbo_decode_tempofast.f90       # also defines bpdecode174_91_beliefs,
                                          #   ft1_interleave_real,
                                          #   compute_branch_metrics_all,
                                          #   force_sync_metrics, ft1_joint_turbo_harq
    ${LIB}/tempofast/tempofast_demod_bcjr.f90         # ft1_demod_bcjr + ft1_bm_prep (per-frame
                                          #   branch-metric prep for joint turbo HARQ)
    ${LIB}/tempofast/tempofast_rv_detect.f90          # ft1_rv_detect (coherent RV discriminator)

    # --- FT1 full RX acquisition pipeline ---
    ${LIB}/tempofast_decode.f90                 # module ft1_decode: OO decoder; also
                                          #   defines sync1d_ft1, subtractft1,
                                          #   ft1_freq_est_acorr
    ${LIB}/tempofast/tempofast_sync.f90               # ft1_sync (Costas candidate search),
                                          #   ft1_baseline
    ${LIB}/tempofast/ir_harq_combine.f90        # module ir_harq_combine_mod (IR-HARQ)
    ${LIB}/tempofast/ldpc348_91.f90             # module ldpc348_91_mod (bpdecode_ext);
                                          #   includes ../ft8/ldpc_174_91_c_parity.f90
    ${LIB}/timer_module.f90               # module timer_module (timer => null_timer
                                          #   no-op). NOT timer_impl.f90 -- that
                                          #   pulls in iso_c_utilities + a C callback.

    # --- shared CPM pulse (lib/ft2) ---
    ${LIB}/ft2/gfsk_pulse.f90

    # --- FEC / FT8 shared (lib/ft8) ---
    ${LIB}/ft8/encode174_91.f90           # includes ldpc_174_91_c_generator.f90
    ${LIB}/ft8/encode174_91_nocrc.f90     # includes ldpc_174_91_c_generator.f90
    ${LIB}/ft8/osd174_91.f90
    ${LIB}/ft8/bpdecode174_91.f90         # standalone BP decoder (ldpc348_91
                                          #   bpdecode_ext fallback); includes
                                          #   ldpc_174_91_c_parity.f90
    ${LIB}/ft8/chkcrc14a.f90
    ${LIB}/ft8/get_crc14.f90

    # --- FT8 native decode (lib/ft8) : Nexus ---
    #     Full-frame decode via the core primitives (ft8apset -> sync8 -> ft8b),
    #     wrapped behind a C ABI in ft8_cabi.f90 (no nzhsym/shmem machinery; the
    #     a7 cross-cycle AP table IS wired in via ft8_a7.f90 below).
    ${CMAKE_CURRENT_SOURCE_DIR}/ft8_cabi.f90
    ${LIB}/ft8/ft8_a7.f90                 # module ft8_a7: cross-cycle AP table
                                          #   (ft8_a7_save + ft8_a7d replay)
    ${LIB}/ft8/genft8.f90                 # genft8 + entry get_ft8_tones_from_77bits
    ${LIB}/ft8/gen_ft8wave.f90            # tones -> waveform (also used by subtractft8)
    ${LIB}/ft8/ft8apset.f90               # a-priori symbol setup (calls stdcall)
    #     stdcall now comes from the REAL upstream source, qra/q65/q65_set_list.f90
    #     (listed in the Q65 block below), which defines both q65_set_list and
    #     stdcall. libtempo/ft8_stdcall.f90 was a standalone copy of that one
    #     routine, added when FT8 needed stdcall but pulling in q65_set_list.f90
    #     "would drag in the whole Q65 modem". Q65 is now in the build, so the copy
    #     became a duplicate definition and the link failed on it. Copy deleted.
    ${LIB}/ft8/sync8.f90                  # Costas candidate search (calls get_spectrum_baseline)
    ${LIB}/ft8/sync8d.f90                 # fine sync / symbol metrics (called by ft8b)
    ${LIB}/ft8/get_spectrum_baseline.f90  # calls baseline, nuttal_window, four2a
    ${LIB}/ft8/baseline.f90               # spectral baseline fit
    ${LIB}/ft8/ft8_downsample.f90         # dd -> complex baseband (called by ft8b)
    ${LIB}/ft8/ft8b.f90                   # per-candidate decode (+ internal subtract);
                                          #   normalizebmet is contained within
    ${LIB}/ft8/subtractft8.f90            # signal subtraction (calls gen_ft8wave, peakup)
    ${LIB}/ft8/decode174_91.f90           # bp+osd decode (called by ft8b)
    ${LIB}/ft8/twkfreq1.f90               # fine frequency tweak (called by ft8b + ft4)
    ${LIB}/peakup.f90                     # parabolic peak interpolation (subtractft8)

    # --- FT4 native decode (lib/ft4 + ft4_decode.f90) : Nexus ---
    #     Clean OO decoder (getcandidates4 -> sync4d -> get_ft4_bitmetrics ->
    #     decode174_91 -> subtractft4), wrapped behind a C ABI in ft4_cabi.f90.
    ${CMAKE_CURRENT_SOURCE_DIR}/ft4_cabi.f90
    ${LIB}/ft4_decode.f90                 # module ft4_decode (OO decoder)
    ${LIB}/ft4/genft4.f90                 # genft4 + entry get_ft4_tones_from_77bits
    ${LIB}/ft4/gen_ft4wave.f90            # tones -> waveform (also used by subtractft4)
    ${LIB}/ft4/getcandidates4.f90         # candidate search (calls ft4_baseline)
    ${LIB}/ft4/ft4_baseline.f90           # spectral baseline
    ${LIB}/ft4/ft4_downsample.f90         # dd -> complex baseband
    ${LIB}/ft4/sync4d.f90                 # sync power metric
    ${LIB}/ft4/get_ft4_bitmetrics.f90     # bit metrics (calls normalizebmet from ft8b)
    ${LIB}/ft4/subtractft4.f90            # signal subtraction (calls gen_ft4wave)

    # --- FST4 native decode (lib/fst4 + fst4_decode.f90) : Nexus, RX-ONLY ---
    #     Same clean OO+callback shape as FT4 (no nzhsym/a7/shmem), wrapped behind
    #     a C ABI in fst4_cabi.f90. Decode only: there is no fst4_encode /
    #     fst4_gen_wave, and the Rust ModeKind reports Capabilities{tx:false} so
    #     modes::tx_mode() refuses to hand it to the transmit path.
    #
    #     NOTE gen_fst4wave IS on the RECEIVE path despite the name: fst4_decode
    #     calls it to regenerate and subtract a decoded signal.
    #
    #     The 15 s T/R period only. fst4_decode sizes its frame from ntrperiod
    #     (15/30/60/120/300/900/1800 s -> 180000..21600000 samples), and
    #     fst4_cabi pins FST4_NTRPERIOD=15 so the 180000-sample contract is fixed.
    ${CMAKE_CURRENT_SOURCE_DIR}/fst4_cabi.f90
    ${LIB}/fst4_decode.f90                # module fst4_decode (OO decoder)
    ${LIB}/fst4/genfst4.f90               # REQUIRED FOR DECODE despite the name:
                                          #   fst4_decode calls its ENTRY point
                                          #   get_fst4_tones_from_bits (genfst4.f90:76)
                                          #   to regenerate a decoded signal for
                                          #   subtraction. A use/call dependency scan
                                          #   misses it — Fortran ENTRY defines extra
                                          #   callable names inside a subroutine.
    ${LIB}/fst4/fst4_baseline.f90         # spectral baseline
    ${LIB}/fst4/gen_fst4wave.f90          # tones -> waveform (decode-path subtract)
    ${LIB}/fst4/get_fst4_bitmetrics.f90   # bit metrics
    ${LIB}/fst4/get_crc24.f90             # CRC-24 over the 240-bit codeword
    ${LIB}/fst4/decode240_101.f90         # LDPC(240,101) BP decoder  (includes parity)
    ${LIB}/fst4/decode240_74.f90          # LDPC(240,74) BP decoder   (includes parity)
    ${LIB}/fst4/osd240_101.f90            # ordered-statistics decode (calls encode240_101)
    ${LIB}/fst4/fastosd240_74.f90         # fast OSD                  (calls encode240_74)
    ${LIB}/fst4/encode240_101.f90         # re-encode for OSD         (includes generator)
    ${LIB}/fst4/encode240_74.f90          # re-encode for fast OSD    (includes generator)
    ${LIB}/blanker.f90                    # noise blanker (fst4_decode's outer loop)
    #     NOT listed and NOT compilable standalone — no program unit, they are
    #     `include`d by the files above: fst4/fst4_params.f90,
    #     fst4/ldpc_240_{101,74}_{parity,generator}.f90

    # --- WSPR native decode + encode (lib/wsprd + wspr_cabi.c) ---
    #     The propagation-beacon mode. 50-bit messages, K=32 r=1/2 convolutional
    #     coding, 110.6 s transmissions in a 2-minute window.
    #
    #     ⭐ WSPR HAS NO LIBRARY-SHAPED DECODER UPSTREAM. WSJT-X runs the `wsprd`
    #     EXECUTABLE as a subprocess (its widgets/mainwindow.cpp:2551), so there
    #     was nothing to call. Integrating it meant converting the PROGRAM: main()
    #     became wspr_decode_core() (wsprd.c), the getopt loop became parameters
    #     and pinned values, five persistent files were removed, and the emit loop
    #     writes the caller's array. Every MODIFIED FOR NEXUS block in wsprd.c
    #     documents one of those. The decode itself is untouched.
    #
    #     ⚠️ LICENSING: fano.c and jelinek.c carry a copyright line and NO license
    #     grant of their own. They are here on the WSJT-X PROJECT-WIDE GPL-3 — an
    #     inference from the surrounding work, not a per-file determination, and a
    #     deliberate operator decision. See NOTICE, which says so plainly and
    #     names the cleaner path (Karn's libfec ships a stated-LGPL fano.c).
    ${CMAKE_CURRENT_SOURCE_DIR}/wspr_cabi.c  # Nexus ENCODE shim -> get_wspr_channel_symbols
                                          #   (already compiled below: the DECODER
                                          #   calls it at wsprd.c:1433 to subtract a
                                          #   decoded signal, so encode/decode share
                                          #   one generator). No waveform here -
                                          #   upstream has no library-shaped WSPR
                                          #   wave generator; that lives in Rust.
    ${LIB}/wsprd/wsprd.c                  # the decode core (converted from main)
    ${LIB}/wsprd/wsprd_utils.c            # message unpacking, hashing helpers
    ${LIB}/wsprd/wsprsim_utils.c          # shared pack/unpack with the simulator
    ${LIB}/wsprd/fano.c                   # Fano sequential decoder (Karn)
    ${LIB}/wsprd/jelinek.c                # stack-bucket decoder (K9AN)
    ${LIB}/wsprd/tab.c                    # bit-count table
    ${LIB}/wsprd/osdwspr.f90              # ordered-statistics decoder. OSD is
                                          #   PINNED OFF (ndepth=0, upstream's
                                          #   default) but wsprd.c calls it
                                          #   unconditionally in source, so the
                                          #   symbol must exist to link.
    #     NOT listed: wsprd/metric_tables.c is `#include`d into wsprd.c:835, and
    #     wsprd/nhash.c is already compiled above for MSK144's hashing.

    # --- JT65 native decode (jt65_decode.f90 + lib/ftrsd) : Nexus, RX-ONLY ---
    #     The classic EME / weak-signal mode: 65-tone MFSK, a (63,12) Reed-Solomon
    #     code, and the LEGACY 72-bit packjt message layer (character*22 decodes),
    #     not the 77-bit packjt77 everything else here uses.
    #
    #     Clean OO+callback decoder like FT4/FST4/Q65 — no shmem, no streaming
    #     ladder. Decode only; Capabilities{tx:false}.
    #
    #     ⭐ NOT KVASD. JT65's historical Reed-Solomon decoder was the non-free
    #     KVASD binary. WSJT-X 3.0.2 uses ftrsd instead — the Franke-Taylor
    #     soft-decision decoder written to replace it — and the only KVASD
    #     reference left in the upstream tree is a CLI help string in jt9.f90.
    ${CMAKE_CURRENT_SOURCE_DIR}/jt65_cabi.f90
    ${LIB}/jt65_decode.f90                # module jt65_decode (OO decoder)
    ${LIB}/jt65_mod.f90                   # module jt65_mod (shared state)
    ${LIB}/symspec65.f90                  # symbol spectra
    ${LIB}/sync65.f90                     # sync search
    ${LIB}/decode65a.f90                  # per-candidate decode
    ${LIB}/decode65b.f90
    ${LIB}/extract.f90                    # soft symbols -> RS decode (calls ftrsdap)
    ${LIB}/hint65.f90                     # deep-search hypotheses (CALL3.TXT excised)
    ${LIB}/chkhist.f90                    # histogram sanity check
    ${LIB}/afc65b.f90                     # AFC
    ${LIB}/filbig.f90                     # wideband filter
    ${LIB}/fil6521.f90                    # 6.25:1 decimation
    ${LIB}/flat65.f90                     # spectrum flattening
    ${LIB}/setup65.f90                    # tone/sync setup
    ${LIB}/graycode.f90                   # Gray coding
    ${LIB}/graycode65.f90
    ${LIB}/interleave63.f90               # (63,12) interleaver
    ${LIB}/subtract65.f90                 # signal subtraction (multi-pass)
    ${LIB}/twkfreq65.f90                  # frequency tweak
    ${LIB}/xcor.f90                       # cross-correlation
    ${LIB}/sh65.f90                       # shorthand messages
    ${LIB}/sh65snr.f90
    ${LIB}/lorentzian.f90                 # Lorentzian fit
    ${LIB}/move.f90                       # array move helper
    #     The Reed-Solomon layer. Phil Karn's (KA9Q) RS codec under an explicit
    #     unversioned GPL grant + the Franke-Taylor soft-decision wrapper. See
    #     NOTICE — Karn is a SEPARATE copyright holder from the WSJT Development
    #     Group and is credited as such.
    ${LIB}/ftrsd/ftrsdap.c                # Franke-Taylor soft-decision RS decoder
    ${LIB}/ftrsd/decode_rs.c              # Karn Berlekamp-Massey decoder
    ${LIB}/ftrsd/encode_rs.c              # Karn RS encoder (used by the soft path)
    ${LIB}/ftrsd/init_rs.c                # Karn RS codec init
    ${LIB}/wrapkarn.c                     # rs_encode_: the Fortran-callable RS
                                          #   encoder hint65.f90:91 calls
    ${LIB}/igray.c                        # igray_: Gray code, used by
                                          #   graycode.f90:5 and graycode65.f90:5
    #     Five sources my dependency scan MISSED and the audit surfaced:
    #     demod64a (called from a LABELLED statement, extract.f90:120 `1 call ...`,
    #     which a line-anchored regex does not match), fchisq0, fchisq65, slope,
    #     and the two C files above. The archive linked without them because a
    #     static archive tolerates undefined symbols; only the final link fails.
    ${LIB}/demod64a.f90                   # 64-FSK demodulator (extract.f90:120)
    ${LIB}/fchisq0.f90                    # chi-squared fit
    ${LIB}/fchisq65.f90                   # chi-squared fit, JT65 variant
    ${LIB}/slope.f90                      # linear detrend
    ${LIB}/ccf2.f90                       # cross-correlation (fchisq65.f90:58)

    # --- MSK144 native decode (mskrtd + msk144/msk40 family) : Nexus, RX-ONLY ---
    #     The METEOR-SCATTER mode. 72 ms frames repeated through the period, so a
    #     single ionised trail lasting a tenth of a second can carry a message.
    #
    #     ⭐ Shaped differently from every other mode here: mskrtd is a SLIDING-
    #     WINDOW decoder analysing one 7168-sample block per call, driven at
    #     half-block steps across the period, and it returns a FORMATTED TEXT LINE
    #     rather than calling a callback. msk144_cabi owns the slide and reads the
    #     line back with an internal READ on upstream's own format.
    #
    #     Decode only: no encode path is wired, and the Rust ModeKind reports
    #     Capabilities{tx:false} so modes::tx_mode() refuses the transmit path.
    ${CMAKE_CURRENT_SOURCE_DIR}/msk144_cabi.f90
    ${LIB}/mskrtd.f90                     # the real-time block decoder
    ${LIB}/msk144spd.f90                  # single-ping ("&") fast decode
    ${LIB}/msk144sync.f90                 # frame sync / averaging patterns
    ${LIB}/msk144_freq_search.f90         # frequency search
    ${LIB}/msk144decodeframe.f90          # one frame -> softbits -> LDPC
    ${LIB}/msk144signalquality.f90        # SNR + phase-equalizer training
    ${LIB}/bpdecode128_90.f90             # LDPC(128,90) BP decoder
    ${LIB}/encode_128_90.f90              # re-encode for the OSD/quality path
    ${LIB}/genmsk_128_90.f90              # REQUIRED FOR DECODE despite the name:
                                          #   msk144signalquality regenerates the
                                          #   candidate to measure against.
    #     MSK40 = the SHORTHAND message variant. Gated off at run time
    #     (bshmsg/bswl are pinned false in msk144_cabi), but mskrtd calls it
    #     unconditionally in SOURCE, so the symbols must exist to link.
    ${LIB}/msk40spd.f90
    ${LIB}/msk40sync.f90
    ${LIB}/msk40_freq_search.f90
    ${LIB}/msk40decodeframe.f90
    ${LIB}/bpdecode40.f90                 # LDPC(40,...) BP decoder
    ${LIB}/encode_msk40.f90
    ${LIB}/genmsk40.f90
    ${LIB}/update_msk40_hasharray.f90     # the hashed-callsign table MSK40 uses
    ${LIB}/hash.f90                       # callsign hashing for MSK40 shorthand
    ${LIB}/hashing.f90                    #   (hash.f90 wraps hashing.f90)
    ${LIB}/wsprd/nhash.c                  #   ... and hash.f90:7 calls nhash() from
                                          #   here, via the bind(C,name="nhash")
                                          #   interface in hashing.f90:3.
                                          #
                                          #   lib/wsprd/, NOT lib/wsprcode/: the
                                          #   wsprcode copy exports only the
                                          #   Fortran-convention `nhash_` with
                                          #   pointer args, so it links but does
                                          #   not satisfy the bind(C) name.
                                          #   Upstream builds the wsprd one for
                                          #   the same reason (its CMakeLists:770).
                                          #   And NOT lib/superfox/qpc/nhash2.c —
                                          #   that directory holds the non-free
                                          #   SuperFox tables NOTICE forbids.
    #     Shared support first needed by MSK144:
    ${LIB}/analytic.f90                   # real -> analytic signal
    ${LIB}/tweak1.f90                     # complex frequency shift
    ${LIB}/ft8/chkcrc13a.f90              # CRC-13 check on the 128,90 codeword

    # --- Q65 native decode (lib/qra/q65 + q65_decode.f90) : Nexus, RX-ONLY ---
    #     Q65 is the first vendored mode with a substantial C layer: the Q-ary RA
    #     LDPC codec is Nico Palermo's (IV3NWV) qracodes, C rather than Fortran,
    #     reached through the q65_subs.c shim. See NOTICE — it is a SEPARATE
    #     copyright from the WSJT Development Group's.
    #
    #     TRANSMIT AND RECEIVE. q65_cabi exposes q65_encode_msg / q65_gen_wave
    #     alongside q65_decode_frame. The encode chain needed NO new sources: the
    #     DECODER already calls genq65 (below) to regenerate a candidate's tone
    #     sequence, so the symbol generator and the qracodes C layer were linked
    #     in all along. Encode and decode therefore share one generator.
    #
    #     ⚠️ q65_encode_msg, NOT q65_encode — the latter is upstream's own
    #     qracodes codeword API (q65.h:65) and collides at link time.
    #
    #     ⭐ ALL 5 PERIODS x ALL 5 SUBMODES, on both paths. Period sets the symbol
    #     length (Q65_NSPS) and submode scales the TONE SPACING by 2**submode; the
    #     manifest flags anything cached on either as chain-specific.
    ${CMAKE_CURRENT_SOURCE_DIR}/q65_cabi.f90
    ${LIB}/q65_decode.f90                 # module q65_decode (OO decoder)
    ${LIB}/qra/q65/q65.f90                # module q65: shared state + q65_dec0,
                                          #   q65_symspec, q65_hist, q65_bzap,
                                          #   q65_write_red, q65_clravg, q65_snr
    ${LIB}/qra/q65/q65_ap.f90             # a-priori decoding hypotheses
    ${LIB}/qra/q65/q65_loops.f90          # sync/decode search loops
    ${LIB}/qra/q65/q65_set_list.f90       # full-AP candidate codeword list
    ${LIB}/qra/q65/q65_set_list2.f90      # secondary candidate list
    ${LIB}/qra/q65/genq65.f90             # REQUIRED FOR DECODE despite the name:
                                          #   q65_decode calls it to regenerate a
                                          #   candidate's tone sequence, the same
                                          #   shape as genfst4 above.
    #     The qracodes C layer. fadengauss.c / fadenlorentz.c are deliberately
    #     ABSENT: they are `#include`d into q65.c's translation unit at
    #     q65.c:273-274, so compiling them separately would be dead objects.
    #     They are still vendored, still scanned by the manifest gate, and the
    #     codec does not build without them present on disk.
    ${LIB}/qra/q65/q65.c                  # the Q65 codec proper
    ${LIB}/qra/q65/q65_subs.c             # Fortran <-> C shim (holds `codec`)
    ${LIB}/qra/q65/qracodes.c             # generic Q-ary RA codec
    ${LIB}/qra/q65/npfwht.c               # fast Walsh-Hadamard transform
    ${LIB}/qra/q65/pdmath.c               # probability-distribution math
    ${LIB}/qra/q65/qra15_65_64_irr_e23.c  # the ONLY code Q65 instantiates —
                                          #   q65_subs.c:38 passes
                                          #   &qra15_65_64_irr_e23 to q65_init
    #     NOT listed, deliberately:
    #       qra/qracodes/qra12_63_64_irr_b.c, qra13_64_64_irr_e.c — QRA64 codes,
    #         not Q65's. Their headers are commented out in qracodes.h:63-65 and
    #         nothing in the Q65 path names them. Still vendored and still
    #         classified in the manifest, just not linked.
    #       qra/q65/normrnd.c — a Gaussian noise generator referenced only by its
    #         own header; upstream uses it in sim/test harnesses.
    #     NOTE the vendored tree keeps qracodes.c under qra/q65/, not
    #     qra/qracodes/ — upstream ships copies in both and the drop took q65's.

    #     Q65 decode-path support (lib), not previously needed by FT8/FT4/FST4:
    ${LIB}/ana64.f90                      # iwave -> analytic c00 at 6000 Sa/s
    ${LIB}/spec64.f90                     # 64-bin symbol spectra
    ${LIB}/sec0.f90                       # elapsed-time helper
    ${LIB}/twkfreq.f90                    # complex frequency/phase tweak
    ${LIB}/smo121.f90                     # 1-2-1 smoothing
    ${LIB}/types.f90                      # module types: q3list + kind params

    # --- 77-bit message packing (lib/77bit) ---
    # NOTE: call_to_c28, encode77, free_text, hash22calc, nonstd_to_c58, test28
    # are standalone TEST PROGRAMS (each has its own `program main`), NOT library
    # sources -- including them caused "multiple definition of main". The real
    # pack/unpack subroutines (pack77, unpack77, mp_short_*, hash22, ...) all
    # live inside packjt77.f90's module. parse77/my_hash are not in the genft1
    # encode->decode path, so they are omitted.
    ${LIB}/77bit/packjt77.f90             # module packjt77

    # --- misc shared (lib) ---
    ${LIB}/chkcall.f90                    # called by pack77 (callsign validation)
    ${LIB}/fmtmsg.f90                     # message formatting (pack/unpack)
    ${LIB}/grid2deg.f90                   # grid <-> lat/long (unpack77 grid)
    ${LIB}/deg2grid.f90                   # grid <-> lat/long (unpack77 grid)
    ${LIB}/packjt.f90                     # module packjt (used by packjt77)
    ${LIB}/crc.f90                        # module crc (binds C crc14/get_crc14)
    ${LIB}/db.f90                         # power -> dB (ft8_a7d SNR estimate)
    ${LIB}/indexx.f90
    ${LIB}/platanh.f90
    ${LIB}/nuttal_window.f90              # spectral window (ft1_sync)
    ${LIB}/pctile.f90                     # percentile (ft1_baseline); calls shell
    ${LIB}/polyfit.f90                    # polynomial fit (ft1_sync baseline)
    ${LIB}/determ.f90                     # matrix determinant (polyfit)
    ${LIB}/shell.f90                      # shellsort (pctile)
    ${LIB}/fftw3mod.f90                   # module fftw3 (wraps fftw3.f03)
    ${LIB}/four2a.f90                     # FFT via FFTW3f

    # --- C / C++ infra ---
    ${LIB}/crc14.cpp                      # Boost CRC-14 (crc14 / crc14_check)
    ${LIB}/crc13.cpp                      # Boost CRC-13, for MSK144's (128,90)
                                          #   codeword: chkcrc13a.f90:18 calls it
)

# ---------------------------------------------------------------------------
# Include directories for Fortran `include` statements:
#   tempofast_params.f90        -> -I lib/tempofast
#   ldpc_174_91_c_generator.f90 -> -I lib/ft8
#   ldpc_174_91_c_parity.f90    -> -I lib/ft8
#   fftw3.f03                   -> -I <fftw include dir> (usually /usr/include)
#
# NOTE: gfortran resolves `include` relative to the including file first, so these
# -I entries are a backstop rather than the primary mechanism. That is why the stale
# `${LIB}/ft1` entry here (the pre-2026-07-21 name for tempofast/) produced only a
# -Wmissing-include-dirs warning instead of a build failure, and sat unnoticed.
# ---------------------------------------------------------------------------
set(LIBFT1_INCLUDE_DIRS
    ${LIB}/tempofast
    ${LIB}/ft8
    ${LIB}
    ${FFTW3F_INCLUDE_DIRS}   # fftw3.f03 (native: from pkg-config /usr/include;
                             #            cross: from the MinGW FFTW prefix)
)
if(NOT CMAKE_CROSSCOMPILING)
    # fftw3.f03 lives in /usr/include on this native system; the MinGW prefix
    # ships its own copy (already in FFTW3F_INCLUDE_DIRS) for the cross build.
    list(APPEND LIBFT1_INCLUDE_DIRS /usr/include)
endif()
# For the cross build, crc14.cpp does #include <boost/crc.hpp> (header-only).
# We expose only the host Boost headers, and via -idirafter so they are searched
# AFTER the MinGW system headers — putting /usr/include on the normal -I path
# would shadow MinGW's libc headers (wchar.h, stdlib.h, ...) and break the build.
# fftw3.f03 for Fortran comes from the MinGW FFTW prefix (in FFTW3F_INCLUDE_DIRS).
if(CMAKE_CROSSCOMPILING AND CMAKE_SYSTEM_NAME STREQUAL "Windows")
    set(BOOST_HEADER_FLAG "-idirafter" "/usr/include")
endif()

# ---------------------------------------------------------------------------
# Static + shared libtempo.
# ---------------------------------------------------------------------------
# ---------------------------------------------------------------------------
# Karn's RS codec selects its symbol width with a preprocessor switch:
#   #ifdef FIXED -> fixed.h ; #elif BIGSYM -> int.h ; #else -> char.h
# Only int.h is shipped (char.h and fixed.h do not exist in the tree), so BIGSYM
# is REQUIRED, not optional — without it the build fails on a missing char.h.
# Upstream sets it globally (its CMakeLists:1288 `-DBIGSYM=1`); scoped to these
# four files here so the define cannot leak into unrelated C.
# ---------------------------------------------------------------------------
set_source_files_properties(
    ${LIB}/ftrsd/ftrsdap.c
    ${LIB}/ftrsd/decode_rs.c
    ${LIB}/ftrsd/encode_rs.c
    ${LIB}/ftrsd/init_rs.c
    PROPERTIES COMPILE_DEFINITIONS "BIGSYM=1")

add_library(tempo_static STATIC ${LIBTEMPO_SOURCES})
add_library(tempo_shared SHARED ${LIBTEMPO_SOURCES})
set_target_properties(tempo_static PROPERTIES OUTPUT_NAME tempo)
set_target_properties(tempo_shared PROPERTIES OUTPUT_NAME tempo)

foreach(tgt tempo_static tempo_shared)
    set(MODDIR "${CMAKE_CURRENT_BINARY_DIR}/modules/${tgt}")
    file(MAKE_DIRECTORY "${MODDIR}")
    set_target_properties(${tgt} PROPERTIES Fortran_MODULE_DIRECTORY "${MODDIR}")
    target_include_directories(${tgt} PRIVATE ${LIBFT1_INCLUDE_DIRS} "${MODDIR}")
    target_include_directories(${tgt} PUBLIC  ${CMAKE_CURRENT_SOURCE_DIR}/include)
    target_link_libraries(${tgt} PUBLIC ${FFTW3F_LIBRARIES})
    target_link_directories(${tgt} PUBLIC ${FFTW3F_LIBRARY_DIRS})
    target_link_libraries(${tgt} PUBLIC ${BOOST_CRC_TARGET})
    # crc14.cpp #include <boost/crc.hpp>: add host Boost headers as a last-resort
    # search path (cross build only; empty natively).
    if(BOOST_HEADER_FLAG)
        target_compile_options(${tgt} PRIVATE
            $<$<COMPILE_LANGUAGE:CXX>:${BOOST_HEADER_FLAG}>)
    endif()
endforeach()

# ---------------------------------------------------------------------------
# Verification executable A: build the EXISTING ft1_test.f90 against libtempo.
# It also needs the C Gaussian-noise generators gran.c / sgran.c (used only by
# the test harness, not the modem itself).
# ---------------------------------------------------------------------------
add_executable(tempofast_test_standalone
    ${LIB}/tempofast/tempofast_test.f90
    ${LIB}/gran.c
    ${LIB}/sgran.c
    ${LIB}/init_random_seed.c            # sgran() calls init_random_seed()
)
target_include_directories(tempofast_test_standalone PRIVATE
    ${LIBFT1_INCLUDE_DIRS}
    "${CMAKE_CURRENT_BINARY_DIR}/modules/tempo_static")
target_link_libraries(tempofast_test_standalone PRIVATE tempo_static)

# ---------------------------------------------------------------------------
# Verification executable A2: DX1-S AWGN + Rayleigh-fading decode-rate sweep.
# Like tempofast_test_standalone: links tempo_static + the C Gaussian-noise generators.
# ---------------------------------------------------------------------------
add_executable(tempodeep_test_standalone
    ${CMAKE_CURRENT_SOURCE_DIR}/tempodeep/tempodeep_test.f90
    ${LIB}/gran.c
    ${LIB}/sgran.c
    ${LIB}/init_random_seed.c
)
target_include_directories(tempodeep_test_standalone PRIVATE
    ${LIBFT1_INCLUDE_DIRS}
    "${CMAKE_CURRENT_BINARY_DIR}/modules/tempo_static")
target_link_libraries(tempodeep_test_standalone PRIVATE tempo_static)

# ---------------------------------------------------------------------------
# Verification executable A3: DX1-S full-passband acquisition (dx1_decode_all):
# 3 signals at distinct carriers + times -> all decoded; plus a scan-budget gate.
# ---------------------------------------------------------------------------
add_executable(tempodeep_band_test_standalone
    ${CMAKE_CURRENT_SOURCE_DIR}/tempodeep/tempodeep_band_test.f90
    ${LIB}/gran.c
    ${LIB}/sgran.c
    ${LIB}/init_random_seed.c
)
target_include_directories(tempodeep_band_test_standalone PRIVATE
    ${LIBFT1_INCLUDE_DIRS}
    "${CMAKE_CURRENT_BINARY_DIR}/modules/tempo_static")
target_link_libraries(tempodeep_band_test_standalone PRIVATE tempo_static)

# ---------------------------------------------------------------------------
# Verification executable B: tiny C harness exercising the C ABI.
# ---------------------------------------------------------------------------
add_executable(roundtrip ${CMAKE_CURRENT_SOURCE_DIR}/../tests/roundtrip.c)
target_link_libraries(roundtrip PRIVATE tempo_static m)
# gfortran runtime needed when linking Fortran static lib into a C executable.
target_link_libraries(roundtrip PRIVATE gfortran)

# ---------------------------------------------------------------------------
# Verification executable C: full RX acquisition decode via the C ABI.
# ---------------------------------------------------------------------------
add_executable(acquire ${CMAKE_CURRENT_SOURCE_DIR}/../tests/acquire.c)
target_link_libraries(acquire PRIVATE tempo_static m gfortran)

# ---------------------------------------------------------------------------
# Verification executable D: native FT8 full-frame acquisition + decode parity.
# ---------------------------------------------------------------------------
add_executable(ft8_acquire ${CMAKE_CURRENT_SOURCE_DIR}/../tests/ft8_acquire.c)
target_link_libraries(ft8_acquire PRIVATE tempo_static m gfortran)

# ---------------------------------------------------------------------------
# Verification executable E: native FT4 full-frame acquisition + decode parity.
# ---------------------------------------------------------------------------
add_executable(ft4_acquire ${CMAKE_CURRENT_SOURCE_DIR}/../tests/ft4_acquire.c)
target_link_libraries(ft4_acquire PRIVATE tempo_static m gfortran)

# ---------------------------------------------------------------------------
# Verification executable F: native FT8 FALSE-ALARM gate — decode noise, expect
# silence. The acquire tests above only measure yield (did we still find the
# signal); nothing asserted that the decoder does not invent signals that are not
# there. In Nexus a false decode reaches log_qso and the connector upload funnel,
# so it is the more expensive direction of the two.
# ---------------------------------------------------------------------------
add_executable(ft8_false_alarm ${CMAKE_CURRENT_SOURCE_DIR}/../tests/ft8_false_alarm.c)
target_link_libraries(ft8_false_alarm PRIVATE tempo_static m gfortran)

# ---------------------------------------------------------------------------
# Verification executable G: the FT4 false-alarm gate (mirror of F).
# ---------------------------------------------------------------------------
add_executable(ft4_false_alarm ${CMAKE_CURRENT_SOURCE_DIR}/../tests/ft4_false_alarm.c)
target_link_libraries(ft4_false_alarm PRIVATE tempo_static m gfortran)
# ---------------------------------------------------------------------------
# Verification executable H: standard-callsign round-trip. unpack28's callok()
# structural check REJECTS malformed 28-bit field values; this proves it never
# rejects a callsign someone actually holds, which a single-message yield ladder
# would not notice.
# ---------------------------------------------------------------------------
add_executable(ft8_callsign_roundtrip ${CMAKE_CURRENT_SOURCE_DIR}/../tests/ft8_callsign_roundtrip.c)
target_link_libraries(ft8_callsign_roundtrip PRIVATE tempo_static m gfortran)

