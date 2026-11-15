! Nexus: C ABI for WSJT-X 3.0.2's multithreaded FT8 decoder (lib/ft8var), the WSJT-X port of
! JTDX's decoder (see NOTICE). WSJT-X runs it from lib/decoder.f90 when Decode > "Use multithreaded
! FT8 decoder" is on (params%lmultift8); Nexus has no decoder.f90, so this module re-expresses that
! dispatch (decoder.f90:110-120, 188-350 and 1061-1136 at v3.0.2) around one call per period, in
! the same order, with the same state, and the same callback (decoder.f90:1598-1697,
! ft8_decodedvar). Every "decoder.f90:N" below is a line of WSJT-X v3.0.2 lib/decoder.f90.
!
! What is NOT here, and why:
!   - The incomplete-period path (ndelay > 0, partintft8, decoder.f90:198-203): Nexus never
!     decodes a late-started period as one, so ndelay is always 0.
!   - Band change mid-period (nsecbandchanged, :205-212): 0. Nexus drops that period.
!   - The single-decode window (nagainfil, :286-298): Nexus's F6 re-decode runs the standard
!     decoder. Passed as .false.
!   - The early decode steps: WSJT-X forces the standard decoder for them
!     (mainwindow.cpp:5559-5562), and so does Nexus. This runs only on the full pass.
!   - JTDX's thread count is applied exactly as decoder.f90:255-279 computes it, including the
!     frequency slices of :351-1060. Those 12 hand-written branches are one loop here; the slice
!     edges are upstream's, quirk included: at 11 and 12 threads upstream adds `nfdelt`, an
!     undeclared (so zero) variable, for the next-to-last edge (:873, :970), which leaves that
!     slice empty and doubles the last one. Kept, so N threads here decode as WSJT-X's N do.
!
! THREADS AND STATE. With more than one thread, the threads share dd8 and subtract from it with
! no lock (upstream, decoder.f90 :352-1060): the result varies run to run. With one thread it is
! deterministic for the same sequence of periods. All state is process-global Fortran module
! storage (ft8_mod1, packjt77's *var tables, jt65_mod9, this module), swapped per radio chain by
! the per-chain context in ft8_cabi.f90 (see modem-state-manifest.toml). The caller holds the
! modem lock, and the thread that calls in needs tempo_fast_sys::OPENMP_THREAD_STACK of stack
! (this file and ft8var are compiled -fopenmp: their local arrays live on the stack).

! decoder.f90's SAVEd locals that the ft8var path reads across calls (decoder.f90:60-66, `save`),
! in a module of their own so ft8_cabi.f90's per-chain context can name them (ft8_cabi cannot use
! ft8var_cabi: that would be a cycle).
module ft8var_dispatch_state
  implicit none
  logical    :: firstsd = .true.
  logical(1) :: lhoundprev = .false.
  integer    :: nintcount = 0
  ! The decoding thread's copy of packjt77's recent_callsvar, which is `!$omp threadprivate`
  ! (thread-local): copied in before each decode and out after it, so the per-radio context
  ! (ft8_cabi.f90) can carry it whatever thread a decode runs on. Sized as packjt77's.
  character(len=13) :: recent_shadow(10) = ''
end module ft8var_dispatch_state

module ft8var_cabi
  use iso_c_binding
  use ft8_decodevar
  use ft8var_dispatch_state
  implicit none

  ! The decode record (libtempo.h ft8var_decode_t): ft8_decode_t's fields, then `unchecked`, 1 for
  ! a message no error check accepted (ft8var_hint.f90; JTDX's '^'). 68 bytes. The callback is
  ! tempo_decode_cb's shape over this record (libtempo.h ft8var_decode_cb).
  type, bind(C) :: ft8var_decode_t
     real(c_float)          :: sync
     integer(c_int)         :: snr
     real(c_float)          :: dt
     real(c_float)          :: freq
     character(kind=c_char) :: message(38)
     integer(c_int)         :: nap
     real(c_float)          :: qual
     integer(c_int)         :: unchecked
  end type ft8var_decode_t
  abstract interface
     function ft8var_decode_cb(user, rec) bind(C) result(istop)
       import :: c_ptr, c_int
       type(c_ptr), value :: user
       type(c_ptr), value :: rec
       integer(c_int)     :: istop
     end function ft8var_decode_cb
  end interface

  ! decoder.f90:44-47
  type, extends(ft8_decodervar) :: counting_ft8_decodervar
     integer :: decodedvar
     real :: xdtt(200)
  end type counting_ft8_decodervar

  ! Everything one decode takes besides the audio. Layout MUST match ft8var_params_t in
  ! libtempo.h (41 4-byte fields, then the 12/12/12/12/6-byte strings and 2 pad bytes: 220 bytes). Logical
  ! fields are int 0/1. The decoder.f90 / mainwindow.cpp names are in the comments.
  type, bind(C) :: ft8var_params_t
     integer(c_int) :: nzhsym          ! samples decoded = nzhsym*3456 (jt9.f90:470-480; WSJT-X 49)
     integer(c_int) :: nutc            ! period start, hhmmss (params%nutc)
     integer(c_int) :: nfa, nfb        ! search band, Hz
     integer(c_int) :: nfqso           ! RX frequency, Hz
     integer(c_int) :: nftx            ! TX frequency, Hz
     integer(c_int) :: nqso_progress   ! params%nQSOProgress
     integer(c_int) :: napwid          ! AP window, Hz (mainwindow.cpp:5511-5513)
     integer(c_int) :: nmt             ! threads: 0 = auto, 1..12 (params%nmt)
     integer(c_int) :: nft8cycles      ! decoding cycles 1..3
     integer(c_int) :: nft8rxfsens     ! QSO RX-frequency sensitivity 1..3
     integer(c_int) :: ncandthin       ! candidate thinning, percent (100 = keep all)
     integer(c_int) :: ndtcenter       ! DT centre, 1/100 s (WSJT-X: always 0)
     integer(c_int) :: nstophint       ! 0 = hint decoders on (WSJT-X: always 0)
     integer(c_int) :: ncontest        ! 0, or 7 as a Hound (only gates the a8 pass)
     integer(c_int) :: lft8lowth       ! low sync thresholds (FT8 sensitivity 2 and 3)
     integer(c_int) :: lft8subpass     ! the metric sub-pass (FT8 sensitivity 3)
     integer(c_int) :: lft8apon        ! Enable AP
     integer(c_int) :: lhideft8dupes   ! hide duplicate decodes
     integer(c_int) :: lmycallstd      ! my call is a standard call
     integer(c_int) :: lhiscallstd     ! the DX call is a standard call
     integer(c_int) :: lhound          ! Hound
     integer(c_int) :: lcommonft8b     ! m_commonFT8b
     integer(c_int) :: lenabledxcsearch! DX call set, not transmitting, not yet logged
     integer(c_int) :: lwidedxcsearch  ! band-wide DX-call search
     integer(c_int) :: ltxing          ! WSJT-X 3.0.2: always 0
     integer(c_int) :: lapmyc          ! WSJT-X 3.0.2: always 0
     integer(c_int) :: nlasttx         ! WSJT-X 3.0.2: always 0
     integer(c_int) :: lskiptx1        ! false
     integer(c_int) :: lmultinst       ! false
     integer(c_int) :: lswl            ! JTDX's SWL mode (Nexus; 0 = as WSJT-X)
     integer(c_int) :: nreserved(10)   ! zero
     character(kind=c_char) :: mycall(12), hiscall(12)    ! blank-padded, not NUL-terminated
     character(kind=c_char) :: mybcall(12), hisbcall(12)  ! base calls (Radio::base_callsign)
     character(kind=c_char) :: hisgrid(6), pad(2)
  end type ft8var_params_t

  ! cwfilter's `first` (decoder.f90:214-217): the callsign database is loaded once per process.
  logical    :: db_loaded = .false.
  ! Where the last decode's caller had its stack (see ft8var_decode_frame). 0: none yet.
  integer(c_intptr_t) :: last_stack = 0

  ! The current call's callback target. Set and cleared by ft8var_decode_frame, read by
  ! ft8var_decoded; module scope because ft8_decodervar's callback carries no user pointer.
  type(c_funptr) :: cur_cb = c_null_funptr
  type(c_ptr)    :: cur_user = c_null_ptr
  type(c_ptr)    :: cur_out = c_null_ptr
  integer        :: cur_max = 0, cur_n = 0, cur_nfqso = 0

contains

  !-------------------------------------------------------------------------
  ! ft8var_set_share_dir : where ALLCALL7.TXT is (prog_args%share_dir; jt9 --share-dir,
  !                        jt9.f90:138). Call before the first decode of FT8 or FT4.
  !-------------------------------------------------------------------------
  subroutine ft8var_set_share_dir(path, n) bind(C, name="ft8var_set_share_dir")
    use prog_args, only: share_dir
    character(kind=c_char), intent(in) :: path(*)
    integer(c_int), value, intent(in) :: n
    integer :: i, m
    m = min(int(n), len(share_dir))
    share_dir = ' '
    do i = 1, m
       share_dir(i:i) = path(i)
    enddo
  end subroutine ft8var_set_share_dir

  !-------------------------------------------------------------------------
  ! ft8var_load_callsign_db : load ALLCALL7.TXT into jt65_mod9 (cwfilter(.true.),
  !                           decoder.f90:214-217), once per process. Idempotent. Every later
  !                           call returns at once. A missing or short file leaves
  !                           ldbvalid=.false., as cwfilter does, and ft8var then filters
  !                           nothing against it. Returns 1 when the database is valid.
  !                           Shared with the JTDX FT4 decoder, whose checks need it too.
  !-------------------------------------------------------------------------
  function ft8var_load_callsign_db() result(nvalid) bind(C, name="ft8var_load_callsign_db")
    use jt65_mod9, only: ldbvalid
    integer(c_int) :: nvalid
    if (.not. db_loaded) then
       call cwfilter(.true.)
       db_loaded = .true.
    endif
    nvalid = merge(1, 0, ldbvalid)
  end function ft8var_load_callsign_db

  !-------------------------------------------------------------------------
  ! ft8var_reset : decoder.f90:190-196, run there when the mode changes (params%lmodechanged):
  !                the running average DT, the *var hash tables, and the DT fast track. Nexus
  !                calls it where it resets the standard decoder's a7 table: a band, mode or
  !                radio change.
  !-------------------------------------------------------------------------
  subroutine ft8var_reset() bind(C, name="ft8var_reset")
    use ft8_mod1, only: avexdt
    use packjt77, only: ihash22var, calls22var, calls12var
    avexdt = 0.
    ihash22var = -1
    calls22var = ''
    calls12var = ''
    nintcount = 3
  end subroutine ft8var_reset

  !-------------------------------------------------------------------------
  ! ft8var_threads : the thread count decoder.f90:255-279 uses for a requested nmt.
  !-------------------------------------------------------------------------
  function ft8var_threads(nmt) result(numthreads) bind(C, name="ft8var_threads")
    !$ use omp_lib
    integer(c_int), value, intent(in) :: nmt
    integer(c_int) :: numthreads
    integer :: numcores
    numcores = 1
    !$ numcores = omp_get_num_procs()
    numthreads = 1                                       ! fallback
    if (nmt .eq. 0) then                                 ! auto
       if (numcores .eq. 1) then
          numthreads = 1
       else if (numcores .gt. 1 .and. numcores .lt. 5) then
          numthreads = numcores - 1
       else if (numcores .gt. 4 .and. numcores .lt. 9) then
          numthreads = numcores - 2
       else if (numcores .gt. 8 .and. numcores .lt. 16) then
          numthreads = numcores - 3
       else
          numthreads = 12
       endif
    else if (nmt .gt. 0 .and. nmt .lt. 13) then
       ! number of threads shall not exceed number of logical cores
       if (numcores .ge. nmt) then
          numthreads = nmt
       else
          numthreads = numcores
       endif
    endif
  end function ft8var_threads

  !-------------------------------------------------------------------------
  ! ft8var_slices : the frequency slice each of n threads decodes (decoder.f90:351-1060), into
  !                 lo(1:n), hi(1:n). Exposed so a test can hold it to upstream's table.
  !-------------------------------------------------------------------------
  subroutine ft8var_slices(nfa, nfb, n, lo, hi) bind(C, name="ft8var_slices")
    integer(c_int), value, intent(in) :: nfa, nfb, n
    integer(c_int), intent(out) :: lo(*), hi(*)
    integer :: nfdelta, k, nfmid(12)
    if (n .le. 1) then
       lo(1) = nfa
       hi(1) = nfb
       return
    endif
    ! :352-353 for 2 threads, the same formula as :376 for 3 and more.
    nfdelta = nint(abs(nfb - nfa) / real(n))
    nfmid(1) = nfa + nfdelta
    do k = 2, n - 1
       nfmid(k) = nfmid(k-1) + nfdelta
       ! :873 and :970: `+nfdelt`, an undeclared variable (0), for the last edge of 11 and 12.
       if ((n .eq. 11 .and. k .eq. 10) .or. (n .eq. 12 .and. k .eq. 11)) nfmid(k) = nfmid(k-1)
    enddo
    if ((nfmid(n-1) + 1) .gt. nfb) nfmid(n-1) = nfb - 1
    lo(1) = nfa
    hi(1) = nfmid(1)
    do k = 2, n - 1
       lo(k) = nfmid(k-1) + 1
       hi(k) = nfmid(k)
    enddo
    lo(n) = nfmid(n-1) + 1
    hi(n) = nfb
  end subroutine ft8var_slices

  !-------------------------------------------------------------------------
  ! ft8var_decode_frame : one period through ft8var, as decoder.f90 runs it with lmultift8 on.
  !
  !   iwave   : int16(180000), the period's audio at 12 kHz; the first p%nzhsym*3456 samples are
  !             decoded (jt9.f90:470-480 fills dd8 the same way)
  !   p       : ft8var_params_t
  !   out     : ft8var_decode_t(max_out); each decode as the callback receives it (sync is 0:
  !             ft8var reports none; `unchecked` 1 for a message no error check accepted)
  !   cb      : NULL, or tempo_decode_cb, called with each decode the moment it is accepted, as
  !             WSJT-X's ft8_decodedvar prints and flushes it. Its answer is ignored: ft8var has
  !             no give-up point. With several threads it is called from them, one at a time.
  !   returns : the number of decodes written, or 0 under WSJT-X's 0.5 LSB rms floor
  !             (decoder.f90:92-93).
  !-------------------------------------------------------------------------
  function ft8var_decode_frame(iwave, p, out, max_out, cb, cb_user) result(ndec) &
       bind(C, name="ft8var_decode_frame")
    !$ use omp_lib
    use ft8var_swl, only : lswl
    use ft8_mod1, only : ndecodes,allmessages,allsnrs,allfreq,mycall12_0,         &
         mycall12_00,hiscall12_0,nmsg,odd,even,oddcopy,evencopy,nlasttx,          &
         lqsomsgdcd,mycalllen1,msgroot,msgrootlen,lapmyc,avexdt,                  &
         nfawide,nfbwide,mycall,hiscall,lhound,mybcall,hisbcall,lenabledxcsearch, &
         lwidedxcsearch,hisgrid4,lmultinst,dd8,nft8cycles,lskiptx1,ncandallthr,   &
         nincallthr,incall,msgincall,xdtincall,maskincallthr,ltxing,hisgrid
    use packjt77, only : lcommonft8b, recent_callsvar
    integer(c_int16_t),    intent(in)  :: iwave(180000)
    type(ft8var_params_t), intent(in)  :: p
    type(ft8var_decode_t), intent(out), target :: out(*)
    integer(c_int), value, intent(in)  :: max_out
    type(c_funptr), value, intent(in)  :: cb
    type(c_ptr),    value, intent(in)  :: cb_user
    integer(c_int)                     :: ndec

    type(counting_ft8_decodervar) :: my_ft8var
    integer :: npts, i, m, nindex, numthreads, nfa, nfb, nfqso, nutc, nsec, nFT8decd
    integer :: lo(12), hi(12), ncontest
    real :: rms, sumxdt, dtmed, forcedt
    complex :: cdummy(1)
    logical(1) :: lmycallstd, lhiscallstd, nstophint, lft8lowth, lft8subpass, lhideft8dupes
    logical(1) :: lft8apon
    interface
       integer(c_int) function tempo_omp_env_linked() bind(C, name='tempo_omp_env_linked')
         import :: c_int
       end function tempo_omp_env_linked
    end interface

    ndec = 0
    ! Keeps omp_env.c's stack-size constructor in the link (see omp_env.c).
    i = tempo_omp_env_linked()
    if (max_out .le. 0) return

    ! four2avar caches an FFTW plan per array ADDRESS, and ft8var's FFT arrays are locals on the
    ! decoding threads' stacks, so every new decoding thread adds plans, and at 2100 four2avar
    ! STOPs the process (`stop 'Too many FFTW plans requested.'`). WSJT-X's jt9 decodes from one
    ! thread for its whole life; Nexus's decode thread can be replaced (a radio switch restarts
    ! its loop), and the tests decode from many. So when the caller's stack is not where the last
    ! one's was, the plans are dropped and made again (FFTW_ESTIMATE planning: fast).
    if (int(loc(npts), c_intptr_t) .ne. last_stack) then
       call four2avar(cdummy, -1, 1, 1, 1)
       last_stack = int(loc(npts), c_intptr_t)
    endif

    ! decoder.f90:91-93: under 0.5 LSB rms nothing is decoded and nothing is touched.
    rms = sqrt(dot_product(real(iwave(1:180000)), real(iwave(1:180000))) / 180000.0)
    if (rms .lt. 0.5) return

    ! This radio's recent calls into the decoding thread's thread-local list (see
    ! ft8var_dispatch_state); copied back out at the end.
    recent_callsvar = recent_shadow

    ! decoder.f90:110-120
    my_ft8var%decodedvar = 0
    my_ft8var%xdtt = 0.
    ncandallthr = 0
    ltry_a8 = .true.

    ! jt9.f90:470-480: dd8 holds the first nzhsym*3456 samples, the rest zero.
    npts = max(0, min(180000, int(p%nzhsym) * 3456))
    dd8 = 0.
    dd8(1:npts) = real(iwave(1:npts))

    ! decoder.f90:96-103, the strings ft8_mod1 holds
    mycall = transfer(p%mycall, mycall)
    hiscall = transfer(p%hiscall, hiscall)
    mybcall = transfer(p%mybcall, mybcall)
    hisbcall = transfer(p%hisbcall, hisbcall)
    hisgrid = transfer(p%hisgrid, hisgrid)
    hisgrid4 = hisgrid(1:4)

    lmycallstd = p%lmycallstd .ne. 0
    lhiscallstd = p%lhiscallstd .ne. 0
    nstophint = p%nstophint .ne. 0
    lft8lowth = p%lft8lowth .ne. 0
    lft8subpass = p%lft8subpass .ne. 0
    lhideft8dupes = p%lhideft8dupes .ne. 0
    lft8apon = p%lft8apon .ne. 0
    lswl = p%lswl .ne. 0                                  ! JTDX's SWL mode (ft8var_swl.f90)
    ncontest = p%ncontest

    lqsomsgdcd = .false.                                  ! :197 (ndelay is 0)
    nutc = p%nutc                                         ! :213

    if (.not. db_loaded) i = ft8var_load_callsign_db()    ! :214-217
    lenabledxcsearch = p%lenabledxcsearch .ne. 0          ! :218-219
    lwidedxcsearch = p%lwidedxcsearch .ne. 0
    lmultinst = p%lmultinst .ne. 0                        ! :221-223
    lskiptx1 = p%lskiptx1 .ne. 0
    ltxing = p%ltxing .ne. 0

    mycalllen1 = len_trim(mycall) + 1                     ! :225-229
    msgroot = ''
    msgroot = trim(mycall)//' '//trim(hiscall)//' '
    msgrootlen = len_trim(msgroot)
    lcommonft8b = p%lcommonft8b .ne. 0
    lhound = p%lhound .ne. 0
    nft8cycles = p%nft8cycles
    forcedt = 0.

    ! :233-247
    if ((hiscall .ne. hiscall12_0 .and. hiscall .ne. '            ')          &
         .or. (mycall .ne. mycall12_0 .and. mycall .ne. '            ') .or. &
         (lhound .neqv. lhoundprev)) then
       if (hiscall .ne. '            ') then
          call tone8(lmycallstd, lhiscallstd)
          hiscall12_0 = hiscall
          mycall12_0 = mycall
       endif
       lhoundprev = lhound
    endif
    if (lmycallstd .and. mycall .ne. '            ' .and. mycall12_00 .ne. mycall) then
       call tone8myc()
       mycall12_00 = mycall
    endif

    ndecodes = 0                                          ! :249-252
    allmessages = ""
    allsnrs = 0
    allfreq = 0.
    numthreads = ft8var_threads(p%nmt)                    ! :253-279

    !$ call omp_set_dynamic(.false.)                      ! :281-282
    !$ call omp_set_nested(.true.)

    nfa = p%nfa                                           ! :284-288
    nfb = p%nfb
    nfqso = p%nfqso
    nfawide = p%nfa
    nfbwide = p%nfb

    nsec = mod(nutc, 100)                                 ! :300-332
    nmsg = 0
    if (nsec .ne. 0 .and. nsec .ne. 15 .and. nsec .ne. 30 .and. nsec .ne. 45) then
       ! Reading simulated wav file
       odd%lstate = .false.
       even%lstate = .false.
       oddcopy%lstate = .false.
       evencopy%lstate = .false.
    endif
    if (firstsd) then
       odd%lstate = .false.
       even%lstate = .false.
       firstsd = .false.
    endif
    if (nsec .eq. 0 .or. nsec .eq. 30) then
       evencopy%msg = even%msg
       evencopy%freq = even%freq
       evencopy%dt = even%dt
       evencopy%lstate = even%lstate
       even%lstate = .false.
    endif
    if (nsec .eq. 15 .or. nsec .eq. 45) then
       oddcopy%msg = odd%msg
       oddcopy%freq = odd%freq
       oddcopy%dt = odd%dt
       oddcopy%lstate = odd%lstate
       odd%lstate = .false.
    endif

    nlasttx = p%nlasttx                                   ! :334-341
    lapmyc = p%lapmyc .ne. 0
    nFT8decd = 0
    sumxdt = 0.0
    call fillhashvar(numthreads, .false.)
    call ft8apsetvar(lmycallstd, lhiscallstd, numthreads)

    cur_cb = cb
    cur_user = cb_user
    cur_out = c_loc(out(1))
    cur_max = max_out
    cur_n = 0
    cur_nfqso = nfqso

    ! :342-1060
    call ft8var_slices(nfa, nfb, numthreads, lo, hi)
    if (numthreads .eq. 1) then
       call my_ft8var%decodevar(ft8var_decoded, p%nqso_progress, nfqso, p%nft8rxfsens, &
            p%nftx, nutc, nfa, nfb, p%ncandthin, p%ndtcenter, nsec, p%napwid,           &
            lmycallstd, lhiscallstd, nstophint, 1, numthreads, .false., lft8lowth,       &
            lft8subpass, lhideft8dupes, lft8apon, ncontest)
    else
       !$omp parallel do num_threads(numthreads) schedule(static,1) default(shared) private(i)
       do i = 1, numthreads
          call my_ft8var%decodevar(ft8var_decoded, p%nqso_progress, nfqso, p%nft8rxfsens, &
               p%nftx, nutc, lo(i), hi(i), p%ncandthin, p%ndtcenter, nsec, p%napwid,       &
               lmycallstd, lhiscallstd, nstophint, i, numthreads, .false., lft8lowth,      &
               lft8subpass, lhideft8dupes, lft8apon, ncontest)
       enddo
       !$omp end parallel do
    endif

    do i = 1, numthreads                                  ! :1062-1069
       do m = 1, nincallthr(i)
          nindex = maskincallthr(i) + m
          incall(30:2:-1) = incall(30-1:1:-1)
          incall(1)%msg = msgincall(nindex)
          incall(1)%xdt = xdtincall(nindex)
       enddo
    enddo

    if (nsec .eq. 0 .or. nsec .eq. 30) even(nmsg+1:130)%lstate = .false.   ! :1071-1072
    if (nsec .eq. 15 .or. nsec .eq. 45) odd(nmsg+1:130)%lstate = .false.

    ! :1074-1130, ndelay = 0: the running average DT, from the decodes' median DT
    nFT8decd = my_ft8var%decodedvar
    dtmed = 0.
    if (nintcount .gt. 0) then
       nintcount = nintcount - 1
    endif
    if (nFT8decd .eq. 0) then
       avexdt = forcedt
    else
       if (nFT8decd .gt. 2) then
          do i = 1, nFT8decd
             if (i .lt. nFT8decd - 1) then
                if ((my_ft8var%xdtt(i) .gt. my_ft8var%xdtt(i+1) .and.   &
                     my_ft8var%xdtt(i) .lt. my_ft8var%xdtt(i+2)) .or.  &
                     (my_ft8var%xdtt(i) .lt. my_ft8var%xdtt(i+1) .and. &
                     my_ft8var%xdtt(i) .gt. my_ft8var%xdtt(i+2))) then
                   dtmed = my_ft8var%xdtt(i)
                else if ((my_ft8var%xdtt(i+1) .gt. my_ft8var%xdtt(i) .and. &
                     my_ft8var%xdtt(i+1) .lt. my_ft8var%xdtt(i+2)) .or.   &
                     (my_ft8var%xdtt(i+1) .lt. my_ft8var%xdtt(i) .and.    &
                     my_ft8var%xdtt(i+1) .gt. my_ft8var%xdtt(i+2))) then
                   dtmed = my_ft8var%xdtt(i+1)
                else if ((my_ft8var%xdtt(i+2) .gt. my_ft8var%xdtt(i) .and. &
                     my_ft8var%xdtt(i+2) .lt. my_ft8var%xdtt(i+1)) .or.   &
                     (my_ft8var%xdtt(i+2) .lt. my_ft8var%xdtt(i) .and.    &
                     my_ft8var%xdtt(i+2) .gt. my_ft8var%xdtt(i+1))) then
                   dtmed = my_ft8var%xdtt(i+2)
                else
                   dtmed = my_ft8var%xdtt(i)
                endif
                sumxdt = sumxdt + dtmed
             else
                sumxdt = sumxdt + dtmed ! use last median value
             endif
          enddo
          if (nFT8decd .gt. 5) then
             avexdt = (avexdt + sumxdt/nFT8decd)/2
          else if (nFT8decd .eq. 5) then
             avexdt = (1.1*avexdt + 0.9*sumxdt/nFT8decd)/2
          else if (nFT8decd .eq. 4) then
             avexdt = (1.25*avexdt + 0.75*sumxdt/nFT8decd)/2
          else if (nFT8decd .eq. 3) then
             avexdt = (1.35*avexdt + 0.65*sumxdt/nFT8decd)/2
          endif
       else if (nFT8decd .gt. 0) then
          sumxdt = sum(my_ft8var%xdtt(1:nFT8decd))
          if (nFT8decd .eq. 2) then
             avexdt = (1.5*avexdt + 0.5*sumxdt/nFT8decd)/2
          else if (nFT8decd .eq. 1) then
             avexdt = (1.75*avexdt + 0.25*sumxdt)/2
          endif
       endif
    endif

    if (nFT8decd .gt. 10 .and. nintcount .eq. 1) avexdt = sumxdt/nFT8decd   ! :1132-1135
    call fillhashvar(numthreads, .true.)
    if (nFT8decd .eq. 0) avexdt = 0.

    recent_shadow = recent_callsvar
    ndec = cur_n
    cur_cb = c_null_funptr
    cur_user = c_null_ptr
    cur_out = c_null_ptr
    cur_max = 0
  end function ft8var_decode_frame

  !-------------------------------------------------------------------------
  ! ft8var_decoded : decoder.f90:1598-1697 (ft8_decodedvar), Nexus's way. Upstream prints the
  ! decode, appends '?' when nap /= 0 and qual < 0.17 and writes Fox's caller list; here the
  ! record goes to `out` and the callback, with nap and qual as they came (the caller marks '?'
  ! as it does for the standard decoder; Nexus is never a Fox). What it keeps of upstream: the
  ! a8 gate (:1636-1637), the count and the DT list the average DT is made from.
  ! Serialised: with several threads, decodevar calls it from all of them.
  !-------------------------------------------------------------------------
  subroutine ft8var_decoded(this, snr, dt, freq, decodedvar, nap, qual)
    use ft8var_hint, only: lhintdec
    class(ft8_decodervar), intent(inout) :: this
    integer, intent(in) :: snr
    real, intent(in) :: dt
    real, intent(in) :: freq
    real, intent(in) :: qual
    character(len=37), intent(in) :: decodedvar
    integer, intent(in) :: nap
    type(ft8var_decode_t), pointer :: outv(:)
    type(ft8var_decode_t), target :: rec
    procedure(ft8var_decode_cb), pointer :: fcb
    integer :: i, n, istop

    !$omp critical(ft8var_decoded)
    if (abs(freq - cur_nfqso) .lt. 3.0) ltry_a8 = .false.   ! :1636-1637

    rec%sync = 0.
    rec%snr = snr
    rec%dt = dt
    rec%freq = freq
    rec%nap = nap
    rec%qual = qual
    rec%unchecked = merge(1, 0, lhintdec)
    rec%message = c_null_char
    n = len_trim(decodedvar)
    do i = 1, min(n, 37)
       rec%message(i) = decodedvar(i:i)
    enddo

    if (c_associated(cur_out) .and. cur_n .lt. cur_max) then
       call c_f_pointer(cur_out, outv, [cur_max])
       cur_n = cur_n + 1
       outv(cur_n) = rec
       if (c_associated(cur_cb)) then
          call c_f_procpointer(cur_cb, fcb)
          istop = fcb(cur_user, c_loc(rec))
       endif
    endif

    ! Upstream writes xdtt past its 200 entries on a 201st decode (WSJT-X's release build
    ! bounds-checks and would stop there); the count still counts, the DT list stops at 200.
    select type (this)
    type is (counting_ft8_decodervar)
       this%decodedvar = this%decodedvar + 1
       if (this%decodedvar .le. size(this%xdtt)) this%xdtt(this%decodedvar) = dt
    end select
    !$omp end critical(ft8var_decoded)
  end subroutine ft8var_decoded

end module ft8var_cabi
