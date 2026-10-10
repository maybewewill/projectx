import pefile,sys,re
from capstone import *
pe=pefile.PE(sys.argv[1]); base=pe.OPTIONAL_HEADER.ImageBase; img=pe.get_memory_mapped_image()
def cstr(va):
    o=va-base
    if not (0<=o<len(img)): return None
    e=img.find(b'\0',o); s=img[o:e]
    return s.decode('latin1') if 0<len(s)<200 and all(32<=c<127 for c in s) else None
start=int(sys.argv[2],16); size=int(sys.argv[3])
md=Cs(CS_ARCH_X86,CS_MODE_32)
pushes=[]; out=[]; callee={}
for ins in md.disasm(img[start-base:start-base+size],start):
    if ins.mnemonic=='push' and ins.op_str.startswith('0x'):
        pushes.append(int(ins.op_str,16))
    elif ins.mnemonic=='mov' and ins.op_str.startswith(('edx, 0x','ecx, 0x')):
        pushes.append(int(ins.op_str.split(', ')[1],16))
    elif ins.mnemonic=='call':
        strs=[(p,cstr(p)) for p in pushes]
        sig=[s for p,s in strs if s and s.startswith('(')]
        name=[s for p,s in strs if s and not s.startswith('(')]
        fn=[p for p,s in strs if s is None and base<=p<base+0x86d000]
        if sig and name: out.append((name[-1],sig[-1],hex(fn[-1]) if fn else '?'))
        pushes=[]
for n,s,f in out: print(f"{n}\t{s}\t{f}")
