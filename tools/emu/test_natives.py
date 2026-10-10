import random, struct, sys
sys.path.insert(0,__import__('os').path.dirname(__import__('os').path.abspath(__file__)))
from emu import *
from test_emu import py_seed, py_step, py_int
M=0xffffffff
def s32(x): return ((x+2**31)%2**32)-2**31
random.seed(7)
# GetRandomInt
bad=0; n=0
for trial in range(300):
    seed=random.getrandbits(32)
    lo=random.randint(-1000,1000); hi=random.randint(-1000,1000)
    if trial%7==0: hi=lo
    write_state(GADDR,*py_seed(seed))
    st0=read_state(GADDR)
    got=s32(call(0x6f3b3560, stack_args=[lo,hi]))
    st1=read_state(GADDR)
    if lo==hi:
        exp_state=st0
        exp=lo
    else:
        exp_state=py_step(*st0)
        r=exp_state[0]
        exp=py_int(lo,hi,r)
    n+=1
    if got!=exp or st1!=exp_state:
        bad+=1
        if bad<6: print('INT MISMATCH',lo,hi,got,exp,st1==exp_state)
print('GetRandomInt cases',n,'mismatches',bad)
# GetRandomReal: args are pointers to float32 values
def f2b(f): return struct.unpack('<I',struct.pack('<f',f))[0]
def b2f(b): return struct.unpack('<f',struct.pack('<I',b&M))[0]
def f32(x): return struct.unpack('<f',struct.pack('<f',x))[0]
PA=DATA+0x200; PB=DATA+0x204
bad=0; n=0; diffs=[]
for trial in range(300):
    seed=random.getrandbits(32)
    a=random.uniform(-500,500); b=random.uniform(-500,500)
    if trial%9==0: b=a
    a=f32(a); b=f32(b)
    mu.mem_write(PA, struct.pack('<f',a)); mu.mem_write(PB, struct.pack('<f',b))
    write_state(GADDR,*py_seed(seed))
    st0=read_state(GADDR)
    outbits=call(0x6f3b35a0, stack_args=[PA,PB])
    st1=read_state(GADDR)
    A2,B2=py_step(*st0)
    u_bits=(A2 & 0x7fffff) | 0x3f800000
    U=f32(b2f(u_bits)-1.0)
    rng=f32(abs(f32(b-a)))
    exp=f32(a+f32(rng*U))
    got=b2f(outbits)
    n+=1
    diffs.append(abs(got-exp))
    if st1!=(A2,B2): print('state mismatch')
    if got!=exp and not (got!=got and exp!=exp):
        bad+=1
        if bad<6: print('REAL MISMATCH a=%r b=%r got=%r exp=%r'%(a,b,got,exp))
print('GetRandomReal cases',n,'mismatches',bad,'max abs diff',max(diffs))
