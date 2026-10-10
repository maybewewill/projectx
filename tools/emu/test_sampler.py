import random, struct, sys
sys.path.insert(0,__import__('os').path.dirname(__import__('os').path.abspath(__file__)))
from emu import *
from test_emu import py_seed, py_step
M=0xffffffff
random.seed(5)
bad=0; n=0
SPTR=DATA+0x400
for trial in range(300):
    seed=random.getrandbits(32); count=random.randint(1,16); sides=random.randint(1,100)
    write_state(SPTR,*py_seed(seed)); st=read_state(SPTR)
    # emulated: ecx=sides, edx=count, stack=stream pointer
    got=call(0x6f26cad0, ecx=sides, edx=count, stack_args=[SPTR])
    cur=st; tot=0
    for _ in range(count):
        cur=py_step(*cur)
        tot=(tot + ((cur[0]*sides)>>32))&M
    if got!=tot or read_state(SPTR)!=cur:
        bad+=1
        if bad<4: print('SAMPLER MISMATCH', count, sides, got, tot)
    n+=1
print('sampler (count<=16) cases',n,'mismatches',bad)
