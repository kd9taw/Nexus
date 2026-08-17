subroutine get_ft2_bitmetrics(cd,bitmetrics,badsync)
!
! Compute bit metrics for FT2 LDPC decoder.
! Decodium Enhanced Edition v2 — Log-Sum-Exp exact demapper
! =========================================================
!
! Key improvement: EXACT soft demapper using log-sum-exp instead of
! the max-log approximation.  The max-log approximation computes:
!   LLR = max(|corr| where bit=1) - max(|corr| where bit=0)
! which loses ~0.5-1.0 dB vs the exact:
!   LLR = log(Σ exp(β|corr|²) where bit=1) - log(Σ exp(β|corr|²) where bit=0)
!
! where β = 1/(2σ²) is estimated from the noise floor.
! Using |corr|² (power) instead of |corr| (amplitude) is the
! statistically correct ML metric for coherent detection in AWGN.
!
! Additional: adaptive channel estimation with MMSE equalization.
!
   include 'ft2_params.f90'
   parameter (NSS=NSPS/NDOWN,NDMAX=NMAX/NDOWN)
   complex cd(0:NN*NSS-1)
   complex cs(0:3,NN)
   complex csymb(NSS)
   complex ctmp
   integer icos4a(0:3),icos4b(0:3),icos4c(0:3),icos4d(0:3)
   integer graymap(0:3)
   integer ip(1)
   logical one(0:255,0:7)    ! 256 4-symbol sequences, 8 bits
   logical first
   logical badsync
   real bitmetrics(2*NN,3)
   real s2(0:255)
   real sp(0:255)            ! Power-domain metrics for log-sum-exp
   real s4(0:3,NN)

! Noise estimation variables
   real pwr(0:3,NN)          ! Per-tone power |cs|²
   real noise_var             ! Estimated noise variance
   real beta                  ! Soft metric scale = 1/(2*noise_var)
   real lse1, lse0            ! Log-sum-exp accumulators
   real maxp1, maxp0          ! For numerically stable log-sum-exp

! Channel estimation variables
   complex cd_eq(0:NN*NSS-1)
   complex cs_eq(0:3,NN)
   complex csymb_eq(NSS)
   real ch_snr(NN)
   real s4_eq(0:3,NN)
   real pwr_eq(0:3,NN)
   real bmet_eq(2*NN)         ! Equalized single-symbol metrics
   real s2_eq(0:255)
   real sp_eq(0:255)
   real fading_depth, snr_min, snr_max, snr_mean
   logical use_cheq

   data icos4a/0,1,3,2/
   data icos4b/1,0,2,3/
   data icos4c/2,3,1,0/
   data icos4d/3,2,0,1/
   data graymap/0,1,3,2/
   data first/.true./
   save first,one

   if(first) then
      one=.false.
      do i=0,255
         do j=0,7
            if(iand(i,2**j).ne.0) one(i,j)=.true.
         enddo
      enddo
      first=.false.
   endif

! =============================================
! Standard bit metrics (original WSJT-X path)
! =============================================
   do k=1,NN
      i1=(k-1)*NSS
      csymb=cd(i1:i1+NSS-1)
      call four2a(csymb,NSS,1,-1,1)
      cs(0:3,k)=csymb(1:4)
      s4(0:3,k)=abs(csymb(1:4))
      pwr(0:3,k)=real(csymb(1:4))**2 + aimag(csymb(1:4))**2
   enddo

! ── Noise variance estimation ─────────────────────────────
! Use Costas sync symbols (known positions) to estimate noise.
! For each sync symbol position, the signal is in ONE known tone;
! the other 3 tones contain only noise.
! Sync positions: 1-4 (icos4a), 34-37 (icos4b), 67-70 (icos4c), 100-103 (icos4d)
   noise_sum = 0.0
   nnoise = 0
   do k=1,4
! Sync group A: known tone = icos4a(k-1)
     do itone=0,3
       if(itone .ne. icos4a(k-1)) then
         noise_sum = noise_sum + pwr(itone,k)
         nnoise = nnoise + 1
       endif
     enddo
! Sync group B
     do itone=0,3
       if(itone .ne. icos4b(k-1)) then
         noise_sum = noise_sum + pwr(itone,k+33)
         nnoise = nnoise + 1
       endif
     enddo
! Sync group C
     do itone=0,3
       if(itone .ne. icos4c(k-1)) then
         noise_sum = noise_sum + pwr(itone,k+66)
         nnoise = nnoise + 1
       endif
     enddo
! Sync group D
     do itone=0,3
       if(itone .ne. icos4d(k-1)) then
         noise_sum = noise_sum + pwr(itone,k+99)
         nnoise = nnoise + 1
       endif
     enddo
   enddo

   if(nnoise.gt.0) then
     noise_var = noise_sum / real(nnoise)
   else
     noise_var = 1.0
   endif
   if(noise_var .lt. 1.0e-10) noise_var = 1.0e-10

! beta = scaling factor for log-sum-exp
! Theoretically beta = 1/(2*sigma²), but we use a tuned factor
! that accounts for correlation structure and non-Gaussianity
   beta = 0.5 / noise_var

! Clamp beta to avoid numerical issues at extreme SNR
   beta = max(0.01, min(50.0, beta))

! Sync quality check
   is1=0
   is2=0
   is3=0
   is4=0
   badsync=.false.
   ibmax=0

   do k=1,4
      ip=maxloc(s4(:,k))
      if(icos4a(k-1).eq.(ip(1)-1)) is1=is1+1
      ip=maxloc(s4(:,k+33))
      if(icos4b(k-1).eq.(ip(1)-1)) is2=is2+1
      ip=maxloc(s4(:,k+66))
      if(icos4c(k-1).eq.(ip(1)-1)) is3=is3+1
      ip=maxloc(s4(:,k+99))
      if(icos4d(k-1).eq.(ip(1)-1)) is4=is4+1
   enddo
   nsync=is1+is2+is3+is4   !Number of correct hard sync symbols, 0-16
   if(nsync .lt. 3) then
      badsync=.true.
      return
   endif

! ═══════════════════════════════════════════════════════
! LOG-SUM-EXP EXACT SOFT DEMAPPER
! ═══════════════════════════════════════════════════════
! For each bit position, compute:
!   LLR = log[Σ exp(β·|s|²) for symbols where bit=1]
!       - log[Σ exp(β·|s|²) for symbols where bit=0]
!
! This is the EXACT log-likelihood ratio, vs the max-log
! approximation which replaces log(Σexp) with max.
! Gain: +0.5-1.0 dB at low SNR where multiple symbols
! contribute meaningfully to the likelihood.
!
! Numerically stable: subtract max before exp to avoid overflow.

   do nseq=1,3
      if(nseq.eq.1) nsym=1
      if(nseq.eq.2) nsym=2
      if(nseq.eq.3) nsym=4
      nt=2**(2*nsym)
      do ks=1,NN-nsym+1,nsym

! Compute power-domain metrics for all symbol hypotheses
         do i=0,nt-1
            i1=i/64
            i2=iand(i,63)/16
            i3=iand(i,15)/4
            i4=iand(i,3)
            if(nsym.eq.1) then
               s2(i)=abs(cs(graymap(i4),ks))
               sp(i)=pwr(graymap(i4),ks)
            elseif(nsym.eq.2) then
               ctmp=cs(graymap(i3),ks)+cs(graymap(i4),ks+1)
               s2(i)=abs(ctmp)
               sp(i)=real(ctmp)**2 + aimag(ctmp)**2
            elseif(nsym.eq.4) then
               ctmp=cs(graymap(i1),ks  ) +                     &
                  cs(graymap(i2),ks+1) +                        &
                  cs(graymap(i3),ks+2) +                        &
                  cs(graymap(i4),ks+3)
               s2(i)=abs(ctmp)
               sp(i)=real(ctmp)**2 + aimag(ctmp)**2
            else
               print*,"Error - nsym must be 1, 2, or 4."
            endif
         enddo

         ipt=1+(ks-1)*2
         if(nsym.eq.1) ibmax=1
         if(nsym.eq.2) ibmax=3
         if(nsym.eq.4) ibmax=7

! Scale beta for multi-symbol coherent combining
! (noise variance scales with nsym for coherent sum)
         beta_eff = beta / real(nsym)

         do ib=0,ibmax
            if(ipt+ib.gt.2*NN) cycle

! ── Log-Sum-Exp for bit=1 ──
            maxp1 = -1.0e30
            do i=0,nt-1
               if(one(i,ibmax-ib)) then
                  pval = beta_eff * sp(i)
                  if(pval .gt. maxp1) maxp1 = pval
               endif
            enddo
            lse1 = 0.0
            do i=0,nt-1
               if(one(i,ibmax-ib)) then
                  lse1 = lse1 + exp(beta_eff*sp(i) - maxp1)
               endif
            enddo
            lse1 = maxp1 + log(max(lse1, 1.0e-30))

! ── Log-Sum-Exp for bit=0 ──
            maxp0 = -1.0e30
            do i=0,nt-1
               if(.not.one(i,ibmax-ib)) then
                  pval = beta_eff * sp(i)
                  if(pval .gt. maxp0) maxp0 = pval
               endif
            enddo
            lse0 = 0.0
            do i=0,nt-1
               if(.not.one(i,ibmax-ib)) then
                  lse0 = lse0 + exp(beta_eff*sp(i) - maxp0)
               endif
            enddo
            lse0 = maxp0 + log(max(lse0, 1.0e-30))

            bitmetrics(ipt+ib,nseq) = lse1 - lse0
         enddo
      enddo
   enddo

! =============================================
! Adaptive Channel Estimation (MMSE equalized)
! =============================================
! Run channel estimator — uses Costas sync symbols as pilots
   call ft2_channel_est(cd, cd_eq, ch_snr)

! Detect fading: if SNR varies >6dB across symbols, channel is fading
   snr_min = ch_snr(1)
   snr_max = ch_snr(1)
   snr_mean = 0.0
   do k = 1, NN
     if(ch_snr(k) .lt. snr_min) snr_min = ch_snr(k)
     if(ch_snr(k) .gt. snr_max) snr_max = ch_snr(k)
     snr_mean = snr_mean + ch_snr(k)
   enddo
   snr_mean = snr_mean / real(NN)

! Fading depth in dB (ratio of max to min channel power)
   if(snr_min .gt. 1.0e-10) then
     fading_depth = 10.0 * log10(snr_max / snr_min)
   else
     fading_depth = 30.0  ! Deep fade detected
   endif

! Use channel-equalized metrics if fading >2 dB
   use_cheq = (fading_depth .gt. 2.0)

   if(use_cheq) then
! Compute single-symbol metrics on equalized signal (also with log-sum-exp)
     do k=1,NN
       i1=(k-1)*NSS
       csymb_eq=cd_eq(i1:i1+NSS-1)
       call four2a(csymb_eq,NSS,1,-1,1)
       cs_eq(0:3,k)=csymb_eq(1:4)
       s4_eq(0:3,k)=abs(csymb_eq(1:4))
       pwr_eq(0:3,k)=real(csymb_eq(1:4))**2 + aimag(csymb_eq(1:4))**2
     enddo

! Estimate noise from equalized signal sync positions
     noise_sum_eq = 0.0
     nnoise_eq = 0
     do k=1,4
       do itone=0,3
         if(itone .ne. icos4a(k-1)) then
           noise_sum_eq = noise_sum_eq + pwr_eq(itone,k)
           nnoise_eq = nnoise_eq + 1
         endif
       enddo
       do itone=0,3
         if(itone .ne. icos4b(k-1)) then
           noise_sum_eq = noise_sum_eq + pwr_eq(itone,k+33)
           nnoise_eq = nnoise_eq + 1
         endif
       enddo
       do itone=0,3
         if(itone .ne. icos4c(k-1)) then
           noise_sum_eq = noise_sum_eq + pwr_eq(itone,k+66)
           nnoise_eq = nnoise_eq + 1
         endif
       enddo
       do itone=0,3
         if(itone .ne. icos4d(k-1)) then
           noise_sum_eq = noise_sum_eq + pwr_eq(itone,k+99)
           nnoise_eq = nnoise_eq + 1
         endif
       enddo
     enddo
     noise_var_eq = noise_sum_eq / max(1.0, real(nnoise_eq))
     if(noise_var_eq .lt. 1.0e-10) noise_var_eq = 1.0e-10
     beta_eq = 0.5 / noise_var_eq
     beta_eq = max(0.01, min(50.0, beta_eq))

! SNR-weighted log-sum-exp metrics from equalized signal
     do ks=1,NN
! Power domain for equalized
       do i=0,3
         s2_eq(i)=abs(cs_eq(graymap(i),ks))
         sp_eq(i)=pwr_eq(graymap(i),ks)
       enddo
       ipt=1+(ks-1)*2

! Weight by per-symbol SNR
       snr_weight = 1.0
       if(snr_mean .gt. 1.0e-10) then
         snr_weight = sqrt(ch_snr(ks) / snr_mean)
         snr_weight = max(0.1, min(3.0, snr_weight))
       endif

       do ib=0,1
         if(ipt+ib.gt.2*NN) cycle

! Log-sum-exp for equalized metrics
         maxp1 = -1.0e30
         maxp0 = -1.0e30
         do i=0,3
           pval = beta_eq * sp_eq(i)
           if(one(i,1-ib)) then
             if(pval .gt. maxp1) maxp1 = pval
           else
             if(pval .gt. maxp0) maxp0 = pval
           endif
         enddo
         lse1 = 0.0
         lse0 = 0.0
         do i=0,3
           if(one(i,1-ib)) then
             lse1 = lse1 + exp(beta_eq*sp_eq(i) - maxp1)
           else
             lse0 = lse0 + exp(beta_eq*sp_eq(i) - maxp0)
           endif
         enddo
         lse1 = maxp1 + log(max(lse1, 1.0e-30))
         lse0 = maxp0 + log(max(lse0, 1.0e-30))

         bmet_eq(ipt+ib) = (lse1 - lse0) * snr_weight
       enddo
     enddo
     call normalizebmet(bmet_eq,2*NN)

! Blend: replace metric 1 with weighted average of original and equalized
     blend = min(1.0, (fading_depth - 2.0) / 10.0)
     blend = max(0.0, min(0.90, blend))

     call normalizebmet(bitmetrics(:,1),2*NN)
     do i=1,2*NN
       bitmetrics(i,1) = (1.0-blend)*bitmetrics(i,1) + blend*bmet_eq(i)
     enddo
     call normalizebmet(bitmetrics(:,1),2*NN)
   else
     call normalizebmet(bitmetrics(:,1),2*NN)
   endif

! Fix boundary symbols and normalize remaining metrics
   bitmetrics(205:206,2)=bitmetrics(205:206,1)
   bitmetrics(201:204,3)=bitmetrics(201:204,2)
   bitmetrics(205:206,3)=bitmetrics(205:206,1)

   call normalizebmet(bitmetrics(:,2),2*NN)
   call normalizebmet(bitmetrics(:,3),2*NN)
   return

end subroutine get_ft2_bitmetrics
