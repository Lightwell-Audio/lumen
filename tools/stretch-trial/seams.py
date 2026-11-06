"""Compares the prepared-ahead (chunked) renders with the straight ones and looks for seams.
Usage: python3 -I seams.py <the trial's out dir>"""
import numpy as np, struct, sys, os
OUT = sys.argv[1]
def read(p):
    d=open(p,'rb').read(); i=d.find(b'data'); n=struct.unpack('<I',d[i+4:i+8])[0]
    return np.frombuffer(d[i+8:i+8+n],dtype='<f4').reshape(-1,2).astype(np.float64)
L={'A':'bungee','B':'rb-R2','C':'signalsmith','D':'timestretch','E':'rb-R3'}
print("source      cand          SNR chunked vs whole   seam HF / elsewhere HF")
for src in sorted(os.listdir(f'{OUT}/pack')):
    for k in 'ABCDE':
        c=read(f'{OUT}/pack/{src}/chunked-tempo-0.9/{k}.wav'); w=read(f'{OUT}/pack/{src}/whole-tempo-0.9/{k}.wav')
        n=min(len(c),len(w)); c,w=c[:n],w[:n]
        snr=10*np.log10((w**2).sum()/(((c-w)**2).sum()+1e-12))
        hf=np.abs(np.diff(c.mean(1)))
        # Where the pieces meet: every 3,686 input frames (4,096 output
        # frames at 0.9), placed as render_chunked places them, with the
        # 144-frame crossfade after each seam and 48 frames either side.
        step_in=round(4096*0.9)
        seams=[round(j*step_in/0.9) for j in range(1, n//step_in + 2) if round(j*step_in/0.9) < n-200]
        near=np.zeros(len(hf),bool)
        for s in seams: near[max(0,s-48):s+144+48]=True
        # a click shows as a seam window whose HF peak stands out from the same signal elsewhere
        ratio=np.percentile(hf[near],99.5)/np.percentile(hf[~near],99.5)
        hw=np.abs(np.diff(w.mean(1)))
        ref=np.percentile(hw[near],99.5)/np.percentile(hw[~near],99.5)
        print(f"{src:11} {L[k]:12} {snr:7.1f} dB             {ratio:5.2f}  (same spots in whole render {ref:4.2f})")
