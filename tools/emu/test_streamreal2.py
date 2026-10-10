import random, struct, sys
sys.path.insert(0,__import__('os').path.dirname(__import__('os').path.abspath(__file__)))
from emu import *
from test_emu import py_seed, py_step
import test_streams as ts
M=0xffffffff
def b2f(b): return struct.unpack('<f',struct.pack('<I',b&M))[0]
mu.mem_write(0x6faae470, struct.pack('<f',0.0)); mu.mem_write(0x6faae4f4, struct.pack('<f',1.0))
random.seed(21)
cands={'u(step1)-1':0,'u(step2)-1':1,'u(A0)-1':2,'1-u(step1)+...':3}
hits={k:0 for k in cands}; N=100; sample=[]
for trial in range(N):
    k=random.randint(0,44); s=random.getrandbits(32)
    write_state(ts.BASE+4+8*k,*py_seed(s)); st=read_state(ts.BASE+4+8*k)
    OUT=DATA+0x700
    call(0x6f2853d0, ecx=OUT, edx=k)
    got=b2f(struct.unpack('<I',bytes(mu.mem_read(OUT,4)))[0])
    s1=py_step(*st); s2=py_step(*s1)
    def U(A): return b2f((A&0x7fffff)|0x3f800000)-1.0
    vals=[U(s1[0]),U(s2[0]),U(st[0]),1.0-U(s1[0])]
    for i,(nm) in enumerate(cands):
        if vals[i]==got: hits[nm]+=1
    if trial<3: sample.append((got,vals))
print(hits, 'of',N); print(sample)
