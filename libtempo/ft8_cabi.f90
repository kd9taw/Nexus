! Nexus: C ABI wrappers for native FT8 decode/encode, built on the vendored
! WSJT-X GPL modem sources (lib/ft8). Exposes a clean iso_c_binding interface so
! the FT8 mode can be driven headlessly from C / Rust, mirroring tempofast_cabi.f90.
! No Qt, no GUI, and none of WSJT-X's nzhsym streaming / shmem machinery: this
! decodes one complete 15 s frame by calling the core decode primitives directly
! (ft8apset -> sync8 -> ft8b), exactly as the inner loop of ft8_decode::decode
! does (ft8_decode.f90 lines ~172-239). ft8b performs its own signal subtraction
! (lsubtract), so multi-pass weak-signal recovery works.
!
! The a7 CROSS-CYCLE AP path (WSJT-X iaptype=7) IS wired in, reusing the vendored
! ft8_a7 module verbatim: every direct decode of the authoritative pass is saved
! into ft8_a7's per-parity slot table (ft8_a7_save), and after the pass loop each
! remembered call pair from the PREVIOUS same-parity slot is replayed as ~206
! QSO-continuation hypotheses against the residual audio (ft8_a7d), recovering
! RR73/73/RRR/report/grid continuations a few dB below the direct threshold. The
! caller supplies the slot key `nutc` (slot UTC seconds-of-day) and `la7final`
! (1 = authoritative full-audio pass: save + replay; 0 = early partial pass:
! slot bookkeeping only). ft8_a7_reset() clears the table on band/QSO change.
!
! Underlying Fortran routines wrapped here:
!   genft8       (ft8/genft8.f90)      - message -> 79 channel tones
!   gen_ft8wave  (ft8/gen_ft8wave.f90) - tones -> real audio waveform
!   ft8apset     (ft8/ft8apset.f90)    - a-priori symbol setup from callsigns
!   sync8        (ft8/sync8.f90)       - Costas sync candidate search
!   ft8b         (ft8/ft8b.f90)        - per-candidate decode (+ internal subtract)
!
! Frame / array constants (from ft8/ft8_params.f90):
!   NN   = 79      total channel symbols
!   NSPS = 1920    samples per symbol @ 12 kHz
!   NMAX = 180000  raw audio samples (15.0 s @ 12 kHz)
!   NH1  = 1920    half-FFT length (sbase sizing)
!   NZ   = 151680  samples in the full 12.64 s waveform (NSPS*NN)

module ft8_cabi
  use iso_c_binding
  ! Vendored WSJT-X a7 cross-cycle AP table (process-global SAVE state). Renamed
  ! imports keep the short table names (f0, ndec, ...) from colliding with this
  ! module's dummy args and result variables. One-directional: ft8_a7 does not
  ! use ft8_cabi, so no module cycle.
  use ft8_a7, only: a7_dt0 => dt0, a7_f0 => f0, a7_msg0 => msg0, &
       a7_jseq => jseq, a7_ndec => ndec, ft8_a7_save, ft8_a7d
  ! Per-chain decoder context (tempo_ctx_size/reset/save/restore, below). Every
  ! import is renamed so nothing shadows a dummy argument or local in this file;
  ! these names appear ONLY in the ctx type and in ctx_xfer. See the block comment
  ! above `tempo_ctx_t` for why each symbol is here.
  use ft8_a7, only: cxv_a7_dt0 => dt0, cxv_a7_f0 => f0, cxv_a7_msg0 => msg0
  use ft8_downsample_state,       only: cxv_ft8_spec => cx
  use ft4_downsample_state,       only: cxv_ft4_spec => cx
  use tempofast_downsample_state, only: cxv_ft1_spec => cx
  use ir_harq_combine_mod,        only: harq_slot, cxv_slots => slots, &
       cxv_harq_init => harq_initialized
  use packjt77, only: cxv_calls10 => calls10, cxv_calls12 => calls12,   &
       cxv_calls22 => calls22, cxv_recent => recent_calls,              &
       cxv_mycall13 => mycall13, cxv_dxcall13 => dxcall13,              &
       cxv_ihash22 => ihash22, cxv_nzhash => nzhash,                    &
       cxv_mycall13_0 => mycall13_0, cxv_dxcall13_0 => dxcall13_0,      &
       cxv_mycall13_set => mycall13_set,                                &
       cxv_dxcall13_set => dxcall13_set,                                &
       cxv_hashmy10 => hashmy10, cxv_hashmy12 => hashmy12,              &
       cxv_hashmy22 => hashmy22, cxv_hashdx10 => hashdx10,              &
       cxv_hashdx12 => hashdx12, cxv_hashdx22 => hashdx22
  use ft4_decode_apstate, only: cxv_ft4_apbits => apbits,               &
       cxv_ft4_apmy_ru => apmy_ru, cxv_ft4_aphis_fd => aphis_fd,        &
       cxv_ft4_mycall0 => mycall0, cxv_ft4_hiscall0 => hiscall0
  use ft1_decode_apstate, only: cxv_ft1_apbits => apbits,               &
       cxv_ft1_mycall0 => mycall0, cxv_ft1_hiscall0 => hiscall0
  implicit none

  ! Last slot key (nutc) seen by ft8_decode_frame; -1 = virgin/reset. Drives the
  ! once-per-slot k=1 -> k=0 shuffle of the a7 table (module scope => SAVE).
  integer :: nutc0_a7 = -1

  integer, parameter :: F8_NN      = 79
  integer, parameter :: F8_NSPS    = 1920
  integer, parameter :: F8_NMAX    = 180000          ! 15 * 12000
  integer, parameter :: F8_NH1     = 1920            ! NFFT1/2 = (2*1920)/2
  integer, parameter :: F8_NZ      = F8_NSPS * F8_NN ! 151680
  integer, parameter :: F8_MAXCAND = 600
  integer, parameter :: F8_MAXDEC  = 200   ! stock WSJT-X per-period cap (MAXDEC)

  ! Interop result struct. Layout MUST match ft8_decode_t in libtempo.h.
  !   off 0  float sync; 4 int snr; 8 float dt; 12 float freq;
  !   16 char message[38]; 54 (pad 2) int nap; 60 float qual; total 64.
  type, bind(C) :: ft8_decode_t
     real(c_float)          :: sync         ! sync metric
     integer(c_int)         :: snr          ! SNR estimate, dB (rounded)
     real(c_float)          :: dt           ! time offset, seconds (xdt-0.5)
     real(c_float)          :: freq         ! audio frequency, Hz
     character(kind=c_char) :: message(38)  ! NUL-terminated decoded text
     integer(c_int)         :: nap          ! AP type used (iaptype; 0 = none)
     real(c_float)          :: qual         ! decode quality metric [0,1]
  end type ft8_decode_t

  ! What WSJT-X's decoder keeps of each early decode so the full pass can start from audio with it
  ! subtracted (ft8_decode.f90:67-69 itone_save/f1_save/xdt_save, :55 lsubtracted; saved at
  ! :229-231). Layout MUST match ft8_sub_t in libtempo.h: 79 int tones, float f1, float xdt0,
  ! int ok, int done = 332 bytes.
  type, bind(C) :: ft8_sub_t
     integer(c_int) :: itone(79)    ! the decode's 79 channel tones (ft8b's itone)
     real(c_float)  :: f1           ! carrier, Hz, as the decode reported it
     real(c_float)  :: xdt0         ! ft8b's start time, s: the record's dt + 0.5 (xdt_save, :230)
     integer(c_int) :: ok           ! 1 = a direct decode, tones known; 0 = not subtractable (a7)
     integer(c_int) :: done         ! 1 = already subtracted from the residual (lsubtracted)
  end type ft8_sub_t

  ! The caller's callback for ft8_decode_frame_cb (tempo_decode_cb in libtempo.h). `rec` points
  ! at a decode the pass has just accepted, or is NULL after each sync candidate is tried; a
  ! nonzero answer to a NULL `rec` stops the pass there.
  abstract interface
     function ft8_decode_cb(user, rec) bind(C) result(istop)
       import :: c_ptr, c_int
       type(c_ptr), value :: user
       type(c_ptr), value :: rec
       integer(c_int)     :: istop
     end function ft8_decode_cb
  end interface

  !-------------------------------------------------------------------------
  ! PER-CHAIN DECODER CONTEXT
  !
  ! The modem's decode state is process-global Fortran SAVE storage, so two
  ! radio chains decoding two bands in ONE process share every byte of it.
  ! That does not crash: chain A's a7 replay list / callsign hash table /
  ! IR-HARQ slot pool / cached wideband spectrum, consumed by chain B, yields a
  ! CRC-valid, syntactically perfect, WRONG decode - logged and uploaded, and
  ! indistinguishable afterwards from a real QSO. `tempo_ctx_t` is one chain's
  ! private copy of that state; the C entry points below swap it in and out
  ! around a decode() call.
  !
  ! WHICH SYMBOLS. libtempo/modem-state-manifest.toml is authoritative: its
  ! class-1 rows are the candidate set. Every one of them that carries
  ! CHAIN-SPECIFIC content is a component below - 43 of the 68 rows.
  !
  ! Reaching them took hoisting. A SAVE/DATA local inside a vendored subroutine
  ! is a FILE-LOCAL symbol (`nm` shows them lowercase: `apbits.5`,
  ! `hashmy10.13`), which no other compilation unit can name, so it cannot be
  ! swapped at all until it moves to module scope. Five vendored files carry a
  ! MODIFIED FOR NEXUS header for exactly that: ft8/ft4/tempofast_downsample
  ! (the wideband spectra), ft4_decode.f90 + tempofast_decode.f90 (the AP
  ! codeword caches, into *_decode_apstate) and 77bit/packjt77.f90 (unpack77's
  ! callsign hash memos, up to module scope).
  !
  ! The 25 class-1 rows deliberately NOT here, and why each is safe shared:
  !   * ft8b.f90 mcq/mcqru/mcqfd/mcqtest/mcqww/mrrr/m73/mrr73 + first +
  !     ncontest0. The mcq family's post-transform content is a fixed table -
  !     `mcq=2*mcq-1` over a DATA literal - with no chain-specific input, so
  !     there is nothing chain-specific to swap. The hazard the manifest files
  !     them under is that the transform is NON-IDEMPOTENT and re-fires whenever
  !     ncontest differs from the previous call's; that is an upstream WSJT-X
  !     defect which corrupts a SINGLE chain just as surely (toggle contest mode
  !     twice) and which per-chain copies would mask rather than fix. ncontest is
  !     the literal 0 at ft8_decode_frame below, so the guard fires exactly once
  !     per process. If ncontest ever becomes runtime state, fix the transform.
  !   * gen_ft4wave.f90 dt and gen_tempofastwave.f90 pulse/first/twopi/dt/hmod/
  !     npulse/nsps_real/dphi_peak/nramp_out. These latch the FIRST caller's
  !     `fsample` for the life of the process. That IS a live defect - callers
  !     disagree (48 kHz from the TX entry points, 12 kHz from subtractft1) -
  !     but it is a defect WITHIN one chain, and per-chain copies would not fix
  !     it because one chain both transmits and decodes. It also could not be
  !     swapped here even in principle: the TX entry points are not bracketed by
  !     tempo_ctx_restore/save. The fix is to recompute when fsample changes,
  !     which is a waveform change and needs its own approval.
  !   * osd174_91.f90 inext. A linked-list cursor within ONE osd174_91 call. It
  !     is read before write on the first fetchit91 of a call, but the sibling
  !     `lastpat` is force-reset to -1 on every entry and ipat is provably >= 0,
  !     so the branch that would consume the stale value is unreachable. Its
  !     partner is class 3; splitting the pair is worse than leaving both.
  !   * ft4_decode.f90 napwid. Not static at all - a stack local (no `nm`
  !     symbol), so there is no storage to save. It is a genuine upstream
  !     read-before-write (`napwid=80` sits inside `if(ipass.gt.3)` while the
  !     read at the OSD-depth test runs on every pass); fixing that changes which
  !     codewords are accepted, so it is a decode-behavior change, not this one.
  !   * tempofast_decode.f90 cur_rv/frame_time_ms/callback. Components of
  !     `type(ft1_decoder)`, and tempofast_cabi.f90 declares that object as a
  !     LOCAL of ft1_decode_frame - a fresh instance per decode call, so they are
  !     already per-chain by construction. Keep it that way: a module-scope or
  !     `save`d decoder object would deliver one band's decodes to the other
  !     band's callback.
  !
  ! LAYOUT. Bounds and character lengths are taken from the LIVE declarations
  ! (`size(...)` / `len(...)` of the use-associated symbol), never re-typed, so a
  ! vendor refresh that resizes a table resizes this context with it instead of
  ! silently truncating a copy.
  !
  ! FRESH CONTEXTS ARE NOT ZERO. Several of these symbols are DATA-initialized
  ! (.data, not .bss): ihash22 starts at -1 (0 means "hash slot 0 is OCCUPIED",
  ! by a blank callsign), every callsign table starts SPACE-filled (NUL-filled,
  ! `len(trim(...))` is 13 and hash10/hash12 return a callsign of NULs instead
  ! of `<...>`), and nutc0_a7 starts at -1 (0 is a valid slot key). Zero-filling
  ! a context and restoring it would corrupt the modem, which is why
  ! tempo_ctx_reset() exists and why every row of ctx_xfer's list carries its
  ! own load-time value.
  !
  ! Verified byte-for-byte against a virgin process (save the statics before
  ! anything runs, reset a second buffer, memcmp): 3,476,200 of 3,505,808 bytes
  ! are identical, and the two regions that differ are deliberate - a7_msg0 and
  ! /pfxcom/ addpfx are blank-filled here where the loader leaves .bss NULs.
  ! (Re-run 2026-07-21 after the AP-cache and hash-memo hoists: the context grew
  ! by 1,724 bytes and the identical count grew by exactly 1,724, so every new
  ! row's load-time value matches the loader's image and the differing byte
  ! count is unchanged at 29,608.)
  ! Blank is the correct empty for both (ft8_a7_reset below already assigns
  ! `a7_msg0 = ' '`, and getpfx1 tests `index(addpfx,' ')`), and neither is read
  ! before it is written in a fresh context.
  !
  ! The type deliberately declares NO default initialization, direct or inherited
  ! from a component's type. gfortran materializes a large derived type's default
  ! initializer as a STACK temporary at `allocate` - that overflowed a 2 MiB
  ! worker-thread stack for a 3.4 MiB context (measured: SIGSEGV at any stack
  ! below ~4 MiB; fine at 256 KiB once the initializers were gone) - and, if any
  ! byte of the template is non-zero, puts 3.4 MiB in .rodata that every shipped
  ! binary carries (measured with `size -A`: libtempo.so 6.30 MB -> 2.79 MB).
  ! CTX_INIT does the same job with neither cost.
  !-------------------------------------------------------------------------

  ! 8-byte words needed to hold ir_harq_combine_mod's whole slot pool, taken
  ! from the live declaration (element size x count, rounded up).
  integer, parameter :: CTX_HARQ_WORDS = &
       (size(cxv_slots) * (storage_size(cxv_slots) / 8) + 7) / 8

  type :: tempo_ctx_t
     ! --- FT8 -------------------------------------------------------------
     ! ft8_downsample_state cx/x: this chain's whole 192000-point wideband
     ! spectrum, refreshed only under the caller-owned `newdat`. Both call
     ! sites pass .false., so most calls CONSUME a spectrum they did not
     ! compute - shared, that downsamples chain B's audio at chain A's
     ! frequency. cx is the complex face of the EQUIVALENCE and is >= the real
     ! face `x`, so copying cx carries the whole block.
     complex           :: ft8_spec(size(cxv_ft8_spec))
     ! ft8_a7 cross-cycle AP table: the prior same-parity slot's decoded call
     ! pairs, replayed as ~206 QSO-continuation hypotheses. Shared, chain A's
     ! call pairs are replayed against chain B's audio (iaptype 7). These five
     ! move as ONE unit - the replay loop is bounded solely by ndec.
     real              :: a7_dt0(size(cxv_a7_dt0,1), 0:1, 0:1)
     real              :: a7_f0(size(cxv_a7_f0,1), 0:1, 0:1)
     character(len=len(cxv_a7_msg0)) :: a7_msg0(size(cxv_a7_msg0,1), 0:1, 0:1)
     integer           :: a7_ndec(0:1, 0:1)
     integer           :: a7_jseq
     ! ft8_cabi's own slot tracker. Splitting it from a7_jseq desynchronizes
     ! the even/odd rollover, so it lives in the same context. -1 = virgin.
     integer           :: cabi_nutc0_a7
     ! --- FT4 -------------------------------------------------------------
     complex           :: ft4_spec(size(cxv_ft4_spec))
     ! ft4_decode_apstate: the a-priori LDPC codeword for THIS chain's
     ! (mycall,hiscall), plus the two contest variants and the callsign pair
     ! that keys them. The key guard rebuilds on mismatch, so the cache alone
     ! cannot print chain A's callsign on chain B - but the rebuild re-seeds the
     ! (already per-chain) packjt77 hash tables, so a shared cache would let one
     ! chain's cache HIT suppress the other chain's seeding. Cache and key move
     ! as one unit; a partial split is the failure mode, not a mitigation.
     integer           :: ft4_apbits(size(cxv_ft4_apbits))
     integer           :: ft4_apmy_ru(size(cxv_ft4_apmy_ru))
     integer           :: ft4_aphis_fd(size(cxv_ft4_aphis_fd))
     character(len=len(cxv_ft4_mycall0))  :: ft4_mycall0
     character(len=len(cxv_ft4_hiscall0)) :: ft4_hiscall0
     ! --- FT1 / TempoFast --------------------------------------------------
     complex           :: ft1_spec(size(cxv_ft1_spec))
     ! ft1_decode_apstate: same cache and same key, for TempoFast.
     integer           :: ft1_apbits(size(cxv_ft1_apbits))
     character(len=len(cxv_ft1_mycall0))  :: ft1_mycall0
     character(len=len(cxv_ft1_hiscall0)) :: ft1_hiscall0
     ! IR-HARQ soft-combining pool - the single largest object here. Shared,
     ! chain A's RV0 frame combines with chain B's RV1.
     !
     ! Held as OPAQUE 8-byte words rather than `type(harq_slot)` directly, for
     ! one reason: harq_slot carries component initializers (rv_count = -1,
     ! snr_est = -99.0, npts = HARQ_NDMAX), a derived-type component propagates
     ! them into the compiler-generated default-initializer template for the
     ! WHOLE of tempo_ctx_t, and a template with any non-zero byte lands in
     ! .rodata - 3.4 MiB of it, in every shipped binary, referenced by nothing at
     ! run time (measured with `size -A`; all-zero, it lands in .bss and costs
     ! no file bytes). Sized from the live symbol, and ctx_xfer maps it straight
     ! back to `type(harq_slot)` so the copy itself is still fully type-checked.
     integer(c_int64_t) :: harq_pool(CTX_HARQ_WORDS)
     logical           :: harq_init
     ! --- shared 77-bit / callsign machinery -------------------------------
     ! packjt77's hash tables: how a hashed <...> token resolves to a full
     ! callsign. THE cross-contamination vector - shared, chain B prints chain
     ! A's callsigns for hashes it never heard.
     character(len=len(cxv_calls10)) :: calls10(size(cxv_calls10))
     character(len=len(cxv_calls12)) :: calls12(size(cxv_calls12))
     character(len=len(cxv_calls22)) :: calls22(size(cxv_calls22))
     character(len=len(cxv_recent))  :: recent_calls(size(cxv_recent))
     character(len=len(cxv_mycall13)) :: mycall13
     character(len=len(cxv_dxcall13)) :: dxcall13
     integer           :: ihash22(size(cxv_ihash22))
     integer           :: nzhash
     ! unpack77's callsign-substitution memos, hoisted to packjt77 module scope.
     ! Unlike the AP caches above these do NOT self-heal on a chain switch:
     ! dxcall13_set latches .true. with no else-branch, so a chain calling CQ
     ! with an empty hiscall keeps the OTHER chain's dxcall13_0 + hashdx* and
     ! substitutes that station's callsign for a received 22-bit hash; and a
     ! short mycall13 clears mycall13_set without clearing mycall13_0, so the
     ! chain that owns the call never re-arms its own substitution. Both are
     ! well-formed wrong output, which is why these are in the context.
     character(len=len(cxv_mycall13_0)) :: mycall13_0
     character(len=len(cxv_dxcall13_0)) :: dxcall13_0
     logical           :: mycall13_set
     logical           :: dxcall13_set
     integer           :: hashmy10, hashmy12, hashmy22
     integer           :: hashdx10, hashdx12, hashdx22
     ! COMMON /pfxcom/ - the add-on prefix used when packing/unpacking a
     ! compound call. Per-QSO; a wrong prefix is a well-formed WRONG callsign.
     ! Declared character*8 at packjt.f90:753 (re-declared in ctx_xfer).
     character(len=8)  :: addpfx
  end type tempo_ctx_t

  ! What ctx_xfer's single list is being walked FOR.
  integer, parameter :: CTX_SAVE    = 1   ! live statics -> context
  integer, parameter :: CTX_RESTORE = 2   ! context -> live statics
  integer, parameter :: CTX_INIT    = 3   ! load-time value -> context

  ! Mode-driven copy helpers, one per (type, rank) the context holds. They exist
  ! so ctx_xfer can state the symbol list ONCE: save, restore and init all walk
  ! the SAME list and cannot drift apart. The compiler resolves the generic by
  ! type+rank, so a component that drifts from its symbol fails to compile.
  interface ctx_move
     module procedure ctx_move_c1, ctx_move_r3, ctx_move_a3, ctx_move_i2, &
          ctx_move_i1, ctx_move_i0, ctx_move_l0, ctx_move_a1, ctx_move_a0, &
          ctx_move_h1
  end interface ctx_move

contains

  !-------------------------------------------------------------------------
  ! ft8_encode : message text -> 79 channel tones {0..7}
  !   msg       : NUL/space-terminated C string (<= 37 chars)
  !   msg_len   : valid chars in msg
  !   itone_out : output 79 tones
  !   nsym_out  : symbols written (79), or -1 on bad message
  !-------------------------------------------------------------------------
  subroutine ft8_encode(msg, msg_len, itone_out, nsym_out) bind(C, name="ft8_encode")
    character(kind=c_char), intent(in)  :: msg(*)
    integer(c_int), value,  intent(in)  :: msg_len
    integer(c_int),         intent(out) :: itone_out(F8_NN)
    integer(c_int),         intent(out) :: nsym_out

    character(len=37) :: msg37, msgsent37
    integer(kind=1)   :: msgbits(77)
    integer           :: itone(F8_NN)
    integer           :: i, n, i3, n3

    msg37 = ' '
    n = min(msg_len, 37)
    do i = 1, n
       if (msg(i) == c_null_char) exit
       msg37(i:i) = msg(i)
    end do

    call genft8(msg37, i3, n3, msgsent37, msgbits, itone)
    if (i3 < 0 .and. n3 < 0) then
       nsym_out = -1
       return
    end if
    itone_out(1:F8_NN) = itone(1:F8_NN)
    nsym_out = F8_NN
  end subroutine ft8_encode

  !-------------------------------------------------------------------------
  ! ft8_gen_wave : tones -> real audio waveform
  !   itone     : input tones (length nsym)
  !   nsym      : number of tones (79)
  !   fsample   : output sample rate (Hz), e.g. 12000.0
  !   f0        : audio carrier (Hz), e.g. 1500.0
  !   wave_out  : caller buffer (capacity nwave_out)
  !   nwave_out : in = capacity; out = samples produced (nsym*NSPS)
  !-------------------------------------------------------------------------
  subroutine ft8_gen_wave(itone, nsym, fsample, f0, wave_out, nwave_out) &
       bind(C, name="ft8_gen_wave")
    integer(c_int),        intent(in)    :: itone(*)
    integer(c_int), value, intent(in)    :: nsym
    real(c_float),  value, intent(in)    :: fsample, f0
    real(c_float),         intent(inout) :: wave_out(*)
    integer(c_int),        intent(inout) :: nwave_out

    integer :: nwave, itone_l(nsym)
    real    :: f0_l, fs_l, bt
    complex :: cwave(nsym*F8_NSPS)

    nwave = nsym * F8_NSPS
    if (nwave_out < nwave) then
       nwave_out = -1
       return
    end if
    itone_l(1:nsym) = itone(1:nsym)
    f0_l = f0
    fs_l = fsample
    bt   = 2.0   ! FT8 Gaussian BT
    call gen_ft8wave(itone_l, nsym, F8_NSPS, bt, fs_l, f0_l, cwave, &
                     wave_out, 0, nwave)
    nwave_out = nwave
  end subroutine ft8_gen_wave

  !-------------------------------------------------------------------------
  ! ft8_decode_frame : decode EVERY FT8 signal in a complete 15 s frame.
  !
  !   iwave         : F8_NMAX (180000) int16 audio samples @ 12 kHz
  !   nfa, nfb      : frequency search band edges (Hz)
  !   ndepth        : 1..3 (3 = full bp+osd, 3 passes; <=0 defaults to 3)
  !   mycall,hiscall: NUL/space-terminated callsigns for AP (may be empty)
  !   nqso_progress : QSO progress index (AP pass schedule)
  !   nfqso_in      : QSO/RX audio freq (Hz) the operator is working — WSJT-X's
  !                   nfqso. The deep a-priori passes (iaptype>=3, the MyCall+
  !                   DxCall masks) only fire within napwid of this, and sync8
  !                   prioritizes near it. Pass 0 / out-of-band ⇒ band center.
  !   nftx_in       : TX audio freq (Hz) — WSJT-X's nftx (mainwindow.cpp:3722,
  !                   `dec_data.params.nftx = ui->TxFreqSpinBox->value()`), the
  !                   SECOND deep-AP window. ft8b.f90:305 cycles a candidate
  !                   only when it is outside BOTH nfqso±napwid AND nftx±napwid,
  !                   so upstream deep-AP coverage is two windows, not one — the
  !                   second exists because a station answering your CQ usually
  !                   answers ON your TX frequency. Pass 0 / out-of-band ⇒ nfqso
  !                   (the two windows collapse into one, which is exactly right
  !                   when the operator has not split RX from TX).
  !   nzhsym_in     : half-symbol frame count for this pass — WSJT-X's nzhsym
  !                   (mainwindow.cpp:1877-1879). 50 = the full 14.4 s frame
  !                   (m_hsymStop, mainwindow.cpp:1678); 41 = the 11.808 s EARLY
  !                   partial pass (m_earlyDecode, mainwindow.h:533). It is a
  !                   decoder-cost control, not a buffer length: sync8 is always
  !                   handed the full NPTS (ft8_decode.f90:194) and nothing here
  !                   truncates dd. At 41 upstream runs a deliberately CHEAP
  !                   pass — syncmin 2.0 instead of 1.3 (ft8_decode.f90:178) and
  !                   npasses clamped to 4, i.e. the AP passes 5-8 are OFF
  !                   (ft8b.f90:275, `if(nzhsym.lt.50) npasses=4`) — because the
  !                   authoritative pass re-decodes the same audio 2.6 s later.
  !                   Measured A/B on decode_bench (FT8, 12 signals/slot, 8
  !                   slots, ndepth 3): the early pass costs p50 362 ms at 50 and
  !                   89 ms at 41 — 4.1x — while the BOUNDARY pass finds the same
  !                   11.9 decodes/slot either way. The early pass itself does
  !                   give up a little (11.8 -> 11.2 decodes/slot); that is the
  !                   trade upstream makes on purpose, and it is safe here for
  !                   the same reason it is safe there — the boundary pass is an
  !                   independent full re-decode, so an early miss is recovered
  !                   3.2 s later rather than lost. What it buys is an early pass
  !                   that cannot overrun the slot: an early decode still running
  !                   at the boundary makes the WHOLE period get dropped.
  !                   Anything other than 41 is treated as 50. Upstream's third
  !                   value, 47, is its residual-carry step (ft8_decode.f90:
  !                   124-150): ft8_subtract_early below, whose residual
  !                   ft8_decode_frame_dp's full pass starts from. This entry
  !                   never starts from one.
  !   nutc          : slot key = slot UTC seconds-of-day (0..86399; slot*15 for
  !                   FT8). Keys the a7 cross-cycle table: parity = mod(nutc/5,2)
  !                   and a nutc change rolls the per-parity table over. A nutc
  !                   BEHIND the last seen slot (a redecode of an older capture)
  !                   suppresses all a7 state changes so a replayed old slot
  !                   cannot scramble the live even/odd ping-pong.
  !   la7final      : 1 on the authoritative full-audio (boundary) pass — saves
  !                   direct decodes into the a7 table and runs the a7 replay.
  !                   0 on the early/partial pass — slot bookkeeping only.
  !   lft8apon_in   : nonzero = a-priori decoding ON (WSJT-X "Enable AP",
  !                   ft8b's lft8apon: AP passes 5-8 run, and the a7 replay is
  !                   allowed). 0 = AP fully off — ft8b runs only the four
  !                   regular passes AND the a7 cross-cycle replay is skipped
  !                   (both consumers of the flag; a7 slot BOOKKEEPING still
  !                   runs so re-enabling AP next slot has a live table).
  !   lapcqonly_in  : nonzero = restrict AP to the CQ hypothesis (ft8b's
  !                   lapcqonly: one AP pass, iaptype=1). WSJT-X sets this
  !                   automatically after >300 s without transmitting; here the
  !                   caller decides. Ignored when lft8apon_in = 0.
  !   lactivity_in  : nonzero = the operator is in a Special Operating Activity
  !                   (Nexus: Contest mode, or a Hound) — upstream's ncontest /= 0
  !                   as ft8b's rover and 'TU; ' filter reads it, and only there:
  !                   0 drops type 1-3 decodes holding '/R' or starting 'TU; ',
  !                   as WSJT-X does with no activity selected; nonzero keeps
  !                   them. ncontest itself stays 0 (see below).
  !   out           : caller array of ft8_decode_t (capacity max_out)
  !   max_out       : capacity of out
  !
  !   Returns the number of decodes written (>= 0), or -1 on error.
  !
  ! NOT thread-safe (the modem keeps process-global SAVE state + FFTW plans).
  !-------------------------------------------------------------------------
  function ft8_decode_frame(iwave, nfa, nfb, ndepth, mycall, hiscall, &
       nqso_progress, nfqso_in, nftx_in, nzhsym_in, nutc, la7final, &
       lft8apon_in, lapcqonly_in, lactivity_in, out, max_out) result(ndec) &
       bind(C, name="ft8_decode_frame")
    integer(c_int16_t),     intent(in)  :: iwave(F8_NMAX)
    integer(c_int), value,  intent(in)  :: nfa, nfb, ndepth, nqso_progress, nfqso_in
    integer(c_int), value,  intent(in)  :: nftx_in, nzhsym_in
    integer(c_int), value,  intent(in)  :: nutc, la7final, max_out
    integer(c_int), value,  intent(in)  :: lft8apon_in, lapcqonly_in, lactivity_in
    character(kind=c_char), intent(in)  :: mycall(*)
    character(kind=c_char), intent(in)  :: hiscall(*)
    type(ft8_decode_t),     intent(out) :: out(*)
    integer(c_int)                      :: ndec

    ndec = ft8_decode_frame_cb(iwave, nfa, nfb, ndepth, mycall, hiscall, &
         nqso_progress, nfqso_in, nftx_in, nzhsym_in, nutc, la7final, &
         lft8apon_in, lapcqonly_in, lactivity_in, out, max_out, c_null_funptr, c_null_ptr)
  end function ft8_decode_frame

  !-------------------------------------------------------------------------
  ! ft8_decode_frame_cb : ft8_decode_frame, handing each decode to `cb` the moment the pass
  !                       accepts it, as WSJT-X's decoder hands each one to its callback
  !                       (ft8_decode.f90:233-238), which jt9 writes and flushes at once
  !                       (decoder.f90 ft8_decoded, flush(6)) for the GUI's auto-sequencer to read
  !                       line by line (mainwindow.cpp readFromStdout). The decodes come in the
  !                       pass's own order: the sync candidates within 10 Hz of nfqso first
  !                       (sync8.f90:155-163), then by sync; the a7 replay's after the passes.
  !
  !   cb      : tempo_decode_cb, or NULL for no callback (exactly ft8_decode_frame). Called with
  !             a pointer to a copy of the record just written to `out`, and with NULL after every
  !             sync candidate of the direct passes, where WSJT-X asks its early pass's give-up
  !             (ft8_decode.f90:240-242). A nonzero answer to the NULL call ends the pass there:
  !             no further candidate, pass or a7 replay, and `out` holds what was found. The
  !             answer to a decode is ignored.
  !   cb_user : handed back to `cb` untouched.
  !
  ! Everything else, and the decodes themselves, are ft8_decode_frame's. Not reentrant: `cb` must
  ! not call back into the modem. ft8_decode_frame_dp with no carry.
  !-------------------------------------------------------------------------
  function ft8_decode_frame_cb(iwave, nfa, nfb, ndepth, mycall, hiscall, &
       nqso_progress, nfqso_in, nftx_in, nzhsym_in, nutc, la7final, &
       lft8apon_in, lapcqonly_in, lactivity_in, out, max_out, cb, cb_user) result(ndec) &
       bind(C, name="ft8_decode_frame_cb")
    integer(c_int16_t),     intent(in)  :: iwave(F8_NMAX)
    integer(c_int), value,  intent(in)  :: nfa, nfb, ndepth, nqso_progress, nfqso_in
    integer(c_int), value,  intent(in)  :: nftx_in, nzhsym_in
    integer(c_int), value,  intent(in)  :: nutc, la7final, max_out
    integer(c_int), value,  intent(in)  :: lft8apon_in, lapcqonly_in, lactivity_in
    character(kind=c_char), intent(in)  :: mycall(*)
    character(kind=c_char), intent(in)  :: hiscall(*)
    type(ft8_decode_t),     intent(out) :: out(*)
    type(c_funptr), value,  intent(in)  :: cb
    type(c_ptr),    value,  intent(in)  :: cb_user
    integer(c_int)                      :: ndec

    ndec = ft8_decode_frame_dp(iwave, nfa, nfb, ndepth, mycall, hiscall, &
         nqso_progress, nfqso_in, nftx_in, nzhsym_in, nutc, la7final, &
         lft8apon_in, lapcqonly_in, lactivity_in, out, max_out, cb, cb_user, &
         c_null_ptr, 0, c_null_ptr, c_null_ptr, c_null_ptr)
  end function ft8_decode_frame_cb

  !-------------------------------------------------------------------------
  ! ft8_decode_frame_dp : ft8_decode_frame_cb, plus WSJT-X's carry between the passes of one
  !                       period: the full pass starts from audio with the early pass's decodes
  !                       already subtracted, and never searches for or reports them again
  !                       (ft8_decode.f90:121 ndecodes=ndec_early, :152-162). The one decode body;
  !                       every other entry above delegates here.
  !
  !   subs       : NULL, or ft8_sub_t(max_out): for each record written to `out`, what the full
  !                pass needs to subtract it (ok=1 for a direct decode: its tones, f1 and ft8b's
  !                xdt, as WSJT-X saves them at :229-231; ok=0 for an a7 decode, whose tones are not
  !                known here; the carried early decodes' own records, below).
  !   nearly     : how many early decodes the caller carries (0 = none). With 1..F8_MAXDEC, all
  !                three pointers below non-NULL, nzhsym 50 and ndepth >= 2 (WSJT-X runs no early
  !                pass at depth 1, :102-105), this pass starts from the residual:
  !                  dd(1:47*3456)  = resid(1:47*3456), dd(47*3456+1:) = iwave (:153-155);
  !                  each early decode not yet subtracted is subtracted with DT refinement
  !                  (:157-160);
  !                  the early decodes open `out` and allmessages, in their order, so the search
  !                  dedupes against them and never hands one to `cb` again (:121, :219-222);
  !                  on the authoritative pass they enter the a7 table first, as WSJT-X entered
  !                  them during its early pass (:237).
  !                Anything else decodes the raw audio, exactly as ft8_decode_frame_cb.
  !   la7final 2 : Nexus's rescue pass, run after the full pass from the residual: the raw audio
  !                again, as the full pass decoded it before the carry (the 47 step's subtraction
  !                damages a few decodes the raw search finds). `early` then holds the
  !                1..F8_MAXDEC (and <= max_out) records the period's passes have already reported,
  !                and `early_subs` and `resid` are NULL. Those records open `out` and
  !                allmessages, so the search dedupes against them, never hands one to `cb` again
  !                and never saves one into the a7 table a second time: the first time the search
  !                finds one again, its entry takes this raw-audio dt and frequency instead, as the
  !                raw full pass saved them (the early pass's are rougher), and each a7 decode among
  !                them is tried again on this audio, its entry taking this audio's values. A new
  !                decode is saved once, and the a7 replay runs as on the authoritative pass (a pair
  !                already decoded this period is marked and skipped). Without such records, 2 is 1.
  !   early      : ft8_decode_t(nearly), the early pass's records, as it returned them.
  !   early_subs : ft8_sub_t(nearly), their ft8_sub_t from the early pass, `done` set by
  !                ft8_subtract_early for each the residual already has subtracted.
  !   resid      : real(F8_NMAX), ft8_subtract_early's residual.
  !
  ! The caller owns the carry and decides it belongs to this audio: this routine keeps NOTHING
  ! between calls (no SAVE state is added; the per-chain context below is untouched). Nexus's
  ! caller hands it over only when the first 47*3456 samples of `iwave` are the very samples the
  ! residual was made from, so a carry from another period, band, mode or radio chain can never be
  ! spliced onto this audio (crates/ft8, Carry).
  !-------------------------------------------------------------------------
  function ft8_decode_frame_dp(iwave, nfa, nfb, ndepth, mycall, hiscall, &
       nqso_progress, nfqso_in, nftx_in, nzhsym_in, nutc, la7final, &
       lft8apon_in, lapcqonly_in, lactivity_in, out, max_out, cb, cb_user, &
       subs, nearly, early, early_subs, resid) result(ndec) &
       bind(C, name="ft8_decode_frame_dp")
    integer(c_int16_t),     intent(in)  :: iwave(F8_NMAX)
    integer(c_int), value,  intent(in)  :: nfa, nfb, ndepth, nqso_progress, nfqso_in
    integer(c_int), value,  intent(in)  :: nftx_in, nzhsym_in
    integer(c_int), value,  intent(in)  :: nutc, la7final, max_out
    integer(c_int), value,  intent(in)  :: lft8apon_in, lapcqonly_in, lactivity_in
    character(kind=c_char), intent(in)  :: mycall(*)
    character(kind=c_char), intent(in)  :: hiscall(*)
    type(ft8_decode_t),     intent(out) :: out(*)
    type(c_funptr), value,  intent(in)  :: cb
    type(c_ptr),    value,  intent(in)  :: cb_user
    type(c_ptr),    value,  intent(in)  :: subs, early, early_subs, resid
    integer(c_int), value,  intent(in)  :: nearly
    integer(c_int)                      :: ndec

    ! Not initialized in the declaration: that would make it SAVEd module state.
    procedure(ft8_decode_cb), pointer :: fcb
    type(ft8_sub_t),    pointer :: psubs(:), pesub(:)
    type(ft8_decode_t), pointer :: pearly(:)
    real(c_float),      pointer :: presid(:)
    type(ft8_decode_t), target :: rec
    logical :: streaming, stopped, want_subs, from_resid, rescue
    integer(c_int) :: istop
    real              :: dd(F8_NMAX)
    real              :: candidate(3, F8_MAXCAND)
    real              :: sbase(F8_NH1)
    integer           :: apsym(58), aph10(10)
    integer           :: itone(F8_NN), itone_e(F8_NN)
    character(len=37) :: allmessages(F8_MAXDEC)
    ! What this call's own search has found, the reported records it re-finds included: the
    ! rescue pass's pass control counts these, as the raw full pass counts its decodes.
    character(len=37) :: ownmessages(F8_MAXDEC)
    character(len=37) :: msg37, msg
    character(len=12) :: mycall12, hiscall12
    character(len=12) :: call_1, call_2
    character(len=4)  :: grid4
    integer :: ncontest, nfqso, nftx, nzhsym, ndepth_l, ndeep, npass, ipass
    integer :: maxc, ncand, icand, id, n, j, ib, n47
    integer :: iaptype, nharderrors, nbadcrc, iappass
    integer :: nsnr, ndecodes, n2, napwid, nown, ncount
    integer :: i, i1, i2, iz, ndelta
    real    :: sync, f1, xdt, xbase, dmin, xsnr, syncmin, f1_e, xdt_e
    logical :: newdat, lsubtract, ldupe, lft8apon, lapcqonly, nagain, lownnew, lactivity
    logical :: is_new_slot, is_stale

    ndec = 0
    if (max_out <= 0) return
    fcb => null()
    streaming = c_associated(cb)
    if (streaming) call c_f_procpointer(cb, fcb)
    stopped = .false.
    want_subs = c_associated(subs)
    if (want_subs) call c_f_pointer(subs, psubs, [max_out])

    ! The full pass on the early pass's residual, as WSJT-X's (header above). Every condition is
    ! checked before any pointer is followed; failing one is the raw full re-decode.
    ndepth_l = ndepth
    if (ndepth_l <= 0) ndepth_l = 3
    from_resid = nearly >= 1 .and. nearly <= F8_MAXDEC .and. nearly <= max_out .and. &
         nzhsym_in /= 41 .and. ndepth_l >= 2 .and. c_associated(early) .and. &
         c_associated(early_subs) .and. c_associated(resid)
    if (from_resid) then
       call c_f_pointer(early, pearly, [nearly])
       call c_f_pointer(early_subs, pesub, [nearly])
       call c_f_pointer(resid, presid, [F8_NMAX])
    end if
    ! The rescue pass (header above): raw audio, the records already reported carried in `early`.
    rescue = la7final == 2 .and. .not. from_resid .and. nearly >= 1 .and. &
         nearly <= F8_MAXDEC .and. nearly <= max_out .and. nzhsym_in /= 41 .and. &
         c_associated(early) .and. .not. c_associated(early_subs) .and. &
         .not. c_associated(resid)
    if (rescue) call c_f_pointer(early, pearly, [nearly])

    ! Raw audio, unless the caller carries the residual (spliced in below, after the a7
    ! bookkeeping, once ndecodes is known).
    dd(1:F8_NMAX) = real(iwave(1:F8_NMAX))
    call c_to_fstr12(mycall,  mycall12)
    call c_to_fstr12(hiscall, hiscall12)

    ! DELIBERATE DIVERGENCE, DOCUMENTED NOT CLOSED. Upstream derives ncontest
    ! from the operator's Special Operating Activity — decoder.f90:103,
    ! `ncontest=iand(params%nexp_decode,7)`, built at mainwindow.cpp:3796-3802
    ! from int(m_specOp) — with values 0 NONE / 1 NA_VHF / 2 EU_VHF / 3 FIELD
    ! DAY / 4 RTTY / 5 WW_DIGI / 6 FOX / 7 HOUND / 8 ARRL_DIGI (ft8b.f90:
    ! 294-302). We pass the literal 0, and Nexus HAS both live consumers: Field
    ! Day (Mode::FieldDay transmits the i3=3 exchange, so ncontest should be 3)
    ! and Hound (SpecialOp::Hound, ncontest 7 — and ft8b.f90:266 gives Hounds AP
    ! unconditionally, `if(lapon.or.ncontest.eq.7)`). The measured cost is real:
    ! ~1.5-2 dB on the Field Day exchange, because at ncontest=0 the AP masks
    ! (ft8b.f90:315-413) assert i3=1 message-type bits against an i3=3 message.
    !
    ! It is not closed here because making ncontest RUNTIME STATE trips two
    ! things that a one-line change would ship silently:
    !   1. ft8b.f90:51 `if(first.or.(ncontest.ne.ncontest0))` re-runs
    !      `mcq=2*mcq-1` (and its seven siblings). The transform is NOT
    !      idempotent — 0/1 -> -1/+1 on the first fire, then -3/+1 on the second
    !      — so the first entry into or out of Field Day would corrupt every CQ
    !      AP mask for the life of the process. Fixing it means editing a
    !      vendored file (move the +-1 mapping under `first` only; it has no
    !      ncontest dependence). See also the class-1 note at the top of this
    !      module, which files this exact hazard.
    !   2. Upstream's rover and 'TU; ' filter in ft8b also reads ncontest. It
    !      reads `lactivity` (from lactivity_in) here instead, which is already
    !      runtime state, so that filter does not wait on this.
    ncontest  = 0
    ! Center deep AP + sync on the operator's QSO/RX freq when supplied; else the
    ! band midpoint (legacy behavior).
    if (nfqso_in >= nfa .and. nfqso_in <= nfb) then
       nfqso = nfqso_in
    else
       nfqso = (nfa + nfb) / 2
    end if
    ! Second deep-AP window: the operator's TX offset (mainwindow.cpp:3722).
    ! Same in-band guard as nfqso; out of band (or the caller passing 0 because
    ! it has no split) collapses it onto nfqso, which is byte-for-byte the
    ! behavior that shipped before this parameter existed.
    if (nftx_in >= nfa .and. nftx_in <= nfb) then
       nftx = nftx_in
    else
       nftx = nfqso
    end if
    ! 41 = the early partial pass, anything else = the full frame. See the
    ! header block; consumed by syncmin below and by ft8b (npasses).
    if (nzhsym_in == 41) then
       nzhsym = 41
    else
       nzhsym = 50
    end if
    ! Operator AP controls (the decode-config surface). lft8apon is consumed
    ! twice: by ft8b (AP passes 5-8) and by the a7 replay gate below — AP off
    ! must silence BOTH a-priori paths. lapcqonly rides into ft8b only.
    lft8apon  = (lft8apon_in /= 0)
    lapcqonly = (lapcqonly_in /= 0)
    lactivity = (lactivity_in /= 0)
    nagain    = .false.
    ! DELIBERATE DIVERGENCE, MEASURED AND KEPT. Upstream sets napwid=50 for FT8
    ! unconditionally (mainwindow.cpp:3776). Ours is 75, and that is a GAIN, not
    ! an accident: measured 12 seeds/point on "KD9TAW W1AW RR73" at
    ! nqso_progress=4 with AP armed, at carriers 60 Hz and 70 Hz off nfqso (i.e.
    ! inside 75 but outside 50), 75 recovers ~2 dB deeper than 50 — 12/12 vs
    ! 6/12 at -21 dB at 60 Hz — while a 90 Hz control outside BOTH windows is
    ! identical under either setting, which is what pins the difference on
    ! napwid. False-alarm cost 0/200 noise-only frames at both values. Narrowing
    ! to 50 for symmetry would be a pure regression. The only other reader is
    ! ft8b.f90:421-423, whose branch sets maxosd=2 over the initializer of 2 at
    ! :418 and is therefore DEAD in current upstream — do not reason about
    ! napwid from it.
    napwid    = 75

    call ft8apset(mycall12, hiscall12, ncontest, apsym, aph10)

    ! --- a7 cross-cycle slot bookkeeping (mirrors ft8_decode.f90 84-95) -----
    ! On a NEW slot, roll the a7 table for the PREVIOUS slot's parity (module
    ! a7_jseq, still the old value here) from current (k=1) to previous (k=0);
    ! the replay below then reads k=0 = the last same-parity slot's decodes.
    ! A nutc BEHIND nutc0_a7 is a redecode of an OLDER capture (the caller's
    ! F6 path can lag a zero-decode early pass): it must NOT shuffle, re-seed,
    ! or re-key the table — a spurious shuffle here scrambles the even/odd
    ! ping-pong for the following live slots. Deltas <= -43200 (over half a
    ! day back) are a UTC midnight wrap, i.e. genuinely new slots.
    is_stale = .false.
    if (nutc0_a7 < 0) then
       is_new_slot = .true.
    else
       ndelta = nutc - nutc0_a7
       is_new_slot = (ndelta > 0) .or. (ndelta <= -43200)
       is_stale = (ndelta < 0) .and. (ndelta > -43200)
    end if
    if (is_new_slot) then
       iz = a7_ndec(a7_jseq, 1)
       a7_dt0(1:iz, a7_jseq, 0)  = a7_dt0(1:iz, a7_jseq, 1)
       a7_f0(1:iz, a7_jseq, 0)   = a7_f0(1:iz, a7_jseq, 1)
       a7_msg0(1:iz, a7_jseq, 0) = a7_msg0(1:iz, a7_jseq, 1)
       a7_ndec(a7_jseq, 0) = iz
       a7_ndec(a7_jseq, 1) = 0
       a7_dt0(:, a7_jseq, 1) = 0.
       a7_f0(:, a7_jseq, 1)  = 0.
       nutc0_a7 = nutc
    end if
    ! Deterministic parity set (hardening over stock, which relies on
    ! ft8_a7_save to set jseq): equal to what ft8_a7_save would set whenever
    ! >= 1 decode exists, and keeps the ping-pong keyed on zero-decode slots.
    ! With nutc = slot*15 sec-of-day, mod(nutc/5,2) = slot parity.
    if (.not. is_stale) a7_jseq = mod(nutc/5, 2)

    ndecodes = 0
    nown = 0
    allmessages = ' '
    n2 = 0

    ! --- The early pass's carry (ft8_decode.f90:121, :152-162) --------------
    ! The residual from ft8_subtract_early over the first 47*3456 samples, fresh audio after; then
    ! the early decodes the 47 step did not reach, subtracted with DT refinement as WSJT-X's 50
    ! pass does. The early decodes then stand first in allmessages and `out`, so a candidate that
    ! is one of them again is a dupe, never re-reported, and none is handed to `cb`: the early pass
    ! already did. On the authoritative pass they enter the a7 table first (WSJT-X's early pass
    ! entered them at :237); that also marks their pairs as decoded for the replay below
    ! (ft8_a7_save's -98 flag), exactly as the raw full pass's re-decode of them did.
    if (from_resid) then
       n47 = 47 * 3456
       dd(1:n47) = presid(1:n47)
       dd(n47+1:F8_NMAX) = real(iwave(n47+1:F8_NMAX))
       do i = 1, nearly
          if (pesub(i)%done /= 0 .or. pesub(i)%ok == 0) cycle
          itone_e(1:F8_NN) = pesub(i)%itone(1:F8_NN)
          f1_e  = pesub(i)%f1
          xdt_e = pesub(i)%xdt0
          call subtractft8(dd, itone_e, f1_e, xdt_e, .true.)
       end do
    end if
    ! The rescue pass's records open `out` and allmessages the same way, on the raw audio. They
    ! are in the a7 table already (the full pass saved them): not saved again.
    if (from_resid .or. rescue) then
       do i = 1, nearly
          msg37 = ' '
          do j = 1, 37
             if (pearly(i)%message(j) == c_null_char) exit
             msg37(j:j) = pearly(i)%message(j)
          end do
          ndecodes = ndecodes + 1
          allmessages(ndecodes) = msg37
          out(ndecodes) = pearly(i)
          if (want_subs) then
             if (from_resid) then
                psubs(ndecodes) = pesub(i)
             else
                psubs(ndecodes)%itone(1:F8_NN) = 0
                psubs(ndecodes)%f1   = pearly(i)%freq
                psubs(ndecodes)%xdt0 = pearly(i)%dt + 0.5
                psubs(ndecodes)%ok   = 0
                psubs(ndecodes)%done = 0
             end if
          end if
          if (from_resid .and. la7final /= 0 .and. .not. is_stale) then
             call ft8_a7_save(nutc, pearly(i)%dt, pearly(i)%freq, msg37)
          end if
       end do
    end if

    ! Mirror ft8_decode::decode pass logic. ft8b subtracts decoded signals from
    ! dd internally when lsubtract is set, so later passes find weaker stations.
    npass = 3
    if (ndepth_l == 1) npass = 2
    do ipass = 1, npass
       newdat  = .true.
       syncmin = 1.3
       if (ndepth_l <= 2) syncmin = 1.6
       ! The early pass searches only strongly-synced candidates
       ! (ft8_decode.f90:178). Half of what makes upstream's early pass cheap;
       ! the other half is ft8b's npasses clamp, driven by the same nzhsym.
       if (nzhsym == 41) syncmin = 2.0
       if (ipass == 1) then
          lsubtract = .true.
          ndeep = ndepth_l
          if (ndepth_l == 3) ndeep = 2
       else if (ipass == 2) then
          ! The rescue pass runs its passes as the raw full pass would: counting what its own
          ! search found, not the reported records it was handed (they are never new here).
          ncount = ndecodes
          if (rescue) ncount = nown
          n2 = ncount
          if (ncount == 0) cycle
          lsubtract = .true.
          ndeep = ndepth_l
       else
          ncount = ndecodes
          if (rescue) ncount = nown
          if ((ncount - n2) == 0) cycle
          lsubtract = .true.
          ndeep = ndepth_l
       end if

       maxc = F8_MAXCAND
       call sync8(dd, F8_NMAX, nfa, nfb, syncmin, nfqso, maxc, candidate, ncand, sbase)

       do icand = 1, ncand
          sync  = candidate(3, icand)
          f1    = candidate(1, icand)
          xdt   = candidate(2, icand)
          ib    = max(1, nint(f1 / 3.125))
          if (ib > F8_NH1) ib = F8_NH1
          xbase = 10.0 ** (0.1 * (sbase(ib) - 40.0))
          msg37 = ' '
          call ft8b(dd, newdat, nqso_progress, nfqso, nftx, ndeep, nzhsym, lft8apon, &
               lapcqonly, napwid, lsubtract, nagain, ncontest, iaptype, mycall12, &
               hiscall12, f1, xdt, xbase, apsym, aph10, nharderrors, dmin, &
               nbadcrc, iappass, msg37, xsnr, itone, lactivity)
          nsnr = nint(xsnr)
          xdt  = xdt - 0.5
          if (nbadcrc == 0) then
             ldupe = .false.
             do id = 1, nown
                if (msg37 == ownmessages(id)) ldupe = .true.
             end do
             lownnew = .not. ldupe
             if (.not. ldupe .and. nown < F8_MAXDEC) then
                nown = nown + 1
                ownmessages(nown) = msg37
             end if
             ldupe = .false.
             do id = 1, ndecodes
                if (msg37 == allmessages(id)) ldupe = .true.
             end do
             ! The rescue's first find of a station the period already reported: the a7 table
             ! takes this raw-audio decode's dt and frequency for it, as the raw full pass saved
             ! them, in place of the early pass's (no second entry).
             if (ldupe .and. lownnew .and. rescue .and. .not. is_stale) then
                call a7_save_refined(nutc, xdt, f1, msg37)
             end if
             if (.not. ldupe) then
                if (ndecodes >= F8_MAXDEC) cycle
                ndecodes = ndecodes + 1
                allmessages(ndecodes) = msg37
                if (ndecodes <= max_out) then
                   out(ndecodes)%sync = sync
                   out(ndecodes)%snr  = nsnr
                   out(ndecodes)%dt   = xdt
                   out(ndecodes)%freq = f1
                   out(ndecodes)%nap  = iaptype
                   out(ndecodes)%qual = 1.0 - (nharderrors + dmin) / 60.0
                   do j = 1, 38
                      out(ndecodes)%message(j) = c_null_char
                   end do
                   n = min(len_trim(msg37), 37)
                   do j = 1, n
                      out(ndecodes)%message(j) = msg37(j:j)
                   end do
                   ! What the full pass needs to subtract this decode, as WSJT-X keeps it
                   ! (ft8_decode.f90:229-231; xdt_save is the reported dt + 0.5).
                   if (want_subs) then
                      psubs(ndecodes)%itone(1:F8_NN) = itone(1:F8_NN)
                      psubs(ndecodes)%f1   = f1
                      psubs(ndecodes)%xdt0 = xdt + 0.5
                      psubs(ndecodes)%ok   = 1
                      psubs(ndecodes)%done = 0
                   end if
                   if (streaming) then
                      rec = out(ndecodes)
                      istop = fcb(cb_user, c_loc(rec))
                   end if
                end if
                ! Seed the a7 table for the NEXT same-parity slot. Only on the
                ! authoritative pass: the early pass would double-save the same
                ! stations (the wrapper resets allmessages per call, unlike
                ! stock's carried ndec_early), and the boundary pass's full-
                ! audio decode set is a superset of the early one (re-decoded
                ! from raw audio, or carried from the early pass and saved
                ! above). xdt here is already xdt-0.5, matching
                ! ft8_decode.f90 line 232.
                if (la7final /= 0 .and. .not. is_stale) then
                   call ft8_a7_save(nutc, xdt, f1, msg37)
                end if
             end if
          end if
          ! WSJT-X asks its early pass's give-up here, after every candidate
          ! (ft8_decode.f90:240-242); the caller decides when that is.
          if (streaming) then
             if (fcb(cb_user, c_null_ptr) /= 0) then
                stopped = .true.
                exit
             end if
          end if
       end do
       if (stopped) exit
    end do

    ! --- The rescue: the a7 table's entries for the period's a7 decodes --------
    ! The full pass's a7 replay ran over the residual, and ft8_a7d refines the dt and frequency it
    ! saves on the audio it searches. The raw full pass's replay refined them on this audio, and the
    ! next same-parity slot's replay starts from them: each a7 decode the period reported is tried
    ! once more here, from where it was found, and its entry takes this audio's values (one entry
    ! still). Its pair is marked as decoded, so the replay below skips it, as before.
    if (rescue .and. .not. is_stale .and. lft8apon .and. .not. stopped) then
       newdat = .true.
       do i = 1, nearly
          if (pearly(i)%nap /= 7) cycle
          msg37 = ' '
          do j = 1, 37
             if (pearly(i)%message(j) == c_null_char) exit
             msg37(j:j) = pearly(i)%message(j)
          end do
          if (index(msg37, '<') >= 1) cycle
          i1 = index(msg37, ' ')
          i2 = index(msg37(i1+1:), ' ') + i1
          if (i1 < 2 .or. i2 <= i1 + 1) cycle
          call_1 = msg37(1:i1-1)
          call_2 = msg37(i1+1:i2-1)
          grid4 = msg37(i2+1:i2+4)
          ! As the replay below reads it from what ft8_a7_save kept: the grid only when the
          ! third word is one, and never RR73 (which has a grid's form).
          if (len_trim(msg37(i2+1:)) /= 4 .or. grid4 == 'RR73' .or. &
               verify(grid4(1:2), 'ABCDEFGHIJKLMNOPQR') /= 0 .or. &
               verify(grid4(3:4), '0123456789') /= 0) grid4 = '    '
          xdt = pearly(i)%dt
          f1 = pearly(i)%freq
          ib = max(1, nint(f1 / 3.125))
          if (ib > F8_NH1) ib = F8_NH1
          xbase = 10.0 ** (0.1 * (sbase(ib) - 40.0))
          msg = ' '
          call ft8_a7d(dd, newdat, call_1, call_2, grid4, xdt, f1, xbase, &
               nharderrors, dmin, msg, xsnr)
          if (nharderrors >= 0 .and. msg == msg37) call a7_save_refined(nutc, xdt, f1, msg37)
       end do
    end if

    ! --- a7 cross-cycle replay (mirrors ft8_decode.f90 245-278) -------------
    ! Authoritative pass only: replay each call pair remembered from the
    ! PREVIOUS same-parity slot as ~206 QSO-continuation hypotheses against
    ! the post-subtraction residual dd (ft8b's lsubtract already removed the
    ! direct decodes — intended, matches WSJT-X). Acceptance is ft8_a7d's
    ! soft-distance gate (dmin<=100 and dmin2/dmin>=1.3, hypotheses are
    ! constructed so CRC validity is implicit); worst case for a stale pair is
    ! wasted CPU, not a false decode. The dedup against allmessages is a small
    ! safe superset of stock (which relies on downstream GUI dedup) keeping
    ! this wrapper's existing no-duplicate-rows contract.
    if (la7final /= 0 .and. .not. is_stale .and. lft8apon .and. &
         .not. stopped .and. a7_ndec(a7_jseq, 0) >= 1) then
       newdat = .true.
       do i = 1, a7_ndec(a7_jseq, 0)
          if (a7_f0(i, a7_jseq, 0) == -99.0) exit
          if (a7_f0(i, a7_jseq, 0) == -98.0) cycle   ! already worked this slot
          if (index(a7_msg0(i, a7_jseq, 0), '<') >= 1) cycle
          msg37 = a7_msg0(i, a7_jseq, 0)
          i1 = index(msg37, ' ')
          i2 = index(msg37(i1+1:), ' ') + i1
          call_1 = msg37(1:i1-1)
          call_2 = msg37(i1+1:i2-1)
          grid4 = msg37(i2+1:i2+4)
          if (grid4 == 'RR73' .or. index(grid4, '+') > 0 .or. &
               index(grid4, '-') > 0) grid4 = '    '
          xdt = a7_dt0(i, a7_jseq, 0)
          f1 = a7_f0(i, a7_jseq, 0)
          ib = max(1, nint(f1 / 3.125))
          if (ib > F8_NH1) ib = F8_NH1
          xbase = 10.0 ** (0.1 * (sbase(ib) - 40.0))
          msg37 = ' '
          call ft8_a7d(dd, newdat, call_1, call_2, grid4, xdt, f1, xbase, &
               nharderrors, dmin, msg37, xsnr)
          if (nharderrors >= 0) then
             ldupe = .false.
             do id = 1, ndecodes
                if (msg37 == allmessages(id)) ldupe = .true.
             end do
             if (.not. ldupe .and. ndecodes < F8_MAXDEC) then
                ndecodes = ndecodes + 1
                allmessages(ndecodes) = msg37
                if (ndecodes <= max_out) then
                   out(ndecodes)%sync = 0.0
                   out(ndecodes)%snr  = nint(xsnr)
                   out(ndecodes)%dt   = xdt      ! ft8_a7d returns t-0.5 already
                   out(ndecodes)%freq = f1
                   out(ndecodes)%nap  = 7
                   out(ndecodes)%qual = 1.0
                   do j = 1, 38
                      out(ndecodes)%message(j) = c_null_char
                   end do
                   n = min(len_trim(msg37), 37)
                   do j = 1, n
                      out(ndecodes)%message(j) = msg37(j:j)
                   end do
                   ! ft8_a7d returns no tones: an a7 decode cannot be subtracted.
                   if (want_subs) then
                      psubs(ndecodes)%itone(1:F8_NN) = 0
                      psubs(ndecodes)%f1   = f1
                      psubs(ndecodes)%xdt0 = xdt + 0.5
                      psubs(ndecodes)%ok   = 0
                      psubs(ndecodes)%done = 0
                   end if
                   if (streaming) then
                      rec = out(ndecodes)
                      istop = fcb(cb_user, c_loc(rec))
                   end if
                end if
                call ft8_a7_save(nutc, xdt, f1, msg37)
             end if
          end if
       end do
    end if

    ! ndec == max_out here means the cap was hit and the weakest decodes were
    ! dropped: raise F8_MAXDEC and the Rust MAX_DECODES (crates/ft8) together.
    ndec = min(ndecodes, max_out)
  end function ft8_decode_frame_dp

  !-------------------------------------------------------------------------
  ! ft8_subtract_early : WSJT-X's 47 step (ft8_decode.f90:111-113, :124-149). From the period's
  !                      first 47*3456 samples (13.536 s; the rest of the frame is taken as zero,
  !                      as jt9 hands it over, jt9.f90:430-433), subtract each early decode that
  !                      ends inside them (dt < 0.396 s, :135) with DT refinement (none at depth
  !                      2 or less, :131-132), and return the residual the full pass starts from
  !                      (ft8_decode_frame_dp).
  !
  !   iwave      : F8_NMAX int16 samples; only the first 47*3456 are read.
  !   ndepth     : 1..3 (<= 0 means 3).
  !   nearly     : 1..F8_MAXDEC early decodes in early_subs.
  !   early_subs : ft8_sub_t(nearly) as the early pass wrote them. `done` is set for each this
  !                step reached and tried, subtracted or not (lsubtracted, :138): the full pass
  !                subtracts the rest.
  !   resid      : real(F8_NMAX) out, the residual (dd1, :148).
  !   cb         : NULL, or called with a NULL record after each early decode, where WSJT-X reads
  !                the clock for its 14.3 s bail-out (:140-145); a nonzero answer stops there and
  !                the residual is what was done so far, as WSJT-X's.
  !
  ! Returns how many decodes were reached (>= 0), or -1 (nearly out of range: nothing written,
  ! so the caller has no residual). Keeps nothing between calls. Not reentrant.
  !-------------------------------------------------------------------------
  function ft8_subtract_early(iwave, ndepth, nearly, early_subs, resid, cb, cb_user) &
       result(nreached) bind(C, name="ft8_subtract_early")
    integer(c_int16_t),     intent(in)    :: iwave(F8_NMAX)
    integer(c_int), value,  intent(in)    :: ndepth, nearly
    type(ft8_sub_t),        intent(inout) :: early_subs(*)
    real(c_float),          intent(out)   :: resid(F8_NMAX)
    type(c_funptr), value,  intent(in)    :: cb
    type(c_ptr),    value,  intent(in)    :: cb_user
    integer(c_int)                        :: nreached

    ! Not initialized in the declaration: that would make it SAVEd module state.
    procedure(ft8_decode_cb), pointer :: fcb
    real    :: dd(F8_NMAX)
    integer :: itone_e(F8_NN), i, n47, ndepth_l
    real    :: f1_e, xdt_e
    logical :: lrefinedt

    nreached = -1
    if (nearly < 1 .or. nearly > F8_MAXDEC) return
    fcb => null()
    if (c_associated(cb)) call c_f_procpointer(cb, fcb)
    ndepth_l = ndepth
    if (ndepth_l <= 0) ndepth_l = 3
    lrefinedt = .true.
    if (ndepth_l <= 2) lrefinedt = .false.

    n47 = 47 * 3456
    dd(1:n47) = real(iwave(1:n47))
    dd(n47+1:F8_NMAX) = 0.
    nreached = 0
    do i = 1, nearly
       if (early_subs(i)%ok /= 0 .and. early_subs(i)%done == 0 .and. &
            early_subs(i)%xdt0 - 0.5 < 0.396) then
          itone_e(1:F8_NN) = early_subs(i)%itone(1:F8_NN)
          f1_e  = early_subs(i)%f1
          xdt_e = early_subs(i)%xdt0
          call subtractft8(dd, itone_e, f1_e, xdt_e, lrefinedt)
          early_subs(i)%done = 1
       end if
       nreached = i
       if (associated(fcb)) then
          if (fcb(cb_user, c_null_ptr) /= 0) exit
       end if
    end do
    resid(1:F8_NMAX) = dd(1:F8_NMAX)
  end function ft8_subtract_early

  !-------------------------------------------------------------------------
  ! ft8_a7_reset : clear the a7 cross-cycle decode table + slot tracker.
  !                Call on band change / QSO change so stale prior-cycle call
  !                pairs are not replayed as AP hypotheses against the new
  !                band's audio. Mirrors ft1_harq_reset.
  !-------------------------------------------------------------------------
  subroutine ft8_a7_reset() bind(C, name="ft8_a7_reset")
    a7_ndec = 0
    a7_jseq = 0
    a7_dt0  = 0.0
    a7_f0   = 0.0
    a7_msg0 = ' '
    nutc0_a7 = -1
  end subroutine ft8_a7_reset

  !-------------------------------------------------------------------------
  ! a7_save_refined : ft8_a7_save for a station this slot may have saved already: an entry of
  !                   this slot with the same saved form takes this dt and f in place (the table
  !                   keeps one entry per station); else it is saved as new. The saved form is
  !                   ft8_a7_save's own (it is called, and its new entry taken back when it
  !                   matches an earlier one), so the two can never disagree about a match.
  !-------------------------------------------------------------------------
  subroutine a7_save_refined(nutc, dt, f, msg)
    integer, intent(in) :: nutc
    real, intent(in) :: dt, f
    character(len=37), intent(in) :: msg
    integer :: j, n0, n1, i
    j = mod(nutc/5, 2)
    n0 = a7_ndec(j, 1)
    call ft8_a7_save(nutc, dt, f, msg)
    n1 = a7_ndec(j, 1)
    if (n1 == n0 .or. n1 > size(a7_dt0, 1)) return
    do i = 1, n0
       if (a7_msg0(i, j, 1) == a7_msg0(n1, j, 1)) then
          a7_dt0(i, j, 1) = a7_dt0(n1, j, 1)
          a7_f0(i, j, 1)  = a7_f0(n1, j, 1)
          a7_dt0(n1, j, 1) = 0.
          a7_f0(n1, j, 1)  = 0.
          a7_msg0(n1, j, 1) = ' '
          a7_ndec(j, 1) = n0
          return
       end if
    end do
  end subroutine a7_save_refined

  !-------------------------------------------------------------------------
  ! ft8_a7_saved : how many decodes the slot decoded last has saved into the a7 table (its
  !                parity's current entries, ft8_a7_save's ndec(jseq,1)). Read only.
  !-------------------------------------------------------------------------
  function ft8_a7_saved() result(n) bind(C, name="ft8_a7_saved")
    integer(c_int) :: n
    n = a7_ndec(a7_jseq, 1)
  end function ft8_a7_saved

  !=========================================================================
  ! PER-CHAIN DECODER CONTEXT - C entry points
  !
  !   tempo_ctx_size()     bytes one context needs. The caller allocates from
  !                        THIS answer, so a vendor refresh that resizes a
  !                        table cannot silently desync the buffer length.
  !   tempo_ctx_reset(p)   write the load-time image into a caller buffer.
  !                        A fresh context is NOT a zeroed one (see tempo_ctx_t).
  !   tempo_ctx_save(p)    copy the live statics OUT into the buffer.
  !   tempo_ctx_restore(p) copy the buffer IN over the live statics.
  !
  ! The buffer is opaque to the caller: sized by tempo_ctx_size(), aligned for
  ! 8-byte scalars, and only ever handed back to these four routines.
  !
  ! NOT thread-safe, by construction. restore -> decode -> save must happen
  ! under the SAME lock that serializes every other modem FFI call
  ! (tempo_fast_sys::MODEM_LOCK); a decode landing between restore and save
  ! is exactly the corruption this exists to prevent.
  !=========================================================================

  !-------------------------------------------------------------------------
  ! tempo_ctx_size : size of one per-chain context, in bytes.
  !-------------------------------------------------------------------------
  function tempo_ctx_size() result(nbytes) bind(C, name="tempo_ctx_size")
    integer(c_size_t) :: nbytes
    type(tempo_ctx_t), allocatable :: probe
    ! tempo_ctx_t has no default initialization (deliberately - see the type),
    ! so this is a plain malloc, not a 3.4 MiB initializer copy.
    allocate(probe)
    nbytes = int(storage_size(probe) / 8, c_size_t)
    deallocate(probe)
  end function tempo_ctx_size

  !-------------------------------------------------------------------------
  ! tempo_ctx_reset : write the modem's LOAD-TIME state into `ptr`.
  !
  ! Reads no modem state and writes none, so it needs no lock - only the
  ! caller's buffer is touched.
  !-------------------------------------------------------------------------
  subroutine tempo_ctx_reset(ptr) bind(C, name="tempo_ctx_reset")
    type(c_ptr), value, intent(in) :: ptr
    type(tempo_ctx_t), pointer     :: p
    if (.not. c_associated(ptr)) return
    call c_f_pointer(ptr, p)
    call ctx_xfer(p, CTX_INIT)
  end subroutine tempo_ctx_reset

  !-------------------------------------------------------------------------
  ! tempo_ctx_save : live statics -> context buffer.
  !-------------------------------------------------------------------------
  subroutine tempo_ctx_save(ptr) bind(C, name="tempo_ctx_save")
    type(c_ptr), value, intent(in) :: ptr
    type(tempo_ctx_t), pointer     :: p
    if (.not. c_associated(ptr)) return
    call c_f_pointer(ptr, p)
    call ctx_xfer(p, CTX_SAVE)
  end subroutine tempo_ctx_save

  !-------------------------------------------------------------------------
  ! tempo_ctx_restore : context buffer -> live statics.
  !-------------------------------------------------------------------------
  subroutine tempo_ctx_restore(ptr) bind(C, name="tempo_ctx_restore")
    type(c_ptr), value, intent(in) :: ptr
    type(tempo_ctx_t), pointer     :: p
    if (.not. c_associated(ptr)) return
    call c_f_pointer(ptr, p)
    call ctx_xfer(p, CTX_RESTORE)
  end subroutine tempo_ctx_restore

  !-------------------------------------------------------------------------
  ! ctx_xfer : THE ORDERED LIST.
  !
  ! Every symbol in the context appears here exactly ONCE, paired with its live
  ! symbol and its LOAD-TIME value; `mode` picks what the walk does. Save,
  ! restore and reset therefore cannot disagree about the member set, because
  ! there is only one member set to disagree with. To add a symbol: one
  ! component on tempo_ctx_t, one row here. Nowhere else.
  !
  ! The load-time values are the third argument. They are not decoration:
  ! ihash22 = -1 marks a hash slot EMPTY (0 means "slot 0 holds calls22(1)", so
  ! a zero-filled table resolves unknown hashes to a blank callsign), the
  ! callsign tables are blank-filled (NULs would print as a callsign of NULs),
  ! and nutc0_a7 = -1 means "no slot seen yet" (0 is a real slot key).
  !-------------------------------------------------------------------------
  subroutine ctx_xfer(p, mode)
    ! `target` so c_loc can take the address of the opaque harq_pool component.
    type(tempo_ctx_t), intent(inout), target :: p
    integer,           intent(in)            :: mode
    ! packjt.f90:753 getpfx1 - re-declared, not use-associated: it is a COMMON
    ! block, and the linker merges this declaration with that one.
    character(len=8) :: addpfx
    common /pfxcom/ addpfx
    ! The opaque harq_pool, viewed as what it actually holds, so the copy below
    ! is a type-checked harq_slot-to-harq_slot assignment. See tempo_ctx_t.
    type(harq_slot), pointer :: ctx_slots(:)

    call c_f_pointer(c_loc(p%harq_pool), ctx_slots, [size(cxv_slots)])

    !            context component     live symbol    mode   load-time value
    ! --- FT8 ---
    call ctx_move(p%ft8_spec,      cxv_ft8_spec,  mode, (0.0, 0.0))
    call ctx_move(p%a7_dt0,        a7_dt0,        mode, 0.0)
    call ctx_move(p%a7_f0,         a7_f0,         mode, 0.0)
    call ctx_move(p%a7_msg0,       a7_msg0,       mode, ' ')
    call ctx_move(p%a7_ndec,       a7_ndec,       mode, 0)
    call ctx_move(p%a7_jseq,       a7_jseq,       mode, 0)
    call ctx_move(p%cabi_nutc0_a7, nutc0_a7,      mode, -1)
    ! --- FT4 ---
    call ctx_move(p%ft4_spec,      cxv_ft4_spec,  mode, (0.0, 0.0))
    call ctx_move(p%ft4_apbits,    cxv_ft4_apbits,   mode, 0)
    call ctx_move(p%ft4_apmy_ru,   cxv_ft4_apmy_ru,  mode, 0)
    call ctx_move(p%ft4_aphis_fd,  cxv_ft4_aphis_fd, mode, 0)
    call ctx_move(p%ft4_mycall0,   cxv_ft4_mycall0,  mode, ' ')
    call ctx_move(p%ft4_hiscall0,  cxv_ft4_hiscall0, mode, ' ')
    ! --- FT1 / TempoFast ---
    call ctx_move(p%ft1_spec,      cxv_ft1_spec,  mode, (0.0, 0.0))
    call ctx_move(ctx_slots,       cxv_slots,     mode)
    call ctx_move(p%harq_init,     cxv_harq_init, mode, .false.)
    call ctx_move(p%ft1_apbits,    cxv_ft1_apbits,   mode, 0)
    call ctx_move(p%ft1_mycall0,   cxv_ft1_mycall0,  mode, ' ')
    call ctx_move(p%ft1_hiscall0,  cxv_ft1_hiscall0, mode, ' ')
    ! --- shared 77-bit / callsign machinery ---
    call ctx_move(p%calls10,       cxv_calls10,   mode, ' ')
    call ctx_move(p%calls12,       cxv_calls12,   mode, ' ')
    call ctx_move(p%calls22,       cxv_calls22,   mode, ' ')
    call ctx_move(p%recent_calls,  cxv_recent,    mode, ' ')
    call ctx_move(p%mycall13,      cxv_mycall13,  mode, ' ')
    call ctx_move(p%dxcall13,      cxv_dxcall13,  mode, ' ')
    call ctx_move(p%ihash22,       cxv_ihash22,   mode, -1)
    call ctx_move(p%nzhash,        cxv_nzhash,    mode, 0)
    call ctx_move(p%mycall13_0,    cxv_mycall13_0,   mode, ' ')
    call ctx_move(p%dxcall13_0,    cxv_dxcall13_0,   mode, ' ')
    call ctx_move(p%mycall13_set,  cxv_mycall13_set, mode, .false.)
    call ctx_move(p%dxcall13_set,  cxv_dxcall13_set, mode, .false.)
    call ctx_move(p%hashmy10,      cxv_hashmy10,  mode, 0)
    call ctx_move(p%hashmy12,      cxv_hashmy12,  mode, 0)
    call ctx_move(p%hashmy22,      cxv_hashmy22,  mode, 0)
    call ctx_move(p%hashdx10,      cxv_hashdx10,  mode, 0)
    call ctx_move(p%hashdx12,      cxv_hashdx12,  mode, 0)
    call ctx_move(p%hashdx22,      cxv_hashdx22,  mode, 0)
    call ctx_move(p%addpfx,        addpfx,        mode, ' ')
  end subroutine ctx_xfer

  ! --- ctx_move specifics. `a` is always the context side, `b` the live symbol.
  !     One per (type, rank) the context holds; the generic resolves by type and
  !     rank, so a component that drifts from its symbol fails to compile.

  subroutine ctx_move_c1(a, b, mode, init)      ! complex, rank 1
    complex, intent(inout) :: a(:), b(:)
    integer, intent(in)    :: mode
    complex, intent(in)    :: init
    select case (mode)
    case (CTX_SAVE)    ; a = b
    case (CTX_RESTORE) ; b = a
    case (CTX_INIT)    ; a = init
    end select
  end subroutine ctx_move_c1

  subroutine ctx_move_r3(a, b, mode, init)      ! real, rank 3
    real,    intent(inout) :: a(:,:,:), b(:,:,:)
    integer, intent(in)    :: mode
    real,    intent(in)    :: init
    select case (mode)
    case (CTX_SAVE)    ; a = b
    case (CTX_RESTORE) ; b = a
    case (CTX_INIT)    ; a = init
    end select
  end subroutine ctx_move_r3

  subroutine ctx_move_a3(a, b, mode, init)      ! character, rank 3
    character(len=*), intent(inout) :: a(:,:,:), b(:,:,:)
    integer,          intent(in)    :: mode
    character(len=*), intent(in)    :: init
    select case (mode)
    case (CTX_SAVE)    ; a = b
    case (CTX_RESTORE) ; b = a
    case (CTX_INIT)    ; a = init
    end select
  end subroutine ctx_move_a3

  subroutine ctx_move_i2(a, b, mode, init)      ! integer, rank 2
    integer, intent(inout) :: a(:,:), b(:,:)
    integer, intent(in)    :: mode, init
    select case (mode)
    case (CTX_SAVE)    ; a = b
    case (CTX_RESTORE) ; b = a
    case (CTX_INIT)    ; a = init
    end select
  end subroutine ctx_move_i2

  subroutine ctx_move_i1(a, b, mode, init)      ! integer, rank 1
    integer, intent(inout) :: a(:), b(:)
    integer, intent(in)    :: mode, init
    select case (mode)
    case (CTX_SAVE)    ; a = b
    case (CTX_RESTORE) ; b = a
    case (CTX_INIT)    ; a = init
    end select
  end subroutine ctx_move_i1

  subroutine ctx_move_i0(a, b, mode, init)      ! integer scalar
    integer, intent(inout) :: a, b
    integer, intent(in)    :: mode, init
    select case (mode)
    case (CTX_SAVE)    ; a = b
    case (CTX_RESTORE) ; b = a
    case (CTX_INIT)    ; a = init
    end select
  end subroutine ctx_move_i0

  subroutine ctx_move_l0(a, b, mode, init)      ! logical scalar
    logical, intent(inout) :: a, b
    integer, intent(in)    :: mode
    logical, intent(in)    :: init
    select case (mode)
    case (CTX_SAVE)    ; a = b
    case (CTX_RESTORE) ; b = a
    case (CTX_INIT)    ; a = init
    end select
  end subroutine ctx_move_l0

  subroutine ctx_move_a1(a, b, mode, init)      ! character, rank 1
    character(len=*), intent(inout) :: a(:), b(:)
    integer,          intent(in)    :: mode
    character(len=*), intent(in)    :: init
    select case (mode)
    case (CTX_SAVE)    ; a = b
    case (CTX_RESTORE) ; b = a
    case (CTX_INIT)    ; a = init
    end select
  end subroutine ctx_move_a1

  subroutine ctx_move_a0(a, b, mode, init)      ! character scalar
    character(len=*), intent(inout) :: a, b
    integer,          intent(in)    :: mode
    character(len=*), intent(in)    :: init
    select case (mode)
    case (CTX_SAVE)    ; a = b
    case (CTX_RESTORE) ; b = a
    case (CTX_INIT)    ; a = init
    end select
  end subroutine ctx_move_a0

  ! type(harq_slot), rank 1. No `init` argument: an empty slot is not one value
  ! but a whole record, and `empty` below IS that record - default-initialized
  ! exactly as ir_harq_combine_mod declares it (active=.false., rv_count=-1,
  ! snr_est=-99.0, npts=HARQ_NDMAX), with the sample buffers zeroed so a fresh
  ! context is byte-deterministic. Mirrors the vendored harq_init().
  subroutine ctx_move_h1(a, b, mode)
    type(harq_slot), intent(inout) :: a(:), b(:)
    integer,         intent(in)    :: mode
    type(harq_slot) :: empty
    integer :: i
    select case (mode)
    case (CTX_SAVE)    ; a = b
    case (CTX_RESTORE) ; b = a
    case (CTX_INIT)
       empty%cd_rv0 = (0.0, 0.0)
       empty%cd_rv1 = (0.0, 0.0)
       empty%cd_rv2 = (0.0, 0.0)
       do i = 1, size(a)
          a(i) = empty
       end do
    end select
  end subroutine ctx_move_h1

  !-------------------------------------------------------------------------
  ! c_to_fstr12 : marshal a NUL/space-terminated C string into character(12),
  !               space-padded (as the FT8 decoder expects callsigns).
  !-------------------------------------------------------------------------
  subroutine c_to_fstr12(cstr, fstr)
    character(kind=c_char), intent(in)  :: cstr(*)
    character(len=12),      intent(out) :: fstr
    integer :: i
    fstr = ' '
    do i = 1, 12
       if (cstr(i) == c_null_char) exit
       fstr(i:i) = cstr(i)
    end do
  end subroutine c_to_fstr12

end module ft8_cabi
