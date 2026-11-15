! Nexus: C ABI for the JTDX-style FT4 decoder (vendor/wsjtx/lib/ft4jtdx/, JTDX 2.2.159's FT4
! decoder). The Standard FT4 decoder (WSJT-X 3.0.2, ft4_cabi.f90) stays linked beside it as the
! fallback; the two share no state but the 77-bit packer's callsign tables, which every FT mode
! shares.
!
! This file does what JTDX's decoder.f90 does around ft4_decode for one period
! (decoder.f90:2328-2352 at jtdx-project/jtdx 2a0e2bea8), minus what Nexus does not have:
!   * the audio goes into module ft4_mod1's dd4 (73728 samples; Nexus's frame is 72576, the rest
!     is zero, as JTDX reads a WAV file);
!   * mycall/hiscall go into module ft8_mod1, where the decoder and chkfalse8var read them;
!   * nFT4decd and sumxdtt(1) are cleared, the decoder runs, and then the running average DT
!     `avexdt` is updated from this period's decodes exactly as JTDX does. The next period's DT
!     search is centred on it (ft4_decode_jtdx.f90's ibottom), which is the one piece of JTDX's
!     FT4 state that carries from period to period. It lives in ft8_mod1 beside the JTDX-style
!     FT8 decoder's, as in JTDX, and is reset on a mode change (ft4_jtdx_reset).
! Not done, deliberately: JTDX's AGC compensation (agccft4, a GUI option off by default), the
! partial-period filler (Nexus never decodes a late-started period), the +/-95 Hz filter (Nexus's
! single-decode setting narrows nfa/nfb instead), the hidden test and
! telemetry message filters (passed .false.: Nexus shows every decode), and JTDX's fillhash.
!
! THE CALLSIGN DATABASE MUST BE LOADED FIRST. The false-decode checks this decoder calls
! (chkflscallvar, chkfalse8var) look calls up in ALLCALL7.TXT. With the database not yet read
! and not marked invalid they find nothing, so every check rejects: CQ decodes below -15 dB are
! dropped and a `<call> <call> R <grid>` decode abandons the rest of the period. ft4_jtdx_decode
! therefore loads it on its first call, whether or not FT8 has ever run.
!
! NOT thread-safe: module state throughout; callers serialize behind the modem lock.

module ft4jtdx_cabi
  use iso_c_binding
  use ft4_decode_jtdx, only: ft4_decoder_jtdx
  use ft4_cabi, only: ft4_decode_t, ft4_decode_cb, F4_NMAX
  implicit none
  private
  public :: ft4_jtdx_decode_frame_cb, ft4_jtdx_reset

  integer, parameter :: DD4_LEN = 73728   ! ft4_mod1's dd4 (JTDX's ft4_params NMAX)

  ! The caller's callback rides on the decoder object, one per call: no module state.
  type, extends(ft4_decoder_jtdx) :: ft4jtdx_stream_decoder
     type(c_funptr) :: cb
     type(c_ptr)    :: user
     type(c_ptr)    :: out_ptr
     integer        :: max_out = 0
     integer        :: count = 0
  end type ft4jtdx_stream_decoder

contains

  !-------------------------------------------------------------------------
  ! ft4_jtdx_collect : the decoder's callback. Each decode is written to the caller's `out`
  ! (while it has room) and handed to the caller's callback the moment it is found.
  !-------------------------------------------------------------------------
  subroutine ft4_jtdx_collect(this, snr, dt, freq, decoded, servis4, nap)
    class(ft4_decoder_jtdx), intent(inout) :: this
    integer,           intent(in) :: snr
    real,              intent(in) :: dt
    real,              intent(in) :: freq
    character(len=37), intent(in) :: decoded
    character(len=1),  intent(in) :: servis4
    integer,           intent(in) :: nap

    type(ft4_decode_t), pointer :: out(:)
    type(ft4_decode_t), target  :: rec
    procedure(ft4_decode_cb), pointer :: fcb
    integer(c_int) :: istop
    integer :: j, n

    select type (this)
    type is (ft4jtdx_stream_decoder)
       this%count = this%count + 1
       ! JTDX's decoder reports no sync metric and no quality figure. Every decode it reports
       ! passed the CRC (both of its LDPC decoders check it), so quality is reported as 1.0.
       rec%sync = 0.0
       rec%snr  = snr
       rec%dt   = dt
       rec%freq = freq
       rec%nap  = nap
       rec%qual = 1.0
       rec%message = c_null_char
       n = min(len_trim(decoded), 37)
       do j = 1, n
          rec%message(j) = decoded(j:j)
       end do
       if (this%count <= this%max_out) then
          call c_f_pointer(this%out_ptr, out, [this%max_out])
          out(this%count) = rec
       end if
       if (c_associated(this%cb)) then
          call c_f_procpointer(this%cb, fcb)
          istop = fcb(this%user, c_loc(rec))
       end if
    end select
    ! servis4 is JTDX's free-text display hint (near/far the RX frequency); Nexus does not use it.
  end subroutine ft4_jtdx_collect

  !-------------------------------------------------------------------------
  ! ft4_jtdx_decode_frame_cb : decode every FT4 signal in one 72576-sample period with the
  ! JTDX-style decoder.
  !
  !   iwave          : F4_NMAX int16 samples @ 12 kHz
  !   nfa, nfb       : search band (Hz)
  !   ndepth         : 1 = one pass, no subtraction; 2 = three passes, no OSD; 3 = full (<=0 -> 3)
  !   mycall,hiscall : NUL-terminated callsigns for AP (may be empty)
  !   nqso_progress  : QSO progress 0..5 (AP pass schedule)
  !   nfqso_in       : RX audio frequency; out of [nfa,nfb] -> band centre
  !   stophint       : nonzero = AP restricted to the CQ hypothesis (JTDX's "stop hint"; the
  !                    same meaning as the Standard decoder's lapcqonly)
  !   swl            : nonzero = JTDX's SWL mode: a wider DT search, about -1.2..+1.7 s where
  !                    the default is -1.0..+1.4 s (both shifted by the running average DT)
  !   out, max_out   : caller array of ft4_decode_t
  !   cb, cb_user    : tempo_decode_cb or NULL; handed each decode as it is found
  !
  ! Returns the number of decodes found (may exceed max_out; `out` holds the first max_out).
  !-------------------------------------------------------------------------
  function ft4_jtdx_decode_frame_cb(iwave, nfa, nfb, ndepth, mycall, hiscall, &
       nqso_progress, nfqso_in, stophint, swl, out, max_out, cb, cb_user) result(ndec) &
       bind(C, name="ft4_jtdx_decode_frame_cb")
    use ft4_mod1, only: dd4, nFT4decd, lfilter, lhidetest, lhidetelemetry, llagcc
    use ft8_mod1, only: sumxdtt, avexdt, ft8_mycall => mycall, ft8_hiscall => hiscall
    integer(c_int16_t),     intent(in)  :: iwave(F4_NMAX)
    integer(c_int), value,  intent(in)  :: nfa, nfb, ndepth, nqso_progress, nfqso_in, max_out
    integer(c_int), value,  intent(in)  :: stophint, swl
    character(kind=c_char), intent(in)  :: mycall(*)
    character(kind=c_char), intent(in)  :: hiscall(*)
    type(ft4_decode_t),     intent(out), target :: out(*)
    type(c_funptr), value,  intent(in)  :: cb
    type(c_ptr),    value,  intent(in)  :: cb_user
    integer(c_int)                      :: ndec

    type(ft4jtdx_stream_decoder) :: decoder
    integer :: nfqso, ndepth_l, n
    logical(1) :: lstop, lswl
    real :: sumxdt

    ndec = 0
    if (max_out <= 0) return
    call ft4_jtdx_load_callsign_db()

    decoder%cb = cb
    decoder%user = cb_user
    decoder%out_ptr = c_loc(out(1))
    decoder%max_out = max_out
    decoder%count = 0

    dd4 = 0.
    dd4(1:F4_NMAX) = real(iwave(1:F4_NMAX))
    call c_to_fstr12(mycall,  ft8_mycall)
    call c_to_fstr12(hiscall, ft8_hiscall)
    lfilter = .false.; lhidetest = .false.; lhidetelemetry = .false.; llagcc = .false.

    ndepth_l = ndepth
    if (ndepth_l <= 0 .or. ndepth_l > 3) ndepth_l = 3
    if (nfqso_in >= nfa .and. nfqso_in <= nfb) then
       nfqso = nfqso_in
    else
       nfqso = (nfa + nfb) / 2
    end if
    lstop = (stophint /= 0)
    lswl = (swl /= 0)

    nFT4decd = 0; sumxdtt(1) = 0.0
    decoder%callback => ft4_jtdx_collect
    call decoder%decode(ft4_jtdx_collect, nqso_progress, nfqso, nfa, nfb, ndepth_l, lstop, lswl)

    ! decoder.f90:2343-2351 (ndelay = 0): the running average DT the next period centres on.
    n = nFT4decd
    sumxdt = sumxdtt(1)
    if (n > 5) then
       avexdt = (avexdt + sumxdt/n)/2
    else if (n == 5) then
       avexdt = (1.1*avexdt + 0.9*sumxdt/n)/2
    else if (n == 4) then
       avexdt = (1.25*avexdt + 0.75*sumxdt/n)/2
    else if (n == 3) then
       avexdt = (1.35*avexdt + 0.65*sumxdt/n)/2
    else if (n == 2) then
       avexdt = (1.5*avexdt + 0.5*sumxdt/n)/2
    end if

    ndec = decoder%count
  end function ft4_jtdx_decode_frame_cb

  !-------------------------------------------------------------------------
  ! ft4_jtdx_reset : what JTDX does on a mode change (decoder.f90:88): the running average DT
  ! starts again from 0. Call it when the radio changes mode.
  !-------------------------------------------------------------------------
  subroutine ft4_jtdx_reset() bind(C, name="ft4_jtdx_reset")
    use ft8_mod1, only: avexdt
    avexdt = 0.
  end subroutine ft4_jtdx_reset

  !-------------------------------------------------------------------------
  ! ft4_jtdx_load_callsign_db : read ALLCALL7.TXT once, through the JTDX-style FT8 decoder's
  ! loader, so both decoders share one copy. See the header for why this cannot wait for FT8.
  !-------------------------------------------------------------------------
  subroutine ft4_jtdx_load_callsign_db()
    use ft8var_cabi, only: ft8var_load_callsign_db
    integer(c_int) :: nvalid
    ! Idempotent: reads the file on the first call only. With no file where ft8var_set_share_dir
    ! pointed (or nowhere set), it marks the database invalid and the checks then pass every
    ! call, which loses JTDX's filtering but never rejects a good decode.
    nvalid = ft8var_load_callsign_db()
  end subroutine ft4_jtdx_load_callsign_db

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

end module ft4jtdx_cabi
