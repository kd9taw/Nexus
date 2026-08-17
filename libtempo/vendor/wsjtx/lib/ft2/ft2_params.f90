! FT2
! LDPC(174,91) code, four 4x4 Costas arrays for sync, ramp-up and ramp-down symbols

parameter (KK=91)                     !Information bits (77 + CRC14)
parameter (ND=87)                     !Data symbols
parameter (NS=16)                     !Sync symbols
parameter (NN=NS+ND)                  !Sync and data symbols (103)
parameter (NN2=NS+ND+2)               !Total channel symbols (105)
parameter (NSPS=288)                  !Samples per symbol at 12000 S/s
parameter (NZ=NSPS*NN)                !Sync and Data samples (29664)
parameter (NZ2=NSPS*NN2)              !Total samples in shaped waveform (30240)
parameter (NMAX=45000)                !Samples in iwave (3.75*12000)
parameter (NFFT1=1152, NH1=NFFT1/2)   !Length of FFTs for symbol spectra
parameter (NSTEP=NSPS)                !Coarse time-sync step size
parameter (NHSYM=(NMAX-NFFT1)/NSTEP)  !Number of symbol spectra (1/4-sym steps)
parameter (NDOWN=9)                   !Downsample factor
