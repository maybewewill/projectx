import random, struct, sys
sys.path.insert(0,__import__('os').path.dirname(__import__('os').path.abspath(__file__)))
from emu import *
M=0xffffffff
def b2f(b): return struct.unpack('<f',struct.pack('<I',b&M))[0]
def f2b(f): return struct.unpack('<I',struct.pack('<f',f))[0]
def rn(x): return struct.unpack('<f',struct.pack('<f',x))[0]
def rz(x):
    f=rn(x)
    if f==x: return f
    if abs(f)>abs(x): # rounded away from zero -> step toward zero
        bits=f2b(f)-1  # for positive f: decrement magnitude; for negative f: bits-1 increases magnitude!
        if f>0: return b2f(bits)
        return b2f(f2b(f)-1 if False else f2b(f)-0) if False else (b2f(f2b(f)-1) if f<0 and False else f)
    return f
def rz2(x):
    # magnitude truncation: pick the float32 neighbour between 0 and x closest to x (toward zero)
    f=rn(x)
    if f==x: return f
    if abs(f)>abs(x):
        bits=f2b(f)
        # step magnitude down by one ulp
        return b2f(bits-1) if f>0 else b2f(bits+1) if f<0 else f
    return f
OUT=DATA+0x300; PX=DATA+0x310; PY=DATA+0x314
random.seed(3)
names={'add 6f6ef000':0x6f6ef000,'sub 6f6eeef0':0x6f6eeef0,'mul 6f6eee20':0x6f6eee20}
for nm,fn in names.items():
    stats={'RN':0,'RZ2':0}; N=400
    for _ in range(N):
        x=rn(random.uniform(-600,600)); y=rn(random.uniform(-600,600))
        if random.random()<0.2: y=rn(random.uniform(0.9,1.1))
        mu.mem_write(PX,struct.pack('<f',x)); mu.mem_write(PY,struct.pack('<f',y))
        call(fn, ecx=OUT, edx=PX, stack_args=[PY])
        got=b2f(struct.unpack('<I',bytes(mu.mem_read(OUT,4)))[0])
        if 'add' in nm: ex=x+y
        elif 'sub' in nm: ex=x-y
        else: ex=x*y
        if got==rn(ex): stats['RN']+=1
        if got==rz2(ex): stats['RZ2']+=1
    print(nm, stats, 'of',N)
