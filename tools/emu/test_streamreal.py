import random, struct, sys
sys.path.insert(0,__import__('os').path.dirname(__import__('os').path.abspath(__file__)))
from emu import *
from test_emu import py_seed, py_step
import test_streams as ts   # sets up stub for slot lookup and BASE
M=0xffffffff
def b2f(b): return struct.unpack('<f',struct.pack('<I',b&M))[0]
random.seed(21)
for c0,c1 in [(0.0,1.0),(0.0,100.0)]:
    mu.mem_write(0x6faae470, struct.pack('<f',c0)); mu.mem_write(0x6faae4f4, struct.pack('<f',c1))
    # stream index k at BASE+4+8k
    res=[]; 
    for trial in range(200):
        k=random.randint(0,44); s=random.getrandbits(32)
        write_state(BASE+4+8*k,*py_seed(s)); st=read_state(BASE+4+8*k)
        OUT=DATA+0x700
        mu.mem_write(OUT, struct.pack('<I',0))
        call(0x6f2853d0, ecx=OUT, edx=k)
        got=b2f(struct.unpack('<I',bytes(mu.mem_read(OUT,4)))[0])
        A2,_=py_step(*st)
        u=b2f((A2&0x7fffff)|0x3f800000)-1.0
        res.append((got,u*(c1-c0)+c0))
    ok=sum(1 for g,e in res if g==e)
    close=sum(1 for g,e in res if abs(g-e)<=1e-5*max(1,abs(e)))
    print('c0=%g c1=%g exact-match %d/200, within 1e-5 rel %d/200'%(c0,c1,ok,close), 'sample',res[:2])
