subroutine flat65(ss,nhsym,maxhsym,nsz,ref)

  real stmp(nsz)
  real ss(maxhsym,nsz)
  real ref(nsz)

  npct=28                                       !Somewhat arbitrary
  do i=1,nsz
     call pctile(ss(1,i),nhsym,npct,stmp(i))
  enddo

  nsmo=33
  ia=nsmo/2 + 1
  ib=nsz - nsmo/2 - 1
  do i=ia,ib
     call pctile(stmp(i-nsmo/2),nsmo,npct,ref(i))
  enddo
  ref(:ia-1)=ref(ia)
  ref(ib+1:)=ref(ib)
  ref=4.0*ref

! MODIFIED FOR NEXUS (KD9TAW, 2026-07-28): floor the reference spectrum above zero.
! ref is the 28th percentile of each column, so a window that is more than 28%
! EXACT digital silence makes it 0 — and symspec65 then computes `savg/ref` and
! `ss/ref` as 0/0 = NaN for the whole spectrum. NaN silently defeats every
! comparison downstream (`x.gt.max` and `x.lt.min` are BOTH false), which is how a
! peak-search variable escapes unassigned and is used as an array index. Nexus can
! present such a window: its RxRing front-zero-pads while filling, and a muted or
! disconnected sound card delivers true silence.
!
! 1.0 is inert for real signals — ref is strictly positive whenever there is any
! noise floor at all, so this branch cannot fire on live audio, and where it does
! fire the numerator is 0, so 0/1 = 0 keeps the silent window silent instead of NaN.
  where(ref.le.0.0) ref=1.0

  return
end subroutine flat65

      
