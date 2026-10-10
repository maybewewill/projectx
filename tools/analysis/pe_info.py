import pefile,sys,collections
pe=pefile.PE(sys.argv[1])
print("ImageBase %x Entry %x TimeDate %x"%(pe.OPTIONAL_HEADER.ImageBase,pe.OPTIONAL_HEADER.AddressOfEntryPoint,pe.FILE_HEADER.TimeDateStamp))
for s in pe.sections: print(s.Name.rstrip(b'\0'),hex(s.VirtualAddress),hex(s.Misc_VirtualSize),hex(s.SizeOfRawData),round(s.get_entropy(),2))
print("Imports:")
for d in pe.DIRECTORY_ENTRY_IMPORT: print(" ",d.dll.decode(),len(d.imports))
if hasattr(pe,'DIRECTORY_ENTRY_EXPORT'):
  ex=pe.DIRECTORY_ENTRY_EXPORT.symbols; print("Exports",len(ex)); [print("  ",e.ordinal,e.name) for e in ex[:30]]
if hasattr(pe,'DIRECTORY_ENTRY_DEBUG'):
  for d in pe.DIRECTORY_ENTRY_DEBUG: print("debug",d.struct.Type, getattr(d.entry,'PdbFileName',None))
print("Rich:", pe.parse_rich_header() and [(hex(x)) for x in pe.parse_rich_header()['values'][::2]][:20])
