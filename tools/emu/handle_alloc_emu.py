# Unicorn emulation of the JASS handle-id allocator in Game.dll 1.26a.
# Real code is executed for: FUN_6f430c80 (create id), FUN_6f42dd40 (release),
# FUN_6f42d3f0 (free), FUN_6f3a3d70 (free-stack pop), FUN_6f3a3d20 (free-stack push),
# FUN_6f42cb50 (table push), FUN_6f42cc10 (table set-size), FUN_6f428080 (AddRef).
# External callees are replaced by Python stubs (see STUBS). Nothing else is modelled.
import sys, struct
import os
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), 'pylib'))
import pefile
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_PROT_ALL
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EAX, UC_X86_REG_ESP, UC_X86_REG_EIP

DLL = os.environ.get('GAME_DLL', 'Game.dll')
pe = pefile.PE(DLL, fast_load=True)
BASE = pe.OPTIONAL_HEADER.ImageBase
SIZE = (pe.OPTIONAL_HEADER.SizeOfImage + 0xfff) & ~0xfff
img = bytes(pe.get_memory_mapped_image())

mu = Uc(UC_ARCH_X86, UC_MODE_32)
mu.mem_map(0, 0x1000, UC_PROT_ALL)                 # FS:[0] (ExceptionList) lives at address 0
mu.mem_map(BASE, SIZE, UC_PROT_ALL)
mu.mem_write(BASE, img[:SIZE].ljust(SIZE, b'\0'))
mu.mem_map(0x00100000, 0x10000, UC_PROT_ALL)       # stack
mu.mem_map(0x00300000, 0x1000, UC_PROT_ALL)        # sentinel return + fake vtable stubs
mu.mem_map(0x00400000, 0x200000, UC_PROT_ALL)      # heap for fake objects
SENT = 0x00300000
TOP = 0x0010f000
_heap = [0x00400000]
def halloc(n):
    p = _heap[0]; _heap[0] += (n + 15) & ~15
    mu.mem_write(p, b'\0' * ((n + 15) & ~15)); return p
def rd(a): return struct.unpack('<I', bytes(mu.mem_read(a, 4)))[0]
def wr(a, v): mu.mem_write(a, struct.pack('<I', v & 0xffffffff))
def reg(r): return mu.reg_read(r)

# ---------- stub implementations (external callees only) ----------
STATE = {'map': {}, 'agent_destroy_calls': 0, 'grow_calls': []}
FAKE_TAG_OBJ = None
def stub_ret(pop, eax=None, ecx_value=None):
    esp = reg(UC_X86_REG_ESP)
    ret = rd(esp)
    if eax is not None: mu.reg_write(UC_X86_REG_EAX, eax & 0xffffffff)
    mu.reg_write(UC_X86_REG_ESP, esp + 4 + pop)
    mu.reg_write(UC_X86_REG_EIP, ret)
def arg(i):  # i-th stack argument of a callee whose return address is at [esp]
    return rd(reg(UC_X86_REG_ESP) + 4 + 4 * i)

def grow(vec, newcap, elem):
    # vector layout: +0 cap, +4 size, +8 data, +0xc granularity
    size = rd(vec + 4); old = rd(vec + 8)
    buf = halloc(newcap * elem)
    if old and size:
        mu.mem_write(buf, bytes(mu.mem_read(old, size * elem)))
    wr(vec + 8, buf); wr(vec + 0, newcap)
    STATE['grow_calls'].append((vec, elem, newcap))

def h_FUN_6f03fa30(uc):   # fastcall(ECX,EDX); returns an "agent-class" object (tag 'lgaa' at +0xc, +0x20==0)
    stub_ret(0, eax=STATE['agent_tag_obj'])
def h_FUN_6f0418a0(uc):   # no args: agent-type check, report false so the vtable[0x5c] notify is skipped
    stub_ret(0, eax=0)
def h_FUN_6f42ccf0(uc):   # thiscall(map, key, tag) -> node or 0
    key = arg(0); stub_ret(8, eax=STATE['map'].get(key, 0))
def h_FUN_6f430580(uc):   # thiscall(map, key, 0, 0) -> new zeroed node registered under key
    key = arg(0); node = halloc(0x20); STATE['map'][key] = node
    stub_ret(12, eax=node)
def h_FUN_6f42b060(uc):   # thiscall(ECX=table vector, newcap) : resize, elem 12
    grow(reg(UC_X86_REG_ECX), arg(0), 12); stub_ret(4)
def h_FUN_6f3a1aa0(uc):   # thiscall(ECX=free-stack vector, newcap) : resize, elem 4
    grow(reg(UC_X86_REG_ECX), arg(0), 4); stub_ret(4)
def h_FUN_6f02f780(uc):   # thiscall(ECX, x): bookkeeping no-op, returns ECX
    stub_ret(4, eax=reg(UC_X86_REG_ECX))
def h_FUN_6f022aa0(uc):   # thiscall(ECX=&entry.agentslot, agent): CAgentPtr assign (+AddRef on agent)
    slot = reg(UC_X86_REG_ECX); ag = arg(0)
    wr(slot, ag); wr(ag + 4, rd(ag + 4) + 1); stub_ret(4, eax=slot)
def h_noop_pop(n):
    def f(uc): stub_ret(n)
    return f
def h_FUN_6f427550(uc): raise RuntimeError('granularity fallback reached (gran should be nonzero)')

STUBS = {
    0x6f03fa30: h_FUN_6f03fa30, 0x6f0418a0: h_FUN_6f0418a0, 0x6f42ccf0: h_FUN_6f42ccf0,
    0x6f430580: h_FUN_6f430580, 0x6f42b060: h_FUN_6f42b060, 0x6f3a1aa0: h_FUN_6f3a1aa0,
    0x6f02f780: h_FUN_6f02f780, 0x6f022aa0: h_FUN_6f022aa0, 0x6f471a70: h_noop_pop(8),
    0x6f4287c0: h_noop_pop(4), 0x6f427550: h_FUN_6f427550, 0x6f3a0480: h_FUN_6f427550,
}
# fake vtables: agent vtable[0] (delete) and map vtable[0] (free node) point into the sentinel page
AGENT_DEL_STUB = 0x00300100
MAP_FREE_STUB = 0x00300110
STUBS[AGENT_DEL_STUB] = h_noop_pop(0)
STUBS[MAP_FREE_STUB] = h_noop_pop(4)

def _hook(uc, addr, size, user):
    f = STUBS.get(addr)
    if f is not None:
        f(uc)
mu.hook_add(UC_HOOK_CODE, _hook, None, 0x00300000, 0x00300200)
for a in list(STUBS):
    if a < 0x00300000 or a >= 0x00300200:
        mu.hook_add(UC_HOOK_CODE, _hook, None, a, a)

def call(fn, ecx=None, args=()):
    esp = TOP
    for a in reversed(args):
        esp -= 4; wr(esp, a)
    esp -= 4; wr(esp, SENT)
    mu.reg_write(UC_X86_REG_ESP, esp)
    if ecx is not None: mu.reg_write(UC_X86_REG_ECX, ecx & 0xffffffff)
    mu.emu_start(fn, SENT, timeout=5_000_000, count=2_000_000)
    return reg(UC_X86_REG_EAX)

F = {
    'create':  0x6f430c80,  # thiscall(this=manager, agent, flag) -> id
    'release': 0x6f42dd40,  # thiscall(this=manager, id)
    'addref':  0x6f428080,  # thiscall(this=manager, id)
    'tbl_set': 0x6f42cc10,  # thiscall(this=table vector, newsize)
}

# ---------- fake manager and agents ----------
M = halloc(0x400)                       # manager object (fields used: +0x184..0x1a4)
VT_MAP = halloc(0x10); wr(VT_MAP, MAP_FREE_STUB)
wr(M + 0x1a4, VT_MAP)                   # the map member at +0x1a4 (its first dword is the vtable)
STATE['agent_tag_obj'] = halloc(0x40); wr(STATE['agent_tag_obj'] + 0xc, 0x2b61676c); wr(STATE['agent_tag_obj'] + 0x20, 0)
VT_AGENT = halloc(0x10); wr(VT_AGENT, AGENT_DEL_STUB)
def fresh_mgr():
    STATE['map'].clear(); STATE['grow_calls'].clear()
    for off in (0x184, 0x194):           # vectors: +0 cap, +4 size, +8 data, +0xc gran
        wr(M + off + 0, 0); wr(M + off + 4, 0); wr(M + off + 8, 0)
    wr(M + 0x190, 1000); wr(M + 0x1a0, 1000)   # granularity, as the reset routine sets it
def new_agent(key):
    a = halloc(0x40); wr(a, VT_AGENT); wr(a + 4, 1); wr(a + 0x10, key); return a

# table vector base = M+0x194, free-stack vector base = M+0x184
TBL = M + 0x194; FREE = M + 0x184
def table_size(): return rd(TBL + 4)
def free_stack():
    n = rd(FREE + 4); d = rd(FREE + 8)
    return [rd(d + 4 * i) for i in range(n)]
def entry(id_):
    base = rd(TBL + 8); return base + (id_ - 0x100000) * 12   # +0 refcount, +4 agent, +8 flag
def refcnt(id_): return rd(entry(id_))
def agent_of(id_): return rd(entry(id_) + 4)

# ---------- scenarios ----------
RESULTS = []
def check(name, cond, detail=''):
    RESULTS.append((name, bool(cond), detail))
    print(('PASS ' if cond else 'FAIL ') + name + ('  ' + detail if detail else ''))

def create(agent):
    return call(F['create'], ecx=M, args=(agent, 1))
def release(id_):
    return call(F['release'], ecx=M, args=(id_,))
def addref(id_):
    return call(F['addref'], ecx=M, args=(id_,))

# S1: plain allocation, LIFO reuse
fresh_mgr()
a = [new_agent(0x1000 + i) for i in range(6)]
i1, i2, i3 = create(a[0]), create(a[1]), create(a[2])
check('S1 first ids are 0x100000,0x100001,0x100002', (i1, i2, i3) == (0x100000, 0x100001, 0x100002), '%x %x %x' % (i1, i2, i3))
check('S1 table size = 3 after three creates', table_size() == 3, str(table_size()))
check('S1 each new id has refcount 1', all(refcnt(i) == 1 for i in (i1, i2, i3)))
check('S1 agent refcount 2 (creator + handle table)', rd(a[0] + 4) == 2, str(rd(a[0] + 4)))
release(i2)
check('S1 release at zero pushes id on free stack', free_stack() == [0x100001], str([hex(x) for x in free_stack()]))
check('S1 freed entry clears agent pointer', agent_of(i2) == 0)
check('S1 freed agent refcount back to 1', rd(a[1] + 4) == 1, str(rd(a[1] + 4)))
release(i1)
check('S1 free stack is [0x100001,0x100000]', free_stack() == [0x100001, 0x100000], str([hex(x) for x in free_stack()]))
j1 = create(a[3])
check('S1 next create reuses most recently freed id (LIFO): 0x100000', j1 == 0x100000, hex(j1))
j2 = create(a[4])
check('S1 following create reuses 0x100001', j2 == 0x100001, hex(j2))
j3 = create(a[5])
check('S1 free stack empty, fresh id = 0x100000 + table size (0x100003)', j3 == 0x100003, hex(j3))
check('S1 table size = 4 (never shrinks)', table_size() == 4, str(table_size()))

# S2: AddRef keeps id alive; freed only when count reaches 0
fresh_mgr()
b = new_agent(0x2000); x = create(b)
addref(x)
check('S2 after AddRef refcount = 2', refcnt(x) == 2, str(refcnt(x)))
release(x)
check('S2 one release leaves id alive (refcount 1, not on free stack)', refcnt(x) == 1 and free_stack() == [], str(refcnt(x)))
check('S2 agent still attached while id alive', agent_of(x) == b)
release(x)
check('S2 second release frees id (on free stack)', free_stack() == [x], str([hex(v) for v in free_stack()]))

# S3: same agent asked again while its id is alive returns the same id, no count change
fresh_mgr()
c = new_agent(0x3000); y = create(c)
y2 = create(c)
check('S3 same agent returns same id', y2 == y, '%x vs %x' % (y2, y))
check('S3 repeated create does not change refcount', refcnt(y) == 1, str(refcnt(y)))
check('S3 repeated create does not allocate a new table slot', table_size() == 1, str(table_size()))

# S4: effect of table-reset routine (FUN_6f42cc10(1) as called from FUN_6f42d2e0)
fresh_mgr()
call(F['tbl_set'], ecx=TBL, args=(1,))
check('S4 after set-size 1 the table has one dummy entry', table_size() == 1, str(table_size()))
d = new_agent(0x4000); z = create(d)
check('S4 first issued id after reset is 0x100001 (0x100000 is the dummy slot)', z == 0x100001, hex(z))

print('\nSUMMARY: %d/%d checks passed' % (sum(1 for r in RESULTS if r[1]), len(RESULTS)))
print('grow calls (vec, elem, newcap) in last scenario:', STATE['grow_calls'])
