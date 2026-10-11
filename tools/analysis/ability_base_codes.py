import pefile,sys,struct
from capstone import *
pe=pefile.PE(sys.argv[1],fast_load=True); B=pe.OPTIONAL_HEADER.ImageBase; img=pe.get_memory_mapped_image()
md=Cs(CS_ARCH_X86,CS_MODE_32)
def thunk_val(fn):
    o=fn-B
    if img[o]==0xB8 and img[o+5]==0xC3: return struct.unpack_from('<I',img,o+1)[0]
    return None
def fourcc(v): return struct.pack('>I',v).decode('latin1')
for line in open(sys.argv[2]):
    if line.startswith('#'): continue
    code,wr,amb=line.split('\t')[:3]
    fn=int(wr.replace('FUN_','0x'),16); base=None; ctor=None
    for n,i in enumerate(md.disasm(img[fn-B:fn-B+0x100],fn)):
        if i.mnemonic=='call' and i.op_str.startswith('0x'):
            t=int(i.op_str,16); v=thunk_val(t)
            if v is not None and base is None: base=fourcc(v)
            elif ctor is None and v is None: ctor=hex(t)
        if i.mnemonic=='ret' or n>60: break
    print(f"{code}\t{wr}\t{base or '?'}\t{ctor}\t{amb}")
