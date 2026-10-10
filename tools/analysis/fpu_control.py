import pefile,sys
from capstone import *
pe=pefile.PE(sys.argv[1]); base=pe.OPTIONAL_HEADER.ImageBase
iat={i.name.decode():i.address for d in pe.DIRECTORY_ENTRY_IMPORT for i in d.imports if i.name}
targets={hex(iat[n]):n for n in ('_control87','_clearfp')}
print(targets)
t=pe.sections[0]; code=pe.get_memory_mapped_image()[t.VirtualAddress:t.VirtualAddress+t.Misc_VirtualSize]
md=Cs(CS_ARCH_X86,CS_MODE_32); md.skipdata=True
win=[]
for ins in md.disasm_lite(code,base+t.VirtualAddress):
    win.append(ins); win=win[-8:]
    a,s,m,o=ins
    if m=='call' and any(k in o for k in targets):
        print("----", targets[[k for k in targets if k in o][0]])
        for w in win: print("  %x  %s %s"%(w[0],w[2],w[3]))
