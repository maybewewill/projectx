import random, struct, sys, math
sys.path.insert(0,__import__('os').path.dirname(__import__('os').path.abspath(__file__)))
from emu import *
from test_emu import py_seed, py_step
M=0xffffffff
def b2f(b): return struct.unpack('<f',struct.pack('<I',b&M))[0]
def f2b(f): return struct.unpack('<I',struct.pack('<f',f))[0]
def rz(x):
    # round a python float (double) to float32 by truncation toward zero
    if x!=x or x in (float('inf'),float('-inf')) or x==0: return x
    f=struct.unpack('<f',struct.pack('<f',x))[0]
    if abs(f)>abs(x) or (f!=0 and (f>0)!=(x>0)):
        # rounded away: step toward zero by one ulp
        bits=struct.unpack('<I',struct.pack('<f',f))[0]
        bits-=1
        f=struct.unpack('<f',struct.pack('<I',bits))[0]
    return f
def rn(x): return struct.unpack('<f',struct.pack('<f',x))[0]
def f32(x): return rn(x)
random.seed(11)
modes={'RN':(rn,rn,rn),'RZ':(rz,rz,rz)}
PA=DATA+0x200; PB=DATA+0x204
res={k:0 for k in modes}; tot=0
for trial in range(300):
    seed=random.getrandbits(32)
    a=f32(random.uniform(-500,500)); b=f32(random.uniform(-500,500))
    if trial%9==0: b=a
    mu.mem_write(PA, struct.pack('<f',a)); mu.mem_write(PB, struct.pack('<f',b))
    write_state(GADDR,*py_seed(seed)); st0=read_state(GADDR)
    got=b2f(call(0x6f3b35a0, stack_args=[PA,PB]))
    A2,_=py_step(*st0)
    u=b2f((A2 & 0x7fffff) | 0x3f800000)
    tot+=1
    for name,(r_sub,r_mul,r_add) in modes.items():
        rng=r_sub(abs(b-a)) if False else r_sub(abs(a-b))
        U=(u-1.0)  # exact
        t=r_mul(rng*U)
        exp=r_add(a+t)
        if exp!=got: res[name]+=1
print('total',tot,'mismatch by mode',res)
