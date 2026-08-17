! Nexus: C ABI wrapper for native FT2 decode + encode, built on the vendored
! Decodium GPL modem sources (lib/ft2 family).
!
! ⭐ WHICH FT2 THIS IS — there are two, and they are incompatible. This is
! DECODIUM's FT2 (IU8LMC, decodium3-build, a WSJT-X 3.0.0-rc1 fork): FT4 with a
! halved symbol time. LDPC(174,91)+CRC14, four 4x4 Costas arrays, 4-GFSK at
! 41.67 baud (NSPS=288 @ 12 kHz), 105 channel symbols = 2.52 s of audio in a
! 3.75 s T/R period. Upstream WSJT-X also carries a lib/ft2 — an ABANDONED
! MSK144-derived experiment (LDPC 128,90, NSPS=160, 2.5 s) that never shipped
! and shares nothing but the name. The on-air FT2 population runs Decodium, so
! Decodium is the compatibility baseline.
!
! ⭐ THE DECODE ENTRY POINT IS ft2_triggered_decode, NOT the OO ft2_decode
! module. Decodium's own runtime calls ft2_triggered_decode straight from C++
! (mainwindow.cpp:19319, inside QtConcurrent::run) — a flat subroutine, no
! callback, no derived types: hand it NMAX=45000 int16 samples (3.75 s at
! 12 kHz), get back up to `nout` formatted text lines. The OO path
! (ft2_async_decode + the ft2_decode module) adds a procedure-pointer callback
! ABI and module-level multi-period averaging state for nothing this wrapper
! needs.
!
! ⭐ OUTPUT IS A FORMATTED TEXT LINE, read back with an internal READ on
! upstream's own format 1001 (ft2_triggered_decode.f90:428):
!     format(i4,f5.1,i5,' ~ ',1x,a37,1x,a2)
! = snr, dt, freq Hz, a literal ' ~ ' marker, the 37-char message, and a 2-char
! annotation: 'T ' for the non-AP passes, 'a1'..'a4' for an a-priori decode of
! that AP type. The annotation maps onto the struct's dtype slot.
!
! ⭐ outlines IS A FIXED 100x80 CHARACTER BLOCK, AND THE CALLEE BLANKS ALL OF
! IT. ft2_triggered_decode.f90:66 does `outlines = ' '` over the full dummy
! extent (100 lines, hardcoded in its declaration) before any decode, so the
! buffer here MUST be exactly that shape — a smaller actual argument is memory
! corruption, not a truncation. In practice nout <= 50 (MAXHITS), but the
! wrapper caps its copy loop anyway.
!
! ⭐ DEAD ARGUMENTS, KEPT FOR THE SIGNATURE: nqsoprogress and ncontest are
! declared by ft2_triggered_decode and never referenced in its body (verified
! against the vendored source). This wrapper pins both to 0.
!
! TRANSMIT AND RECEIVE. ft2_encode_msg (genft2, ichk=0) yields the 103
! sync+data tones; ft2_gen_wave (gen_ft2wave) yields the 30240-sample waveform
! — (nsym+2)*nsps: gen_ft2wave itself appends the raised-cosine ramp-up and
! ramp-down symbols, which is the NN=103 vs NN2=105 distinction. The caller
! sizes for NN2.
!
! NOT thread-safe: the decode chain keeps process-global SAVE state (the
! init-once Costas/window tables, and ft2_downsample's big-FFT buffer reused
! across candidates within one call). Classified in modem-state-manifest.toml;
! run under the same modem lock as every other vendored mode.

module ft2_cabi
  use iso_c_binding
  implicit none

  integer, parameter :: FT2_NMAX     = 45000  ! decode window: 3.75 s @ 12 kHz
  integer, parameter :: FT2_NN       = 103    ! sync+data symbols (genft2's output)
  integer, parameter :: FT2_NN2      = 105    ! + ramp-up/down, what hits the air
  integer, parameter :: FT2_NSPS     = 288    ! samples/symbol -> 41.67 baud
  integer, parameter :: FT2_NWAVE    = FT2_NN2 * FT2_NSPS  ! 30240 = 2.52 s
  integer, parameter :: FT2_MAXLINES = 100    ! outlines dummy extent - ABI, fixed

  ! ft2_triggered_decode.f90:428, format 1001 — the contract this wrapper reads
  ! back. 4x skips the literal ' ~ ' plus its trailing 1x.
  character(len=*), parameter :: FMT_1001 = '(i4,f5.1,i5,4x,a37,1x,a2)'

  ! Interop result struct. Layout MUST match ft2_decode_t in libtempo.h, and is
  ! byte-compatible with the ft8/ft4/msk144/q65 records (64 bytes, 4-byte aligned).
  type, bind(C) :: ft2_decode_t
     ! FT2's line carries no sync metric; always 0.0, kept for record shape.
     real(c_float)          :: sync
     integer(c_int)         :: snr
     real(c_float)          :: dt
     real(c_float)          :: freq
     character(kind=c_char) :: message(38)
     ! 0 = ordinary decode ('T '), 1..4 = a-priori decode of AP type n ('a<n>').
     integer(c_int)         :: dtype
     integer(c_int)         :: reserved
  end type ft2_decode_t

contains

  !-------------------------------------------------------------------------
  ! ft2_decode_frame : decode every FT2 signal in one 3.75 s window.
  !
  !   iwave    : FT2_NMAX int16 samples @ 12 kHz. Always the full window — the
  !              callee declares iwave(NMAX) and reads all of it.
  !   nfa, nfb : search band (Hz), upstream semantics (candidate scan limits).
  !   ndepth   : 1..3; >=3 loosens the sync/OSD thresholds the way Decodium's
  !              deep setting does. <=0 defaults to 3.
  !   mycall   : NUL/space-terminated callsign for the AP passes (may be empty;
  !              empty disables the mycall AP types).
  !   hiscall  : ditto; empty disables AP types that need the DX call.
  !   nfqso    : QSO audio frequency (Hz) — candidate ordering follows it.
  !   out      : caller array of ft2_decode_t (capacity max_out)
  !   max_out  : capacity of out
  !
  !   Returns the number of decodes (>= 0). No error path: a bad parse of one
  !   line skips that line.
  !-------------------------------------------------------------------------
  function ft2_decode_frame(iwave, nfa, nfb, ndepth, mycall, hiscall, &
       nfqso, out, max_out) result(ndec) bind(C, name="ft2_decode_frame")
    integer(c_int16_t),     intent(in)  :: iwave(*)
    integer(c_int), value,  intent(in)  :: nfa, nfb, ndepth, nfqso, max_out
    character(kind=c_char), intent(in)  :: mycall(*)
    character(kind=c_char), intent(in)  :: hiscall(*)
    type(ft2_decode_t),     intent(out) :: out(*)
    integer(c_int)                      :: ndec

    integer(kind=2)   :: iw(FT2_NMAX)
    character(len=12) :: mycall_f, hiscall_f
    character(len=80) :: outlines(FT2_MAXLINES)
    character(len=37) :: msg
    character(len=2)  :: annot
    real              :: xdt
    integer           :: nout, ndepth_l, nsnr, nfreq, ios, j, k, n

    ndec = 0
    if (max_out <= 0) return

    iw = iwave(1:FT2_NMAX)
    call c_to_fstr12_ft2(mycall, mycall_f)
    call c_to_fstr12_ft2(hiscall, hiscall_f)

    ndepth_l = ndepth
    if (ndepth_l <= 0) ndepth_l = 3

    outlines = ' '
    nout = 0
    ! nqsoprogress=0 and ncontest=0: dead arguments, see the banner.
    call ft2_triggered_decode(iw, 0, nfqso, nfa, nfb, ndepth_l, 0, &
         mycall_f, hiscall_f, outlines, nout)

    if (nout > FT2_MAXLINES) nout = FT2_MAXLINES
    do j = 1, nout
       if (ndec >= max_out) exit
       read(outlines(j), FMT_1001, iostat=ios) nsnr, xdt, nfreq, msg, annot
       if (ios /= 0) cycle
       ndec = ndec + 1
       out(ndec)%sync     = 0.0
       out(ndec)%snr      = nsnr
       out(ndec)%dt       = xdt
       out(ndec)%freq     = real(nfreq)
       out(ndec)%dtype    = annot_code(annot)
       out(ndec)%reserved = 0
       do k = 1, 38
          out(ndec)%message(k) = c_null_char
       end do
       n = len_trim(msg)
       if (n > 37) n = 37
       do k = 1, n
          out(ndec)%message(k) = msg(k:k)
       end do
    end do
  end function ft2_decode_frame

  !-------------------------------------------------------------------------
  ! annot_code : the line's 2-char annotation -> the struct's dtype.
  ! 'T ' (and anything unrecognised) -> 0; 'a1'..'a4' -> 1..4.
  !-------------------------------------------------------------------------
  integer function annot_code(annot)
    character(len=*), intent(in) :: annot
    integer :: v, ios
    annot_code = 0
    if (annot(1:1) == 'a') then
       read(annot(2:2), '(i1)', iostat=ios) v
       if (ios == 0 .and. v >= 1 .and. v <= 4) annot_code = v
    end if
  end function annot_code

  !-------------------------------------------------------------------------
  ! ft2_encode_msg : message text -> the 103 FT2 channel tones (0..3).
  !
  !   msg       : message text (C string, <= 37 chars)
  !   msg_len   : length of msg
  !   itone_out : caller buffer, FT2_NN entries
  !   returns   : FT2_NN on success, -1 if the message will not pack
  !
  ! Straight into the vendored genft2 (ichk=0). Failure detection follows
  ! genft2's own contract: pack77 leaves i3/n3 negative when the message cannot
  ! be placed — genft2 does not surface those, but it DOES return msgsent, and a
  ! message that failed to pack round-trips as something other than itself.
  !-------------------------------------------------------------------------
  function ft2_encode_msg(msg, msg_len, itone_out) result(nsym_out) &
       bind(C, name="ft2_encode_msg")
    character(kind=c_char), intent(in)  :: msg(*)
    integer(c_int), value,  intent(in)  :: msg_len
    integer(c_int),         intent(out) :: itone_out(FT2_NN)
    integer(c_int) :: nsym_out

    character(len=37) :: msg37, msgsent37
    integer           :: itone(FT2_NN)
    integer(kind=1)   :: msgbits(77)
    integer           :: i, n

    msg37 = ' '
    n = min(msg_len, 37)
    do i = 1, n
       if (msg(i) == c_null_char) exit
       msg37(i:i) = msg(i)
    end do

    itone = 0
    msgbits = 0
    msgsent37 = ' '
    call genft2(msg37, 0, msgsent37, msgbits, itone)
    if (msgsent37 == ' ') then
       nsym_out = -1
       return
    end if
    itone_out(1:FT2_NN) = itone(1:FT2_NN)
    nsym_out = FT2_NN
  end function ft2_encode_msg

  !-------------------------------------------------------------------------
  ! ft2_gen_wave : channel tones -> real audio at 12 kHz.
  !
  !   itone     : the FT2_NN tones from ft2_encode_msg
  !   nsym      : symbol count (must be FT2_NN)
  !   f0        : audio carrier (Hz)
  !   wave_out  : caller buffer (capacity nwave_cap)
  !   nwave_cap : capacity
  !   returns   : FT2_NWAVE samples produced, or -1 on refusal
  !
  ! gen_ft2wave writes (nsym+2)*nsps samples — it appends the raised-cosine
  ! ramp-up/ramp-down symbols itself — so the caller sizes for FT2_NN2, not
  ! FT2_NN. hmod=1.0 4-GFSK: tone spacing = baud = 41.67 Hz, ~167 Hz occupied.
  !
  ! ⚠️ gen_ft2wave caches its Gaussian pulse from the FIRST call's nsps
  ! (save pulse/first). Every caller must pass FT2_NSPS; this wrapper is the
  ! only caller and pins it.
  !-------------------------------------------------------------------------
  function ft2_gen_wave(itone, nsym, f0, wave_out, nwave_cap) &
       result(nwave_out) bind(C, name="ft2_gen_wave")
    integer(c_int),        intent(in)    :: itone(*)
    integer(c_int), value, intent(in)    :: nsym, nwave_cap
    real(c_float),  value, intent(in)    :: f0
    real(c_float),         intent(inout) :: wave_out(*)
    integer(c_int) :: nwave_out

    integer :: it(FT2_NN)
    real    :: wave(FT2_NWAVE)
    complex :: cwave(1)   ! unused with icmplx=0; a real buffer keeps the ABI honest
    integer :: i

    nwave_out = -1
    if (nsym /= FT2_NN) return
    if (nwave_cap < FT2_NWAVE) return

    it = itone(1:FT2_NN)
    wave = 0.0
    call gen_ft2wave(it, FT2_NN, FT2_NSPS, 12000.0, f0, cwave, wave, 0, FT2_NWAVE)
    do i = 1, FT2_NWAVE
       wave_out(i) = wave(i)
    end do
    nwave_out = FT2_NWAVE
  end function ft2_gen_wave

  !-------------------------------------------------------------------------
  ! c_to_fstr12_ft2 : marshal a NUL/space-terminated C string into character(12).
  ! Suffixed so it cannot collide with the other *_cabi copies.
  !-------------------------------------------------------------------------
  subroutine c_to_fstr12_ft2(cstr, fstr)
    character(kind=c_char), intent(in)  :: cstr(*)
    character(len=12),      intent(out) :: fstr
    integer :: i
    fstr = ' '
    do i = 1, 12
       if (cstr(i) == c_null_char) exit
       fstr(i:i) = cstr(i)
    end do
  end subroutine c_to_fstr12_ft2

end module ft2_cabi
