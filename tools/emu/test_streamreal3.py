import random, struct, sys
sys.path.insert(0,__import__('os').path.dirname(__import__('os').path.abspath(__file__)))
from emu import *
from test_emu import py_seed, py_step
import test_streams as ts
M=0xffffffff
def b2f(b): return struct.unpack('<f',struct.pack('<I',b&M))[0]
def f32(x): return struct.unpack('<f',struct.pack('<f',x))[0]
random.seed(33)
for c0,c1 in [(0.0,1.0),(0.0,2.5),(-3.0,4.0)]:
    mu.mem_write(0x6faae470, struct.pack('<f',c0)); mu.mem_write(0x6faae4f4, struct.pack('<f',c1))
    ok=0; N=150; ex=[]
    for _ in range(N):
        k=random.randint(0,44); s=random.getrandbits(32)
        addr=ts.BASE+4+8*k
        write_state(addr,*py_seed(s)); st=read_state(addr)
        OUT=DATA+0x700
        call(0x6f2853d0, ecx=OUT, edx=k)
        got=b2f(struct.unpack('<I',bytes(mu.mem_read(OUT,4)))[0])
        s1=py_step(*st)
        U=b2f((s1[0]&0x7fffff)|0x3f800000)-1.0
        exp_mod=f32(f32(U*f32(c1-c0))+c0)
        if got==exp_mod: ok+=1
        if len(ex)<2: ex.append((got,exp_mod))
    print('c0=%g c1=%g match %d/%d'%(c0,c1,ok,N), ex)
