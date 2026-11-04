"""Checks that every render lines up with its original in time and level.
Usage: python3 -I check.py <the trial's out dir>"""
import numpy as np, struct, sys, os
OUT = sys.argv[1]
def read(p):
    # 32-bit float WAV via numpy (hound writes a plain header)
    with open(p,'rb') as f: d=f.read()
    i=d.find(b'data'); n=struct.unpack('<I',d[i+4:i+8])[0]
    x=np.frombuffer(d[i+8:i+8+n],dtype='<f4').reshape(-1,2); return x
def env(x, hop=480):
    m=x.mean(1); n=len(m)//hop
    return np.sqrt((m[:n*hop].reshape(n,hop)**2).mean(1))
L={'A':'bungee','B':'rb-R2','C':'signalsmith','D':'timestretch','E':'rb-R3'}
for src in sorted(os.listdir(f'{OUT}/pack')):
    o=read(f'{OUT}/pack/{src}/original.wav'); eo=env(o)
    for t,sp in [('tempo-0.75',0.75),('tempo-1.5',1.5),('pitch-plus-3',1.0)]:
        # original envelope stretched to the output's time
        idx=np.arange(int(len(eo)/sp))*sp
        ref=np.interp(idx,np.arange(len(eo)),eo)
        row=[]
        for k in 'ABCDE':
            x=read(f'{OUT}/pack/{src}/{t}/{k}.wav'); e=env(x)[:len(ref)]
            r=ref[:len(e)]
            lags=range(-20,21)
            c=[np.corrcoef(np.roll(e,l)[25:-25],r[25:-25])[0,1] for l in lags]
            best=lags[int(np.argmax(c))]
            gain=20*np.log10((np.sqrt((x**2).mean())+1e-9)/(np.sqrt((o**2).mean())+1e-9))
            row.append(f"{L[k]}: lag {best*10:+4d} ms r {max(c):.2f} gain {gain:+.1f} dB")
        print(src,t); [print('   ',r) for r in row]
