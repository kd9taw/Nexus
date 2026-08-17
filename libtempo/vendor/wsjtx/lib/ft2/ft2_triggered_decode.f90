subroutine ft2_triggered_decode(iwave, nqsoprogress, nfqso, nfa, nfb, &
     ndepth, ncontest, mycall, hiscall, outlines, nout)

! Level 2: Sync-Triggered FT2 Decoder — Decodium v2
! ===================================================
! Improvements over v1:
!  - AP type 4: hiscall+mycall (61 AP bits)
!  - Soft AP injection (weighted by confidence, not hard +/-1)

  use packjt77

  include 'ft2_params.f90'
  parameter (MAXCAND=300, NSS=NSPS/NDOWN, NDMAX=NMAX/NDOWN)
  parameter (MAXHITS=50)

  integer*2 iwave(NMAX)
  integer nqsoprogress, nfqso, nfa, nfb, ndepth, ncontest
  character*12 mycall, hiscall
  character*80 outlines(100)
  integer nout

! Phase 1 hit storage
  real hit_freq(MAXHITS), hit_sync(MAXHITS), hit_snr(MAXHITS)
  integer hit_ibest(MAXHITS), hit_idf(MAXHITS)
  integer nhits

! Decode variables
  real dd(NMAX)
  real candidate(2,MAXCAND), savg(NH1), sbase(NH1)
  complex cd2(0:NDMAX-1), cb(0:NDMAX-1), cd(0:NN*NSS-1)
  complex ctwk(2*NSS), ctwk2(2*NSS,-16:16)
  real bitmetrics(2*NN,3)
  real llr(2*ND), llra(2*ND), llrb(2*ND), llrc(2*ND)
  real llrd(2*ND), llre(2*ND)
  real a(5)

  integer*1 message77(77), rvec(77), apmask(2*ND), cw(2*ND)
  integer*1 message91(91)
  integer*1 hbits(2*NN)
  integer ncand, ndepth0
  character*37 message, decodes(100)
  character*77 c77
  character*2 annot
  logical badsync, dobigfft, unpk77_success, doosd
  logical first
  real xibest, sm1, sp1, den
  integer ndecodes

! AP decoding variables
  integer*1 apmask_cq(2*ND), apmask_mycall(2*ND), apmask_hiscall(2*ND)
  integer*1 mcq(77), mmycall(77), mhiscall(77)
  character*77 c77_ap
  character*37 apmsg
  integer nap_type, nap_i3, nap_n3

! Soft AP variables
  real ap_weight

  data rvec/0,1,0,0,1,0,1,0,0,1,0,1,1,1,1,0,1,0,0,0,1,0,0,1,1,0,1,1,0, &
     1,0,0,1,0,1,1,0,0,0,0,1,0,0,0,1,0,1,0,0,1,1,1,1,0,0,1,0,1, &
     0,1,0,1,0,1,1,0,1,1,1,1,1,0,0,0,1,0,1/
  data first/.true./
  save first, ctwk2

  nout = 0
  outlines = ' '
  ndecodes = 0

! Initialize frequency tweak factors (once)
  if(first) then
    do idf = -16, 16
      a = 0.
      a(1) = real(idf)
      ctwk = 1.
      call twkfreq1(ctwk, 2*NSS, (12000.0/NDOWN)/2.0, a, ctwk2(:,idf))
    enddo
    first = .false.
  endif

! Initialize packjt77 context
  dxcall13 = hiscall
  mycall13 = mycall

  ndepth0 = iand(ndepth, 7)
  doosd = ndepth0.ge.3
  dd = iwave

! ── AP (A-Priori) mask setup ──────────────────────────────
! AP type 1: CQ           (29 bits)
! AP type 2: mycall        (29 bits)
! AP type 3: CQ + mycall   (58 bits)
! AP type 4: hiscall + mycall (58 bits + 3 type = 61 AP bits) — NEW v2

  apmask_cq = 0
  apmask_mycall = 0
  apmask_hiscall = 0
  mcq = 0
  mmycall = 0
  mhiscall = 0

! Pack CQ bit pattern
  apmsg = 'CQ K1JT FN20'
  nap_i3 = -1; nap_n3 = -1
  call pack77(apmsg, nap_i3, nap_n3, c77_ap)
  if(nap_i3.ge.0) then
    read(c77_ap, '(77i1)') mcq
    mcq = mod(mcq + rvec, 2)
    apmask_cq(1:29) = 1
  endif

! Pack mycall bit pattern (bits 30-58)
  if(len_trim(mycall).ge.3) then
    apmsg = 'CQ '//mycall(1:12)//' AA00'
    nap_i3 = -1; nap_n3 = -1
    call pack77(apmsg, nap_i3, nap_n3, c77_ap)
    if(nap_i3.ge.0) then
      read(c77_ap, '(77i1)') mmycall
      mmycall = mod(mmycall + rvec, 2)
      apmask_mycall(30:58) = 1
    endif
  endif

! Pack hiscall bit pattern (bits 1-29) for AP type 4
  if(len_trim(hiscall).ge.3) then
    apmsg = hiscall(1:12)//' K1JT FN20'
    nap_i3 = -1; nap_n3 = -1
    call pack77(apmsg, nap_i3, nap_n3, c77_ap)
    if(nap_i3.ge.0) then
      read(c77_ap, '(77i1)') mhiscall
      mhiscall = mod(mhiscall + rvec, 2)
      apmask_hiscall(1:29) = 1
    endif
  endif

! ============================================
! PHASE 1: Fast Costas Sync Scan
! ============================================
  nhits = 0

  call getcandidates2(dd, real(nfa), real(nfb), 0.50, nfqso, MAXCAND, &
       savg, candidate, ncand, sbase)

  dobigfft = .true.
  do icand = 1, ncand
    f0 = candidate(1, icand)
    snr0 = candidate(2, icand) - 1.0

    call ft2_downsample(dd, dobigfft, f0, cd2)
    if(dobigfft) dobigfft = .false.

    sum2 = sum(cd2*conjg(cd2))/(real(NMAX)/real(NDOWN))
    if(sum2.gt.0.0) cd2 = cd2/sqrt(sum2)

! Coarse sync search
    ibest_c = -1
    idfbest_c = 0
    smax_c = -99.
    do idf = -12, 12, 3
      do istart = -688, 2024, 4
        call sync2d(cd2, istart, ctwk2(:,idf), 1, sync)
        if(sync.gt.smax_c) then
          smax_c = sync
          ibest_c = istart
          idfbest_c = idf
        endif
      enddo
    enddo

    syncmin_scan = 0.50
    if(ndepth0.ge.3) syncmin_scan = 0.40

    if(smax_c.ge.syncmin_scan .and. nhits.lt.MAXHITS) then
      nhits = nhits + 1
      hit_freq(nhits) = f0
      hit_ibest(nhits) = ibest_c
      hit_idf(nhits) = idfbest_c
      hit_sync(nhits) = smax_c
      hit_snr(nhits) = snr0
    endif
  enddo

! ============================================
! PHASE 2: Targeted Decode at Sync Positions
! ============================================
  do ihit = 1, nhits
    f0 = hit_freq(ihit)
    ibest0 = hit_ibest(ihit)
    idf0 = hit_idf(ihit)
    snr0 = hit_snr(ihit)

    call ft2_downsample(dd, .false., f0, cd2)
    sum2 = sum(cd2*conjg(cd2))/(real(NMAX)/real(NDOWN))
    if(sum2.gt.0.0) cd2 = cd2/sqrt(sum2)

! Fine sync
    ibest = ibest0
    idfbest = idf0
    smax = -99.
    do idf = max(-16, idf0-4), min(16, idf0+4)
      do istart = ibest0-5, ibest0+5
        call sync2d(cd2, istart, ctwk2(:,idf), 1, sync)
        if(sync.gt.smax) then
          smax = sync
          ibest = istart
          idfbest = idf
        endif
      enddo
    enddo

    smaxthresh = 0.50      ! best-3-of-4 Costas: scaled for 3/4 sync sum
    if(ndepth0.ge.3) smaxthresh = 0.38
    if(smax.lt.smaxthresh) cycle

    f1 = f0 + real(idfbest)
    if(f1.le.10.0 .or. f1.ge.4990.0) cycle

    call ft2_downsample(dd, .false., f1, cb)
    sum2 = sum(abs(cb)**2)/(real(NSS)*NN)
    if(sum2.gt.0.0) cb = cb/sqrt(sum2)

    cd = 0.
    if(ibest.ge.0) then
      it = min(NDMAX-1, ibest+NN*NSS-1)
      np = it - ibest + 1
      cd(0:np-1) = cb(ibest:it)
    else
      cd(-ibest:ibest+NN*NSS-1) = cb(0:NN*NSS+2*ibest-1)
    endif

    call get_ft2_bitmetrics(cd, bitmetrics, badsync)
    if(badsync) cycle

! Sub-sample DT refinement
    if(ibest.gt.0 .and. ibest.lt.NDMAX-1) then
      call sync2d(cd2, ibest-1, ctwk2(:,idfbest), 1, sm1)
      call sync2d(cd2, ibest+1, ctwk2(:,idfbest), 1, sp1)
      den = sm1 - 2.0*smax + sp1
      if(abs(den).gt.1.0e-6) then
        xibest = real(ibest) + 0.5*(sm1 - sp1)/den
      else
        xibest = real(ibest)
      endif
    else
      xibest = real(ibest)
    endif

! Sync quality check
    hbits = 0
    where(bitmetrics(:,1).ge.0) hbits = 1
    ns1 = count(hbits(  1:  8).eq.(/0,0,0,1,1,0,1,1/))
    ns2 = count(hbits( 67: 74).eq.(/0,1,0,0,1,1,1,0/))
    ns3 = count(hbits(133:140).eq.(/1,1,1,0,0,1,0,0/))
    ns4 = count(hbits(199:206).eq.(/1,0,1,1,0,0,0,1/))
    nsync_qual = ns1 + ns2 + ns3 + ns4
    nsync_qual_min = 10
    if(ndepth0.ge.3) nsync_qual_min = 8
    if(nsync_qual.lt.nsync_qual_min) cycle

! Scale LLRs from bitmetrics — scalefac 3.2 (standard)
    scalefac = 3.2
    llra(  1: 58) = bitmetrics(  9: 66, 1)
    llra( 59:116) = bitmetrics( 75:132, 1)
    llra(117:174) = bitmetrics(141:198, 1)
    llra = scalefac * llra
    call normalizebmet(llra, 2*ND)
    llrb(  1: 58) = bitmetrics(  9: 66, 2)
    llrb( 59:116) = bitmetrics( 75:132, 2)
    llrb(117:174) = bitmetrics(141:198, 2)
    llrb = scalefac * llrb
    call normalizebmet(llrb, 2*ND)
    llrc(  1: 58) = bitmetrics(  9: 66, 3)
    llrc( 59:116) = bitmetrics( 75:132, 3)
    llrc(117:174) = bitmetrics(141:198, 3)
    llrc = scalefac * llrc
    call normalizebmet(llrc, 2*ND)

! Best-of metric (max |value|) and average
    do i = 1, 2*ND
      if(abs(llra(i)).ge.abs(llrb(i)) .and. &
         abs(llra(i)).ge.abs(llrc(i))) then
        llrd(i) = llra(i)
      elseif(abs(llrb(i)).ge.abs(llrc(i))) then
        llrd(i) = llrb(i)
      else
        llrd(i) = llrc(i)
      endif
      llre(i) = (llra(i) + llrb(i) + llrc(i))/3.0
    enddo

! ── Decode passes: 5 metric × (no-AP + 5 AP types) ──────
! Pass  1-5: no AP (a, b, c, best-of, avg)
! Pass  6:   AP1 — CQ (29 bits)
! Pass  7:   AP2 — mycall (29 bits)
! Pass  8:   AP3 — CQ + mycall (58 bits)
! Pass  9:   AP4 — hiscall + mycall (58 bits) — NEW v2
! Pass 10:   AP4 with average metric
!
! AP uses SOFT injection: ap_weight proportional to sync quality.

    nap_type = 0
    do ipass = 1, 10
      if(ipass.eq.1) then; llr = llra; apmask = 0; endif
      if(ipass.eq.2) then; llr = llrb; apmask = 0; endif
      if(ipass.eq.3) then; llr = llrc; apmask = 0; endif
      if(ipass.eq.4) then; llr = llrd; apmask = 0; endif
      if(ipass.eq.5) then; llr = llre; apmask = 0; endif

! ── AP type 1: CQ ──
      if(ipass.eq.6) then
        llr = llrd
        apmask = apmask_cq
        ap_weight = min(4.0, max(1.5, smax * 3.0))
        do i = 1, 29
          if(apmask(i).eq.1) llr(i) = (mcq(i)*2.0-1.0) * ap_weight
        enddo
        nap_type = 1
      endif

! ── AP type 2: mycall ──
      if(ipass.eq.7 .and. len_trim(mycall).ge.3) then
        llr = llrd
        apmask = apmask_mycall
        ap_weight = min(4.0, max(1.5, smax * 3.0))
        do i = 30, 58
          if(apmask(i).eq.1) llr(i) = (mmycall(i)*2.0-1.0) * ap_weight
        enddo
        nap_type = 2
      endif

! ── AP type 3: CQ + mycall (58 AP bits) ──
      if(ipass.eq.8 .and. len_trim(mycall).ge.3) then
        llr = llrd
        apmask = 0
        apmask(1:29) = apmask_cq(1:29)
        apmask(30:58) = apmask_mycall(30:58)
        ap_weight = min(4.0, max(1.5, smax * 3.0))
        do i = 1, 29
          if(apmask_cq(i).eq.1) llr(i) = (mcq(i)*2.0-1.0) * ap_weight
        enddo
        do i = 30, 58
          if(apmask_mycall(i).eq.1) llr(i) = (mmycall(i)*2.0-1.0) * ap_weight
        enddo
        nap_type = 3
      endif

! ── AP type 4: hiscall + mycall (58 AP bits) — NEW v2 ──
! When both hiscall and mycall are known, inject both as AP.
! hiscall = bits 1-29 (first callsign), mycall = bits 30-58.
! 58+3=61 AP bits — massive help at very low SNR.
      if(ipass.eq.9 .and. len_trim(mycall).ge.3 .and.                   &
         len_trim(hiscall).ge.3) then
        llr = llrd
        apmask = 0
        apmask(1:29) = apmask_hiscall(1:29)
        apmask(30:58) = apmask_mycall(30:58)
        ap_weight = min(4.0, max(1.5, smax * 3.0))
        do i = 1, 29
          if(apmask_hiscall(i).eq.1) llr(i) = (mhiscall(i)*2.0-1.0) * ap_weight
        enddo
        do i = 30, 58
          if(apmask_mycall(i).eq.1) llr(i) = (mmycall(i)*2.0-1.0) * ap_weight
        enddo
        nap_type = 4
      endif

! ── AP type 4 with average metric ──
      if(ipass.eq.10 .and. len_trim(mycall).ge.3 .and.                  &
         len_trim(hiscall).ge.3) then
        llr = llre
        apmask = 0
        apmask(1:29) = apmask_hiscall(1:29)
        apmask(30:58) = apmask_mycall(30:58)
        ap_weight = min(4.0, max(1.5, smax * 3.0))
        do i = 1, 29
          if(apmask_hiscall(i).eq.1) llr(i) = (mhiscall(i)*2.0-1.0) * ap_weight
        enddo
        do i = 30, 58
          if(apmask_mycall(i).eq.1) llr(i) = (mmycall(i)*2.0-1.0) * ap_weight
        enddo
        nap_type = 4
      endif

      message77 = 0
      dmin = 0.0
      maxosd = 4
      if(ndepth0.ge.3) maxosd = 7
      if(.not.doosd) maxosd = -1

      Keff = 91
      call decode174_91_ft2(llr, Keff, maxosd, 3, apmask, message91, cw, &
                            ntype, nharderror, dmin)
      message77 = message91(1:77)

      if(sum(message77).eq.0) cycle
      if(nharderror.ge.0) then
        message77 = mod(message77 + rvec, 2)
        write(c77, '(77i1)') message77(1:77)
        call unpack77(c77, 1, message, unpk77_success)
        if(.not.unpk77_success) cycle

! Check duplicate
        idupe = 0
        do i = 1, ndecodes
          if(decodes(i).eq.message) idupe = 1
        enddo
        if(idupe.eq.1) exit

        ndecodes = ndecodes + 1
        decodes(ndecodes) = message

! Calculate SNR
        if(snr0.gt.0.0) then
          xsnr = 10*log10(snr0) - 13.0
        else
          xsnr = -21.0
        endif
        nsnr = nint(max(-21.0, xsnr))

! DT from known sync position
        xdt = xibest/1333.33 - 0.5
        if(ipass.le.5) then
          annot = 'T '
        else
          write(annot,'(a1,i1)') 'a', nap_type
        endif
        nout = nout + 1
        write(outlines(nout), 1001) nsnr, xdt, nint(f1), message, annot
1001    format(i4,f5.1,i5,' ~ ',1x,a37,1x,a2)
        exit
      endif
    enddo  ! ipass

  enddo  ! ihit

  return
end subroutine ft2_triggered_decode
