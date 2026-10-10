import random, struct, sys
sys.path.insert(0,__import__('os').path.dirname(__import__('os').path.abspath(__file__)))
from emu import *
from test_emu import py_seed, py_step
M=0xffffffff
OBJ=DATA+0x500; P=DATA+0x520; BASE=DATA+0x1000
mu.mem_write(OBJ+0x10, struct.pack('<I',P)); mu.mem_write(P+0xc, struct.pack('<I',BASE))
# stub slot lookup 0x6f4c34d0 : mov eax, OBJ ; ret
mu.mem_write(0x6f4c34d0, b'\xb8'+struct.pack('<I',OBJ)+b'\xc3')
def streams_expected(t):
    loc=py_seed(t); out=[]
    for k in range(45):
        loc=py_step(*loc)
        out.append(py_seed(loc[0]))
    return out
def check(label, got_streams, exp_streams):
    bad=sum(1 for g,e in zip(got_streams,exp_streams) if g!=e)
    print(label, 'streams mismatched:', bad, 'of 45')
random.seed(9)
# 1. stream init directly: 6f2852d0 is __fastcall(ecx=seed)
for trial in range(5):
    s=random.getrandbits(32)
    for k in range(45): write_state(BASE+4+8*k,0,0)
    call(0x6f2852d0, ecx=s)
    got=[read_state(BASE+4+8*k) for k in range(45)]
    check('stream init seed=%08x'%s, got, streams_expected(s))
# 2. SetRandomSeed (JASS): G seeded, G advanced once, then streams initialised from that output
for trial in range(5):
    s=random.getrandbits(32)
    write_state(GADDR,0,0)
    for k in range(45): write_state(BASE+4+8*k,0,0)
    call(0x6f3b36d0, stack_args=[s])
    gst=read_state(GADDR)
    t=py_step(*py_seed(s))[0]
    check('SetRandomSeed s=%08x G-ok=%s'%(s, gst==py_step(*py_seed(s))), [read_state(BASE+4+8*k) for k in range(45)], streams_expected(t))
