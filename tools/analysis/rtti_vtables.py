# Maps MSVC vtables to class names via RTTI Complete Object Locators (x86).
# vtable[-1] -> COL {sig, offset, cdOffset, pTypeDescriptor, pClassHierarchy}; TD name at +8.
import pefile,sys,struct
pe=pefile.PE(sys.argv[1]); base=pe.OPTIONAL_HEADER.ImageBase; img=pe.get_memory_mapped_image()
def u32(va):
    o=va-base
    return struct.unpack_from('<I',img,o)[0] if 0<=o<=len(img)-4 else None
text=pe.sections[0]; tlo=base+text.VirtualAddress; thi=tlo+text.Misc_VirtualSize
rd=[s for s in pe.sections if s.Name.startswith(b'.rdata')][0]
lo=base+rd.VirtualAddress; hi=lo+rd.Misc_VirtualSize
out=[]
for va in range(lo,hi-4,4):
    col=u32(va)
    if col is None or not (lo<=col<hi): continue
    if u32(col)!=0: continue                      # signature 0 on x86
    td=u32(col+12)
    if td is None: continue
    o=td-base+8
    if not (0<=o<len(img)) or img[o:o+4] not in (b'.?AV',b'.?AU'): continue
    f0=u32(va+4)
    if f0 is None or not (tlo<=f0<thi): continue
    name=img[o:img.find(b'\0',o)].decode('latin1')
    n=0; funcs=[]
    while True:
        f=u32(va+4+4*n)
        if f is None or not (tlo<=f<thi): break
        funcs.append(f); n+=1
    out.append((va+4,name,u32(col+4),funcs))
for vt,name,off,funcs in out:
    print(f"{vt:#x}\t{name}\t{off}\t{len(funcs)}\t"+",".join(f"{f:#x}" for f in funcs))
