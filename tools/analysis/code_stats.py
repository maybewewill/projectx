import pefile,sys,re,struct
from capstone import *
pe=pefile.PE(sys.argv[1]); base=pe.OPTIONAL_HEADER.ImageBase
data=pe.get_memory_mapped_image()
t=pe.sections[0]; ts=t.VirtualAddress; te=ts+t.Misc_VirtualSize
code=data[ts:te]
# RTTI
rtti=set(re.findall(rb'\.\?A[VU]([A-Za-z0-9_@?$]+)@@',data))
print("RTTI classes:",len(rtti)); print(sorted(x.decode() for x in rtti)[:60])
# strings
strs=re.findall(rb'[\x20-\x7e]{6,}',data[pe.sections[1].VirtualAddress:pe.sections[2].VirtualAddress+pe.sections[2].Misc_VirtualSize])
print("ascii strings>=6:",len(strs))
srcs=sorted(set(s.decode() for s in strs if re.search(rb'\.(cpp|h)$',s,re.I)))
print("source file refs:",len(srcs)); print(srcs[:80])
# call targets via E8
tg=set()
md=Cs(CS_ARCH_X86,CS_MODE_32); md.skipdata=True
n=0
for i in md.disasm_lite(code,base+ts):
    n+=1
    if i[2]=='call' and i[3].startswith('0x'):
        v=int(i[3],16)
        if base+ts<=v<base+te: tg.add(v)
print("instructions (linear):",n,"direct call targets:",len(tg))
