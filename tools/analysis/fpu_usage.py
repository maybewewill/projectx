import pefile,sys,collections
from capstone import *
pe=pefile.PE(sys.argv[1]); base=pe.OPTIONAL_HEADER.ImageBase
t=pe.sections[0]; code=pe.get_memory_mapped_image()[t.VirtualAddress:t.VirtualAddress+t.Misc_VirtualSize]
md=Cs(CS_ARCH_X86,CS_MODE_32); md.skipdata=True
c=collections.Counter(); fldcw=[]
for a,s,m,o in md.disasm_lite(code,base+t.VirtualAddress):
    if m.startswith('f') : c['x87']+=1; c[m]+=1
    if m=='fldcw': fldcw.append(hex(a))
    if 'xmm' in o: c['sse']+=1
for k in ['x87','sse','fsin','fcos','fsincos','fpatan','fsqrt','fptan','f2xm1','fyl2x','fprem','frndint','fistp','fldcw']: print(k,c[k])
print("fldcw sites:",fldcw[:20])
imp=[i.name.decode() for d in pe.DIRECTORY_ENTRY_IMPORT for i in d.imports if i.name]
print([x for x in imp if 'control' in x.lower() or 'fp' in x.lower() or x in('rand','srand','sin','cos','atan2','sqrt','floor','pow')])
