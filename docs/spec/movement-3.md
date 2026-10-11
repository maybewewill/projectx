# Unit movement, third pass (Game.dll 1.26a)

Builds on movement.md (pass 1) and movement-2.md (pass 2). Labels: CONFIRMED / LIKELY / UNSURE.
Read-only. Nothing in /home/user/projectx was changed by this pass.
Unicorn: the Unicorn checks from pass 2 still hold (scratchpad/mov2_run.py, mov2_t483e70b.py, mov2_tsort.py).

## 1. Feeder of the mover list at world+0x3B0 / +0x3B8  (RESOLVED, CONFIRMED edges)

Corrections to pass 1 and pass 2: the list walked by 0x6F493410 is not an anonymous list. The world constructor
0x6F489750 names the heads (CONFIRMED, from the vtable stores):
- world+0x3B0: CLinkedList<CPrCluster>. Walked by 0x6F483300 through the next pointer at +0x3B8, calling
  0x6F493410 on each node. So each node is a CPrCluster (a mover group), and 0x6F493410 is its update.
- world+0x438: CLinkedList<CPoPosCl>. Walked through +0x440 by 0x6F494200 (per-unit steer/turn, section 5).
- world+0x514: CLinkedList<CPoSeparate>. Walked by 0x6F4961C0 on alternating nodes (pass 1).

Insertion into the cluster list (CONFIRMED):
- The end of the CPrCluster init method 0x6F4924D0 loads ECX = [0x6FAB73D8] + 0x3B0, pushes the new cluster and
  calls 0x6F48E630 (insert after the head: node.prev = head, node.next = head.next, relink). So clusters are
  pushed at the head (LIFO: newest first, i.e. the walk runs newest to oldest).
- 0x6F48E630 and 0x6F48E610 are the same routine (also used at +0x638.., for other lists).
- The same init method is the vtable slot 3 of CPrCluster (0x6F95230C slot 3 = 0x6F4924D0); the allocator
  0x6F48DF00 calls it through slot 0xC (CONFIRMED), after taking a pooled record from 0x6F48D680 (free list at
  pool+0x14).
- Callers of the allocator: 0x6F4934E0 (per slot) and 0x6F48E140 (the save-file loader, tags 0x5E636C75 ...).
- 0x6F4934E0 is called by 0x6F493590, which loops over 12 slots (index 0..11) and calls 0x6F4934E0 for each; a
  slot is used when object+0xAC+4*slot != -1 (CONFIRMED loop). 0x6F493590 returns the number of clusters made.
- 0x6F493590 is called from the movement setup/order code: 0x6F474D40 (the function that writes the unit's
  movement parameters, called from 6 unit-initialisation sites) and from 0x6F474FAA, 0x6F4751F9, 0x6F4752DD,
  0x6F47B67D (jmp), 0x6F4936D6 (CONFIRMED call edges). LIKELY meaning: every new move order (or
  re-configuration of movement) rebuilds the cluster set for the 12 slots.
- The world constructor also makes one cluster directly (0x6F489750 calls 0x6F486A90 at +0x1BE, UNSURE what for).
- Removal: 0x6F493130 unlinks a cluster and frees its sub-objects. 0x6F4833E0 (called from the world reset
  0x6F489E70) empties the cluster list and the +0x38C list by calling each node's slot 0x10 with 0 (CONFIRMED).

Consequence: the per-tick mover loop is over clusters, and each cluster holds its unit records in an array at
+0x28 (count +0x38), which 0x6F490FC0 walks (pass 1). Units therefore move in cluster order (newest cluster
first) and, within a cluster, in record order, with the priority/skip logic of 0x6F490C80 (pass 1 section 5).

## 2. Writer of object+0x88 and the per-second -> per-tick conversion  (PARTIAL)

2.1 Static writers of disp +0x88 (capstone scan of all code, base register any): 27 functions
(scratchpad/specs/w88.txt). None is in the step code (0x6F48xxxx-0x6F4Fxxxx) and none is a speed setter. The
list is almost all constructors. The two that set 0.0 at +0x84 and +0x88 with +0x7C = a handle, +0x80 = 1 are
0x6F040900 and 0x6F040950 (LIKELY initialisers of the step state: they write the triple +0x80/+0x84/+0x88 used by
0x6F4907F0, which reads U+0x80, U+0x84, U+0x88). So the step cap at +0x88 is set at object creation, and a later
update must go through a copy or through a path the scan does not see (UNSURE).

2.2 Speed set chain (CONFIRMED edges):
- SetUnitMoveSpeed 0x6F3C5C40 -> 0x6F26DE00 (speed on the stack). 0x6F26DE00 calls 0x6F202F30 when unit+0x1EC is set.
- 0x6F202F30: calls the vtable slot 0 of the object at unit+0x6C with (speed, 1), then 0x6F201D50.
- 0x6F201D50: calls 0x6F201C60, then takes X = unit+0x30 (or 0x6F472890 if null), calls X's vtable slot 0xB8
  to get Z, then Z's vtable slot 0x18 with the speed (CONFIRMED edges). The class of Z is not identified.
- 0x6F472890: C = handle-resolved current unit (0x6F03FA30); X = C+0x30; if X+0x20 == 0 returns X+0x54.
- GetUnitMoveSpeed 0x6F3C5810 -> 0x6F26DE20 -> 0x6F201C60 -> 0x6F2019F0: the effective speed (per second, LIKELY)
  clamped to per-class ranges read from data 0x6FAB4440/0x6FAB4448 and 0x6FAB443C/0x6FAB4444, defaults
  0x6FAB4450/0x6FAB4458, caps 0x6FAB3AD0/0x6FAB3AD4 (CONFIRMED the reads; values not read here).

2.3 Per-tick conversion: NOT FOUND. No division by 10 or multiplication by 0.1 is in the chain above. The only
0.1 reader in the cluster area is a timer period (movement-2). UNSURE. Next: identify Z's class by the vtable
at Z (read the vtable pointer written by its constructor) and read its slot 0x18.

## 3. Grid object and the value of s = object+0x48  (structure CONFIRMED, value UNSURE)

- The position helper 0x6F497460 (ECX = unit U, out) calls 0x6F4885A0 with ECX = [0x6FAB73D8] = the world W.
  So W is the grid owner, not the unit (CONFIRMED from the push sequence).
- 0x6F4885A0 calls 0x6F483270(W, U): returns G = W+0x14 when (U+0x14) bit 31 is clear, else G = W+0x68
  (CONFIRMED). G is the layer (two layers, presumably the normal and the 'high' one).
- 0x6F4885A0 then calls 0x6F483E70 with ECX = G, a = G+0x40 (float), i = G+0x44 (int), b = U+0x70 (float), j =
  U+0x74 (int). So the scale is s = G+0x48 = W+0x5C (layer 0) or W+0x7C (layer 1); origin G+0x40 = W+0x54 / W+0x74;
  cell G+0x44 = W+0x58 / W+0x78 (CONFIRMED).
- Initialiser of the layer objects: not in 0x6F489750 (its writes at +0x14 region are for other objects). Not found
  in pass 3. Static search: no store of 32.0 (0x42000000) into a +0x48 field; only two unrelated occurrences
  (0x6F2D50FF, 0x6F64CCC1). No I2R(32) call with a store to +0x48 found.
- Value: UNSURE. LIKELY 32.0 from pathing.md (cell = 32 world units, the x32 factor), but this needs the
  layer initialiser (search for writes to W+0x5C/W+0x7C or to the layer base at W+0x14/W+0x68 via the map loader).

## 4. Next-waypoint rule in 0x6F49C990 / 0x6F49C2E0  (PARTIAL)

Confirmed from movement-2 and rechecked: 0x6F49C930 returns the waypoint at P+0x40 + index*8 while index < count
(P+0x50), and uses the end test with 0x6F49BB90 when index >= count. 0x6F49C990 sets P+0x74 = -1 after its
segment step (CONFIRMED store). 0x6F49C2E0 (LIKELY): pops a segment pair from P+0x78 (subtract 2) and
recomputes the next segment with 0x6F49C100; it writes *out = 1 when a segment was loaded.

Arrival / path validity:
- 0x6F493EA0 returns "arrived" when the flag holds and |residual| <= 0.2 (CONFIRMED constant, CONFIRMED test).
- 0x6F49C3B0 compares its distance with MUL(0.49, 0.49) = 0.24009999 (softfloat; checked in Unicorn in pass 2).
  Returns 0 when the distance is larger than 0.2401 (path invalid, search again), 4 when within (LIKELY), 2 when
  blocked (sets P+0x94 >= 20).
- So 0.49 is a squared radius used for path validity; 0.2 is the arrival radius (LIKELY, in cell units).

Not resolved: the code that increments P+0x74 by one after arrival. The next-waypoint rule is therefore
"when 0x6F493EA0 reports arrived, the caller advances to index+1 and loads the next segment" (LIKELY, not seen).

## 5. Per-tick turn formula in 0x6F494200 (PARTIAL)

Structure (CONFIRMED from the decompiled body and the table addresses):
- The node is a CPoPosCl. Row = (node+0xD8 >> 8) & 0xF, row base = 0x6FAB7440 + 16*row: T0 = +0, T1 = +4, T2 = +8,
  T3 = +0xC. Values for row 0 (from the file): T0 = 1.9577e-7, T1 = 3.0958e-7, T2 = 3.9154e-7, T3 = 6.1917e-7.
  Row 1..: 7.83e-7, 1.24e-6, 1.56e-6, 2.18e-6, 2.89e-6 (values from the file; the row stride is 16 bytes).
  These are small; the meaning (rate per tick in radians? per unit?) is UNSURE.
- Heading difference: 0x6F496630 is called with the node's +0x8C angle. It returns the NEGATED value for |x| <= pi
  (pi = 0x6FAAE600, CONFIRMED value 3.1415927). Its |x| > pi branch uses the softfloat SUB with an operand not
  resolved (the prologue leaves EDX unset in the call; UNSURE). So the difference d = -wrap(x) (LIKELY).
- Magnitude a = |d| is compared with 0x6FAB7540 = 0.001 (**correction 2026-10-11, runtime-constants audit**: the earlier 3.4272e-8 was wrong; the start-up float is 0x3A83126F). Below it: the node's virtual slot
  0x58 is called with 0 (no turn, LIKELY "arrived"). Above it: the turn branch runs.
- Turn branch: the sign of d picks T0 or -T0; the result is written to node+0xCC (through ADD with an operand not
  resolved), then clamped to [T1, T2] by compares with T1 (0x6FAB7444) and T2 (0x6FAB7448), with DIV 0x6F6EFE10 when
  a is at least T2 (CONFIRMED calls), and a pow step 0x6F6F0810 (softfloat pow) with exponent T3 (0x6FAB744C) when
  a differs from 1.0 (0x6FAAE4F4 = 1.0, CONFIRMED value). The final value goes to node+0xC8 via 0x6F493910 (not read).
- So the per-tick turn is a clamped, power-law-shaped step, with per-class constants. The exact formula is UNSURE:
  the operand of two softfloat calls and the target of 0x6F493910 were not resolved in this pass.

## 6. Open items (pass 4, in order)

1. Layer initialiser for W+0x14 / W+0x68 (value of s; section 3). Search for writes of the layer fields by the map
   loader (the pathing level constructor 0x6F4A03F0 is a candidate; check 0x6F481340).
2. Class of Z (vtable slot 0xB8 result) and its slot 0x18 (section 2.2): read the vtable pointer written by the
   constructor reached from unit+0x30.
3. Operands of the softfloat calls in 0x6F496630 and 0x6F494200 (EDX/stack operands; section 5), via a full
   disassembly of 0x6F494200 with the register state at each call.
4. The increment of the waypoint index (section 4): look for stores to P+0x74 with an add.
