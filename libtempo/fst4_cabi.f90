! Nexus: C ABI wrapper for native FST4 decode, built on the vendored WSJT-X GPL
! modem sources (lib/fst4 + fst4_decode.f90). Modelled on ft4_cabi.f90, not
! ft8_cabi.f90: like FT4, the WSJT-X FST4 decoder (fst4_decode::decode) is a clean
! self-contained OO decoder driven through a callback, with no nzhsym streaming
! ladder, no a7 cross-cycle table and no shared memory. So it is driven directly
! via a collector callback, exactly as ft4_cabi does.
!
! RX-ONLY, DELIBERATELY. There is no fst4_encode / fst4_gen_wave here. FST4 is
! being added as a decode-only mode: `Capabilities { tx: false }` on the Rust side
! means modes::tx_mode() refuses to hand it to the transmit path. Adding TX means
! adding genfst4 + a gen_fst4wave wrapper here AND flipping that flag AND passing
! the FT-mode TX hard gate — three deliberate steps, not an oversight.
!
! (gen_fst4wave IS compiled into libtempo regardless: fst4_decode calls it to
! regenerate and subtract a decoded signal, so it is on the RECEIVE path.)
!
! Underlying Fortran:
!   fst4_decode  (fst4_decode.f90)  - OO decoder: get_candidates_fst4 -> sync_fst4
!                                     -> fst4_downsample -> get_fst4_bitmetrics
!                                     -> decode240_101 / decode240_74 (+OSD)
!
! ⭐ THE FRAME LENGTH IS PERIOD-DEPENDENT, unlike FT8/FT4.
!   fst4_decode.f90:176-210 sizes nmax from ntrperiod:
!       ntrperiod   15    30    60    120    300     900     1800  (seconds)
!       nmax    180000 360000 720000 1440000 3600000 10800000 21600000  (samples)
!   This wrapper exposes the 15 s period ONLY (FST4_NMAX = 180000, the same frame
!   length as FT8). Supporting the rest means either a per-period entry point or a
!   caller-supplied buffer length, plus the Rust-side ModeKind work to express a
!   selectable period — deferred until a second period is actually wanted. Passing
!   any other ntrperiod would read past the end of a 180000-sample buffer, so the
!   period is fixed here rather than exposed as an argument that could be wrong.

module fst4_cabi
  use iso_c_binding
  use fst4_decode, only: fst4_decoder
  implicit none

  integer, parameter :: FST4_NTRPERIOD = 15               ! seconds; see banner
  integer, parameter :: FST4_NMAX      = 15 * 12000        ! 180000 samples @ 12 kHz
  integer, parameter :: FST4_MAXDEC    = 100               ! matches MAXCAND in fst4_decode

  ! Interop result struct. Layout MUST match fst4_decode_t in libtempo.h.
  ! Identical to ft4_decode_t (64 bytes) so the Rust side can share a shape.
  type, bind(C) :: fst4_decode_t
     real(c_float)          :: sync
     integer(c_int)         :: snr
     real(c_float)          :: dt
     real(c_float)          :: freq
     character(kind=c_char) :: message(38)
     integer(c_int)         :: nap
     real(c_float)          :: qual
  end type fst4_decode_t

  ! Module-level results buffer populated by the collector callback.
  ! NOT thread-safe: fst4_decode_frame() must not be called concurrently.
  type :: fst4_result_rec
     real    :: sync
     integer :: snr
     real    :: dt
     real    :: freq
     character(len=37) :: message
     integer :: nap
     real    :: qual
  end type fst4_result_rec

  type(fst4_result_rec), save :: gs4_results(FST4_MAXDEC)
  integer,               save :: gs4_count = 0

contains

  !-------------------------------------------------------------------------
  ! fst4_collect_cb : collector matching the fst4_decode_callback interface.
  !
  ! FST4's callback carries four arguments FT4's does not: nutc, ntrperiod, and
  ! fmid/w50 (the Doppler-spread pair). fmid/w50 are dropped here — upstream only
  ! ever populates them from dopspread, which was a plotting aid gated on a
  ! `plotspec` marker file and is excised in the headless build, so both now carry
  ! the -999.0 "not measured" sentinel. Nothing in Nexus consumes them.
  !-------------------------------------------------------------------------
  subroutine fst4_collect_cb(this, nutc, sync, nsnr, dt, freq, decoded, &
       nap, qual, ntrperiod, fmid, w50)
    class(fst4_decoder), intent(inout) :: this
    integer,           intent(in) :: nutc
    real,              intent(in) :: sync
    integer,           intent(in) :: nsnr
    real,              intent(in) :: dt
    real,              intent(in) :: freq
    character(len=37), intent(in) :: decoded
    integer,           intent(in) :: nap
    real,              intent(in) :: qual
    integer,           intent(in) :: ntrperiod
    real,              intent(in) :: fmid
    real,              intent(in) :: w50

    if (gs4_count >= FST4_MAXDEC) return
    gs4_count = gs4_count + 1
    gs4_results(gs4_count)%sync    = sync
    gs4_results(gs4_count)%snr     = nsnr
    gs4_results(gs4_count)%dt      = dt
    gs4_results(gs4_count)%freq    = freq
    gs4_results(gs4_count)%message = decoded
    gs4_results(gs4_count)%nap     = nap
    gs4_results(gs4_count)%qual    = qual
  end subroutine fst4_collect_cb

  !-------------------------------------------------------------------------
  ! fst4_decode_frame : decode EVERY FST4 signal in a 180000-sample frame.
  !
  !   iwave         : FST4_NMAX (180000) int16 audio samples @ 12 kHz (15 s)
  !   nfa, nfb      : frequency search band edges (Hz)
  !   ndepth        : 1..3 (3 = full bp+osd; <=0 defaults to 3)
  !   mycall,hiscall: NUL/space-terminated callsigns for AP (may be empty)
  !   nqso_progress : QSO progress index (AP pass schedule)
  !   nfqso_in      : QSO/RX audio freq (Hz) being worked. Pass 0 / out-of-band
  !                   => band centre.
  !   out           : caller array of fst4_decode_t (capacity max_out)
  !   max_out       : capacity of out
  !
  !   Returns the number of decodes found (>= 0), or -1 on error.
  !
  ! NOT thread-safe (the modem keeps process-global SAVE state + FFTW plans). The
  ! per-chain subset is classified in modem-state-manifest.toml GROUP G: 33
  ! symbols, 106,060 bytes, almost all of it in fst4_decode.f90.
  !-------------------------------------------------------------------------
  function fst4_decode_frame(iwave, nfa, nfb, ndepth, mycall, hiscall, &
       nqso_progress, nfqso_in, out, max_out) result(ndec) &
       bind(C, name="fst4_decode_frame")
    integer(c_int16_t),     intent(in)  :: iwave(FST4_NMAX)
    integer(c_int), value,  intent(in)  :: nfa, nfb, ndepth, nqso_progress
    integer(c_int), value,  intent(in)  :: nfqso_in, max_out
    character(kind=c_char), intent(in)  :: mycall(*)
    character(kind=c_char), intent(in)  :: hiscall(*)
    type(fst4_decode_t),    intent(out) :: out(*)
    integer(c_int)                      :: ndec

    type(fst4_decoder) :: decoder
    integer(kind=2)    :: iwave_l(FST4_NMAX)
    character(len=12)  :: mycall_f, hiscall_f
    integer            :: nfqso, ndepth_l, i, j, n, ncopy
    integer            :: nutc, nexp_decode, ntol, iwspr
    real               :: emedelay
    logical            :: lagain, lapcqonly, lprinthash22

    ndec = 0
    if (max_out <= 0) return

    gs4_count = 0
    iwave_l(1:FST4_NMAX) = int(iwave(1:FST4_NMAX), kind=2)
    call c_to_fstr12_fst4(mycall,  mycall_f)
    call c_to_fstr12_fst4(hiscall, hiscall_f)

    ndepth_l = ndepth
    if (ndepth_l <= 0) ndepth_l = 3
    ! Centre the deep AP passes on the operator's QSO/RX freq when supplied,
    ! else band mid — same rule as ft4_cabi.
    if (nfqso_in >= nfa .and. nfqso_in <= nfb) then
       nfqso = nfqso_in
    else
       nfqso = (nfa + nfb) / 2
    end if

    ! Fixed arguments, matching upstream decoder.f90:1245 for the FST4 (not FST4W)
    ! path. nutc is a slot label used only for the diagnostic dump that the
    ! headless build excised, so 0 is safe. iwspr=0 selects FST4 QSO mode; iwspr=1
    ! would select FST4W beacon mode, which needs the hashed-callsign table that
    ! the excised fst4w_calls.txt read used to populate.
    nutc         = 0
    nexp_decode  = 0
    ntol         = 20
    emedelay     = 0.0
    lagain       = .false.
    lapcqonly    = .false.
    iwspr        = 0
    lprinthash22 = .false.

    decoder%callback => fst4_collect_cb
    call decoder%decode(fst4_collect_cb, iwave_l, nutc, nqso_progress, &
         nfa, nfb, nfqso, ndepth_l, FST4_NTRPERIOD, nexp_decode, ntol, &
         emedelay, lagain, lapcqonly, mycall_f, hiscall_f, iwspr, lprinthash22)

    ! ncopy == max_out means the cap was hit and decodes were dropped: raise
    ! FST4_MAXDEC and the Rust-side MAX_DECODES together.
    ncopy = min(gs4_count, max_out)
    do i = 1, ncopy
       out(i)%sync = gs4_results(i)%sync
       out(i)%snr  = gs4_results(i)%snr
       out(i)%dt   = gs4_results(i)%dt
       out(i)%freq = gs4_results(i)%freq
       out(i)%nap  = gs4_results(i)%nap
       out(i)%qual = gs4_results(i)%qual
       do j = 1, 38
          out(i)%message(j) = c_null_char
       end do
       n = len_trim(gs4_results(i)%message)
       if (n > 37) n = 37
       do j = 1, n
          out(i)%message(j) = gs4_results(i)%message(j:j)
       end do
    end do

    ndec = gs4_count
  end function fst4_decode_frame

  !-------------------------------------------------------------------------
  ! c_to_fstr12_fst4 : marshal a NUL/space-terminated C string into character(12).
  ! Module-scoped and suffixed so it cannot collide with ft4_cabi/ft8_cabi's copies.
  !-------------------------------------------------------------------------
  subroutine c_to_fstr12_fst4(cstr, fstr)
    character(kind=c_char), intent(in)  :: cstr(*)
    character(len=12),      intent(out) :: fstr
    integer :: i
    fstr = ' '
    do i = 1, 12
       if (cstr(i) == c_null_char) exit
       fstr(i:i) = cstr(i)
    end do
  end subroutine c_to_fstr12_fst4

end module fst4_cabi
