import pefile,sys
from capstone import *
pe=pefile.PE(sys.argv[1]); base=pe.OPTIONAL_HEADER.ImageBase
tgt=int(sys.argv[2],16)
t=pe.sections[0]; code=pe.get_memory_mapped_image()[t.VirtualAddress:t.VirtualAddress+t.Misc_VirtualSize]
md=Cs(CS_ARCH_X86,CS_MODE_32); md.skipdata=True
for a,s,m,o in md.disasm_lite(code,base+t.VirtualAddress):
    if m in('call','jmp') and o==hex(tgt): print(hex(a),m)
