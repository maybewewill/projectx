import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;
import java.io.*; import java.util.*;
public class Survey extends GhidraScript {
  public void run() throws Exception {
    String out = getScriptArgs()[0];
    FunctionManager fm = currentProgram.getFunctionManager();
    List<Function> all = new ArrayList<>();
    for (Function f : fm.getFunctions(true)) if(!f.isThunk() && !f.isExternal()) all.add(f);
    long totalBytes=0; int[] buckets=new int[6];
    for (Function f: all){ long n=f.getBody().getNumAddresses(); totalBytes+=n;
      buckets[n<32?0:n<128?1:n<512?2:n<2048?3:n<8192?4:5]++; }
    PrintWriter s = new PrintWriter(new FileWriter(out+"/summary.txt"));
    s.println("functions="+all.size()+" bytes="+totalBytes);
    s.println("size buckets <32,<128,<512,<2K,<8K,>=8K: "+Arrays.toString(buckets));
    int named=0; for(Function f:all) if(!f.getName().startsWith("FUN_")) named++;
    s.println("auto-named(non FUN_)="+named);
    DecompInterface di=new DecompInterface(); di.openProgram(currentProgram);
    Collections.shuffle(all,new Random(42));
    int N=Math.min(1500,all.size()), ok=0, warn=0, fail=0; long lines=0, t0=System.currentTimeMillis();
    PrintWriter samp=new PrintWriter(new FileWriter(out+"/sample.c"));
    for(int i=0;i<N;i++){ Function f=all.get(i);
      DecompileResults r=di.decompileFunction(f,60,monitor);
      if(r==null||!r.decompileCompleted()){fail++;continue;}
      String c=r.getDecompiledFunction().getC(); ok++;
      if(c.contains("WARNING")) warn++;
      lines+=c.split("\n").length;
      if(i<60) samp.println("// ==== "+f.getName()+" @ "+f.getEntryPoint()+"\n"+c);
    }
    long dt=System.currentTimeMillis()-t0;
    s.println("decompile sample N="+N+" ok="+ok+" withWARNING="+warn+" fail="+fail+" avgLines="+(ok>0?lines/ok:0)+" ms/func="+(dt/Math.max(1,N)));
    s.println("extrapolated pseudo-C lines total ~= "+(ok>0?lines*all.size()/ok:0));
    samp.close(); s.close();
  }
}
