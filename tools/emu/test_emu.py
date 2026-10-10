import random, struct, sys
sys.path.insert(0,__import__('os').path.dirname(__import__('os').path.abspath(__file__)))
from emu import *
M=0xffffffff
import pefile as _pe
_p = _pe.PE(os.environ.get('GAME_DLL', 'Game.dll'), fast_load=True)
TBL = bytes(_p.get_memory_mapped_image()[0x953650:0x953750])  # permutation table at VA 0x6F953650
def py_seed(s):
    s&=M
    b = ((s%59)<<10) | ((s%61)<<2) | ((s%53)<<18) | ((((s//47)*17 + s) & 0x3f) << 26)
    return s, b & M
def py_step(A,B):
    b1=(B>>24)&0xff; b2=(B>>16)&0xff; b3=(B>>8)&0xff; b4=B&0xff
    def adj(v,d,mod): 
        r=v-d
        return r if r>=0 else r+mod
    i1=adj(b1,4,188); i2=adj(b2,12,212); i3=adj(b3,24,236); i4=adj(b4,28,244)
    def T(i): return struct.unpack_from('<I', TBL, i)[0]
    x1,x2,x3,x4=T(i1),T(i2),T(i3),T(i4)
    rol=lambda x,n: ((x<<n)|(x>>(32-n)))&M
    mix = rol(x3,3) ^ rol(x2,2) ^ x4 ^ rol(x1,1)
    A2=(A+mix)&M
    B2=((i1<<24)|(i2<<16)|(i3<<8)|i4)&M
    return A2,B2
def py_int(lo,hi,r):
    if lo==hi: return lo
    size = (hi-lo+1) if lo<hi else (lo-hi+1)
    v = lo + ((r*size)>>32)
    return ((v+2**31)%2**32)-2**31
random.seed(1)
# 1. seeder
bad=0
for s in [0,1,2,47,59,61,53,0x69707365,0x77617233,123456789,M]+[random.getrandbits(32) for _ in range(40)]:
    write_state(GADDR,0,0)
    call(0x6f4ab660, ecx=GADDR, stack_args=[s])
    got=read_state(GADDR); exp=py_seed(s)
    if got!=exp: bad+=1; print('SEED MISMATCH',hex(s),[hex(x) for x in got],[hex(x) for x in exp]) if bad<4 else None
print('seed tests done, mismatches =',bad)
# 2. step
bad=0
for t in range(200):
    A,B=random.getrandbits(32),random.getrandbits(32)
    write_state(GADDR,A,B)
    out=call(0x6f011390, ecx=GADDR)
    got=read_state(GADDR); exp=py_step(A,B)
    if got!=exp or out!=exp[0]:
        bad+=1
        if bad<4: print('STEP MISMATCH', hex(A),hex(B), [hex(x) for x in got], [hex(x) for x in exp], hex(out))
print('step tests done, mismatches =',bad)
