! Nexus: C ABI wrapper for native JT65 decode, built on the vendored WSJT-X GPL
! modem sources (jt65_decode.f90 + lib/ftrsd).
!
! JT65 is the classic weak-signal / EME mode: 65-tone MFSK, one tone per 372 ms,
! carrying a 72-bit message through a (63,12) Reed-Solomon code. It predates the
! 77-bit era, which shows up in two places a caller must know about:
!
!   * MESSAGES ARE 22 CHARACTERS, not 37. jt65_decode's callback hands back
!     `character(len=22)` from the LEGACY packjt layer, not packjt77. The 38-byte
!     message field in the interop struct is shared with the other modes and is
!     simply never filled past 22 here.
!   * SUBMODES ARE A/B/C, passed as 0/1/2, and the decoder squares them:
!     `mode65 = 2**nsubmode` at jt65_decode.f90:179, giving tone spacings 1x/2x/4x.
!     Wider spacing survives more Doppler spread, which is why EME operators move
!     up the letters as the path degrades.
!
! ⭐ THE FRAME IS 60 s BUT ONLY 52 s ARE DECODED.
!   dd0 is an explicit-shape dummy at NZMAX = 60*12000 = 720000
!   (jt65_decode.f90:3, :52), so the caller must supply the full 720000 samples —
!   but upstream passes npts = 52*12000 = 624000 (decoder.f90:1311), and this
!   wrapper does the same. The tail is buffer the decoder never reads. Passing
!   fewer than 720000 samples is what the explicit-shape dummy forbids, so this is
!   the one place where "supply more than is read" is the correct contract rather
!   than waste. No vendor divergence was needed for it.
!
! ⭐ EVERY CALL IS INDEPENDENT — clearave is pinned .true.
!   JT65 supports multi-period message averaging (avg65 at jt65_decode.f90:344),
!   accumulating across calls exactly as Q65 does. Pinning clearave clears those
!   arrays at the top of every decode so frame N is never influenced by frames
!   1..N-1. Same reasoning as q65_cabi: without it a batch of frames contaminates
!   itself, which is the trap that cost a calibration round on the FT8 side.
!
! ⭐ NOT KVASD. JT65's historical Reed-Solomon decoder was the non-free KVASD
!   binary, shelled out to as a subprocess. This build uses ftrsd — the
!   Franke-Taylor soft-decision decoder written to replace it — and neither ships
!   nor invokes KVASD. See NOTICE: the ftrsd RS codec is Phil Karn's, under a
!   separate and explicitly stated GPL grant.
!
! RX-ONLY, DELIBERATELY. There is no JT65 encode or gen_wave here. The Rust
! ModeKind reports `Capabilities { tx: false }` so modes::tx_mode() refuses the
! transmit path.

module jt65_cabi
  use iso_c_binding
  use jt65_decode, only: jt65_decoder
  implicit none

  integer, parameter :: JT65_NMAX    = 60 * 12000   ! 720000 — the buffer contract
  integer, parameter :: JT65_NPTS    = 52 * 12000   ! 624000 — what is actually read
  integer, parameter :: JT65_MAXDEC  = 50           ! matches dec(50) in jt65_decode
  integer, parameter :: JT65_NSUBMODES = 3          ! A, B, C as 0, 1, 2

  ! JT65 channel symbols per transmission: 63 sync + 63 data, interleaved by
  ! gen65's `nprc` vector. Upstream's NUM_JT65_SYMBOLS.
  integer, parameter :: JT65_NN = 126

  ! Interop result struct. Layout MUST match jt65_decode_t in libtempo.h, and is
  ! byte-compatible with the other modes' records (64 bytes, 4-byte aligned).
  type, bind(C) :: jt65_decode_t
     real(c_float)          :: sync
     integer(c_int)         :: snr
     real(c_float)          :: dt
     real(c_float)          :: freq
     character(kind=c_char) :: message(38)   ! only 22 are ever used — see banner
     integer(c_int)         :: ft            ! decode type: 1 = RS, 2 = deep search
     integer(c_int)         :: qual          ! deep-search confidence; 0 for an RS decode
  end type jt65_decode_t

  type :: jt65_result_rec
     real    :: sync
     integer :: snr
     real    :: dt
     real    :: freq
     character(len=22) :: message
     integer :: ft
     integer :: qual
  end type jt65_result_rec

  type(jt65_result_rec), save :: gj_results(JT65_MAXDEC)
  integer,               save :: gj_count = 0

contains

  !-------------------------------------------------------------------------
  ! jt65_collect_cb : collector matching the jt65_decode_callback interface
  !                   (jt65_decode.f90:12-30).
  !
  ! JT65's callback is the widest in this tree — 13 values. Four are dropped:
  !   drift  Hz/min frequency drift. Genuinely useful for EME and worth surfacing
  !          one day, but the shared 64-byte record has two spare int slots and
  !          ft/qual answer the more important question (how the decode was
  !          obtained and how much to trust it).
  !   nflip  whether the sync vector was inverted; an internal detail.
  !   width  fitted signal width; diagnostic.
  !   nsmo / nsum / minsync  echoes of what we passed in.
  !-------------------------------------------------------------------------
  subroutine jt65_collect_cb(this, sync, snr, dt, freq, drift, nflip, width, &
       decoded, ft, qual, nsmo, nsum, minsync)
    class(jt65_decoder), intent(inout) :: this
    real,              intent(in) :: sync
    integer,           intent(in) :: snr
    real,              intent(in) :: dt
    integer,           intent(in) :: freq
    integer,           intent(in) :: drift
    integer,           intent(in) :: nflip
    real,              intent(in) :: width
    character(len=22), intent(in) :: decoded
    integer,           intent(in) :: ft
    integer,           intent(in) :: qual
    integer,           intent(in) :: nsmo
    integer,           intent(in) :: nsum
    integer,           intent(in) :: minsync

    if (gj_count >= JT65_MAXDEC) return
    gj_count = gj_count + 1
    gj_results(gj_count)%sync    = sync
    gj_results(gj_count)%snr     = snr
    gj_results(gj_count)%dt      = dt
    gj_results(gj_count)%freq    = real(freq)
    gj_results(gj_count)%message = decoded
    gj_results(gj_count)%ft      = ft
    gj_results(gj_count)%qual    = qual
  end subroutine jt65_collect_cb

  !-------------------------------------------------------------------------
  ! jt65_decode_frame : decode EVERY JT65 signal in one 60 s T/R period.
  !
  !   iwave      : JT65_NMAX (720000) int16 audio samples @ 12 kHz. The full 60 s
  !                is required even though only the first 52 s are read — dd0 is
  !                an explicit-shape dummy.
  !   nsubmode   : 0, 1 or 2 for JT65A/B/C. Anything else => -1.
  !   nfa, nfb   : frequency search band edges (Hz)
  !   ndepth     : 1..3. Also sets the Reed-Solomon trial budget; see below.
  !   mycall/hiscall : NUL/space-terminated callsigns for AP (may be empty)
  !   hisgrid    : NUL/space-terminated 6-char grid for AP (may be empty)
  !   nfqso      : QSO/RX audio freq (Hz). 0 / out of band => band centre.
  !   out        : caller array of jt65_decode_t (capacity max_out)
  !   max_out    : capacity of out
  !
  !   Returns the number of decodes found (>= 0), or -1 on error.
  !
  ! NOT thread-safe: jt65_mod holds the shared symbol-spectra state and the
  ! decoder keeps SAVE state besides. Classified in modem-state-manifest.toml.
  !-------------------------------------------------------------------------
  function jt65_decode_frame(iwave, nsubmode, nfa, nfb, ndepth, mycall, &
       hiscall, hisgrid, nfqso_in, out, max_out) result(ndec) &
       bind(C, name="jt65_decode_frame")
    integer(c_int16_t),     intent(in)  :: iwave(JT65_NMAX)
    integer(c_int), value,  intent(in)  :: nsubmode, nfa, nfb, ndepth
    integer(c_int), value,  intent(in)  :: nfqso_in, max_out
    character(kind=c_char), intent(in)  :: mycall(*)
    character(kind=c_char), intent(in)  :: hiscall(*)
    character(kind=c_char), intent(in)  :: hisgrid(*)
    type(jt65_decode_t),    intent(out) :: out(*)
    integer(c_int)                      :: ndec

    type(jt65_decoder) :: decoder
    ! ALLOCATABLE: 720000 reals is 2.9 MB, which is more than belongs on the stack
    ! next to the decoder's own frames.
    real, allocatable  :: dd(:)
    character(len=12)  :: mycall_f, hiscall_f
    character(len=6)   :: hisgrid_f
    integer            :: nfqso, ndepth_l, i, j, n, ncopy
    integer            :: nutc, ntol, minsync, n2pass, ntrials, naggressive
    integer            :: nexp_decode, nqso_progress
    real               :: emedelay
    logical            :: newdat, nagain, nrobust, clearave, ljt65apon

    ndec = 0
    if (max_out <= 0) return

    if (nsubmode < 0 .or. nsubmode >= JT65_NSUBMODES) then
       ndec = -1
       return
    end if

    gj_count = 0
    allocate(dd(JT65_NMAX))
    ! Upstream does exactly this widening at decoder.f90:1328
    ! (`dd(1:npts65)=id2(1:npts65)`); the tail beyond JT65_NPTS is never read but
    ! is zeroed so the buffer is never indeterminate.
    dd = 0.0
    dd(1:JT65_NPTS) = real(iwave(1:JT65_NPTS))

    call c_to_fstr_jt65(mycall,  mycall_f, 12)
    call c_to_fstr_jt65(hiscall, hiscall_f, 12)
    call c_to_fstr_jt65(hisgrid, hisgrid_f, 6)

    ndepth_l = ndepth
    if (ndepth_l <= 0) ndepth_l = 3
    if (nfqso_in >= nfa .and. nfqso_in <= nfb) then
       nfqso = nfqso_in
    else
       nfqso = (nfa + nfb) / 2
    end if

    ! Fixed arguments. Upstream derives most of these from the GUI's params block
    ! (decoder.f90:1332-1338); the values chosen here and why:
    !   newdat   .true.  — this frame is new audio, not a re-decode of the last.
    !   nutc     0       — only a slot label for output formatting.
    !   ntol     nfb-nfa — the caller's band IS the tolerance, matching how the
    !            other mode ABIs here treat nfa/nfb.
    !   minsync  0       — no sync-strength floor; let the decoder decide.
    !   nagain   .false. — this is not upstream's manual "decode again" button.
    !   n2pass   2       — two passes with signal subtraction between them, so a
    !            strong signal does not mask a weak one. Upstream's default.
    !   nrobust  .false. — robust mode is an EME-specific sync variant.
    !   ntrials          — the Reed-Solomon stochastic trial budget, scaled from
    !            ndepth below. Upstream computes it from nranera the same way
    !            (decoder.f90:140-142): more trials find more decodes and cost
    !            time roughly linearly.
    !   naggressive 0    — no aggressive deep-search threshold lowering.
    !   emedelay 0.0     — no EME path delay compensation.
    !   clearave .true.  — see the banner: every frame stands alone.
    !   nexp_decode 0    — not the experimental decoder.
    !   ljt65apon .true. — AP enabled, but it does nothing when mycall/hiscall are
    !            empty, which is how a blind parity run gets a fair comparison.
    newdat        = .true.
    nutc          = 0
    ntol          = max(20, nfb - nfa)
    minsync       = 0
    nagain        = .false.
    n2pass        = 2
    nrobust       = .false.
    naggressive   = 0
    emedelay      = 0.0
    clearave      = .true.
    nexp_decode   = 0
    nqso_progress = 0
    ljt65apon     = .true.

    select case (ndepth_l)
    case (:1)
       ntrials = 100
    case (2)
       ntrials = 1000
    case default
       ntrials = 10000
    end select

    decoder%callback => jt65_collect_cb
    call decoder%decode(jt65_collect_cb, dd, JT65_NPTS, newdat, nutc,       &
         nfa, nfb, nfqso, ntol, nsubmode, minsync, nagain, n2pass, nrobust, &
         ntrials, naggressive, ndepth_l, emedelay, clearave, mycall_f,      &
         hiscall_f, hisgrid_f, nexp_decode, nqso_progress, ljt65apon)
    deallocate(dd)

    ncopy = min(gj_count, max_out)
    do i = 1, ncopy
       out(i)%sync = gj_results(i)%sync
       out(i)%snr  = gj_results(i)%snr
       out(i)%dt   = gj_results(i)%dt
       out(i)%freq = gj_results(i)%freq
       out(i)%ft   = gj_results(i)%ft
       out(i)%qual = gj_results(i)%qual
       do j = 1, 38
          out(i)%message(j) = c_null_char
       end do
       n = len_trim(gj_results(i)%message)
       if (n > 22) n = 22
       do j = 1, n
          out(i)%message(j) = gj_results(i)%message(j:j)
       end do
    end do

    ndec = gj_count
  end function jt65_decode_frame

  !-------------------------------------------------------------------------
  ! c_to_fstr_jt65 : marshal a NUL/space-terminated C string into a Fortran
  ! character of the requested length. Suffixed so it cannot collide with the
  ! other *_cabi copies.
  !-------------------------------------------------------------------------
  subroutine c_to_fstr_jt65(cstr, fstr, nlen)
    character(kind=c_char), intent(in)  :: cstr(*)
    character(len=*),       intent(out) :: fstr
    integer,                intent(in)  :: nlen
    integer :: i
    fstr = ' '
    do i = 1, min(nlen, len(fstr))
       if (cstr(i) == c_null_char) exit
       fstr(i:i) = cstr(i)
    end do
  end subroutine c_to_fstr_jt65

  !-------------------------------------------------------------------------
  ! jt65_encode_msg : message text -> the 126 JT65 channel symbols.
  !
  !   msg       : message text (C string). ⭐ AT MOST 22 CHARACTERS — JT65
  !               predates 77-bit and uses the LEGACY packjt layer, not
  !               packjt77's 37. Anything longer is the caller's bug.
  !   msg_len   : length of msg
  !   itone_out : caller buffer, JT65_NN entries
  !   returns   : JT65_NN on success, -1 if the message will not pack
  !
  ! Wraps the vendored gen65, which is already BIND(c) upstream. Unlike every
  ! other mode here, gen65 was NOT already compiled: Q65/FST4/WSPR/MSK144 all had
  ! their encoders linked in because their DECODERS call them to regenerate a
  ! candidate, and JT65's decoder does not. gen65 + chkmsg were vendored for this.
  !
  ! Everything gen65 needs was already present, though: packjt (packmsg/unpackmsg),
  ! interleave63, graycode65, and rs_encode from Karn's wrapkarn.c — the same
  ! Reed-Solomon codec the JT65 DECODER already uses, so encode and decode share
  ! one RS implementation.
  !
  ! ⭐ TONE VALUES ARE 0 OR 2..65, NOT 0..64. gen65 emits tone 0 for the 63 SYNC
  ! symbols (positions fixed by its `nprc` pseudo-random vector) and `sent(k)+2`
  ! for the 63 data symbols. The +2 offset is real and part of the wire format.
  !-------------------------------------------------------------------------
  function jt65_encode_msg(msg, msg_len, itone_out) result(nsym_out) &
       bind(C, name="jt65_encode_msg")
    character(kind=c_char), intent(in)  :: msg(*)
    integer(c_int), value,  intent(in)  :: msg_len
    integer(c_int),         intent(out) :: itone_out(JT65_NN)
    integer(c_int) :: nsym_out

    ! ⭐ gen65 is BIND(c) UPSTREAM (gen65.f90:1), so its symbol is the unmangled
    ! `gen65`, not gfortran's `gen65_`. Without this explicit interface the call
    ! below emits `gen65_` and fails at LINK time with an undefined symbol —
    ! which is the good failure, but only because nothing else in the tree
    ! happens to define that name.
    interface
       subroutine gen65(msg00, ichk, msgsent0, itone, itype) bind(C)
         import :: c_char, c_int
         character(kind=c_char) :: msg00(23), msgsent0(23)
         integer(c_int) :: ichk, itone(126), itype
       end subroutine gen65
    end interface

    character(kind=c_char) :: msg23(23), msgsent23(23)
    integer(c_int) :: itone(JT65_NN)
    integer(c_int) :: ichk, itype
    integer :: i, n

    nsym_out = -1
    msg23 = ' '
    n = min(msg_len, 22)
    do i = 1, n
       if (msg(i) == c_null_char) exit
       msg23(i) = msg(i)
    end do
    msg23(23) = char(0)

    ichk = 0
    itype = 0
    itone = -1
    call gen65(msg23, ichk, msgsent23, itone, itype)

    ! itype < 0 is gen65's "this message will not pack" signal; a surviving -1 in
    ! itone means nothing was generated.
    if (itype < 0) return
    if (any(itone(1:JT65_NN) < 0)) return

    itone_out(1:JT65_NN) = itone(1:JT65_NN)
    nsym_out = JT65_NN
  end function jt65_encode_msg

end module jt65_cabi
