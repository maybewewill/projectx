# JASS bytecode as emitted by the original compiler (Game.dll 1.26a)

Scope: the original JASS compiler of the user's own Game.dll (1.26a), run inside Unicorn on small JASS texts. The emitted instruction stream was read from emulated memory and decoded. Everything below was observed in that output unless marked otherwise. Nothing in /home/user/projectx was modified.

Confidence key: **CONFIRMED** (seen in emulated output for the stated cases), **LIKELY** (strong inference from the output shape or from the decompile, not directly tested), **UNSURE** (not determined).

Raw dumps and the harness are in `scratchpad/jasscomp/` (see section 12). The interpreter itself was NOT re-verified here; semantics of runtime opcodes are compile-side evidence plus the existing `docs/spec/jass-vm.md` where it agrees.

---

## 0. Summary of what was achieved

- The whole front end runs in Unicorn: scanner, LALR parser, node construction, code generation, and the emit-buffer copy. Compiled 51 JASS texts (47 succeed, 4 are intentional errors listed in section 9). Each text uses a fresh emulator state.
- Output: 8-byte instructions, little-endian dwords, buffer read from emulated memory. Verified by reading back the exact byte count returned by the copy routine.
- Not run: the post-compile script installer (`0x6F454390` onward, which builds the script object). Hooking stopped right before it, so the constant/string tables are not dumped. Their layout is UNSURE (section 7).
- Not reached: native declarations and native calls (opcode 0x15). The `native` keyword is rejected by this compile entry (section 9). Native-call semantics are therefore not verified.

---

## 1. Entry point and what the compiler needs (CONFIRMED unless noted)

| Item | Value / behaviour | Confidence |
|---|---|---|
| Top-level compile function | `0x6F458900` (Compile.cpp). `__fastcall`: ECX = pointer to a segment list, EDX = a reporter callback pointer, `[esp+4]` = p4, `[esp+8]` = p5. The four real callers pass EDX = `0x6F2CFCA0` and p5 = 1. | CONFIRMED (callers, arguments) |
| Segment list | Array of `{ int length; char* text }` pairs, terminated by a pair with `text == 0`. Each text is NUL-terminated. Read by the scanner's character source. | CONFIRMED |
| Scanner object | Lives in the caller's stack frame of `0x6F458900`. This object is the compile context `ctx`; for the top-level call `ctx = entry_esp - 0xE8`. | CONFIRMED |
| Driver | `0x6F458510`: runs the parser `0x6F464460`, then the root node's virtual `Generate` (vtable +8), appends two words `0x01000000, 0`, copies the words with `0x6F4560D0`, stores the buffer at `ctx+0xD0`. | CONFIRMED |
| Output size | Written through the out-pointer passed to `0x6F4560D0` (count of bytes). Read by hooking the copy routine's entry. | CONFIRMED |
| Script installer (not run) | `0x6F454390` (Jass.cpp): constructs the script object from (buffer, size). The harness stops at its entry. | CONFIRMED (location) |
| CRT start-up is required | Without running the C++ static constructors (`_initterm` ranges `0x6F86DAB4..0x6F874E48` and `_initterm_e` range `0x6F874E4C..0x6F874E58`), the CDataAllocator pools have count 0 and the first pool allocation spins in a fill loop until the instruction budget runs out. | CONFIRMED |
| Error reporter | `0x6F2CFCA0`. It is invoked on every compile error (syntax errors, undeclared identifiers, the `native` keyword in this entry). Its arguments were `(kind, line, text_ptr)`; observed `kind=0` for syntax errors and `line` = the source line. The text is looked up in the game's WorldEditStrings table, which is not in the upload, so the message text is not available. | CONFIRMED (arguments), UNSURE (meaning of kind) |

Note: the decompile's `this` pointers in `0x6F458900`/`0x6F458510` are misleading (stack locals shown as arguments). The disassembly was used instead.

---

## 2. Output format (CONFIRMED)

- The emitted code is a flat array of 8-byte instructions. Each instruction is two 32-bit little-endian words: word 0 packs the bytes `b0 b1 b2 op` (b0 = byte 0, op = byte 3), word 1 is the immediate.
- Field layout: `b0` = right operand slot (binary) or unused, `b1` = left operand slot or type or source, `b2` = destination slot (or the operand slot for in-place unary ops), `op` = opcode, `imm` = 32-bit immediate.
- The script ends with one `END` instruction (op 0x01, imm 0) appended by the driver after code generation. Every function ends with `HDR ... RET END_F` as shown in section 5.
- Output sizes: the empty function is 5 instructions (40 bytes). Byte counts are always a multiple of 8.

---

## 3. Opcode table (observed)

Columns: opcode, observed name used in this document, operand usage as observed, confidence, and what the existing `docs/spec/jass-vm.md` says (where it differs).

| Op | Name here | Operand usage (observed) | Confidence | Differs from jass-vm.md? |
|---|---|---|---|---|
| 0x01 | END | imm 0; appended once by the driver | CONFIRMED | no |
| 0x03 | FUNC_HDR | `imm` = function symbol id; `b2` = return type (0 = nothing, 4 = integer) | CONFIRMED (ids, return type), LIKELY (executor no-op) | spec says no-op. Agrees on no-op status, adds the id meaning |
| 0x04 | FUNC_END | no operands; follows the fall-through return | CONFIRMED (position), LIKELY (no-op) | no |
| 0x05 | DECL_LOCAL | `b2` = declared type; `imm` = local symbol id | CONFIRMED | spec says "load literal or global" (LIKELY); observed meaning differs |
| 0x06 | DECL_GLOBAL | `b2` = declared type; `imm` = global symbol id; emitted in the global-init function | CONFIRMED | spec says "create or fetch" |
| 0x07 | DECL_CONST_GLOBAL | as 0x06, for `constant` globals (initialiser follows) | CONFIRMED (one example) | spec does not describe this |
| 0x08 | PARAM_BIND | callee prologue, one per parameter, last parameter first. `b1` = depth from the top of the value stack (1 = top), `b2` = declared type, `imm` = parameter symbol | CONFIRMED (shape), LIKELY (peek, not pop) | spec says call-frame setup. Differs |
| 0x0b | POP_ARGS | after a call with >0 arguments; `b2` = argument count (1, 2, ...) | CONFIRMED | spec says "pop locals". Differs in meaning |
| 0x0c | LOAD_CONST | `b1` = type (0 null, 4 int, 5 real, 6 string, 8 bool); `b2` = dest slot; `imm` = value (int, float32 bits for real, 0/1 for bool, symbol id for string) | CONFIRMED | spec says `b1` = source. Differs |
| 0x0d | MOVE | `b1` = source slot, `b2` = dest slot. Used to put a return value into slot 0 | CONFIRMED | no |
| 0x0e | LOAD_VAR | `b1` = type, `b2` = dest slot, `imm` = variable symbol (local or global, same opcode) | CONFIRMED | spec says "global lookup". Broader |
| 0x0f | LOAD_FUNCREF | `b1` = 3 (code type), `b2` = dest slot, `imm` = function symbol id | CONFIRMED | agrees |
| 0x10 | ARRAY_LOAD | `b0` = element type (4 seen), `b1` = index slot, `b2` = dest slot, `imm` = array symbol | CONFIRMED (shape), LIKELY (b0) | not described |
| 0x11 | STORE_VAR | `b2` = source slot, `imm` = variable symbol (local or global) | CONFIRMED | spec's source slot is not given. Here the source is `b2` |
| 0x12 | ARRAY_STORE | `b1` = value slot, `b2` = index slot, `imm` = array symbol; `b0` = 0 in all cases | CONFIRMED (shape) | not described |
| 0x13 | PUSH | `b2` = slot pushed onto the value stack | CONFIRMED (emitted and balanced by POP), LIKELY (stack) | spec calls it "ready check". Differs |
| 0x14 | POP | `b2` = dest slot; pops the value stack top into that slot | CONFIRMED (balanced with PUSH) | agrees with "restores from the stack" |
| 0x16 | CALL_SCRIPT | `imm` = callee function id. The result is left in slot 0 | CONFIRMED (result in slot 0) | agrees |
| 0x1a | EQ | `b0` = right, `b1` = left, `b2` = dest | CONFIRMED (emitted), semantics per spec | agrees |
| 0x1b | NE | same | CONFIRMED (emitted) | agrees |
| 0x1c | LE | same | CONFIRMED (emitted) | agrees |
| 0x1d | GE | same | CONFIRMED (emitted) | agrees |
| 0x1e | LT | same | CONFIRMED (emitted) | agrees |
| 0x1f | GT | same | CONFIRMED (emitted) | agrees |
| 0x20 | ADD | `b0` right, `b1` left, `b2` dest. Also used for string concatenation | CONFIRMED (emitted for int, real, string) | agrees |
| 0x21 | SUB | same | CONFIRMED (emitted) | agrees |
| 0x22 | MUL | same | CONFIRMED (emitted) | agrees |
| 0x23 | DIV | same | CONFIRMED (emitted) | agrees |
| 0x25 | NEG | unary, operand and destination are `b2` (in place). Note `b1` is 0 | CONFIRMED | spec says operand is `b1`. Differs |
| 0x26 | NOT | unary, in place on `b2` | CONFIRMED | spec says operand is `b1`. Differs |
| 0x27 | RET | no operands. Return value is taken from slot 0 | CONFIRMED | agrees |
| 0x28 | LABEL | `imm` = label id. Labels are numbered once per script (section 7) | CONFIRMED (position), executor semantics UNSURE | spec does not list 0x28 |
| 0x29 | JNZ | `b2` = condition slot, `imm` = label | CONFIRMED | agrees |
| 0x2a | JZ | `b2` = condition slot, `imm` = label | CONFIRMED | agrees |
| 0x2b | JMP | `imm` = label | CONFIRMED | agrees |

Not observed in any stream here: 0x02, 0x09, 0x0a, 0x15 (native call), 0x17, 0x18, 0x19, and anything at 0x2c or above. The compiler does not emit 0x18/0x19 for `and`/`or`; it uses jumps (section 5.4). The native-call opcode 0x15 could not be produced (section 9).

---

## 4. Value types (CONFIRMED from `b1` and `b2` of declarations and constants)

| Code | Type | Evidence |
|---|---|---|
| 0 | null | LOAD_CONST with `b1` = 0 (used for the default return) |
| 3 | code (function reference) | LOAD_FUNCREF `b1` = 3 |
| 4 | integer | integer literals, integer declarations |
| 5 | real | real literals (`imm` = float32 bits: 1.5 -> 0x3FC00000, 2.25 -> 0x40100000) |
| 6 | string | string literals (`imm` = symbol id, see 7) |
| 8 | boolean | `true` = LOAD_CONST type 8 imm 1, `false` = imm 0 |
| 9 | integer array | `local integer array` / `integer array` global declarations (`b2` = 9) |

Codes 3, 4, 5, 6, 8 agree with `docs/spec/jass-vm.md` section 2.1. Code 9 is observed for integer arrays only. Real arrays, boolean arrays and the handle code 7 were not tested (UNSURE).

---

## 5. How each construct compiles

Notation: `HDR` = FUNC_HDR, `END_F` = FUNC_END, `K` = LOAD_CONST, `LD` = LOAD_VAR, `ST` = STORE_VAR, `DL` = DECL_LOCAL, `DG` = DECL_GLOBAL, `LBL` = LABEL, `PUSH`/`POP` = value stack, `RET` = RET. Each function is `HDR ... K RET END_F`, where `K RET` is the default return (slot 0 = null) that always follows the body. Counts are instructions emitted for the construct itself. The full script adds one END at the end.

### 5.1 Function frame and default return (CONFIRMED)

- Empty function: `HDR K RET END_F` = 4 instructions, plus the script END = 5 (test `00_empty`).
- Return type from HDR `b2`. `returns integer` gives `b2` = 4 (test `18_return_literal`).

### 5.2 Declarations and assignments (CONFIRMED)

| Source (single function) | Sequence (excluding HDR, K RET END_F) | Count |
|---|---|---|
| `local integer a` | DL | 1 |
| `local integer a = 5` | DL K ST | 3 |
| `set a = 2` | K ST | 2 |
| `local integer a = -5` | DL K NEG ST | 4 (no constant folding) |
| `set a = -a` | LD NEG ST | 3 |
| `local string s = "hi"` | DL K ST | 3 |
| `local real a = 1.5` | DL K ST | 3 |
| `local integer b = a` | DL LD ST | 3 |
| `local integer x = g` (global) | DL LD ST | 3 |
| `local code c = function g` | DL FREF ST | 3 |

### 5.3 Expressions (CONFIRMED)

- Binary operator, `L op R` with both operands as leaves: `K PUSH K POP OP`, total 5 instructions for two leaves (1 + 1 + 3 overhead). Left is evaluated first, pushed, then the right is evaluated into its own slot, then the left value is popped into a slot. `OP` uses `b1` = left (popped slot), `b0` = right slot, `b2` = destination.
- Precedence is respected by the parser: `1 + 2 * 3` gives `K PUSH K PUSH K POP MUL POP ADD` (multiply is emitted before the add).
- Operators: `+` 0x20, `-` 0x21, `*` 0x22, `/` 0x23, `==` 0x1A, `!=` 0x1B, `<=` 0x1C, `>=` 0x1D, `<` 0x1E, `>` 0x1F, unary `-` 0x25, `not` 0x26. Real arithmetic and comparisons use the same opcodes (type decides).
- `%` (modulo) is a syntax error in JASS (no `%` operator). The reporter is called at the line (test `04_int_mod_ERROR`, line 2).
- Constant folding: none. `1 + 2` and `-5` are computed at run time.

### 5.4 `and` / `or` / `not` (CONFIRMED)

- `not x`: `LD/K NOT ST` (operand in place in `b2`).
- `a and b` (short-circuit, constants shown): `K JZ(L1) K JMP(L2) LBL(L1) K LBL(L2)`. The right operand is evaluated only on the non-short path. Total 7 for two constant leaves plus the store.
- `a or b`: `K JNZ(L3) K JMP(L4) LBL(L3) K LBL(L4)`. Same shape.

### 5.5 `if` / `elseif` / `else` (CONFIRMED)

- `if c then B endif`: `c-code JZ(L1) B JMP(L2) LBL(L1) LBL(L2)`. The JMP and the second label are emitted even without an else arm (test `12_if`: `LD PUSH K POP EQ JZ K ST JMP LBL LBL`).
- Each `elseif` adds: `c-code JZ(next) B JMP(end) LBL(next)`. The `else` arm is emitted without a condition. Test `13_if_elseif_else` has 32 instructions in total, with labels 1..4.
- Condition as a plain variable: `LD JZ` (no PUSH/POP) (test `40_if_call`).

### 5.6 `loop` / `exitwhen` (CONFIRMED)

- `loop ... endloop` produces `LBL(top) BODY JMP(top) LBL(exit)`.
- `exitwhen c` produces `c-code JNZ(exit)`.
- Test `14_loop_exitwhen` (`set i = i + 1`, `exitwhen i == 3`): `LBL LD PUSH K POP ADD ST LD PUSH K POP EQ JNZ JMP LBL`. Per iteration this executes 14 instructions including the top label (6 for the body statement, 6 for the exitwhen test, 1 back-jump, 1 label).

### 5.7 Arrays (CONFIRMED shape)

- `local integer array arr`: `DL` with `b2` = 9 (the array declaration consumes one symbol).
- `arr[i] read` into a variable: index into slot via `K` (or `LD`), then `ALD` with `b1` = index slot, `b2` = dest, `imm` = array symbol. Test `29_array_read`: `DL DL K ALD ST`.
- `set arr[i] = v`: index code, `PUSH`, value code, `POP` (index into a slot), `AST` with `b1` = value slot, `b2` = index slot. Test `15_array_store`: `DL K PUSH K POP AST` (the declaration counts as well). Index and value are evaluated in source order.

### 5.8 Globals (CONFIRMED)

- Globals are declared in a generated global-init function. That function is emitted FIRST in the stream, with its own HDR (its id is the last symbol id assigned, section 7). Each global is a DG (or DCG for `constant`), followed by `K ST` if initialised.
- A global read is LD (same opcode as local reads). A global write is ST (same opcode). Test `16_globals_store` (`set g = 4`): the init function is `HDR DG K ST RET END_F`, then `f` is `HDR K ST K RET END_F`.
- Global array declaration: DG with `b2` = 9 (test `49_global_array`).

### 5.9 Calls and returns (CONFIRMED)

- Callee prologue: `HDR`, then one `BIND` per parameter in reverse order. For `add(a, b)` the first BIND has `b1` = 2 (the `a` value, second from top) and the second has `b1` = 1 (top). `b2` = parameter type, `imm` = parameter symbol.
- `return e`: `e-code MOV(slot -> slot0) RET`. Test `35_params_return` (`return a + b`): `BIND BIND LD PUSH LD POP ADD MOV RET`.
- Fall-through default is `K RET` (null). `return` with no value is just `RET` (test `36_return_nothing`).
- Caller: each argument is evaluated and PUSHed in source order, then `CALL imm=id`, then `POP_ARGS b2=count` if count > 0. The result is in slot 0. To use it, the caller pushes or stores slot 0 (test `21_call_value`: `DL K PUSH CALL POPARG ST`; test `39_call_in_expr`: `DL CALL PUSH K POP ADD ST`).
- A call with no arguments: `CALL` only, with no POP_ARGS (test `38_call_noargs`, test `40_if_call`).
- `call g(1, 2)` with two parameters: `K PUSH K PUSH CALL POPARG`. The callee binds them as in the BIND pattern above.

### 5.10 Strings and function references (CONFIRMED)

- String literal: `K` with `b1` = 6, `imm` = symbol id (section 7). Literals dedupe by text: `"hi"` used twice gives the same id (test `41_string_dedup`).
- Concatenation: `ADD` on two string values (test `24_string_concat`).
- Function reference: `FREF` with `b1` = 3, `imm` = function id (test `20_funcref`: `local code c = function g`).

### 5.11 Instruction counts (CONFIRMED, from the emitted streams)

Totals include the script END. Body = total minus the 4-instruction function frame minus END (single-function scripts: body = total - 5).

| Test | Total | Body (statements only) |
|---|---|---|
| `00_empty` | 5 | 0 |
| `51_decl_only` (`local integer a`) | 6 | 1 |
| `19_string_lit` | 8 | 3 |
| `15_real_literal` | 8 | 3 |
| `05_neg_literal` | 9 | 4 |
| `11_not` | 9 | 4 |
| `53_set_literal` (`local a=1` + `set a=2`) | 10 | 5 |
| `01_int_add` (`local a = 1 + 2`) | 12 | 7 |
| `02_int_prec` (`local a = 1 + 2 * 3`) | 16 | 11 |
| `03_int_sub_div` (two locals) | 19 | 14 |
| `10_and_or` (two locals, `and`, `or`) | 23 | 18 |
| `12_if` (`local a = 1`, `if a == 1 then set a = 2 endif`) | 19 | 14 |
| `13_if_elseif_else` | 32 | 27 |
| `14_loop_exitwhen` | 23 | 18 |
| `46_two_ifs_one_fn` | 30 | 25 |
| `21_call_value` (function plus call) | 19 | see 5.9 |

Rules of thumb (CONFIRMED from the table): a binary operator with two leaf operands costs 5 instructions. A leaf (literal, variable, function reference) costs 1. An assignment or declaration with initialiser costs 1 store plus the initialiser (declaration itself is 1). An `if` costs its condition plus 4 instructions plus the body (JZ, JMP and two labels). A `loop` iteration costs the body plus the exitwhen test plus JMP plus the top label.

---

## 6. Function and call conventions (CONFIRMED unless marked)

- Function ids are symbol ids (section 7). The header `HDR` carries the id in `imm`.
- Parameters are not addressed by slot. They are bound from the value stack in the callee prologue (BIND), so the callee reads parameters through their symbols (LD).
- Return value: slot 0. `MOV` to slot 0 then `RET`. The default return is null in slot 0.
- Argument cleanup: the caller issues POP_ARGS with the argument count after the call. No cleanup is emitted when there are no arguments.
- Temporary slots: each expression result gets a fresh slot number, incremented monotonically within a function (not reused by statement). Slot numbers are 8-bit fields. A function with more than 255 temporaries wraps the slot numbers. Verified with 300 `set a = 1` statements: slot 255 is followed by slot 1, and slot 0 is written once (CONFIRMED). The slot aliasing is probably harmless because temporaries are short-lived (LIKELY), but a faithful reimplementation must reproduce the wrap or reject the function.
- Native calls (opcode 0x15): NOT observed. See section 9.

---

## 7. Symbols, constants and strings (CONFIRMED unless marked)

- One symbol table, keyed by name, shared by identifiers and string literal texts. Identifiers are **case-sensitive** (CORRECTED 2026-10-11: the earlier "case-insensitive" reading came from a harness stub for Storm ordinal 508, which is `SStrCmp`; `set A = 2` and `a` get different ids; see docs/spec/jass-frontend.md section 8.3; parity is unchanged, 150/151). A string literal `"a"` uses the same id as the identifier `a` (test `ids_d_string_vs_name`).
- Symbol ids are assigned in order of first appearance, starting at 2 (first user id observed = 2; ids 0 and 1 presumably reserved, LIKELY). Functions, globals, locals, parameters and string literals all draw from the same counter. A name seen again (even as a local in another function) reuses its id (test `47_labels_two_fns`: `a` gets id 3 in both functions).
- The global-init function takes the last id after all declarations (test `ids_c`: globals 2, 3; function 4; local 5; init function 6).
- Integer, real and boolean literals are immediates (`imm` holds the value or bit pattern). No constant table index is used for them.
- String literals: `imm` = symbol id. The text is passed to the global string intern routine `0x6F454930` (logged for a test with `local string s = "hi"` and `local string t = "yo"`: the calls were `main`, `f`, `s`, `hi`, `t`, `yo`). Whether the id maps to the text through the symbol record or through the intern table is UNSURE; the symbol node stores its name at offset +0x14 (LIKELY from `0x6F458900` region code).
- Constant table in the script object (VM instance +0x2870 per jass-vm.md): NOT dumped. The installer `0x6F454390` was not run. UNSURE whether immediates index it.

---

## 8. Labels

- Labels are numbered once per script (test `47_labels_two_fns`: first function's `if` uses labels 1 and 2, second function's `if` uses 3 and 4). CONFIRMED.
- Jumps reference labels by `imm`. `JZ`/`JNZ` take the condition slot in `b2`. `JMP` has no slot operand.

---

## 9. Errors, limits and blockers

| Input | Result | Confidence |
|---|---|---|
| `local integer a = 7 % 4` | reporter called with line 2 (no `%` operator in JASS) | CONFIRMED |
| `set = 1` | reporter called with line 2 | CONFIRMED |
| `call Undeclared()` | reporter called with line 2 | CONFIRMED |
| `native N takes nothing returns nothing` | reporter called with line 1 (kind 0). The `native` keyword is rejected with the entry flags the callers use. The flags p4 and p5 were varied: with p4 = 1 the kind changes to 1, and the line is still 1 | CONFIRMED |
| `constant function N takes nothing returns nothing` | accepted (compiles to a function, not a native) | CONFIRMED |
| Message text | not available. The reporter formats its message through the WorldEditStrings table, which is not in the upload. The harness stops at the reporter's entry, so the text is never formatted | CONFIRMED |

Limits:

- Slot numbers are 8-bit (section 6).
- Literal integer values are stored as 32-bit immediates without range checks in the observed paths (LIKELY; literals beyond 32 bits were not tested).

Blockers and what was not done:

1. Natives: the `native` keyword is rejected here. Native declarations are presumably registered from the engine's native table (names and signatures at `0x6F3D4020` / `0x6F71D9E0` per the existing docs). Making natives compile requires that registration, which was not reproduced. Opcode 0x15 is therefore UNSURE.
2. Script installer (`0x6F454390`): not run, so the final script object, constant and string tables, and function table were not dumped.
3. Executor: the opcode semantics above were read from compile-side output. The executor (`0x6F45E9D0`) was not re-run.
4. Opcodes not observed: 0x02, 0x09, 0x0a, 0x15, 0x17, 0x18, 0x19, 0x2c and above.

---

## 10. Budget implications (the 300000 op limit)

- Instruction budget per run: 300000 (from `docs/spec/jass-vm.md`, FUN_6f45fd00). The executor's per-fetch counting was not re-verified here (LIKELY).
- Cost per loop iteration in the test `14_loop_exitwhen` (`set i = i + 1` plus `exitwhen i == 3`): 14 instructions including the top label. At 300000 instructions that is about 21,400 iterations per run for that loop shape (arithmetic from CONFIRMED counts; the budget itself LIKELY).
- Cost of a call: argument evaluation (1 + PUSH per argument) + CALL + POP_ARGS + per-parameter BIND + callee body + default return. A function with two parameters and a body `return a + b` costs 1 (HDR) + 2 (BIND) + 6 (LD PUSH LD POP ADD MOV) + 1 (RET) in the callee, plus the caller's argument pushes, CALL, POP_ARGS and the store.
- Long straight-line code costs one instruction per statement for simple assignments (`K ST` = 2 instructions).

---

## 11. Differences from `docs/spec/jass-vm.md` (for whoever updates that file)

- 0x0c: `b1` is the constant type and `imm` is the value. Not a source slot.
- 0x08: a callee-prologue parameter bind, not a call-frame setup.
- 0x0b: pop argument count after a call (`b2`), not "pop locals".
- 0x13: a value-stack push (`b2` slot), not a ready check (LIKELY).
- 0x25 / 0x26: unary operand is `b2`, in place.
- 0x05: local declaration; 0x06 global declaration; 0x07 constant global declaration.
- 0x11: store source slot is `b2`.
- 0x03 is a header with id and return type; 0x04 is a function end marker; 0x28 is a label.
- 0x18 / 0x19 (logical AND/OR opcodes) are not emitted by the compiler; `and`/`or` use jumps.

---

## 12. Harness and raw dumps (what to run)

Files in `scratchpad/jasscomp/`:

- `jcomp.py`: the harness. Maps the user's Game.dll (from `GAME_DLL` or the upload path), stubs the Storm ordinals and MSVCR80/KERNEL32 imports (per-name Python models; an unknown import raises with a backtrace), runs the CRT initialisers, and calls the compile entry with the callers' convention. `compile_text()` returns the buffer and info (`size`, `ctx`, `error`).
- `tests.py`: all JASS texts used (51 entries).
- `run_all.py`: compiles every text with a fresh harness and writes `out/RESULTS.txt` (decoded streams) and `out/results.json` (raw words).
- `experiments.py`, `exp_*.py`: earlier batches (kept for reproducibility; `tests.py` supersedes their text lists).
- `disas.py`, `callers.py`, `importcalls.py`, `imports_all.tsv`: static helpers (disassembly through capstone, caller search, import-call survey).
- `trace_run*.py`, `run1.py`-`run3.py`: debugging scripts used while bringing the harness up.

Run: `cd scratchpad/jasscomp && python3 run_all.py` (about 50 seconds for 51 texts; requires unicorn and pefile; the Unicorn library is taken from `scratchpad/rngtools/pylib`).

Storm and CRT models used (CONFIRMED that they produce the observed outputs; arg counts from call sites; the stdcall convention LIKELY from consistent stack use):

| Import | Model |
|---|---|
| Storm ordinal 401 | alloc(size, file, line, flags), zeroed bump allocator |
| Storm ordinal 403 | free (no-op) |
| Storm ordinal 405 | realloc(ptr, size, file, line, flags) with size tracking |
| Storm ordinal 507 | strdup(str, file, line) |
| Storm ordinal 508 | `SStrCmp`: **case-sensitive** bounded compare, returns 0 when equal (was modelled case-insensitive until 2026-10-11) |
| Storm ordinal 501 | bounded copy(dst, src, size) |
| Storm ordinal 590 | MPQ-style string hash, type 0 (LIKELY SStrHash, as the project's `StringHash` note says) |
| Storm ordinal 578 | no-op (WorldEditStrings warning path; not needed for the observed streams) |
| MSVCR80 `_vsnprintf` | minimal printf (%s %d %u %x %c %%) |
| MSVCR80 `_decode_pointer`, `_encode_pointer` | identity (self-consistent) |
| TLS / last-error / critical section / interlocked | no-op or simple cell |

No decompiled code is reproduced in this document.
