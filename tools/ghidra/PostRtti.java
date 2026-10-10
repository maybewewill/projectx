import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;
import java.io.*; import java.util.*;
public class PostRtti extends GhidraScript {
  public void run() throws Exception {
    String out = getScriptArgs()[0];
    FunctionManager fm = currentProgram.getFunctionManager();
    int total=0, named=0, inClass=0;
    PrintWriter tsv=new PrintWriter(new FileWriter(out+"/functions.tsv"));
    for (Function f : fm.getFunctions(true)) { if(f.isExternal()) continue; total++;
      String ns=f.getParentNamespace().isGlobal()?"":f.getParentNamespace().getName(true);
      if(!f.getName().startsWith("FUN_")) named++;
      if(!ns.isEmpty()) inClass++;
      tsv.println(f.getEntryPoint()+"\t"+f.getBody().getNumAddresses()+"\t"+ns+"\t"+f.getName()); }
    tsv.close();
    PrintWriter s=new PrintWriter(new FileWriter(out+"/post_rtti.txt"));
    s.println("functions="+total+" named="+named+" inClassNamespace="+inClass);
    DecompInterface di=new DecompInterface(); di.openProgram(currentProgram);
    PrintWriter c=new PrintWriter(new FileWriter(out+"/control87_callers.c"));
    for (Symbol sym: currentProgram.getSymbolTable().getSymbols("_control87")) {
      for (Reference r: getReferencesTo(sym.getAddress())) {
        Function f=getFunctionContaining(r.getFromAddress()); if(f==null) continue;
        DecompileResults d=di.decompileFunction(f,60,monitor);
        if(d.decompileCompleted()) c.println("// ref from "+r.getFromAddress()+"\n"+d.getDecompiledFunction().getC());
      }
      // thunk callers
      for (Function th: fm.getFunctions(true)) if(th.isThunk() && th.getThunkedFunction(true)!=null && th.getThunkedFunction(true).getName().equals("_control87"))
        for (Reference r: getReferencesTo(th.getEntryPoint())) { Function f=getFunctionContaining(r.getFromAddress()); if(f==null) continue;
          DecompileResults d=di.decompileFunction(f,60,monitor);
          if(d.decompileCompleted()) c.println("// ref(thunk) from "+r.getFromAddress()+"\n"+d.getDecompiledFunction().getC()); }
    }
    c.close(); s.close();
  }
}
