import pefile,sys,re,collections
pe=pefile.PE(sys.argv[1]); d=pe.get_memory_mapped_image()
sigs=re.findall(rb'\x00(\([A-Za-z;]*\)[A-Za-z][A-Za-z;]*)\x00',d)
print("JASS native signatures:",len(sigs))
paths=set(m.decode().lower() for m in re.findall(rb'e:\\drive1\\temp\\buildwar3x\\[\w\\/.]+',d,re.I))
c=collections.Counter()
for p in paths:
  parts=re.split(r'[\\/]',p); c['/'.join(parts[4:6])]+=1
print("full source paths:",len(paths))
for k,v in c.most_common(40): print(f"  {v:4} {k}")
