# JASS virtual machine (Warcraft III 1.26a Game.dll): behaviour spec

Scope: behaviour of the JASS bytecode interpreter as recovered from the decompiled pseudo-C chunks. Describes behaviour in words and tables only. No code is reproduced.

Confidence labels: CONFIRMED (read directly in the decompile, unambiguous), LIKELY (strong inference from several consistent sites), UNSURE (guess or conflicting evidence).

Key caveat: the decompile recovers thiscall/fastcall argument order poorly. Argument-to-meaning mappings for the executor's call sites are marked UNSURE where it matters.

---

## 0. Map of the relevant code

| Role | Address(es) | Evidence |
|---|---|---|
| Bytecode executor (the interpreter loop) | FUN_6f45e9d0 (3640 bytes) | CONFIRMED: 8-byte fetch, switch on opcode byte, returns status codes |
| Thread start / resume with cleanup | FUN_6f45fa30 | CONFIRMED: creates thread node if none, calls executor, removes node unless status 2/3/4 |
| Thread start from a trigger or function record | FUN_6f45f8b0 | CONFIRMED: resolves function, creates thread node, calls executor |
| Run current thread (budget 300000) | FUN_6f45fd00 | CONFIRMED call shape |
| Call-site wrapper, run with timeout/budget | FUN_6f44b6a0, FUN_6f44b640 | CONFIRMED call shape; argument mapping UNSURE |
| Synchronous script call with 300000 limit | FUN_6f3b5470, FUN_6f3b54b0, FUN_6f3a8340 | CONFIRMED call shape |
| Native-call marshalling (signature-driven) | FUN_6f45d070 | CONFIRMED: switch over signature letters |
| Native function table lookup | FUN_6f44b2c0, FUN_6f45ae80, FUN_6f44cbc0 | CONFIRMED: hash-keyed lookups |
| Thread node allocator (pooled "Frame" objects) | FUN_6f45c0d0 | CONFIRMED: pool free-list, object named AUFrame |
| Value-stack push / pop | FUN_6f45e6d0 (push), FUN_6f45e650 and FUN_6f45e600 (pop) | CONFIRMED |
| Store into slot with type check | FUN_6f45b780 | CONFIRMED: writes type and value |
| Real arithmetic (software float) | FUN_6f6eee20 (multiply), FUN_6f6eeef0 and FUN_6f6ef000 (subtract, add, LIKELY), FUN_6f6ef100 (reciprocal, LIKELY), FUN_6f6efe10 (divide wrapper) | CONFIRMED for multiply; others LIKELY |
| String table access | FUN_6f459640 (index to entry), FUN_6f459660 (addref), FUN_6f45d210 (intern/create) | CONFIRMED |
| Handle table access | FUN_6f459690 | CONFIRMED |
| Thread-suspension helpers | FUN_6f44b2f0, FUN_6f44b340, FUN_6f44b320, FUN_6f44b380, FUN_6f44b3a0 | CONFIRMED accessors; meaning LIKELY |
| Trigger execution state machine | FUN_6f447340, FUN_6f446bb0 | CONFIRMED that they use the helpers; meaning LIKELY |
| Per-tick thread iteration | FUN_6f44b400 (called from FUN_6f3af8d0) | CONFIRMED call; purpose LIKELY |
| Compiler / parser (LR, table-driven) | FUN_6f464460 (6769 bytes), lexer FUN_6f5e7100 and FUN_6f78d7b0 ("Token buffer overflow"), node-building FUN_6f4617c0 | CONFIRMED that these are the front end, not the VM |
| Script loader / function table builder | FUN_6f454390 region, FUN_6f455250 | LIKELY |
| Native registration tables | FUN_6f3d4020 (about 2150 entries) and FUN_6f71d9e0 (about 2320 entries) | CONFIRMED |

The compiler is not the VM. The large switch-heavy function near 6F4572D0 to 6F4607F0 is parser and code-generation machinery (Nodes.h, Compile.h, scanner.cpp, parser.cpp). The interpreter is FUN_6f45e9d0.

Instance state layout (offsets are from the VM instance pointer; names are working labels):

| Offset | Meaning | Confidence |
|---|---|---|
| +0x20 | Program counter (address of next 8-byte instruction) | CONFIRMED |
| +0x28 .. +0x2c | Pointer and count of a per-thread list used by executor helpers | LIKELY |
| +0x44 | Instruction count for this executor invocation (loop bound) | CONFIRMED |
| +0x34, +0x38, +0x3c, +0x40 | Suspension request flags and payload (see section 5) | LIKELY |
| +0x48 | Pointer to per-function table of thread records | LIKELY |
| +0x6c | Last-called function or current frame id | UNSURE |
| +0x70 + k*0x28 region | Register file (slots), see section 2 | CONFIRMED |
| +0x2850 | Owner or trigger id used for thread filtering | LIKELY |
| +0x2858 | Code base table (return addresses are scaled indices into it) | LIKELY |
| +0x2860, +0x2864, +0x2868 | Sentinel and tail pointers of the thread list (insert at tail) | CONFIRMED |
| +0x2870 | Label / constant table (immediates index it) | CONFIRMED |
| +0x2874 | String table (16-byte entries) | CONFIRMED |
| +0x2878 | String-table free list or capacity record | LIKELY |
| +0x2880 | Thread/function record pool | LIKELY |
| +0x2888 | Handle table (4-byte entries) | CONFIRMED |
| +0x2898, +0x289c | Callback pointer and context, invoked after a store when the executor is not in a nested call | LIKELY |
| +0x28a0, +0x28a4 | Second callback pointer and context (invoked when a sync/notify flag is 7 or 6) | UNSURE |

---

## 1. Bytecode format

### 1.1 Instruction size and fields (CONFIRMED)

- Every instruction is exactly 8 bytes.
- Byte 0: first operand slot index (the right-hand operand for binary ops).
- Byte 1: second operand slot index (the left-hand operand for binary ops).
- Byte 2: destination slot index (for jumps: the condition slot).
- Byte 3: opcode.
- Bytes 4 to 7: 32-bit immediate, read as a little-endian word. Meaning depends on opcode (symbol or function index, label index, constant index, or literal).
- The program counter advances by 8 for every instruction. Jumps and returns overwrite it.
- Slot index k addresses a 40-byte record (0x28) at a fixed offset inside the VM instance (base offset for slot k is 0x50 + k*0x28). Slots 0 and 1 are at the start of that region; slot 0 holds the return value at the end of a call (CONFIRMED from the return path).

### 1.2 Slot record layout (CONFIRMED)

| Field | Offset within slot | Meaning |
|---|---|---|
| Runtime type | +0x18 | Type code of the value currently stored |
| Declared type | +0x1c | Type the destination expects (set by the instruction before a store) |
| Value | +0x20 | 32-bit payload (integer, real bits, string index, handle index, boolean 0/1, code pointer) |

### 1.3 Opcode table

Format: opcode (hex), meaning, operands, status. Only opcodes present in the executor switch are listed. Opcodes 0x00, 0x01, 0x03, 0x04, 0x09 and 0x0a have no case in the executor and behave as no-ops here (LIKELY; they may be handled elsewhere).

| Op | Meaning | Operands | Confidence |
|---|---|---|---|
| 0x02, 0x2b | Unconditional jump. Target = label table entry selected by the immediate. | imm = label index | CONFIRMED (target), LIKELY (label table semantics) |
| 0x05 | Load literal or global of a symbol id (creates string entry if needed, type 6) | imm = symbol | LIKELY |
| 0x06, 0x07 | Create or fetch a local/global object by symbol and set its declared type from the instruction | imm = symbol | LIKELY |
| 0x08 | Call-with-frame setup: allocates a new return frame and marks it (call setup for a user function) | imm = symbol, byte 1 = arg count | LIKELY |
| 0x0b | Pop locals: decrements the frame's local counter by byte 2 | byte 2 = count | CONFIRMED (counter), LIKELY (meaning) |
| 0x0c | Copy-and-store: moves a value to dest slot (for type 6, a string is passed through a helper that does a reference-count step) | byte 1 = source, byte 2 = dest, imm | LIKELY |
| 0x0d | Slot-to-slot move (copies type, declared type and value) | byte 1 = source, byte 2 = dest | CONFIRMED |
| 0x0e | Global lookup by symbol into dest slot; result 0 (null) returns status 6 | imm = symbol, byte 2 = dest | LIKELY |
| 0x0f | Load a code (function) reference (type 3, value = function record) | imm = function id, byte 2 = dest | CONFIRMED (type 3 and lookup), LIKELY (meaning) |
| 0x10 | Load a value from a symbol table entry into dest, with default 0 if absent | imm = symbol, byte 2 = dest | LIKELY |
| 0x11 | Store a slot into a global; if the store changed the value and the callback is set (and no nested call), invokes the change callback | imm = symbol, byte 2 = dest | LIKELY |
| 0x12 | Array-element or variable assignment with change notification (compares old vs new handle/value, then invokes callbacks on either side) | imm, byte 0/1/2 | LIKELY |
| 0x13 | Ready check: if the thread is inside a call and the slot's runtime type is 0 (null/unset), abort with status 6; otherwise continue | byte 2 = slot | CONFIRMED (abort path), LIKELY (meaning) |
| 0x14 | Pop: restores a slot from the top of the value stack | byte 2 = dest slot | CONFIRMED |
| 0x15 | Native call: resolve function record from imm, marshal arguments by signature, call native, store result in dest slot; may suspend (status 3 or 4) | imm = function id, byte 2 = result slot | CONFIRMED |
| 0x16 | Script-function call: pushes the current pc and slot state, then transfers to the callee | imm = function id | CONFIRMED (push), LIKELY (transfer) |
| 0x17 | Push or initialise a real/temporary constant into the dest slot as type 5 | byte 2 = dest | LIKELY |
| 0x18 | Logical AND of two slots (booleans): result 1 if both are nonzero, else 0 | byte 0, byte 1 | CONFIRMED |
| 0x19 | Logical OR of two slots: result 1 if either is nonzero, else 0; dest type 8 | byte 0, byte 1, byte 2 | CONFIRMED |
| 0x1a | Equality test. Integer (type 4): value compare. Real (type 5): float compare. String (type 6): string compare through the string helpers. Boolean (type 8): compare truthiness of both sides. Other types: integer compare. Dest type 8. | byte 1 vs byte 0, byte 2 | CONFIRMED (dispatch), LIKELY (string compare body) |
| 0x1b | Inequality (negation of 0x1a semantics) | as 0x1a | CONFIRMED |
| 0x1c | Less-or-equal (left <= right) | left = byte 1, right = byte 0 | CONFIRMED |
| 0x1d | Greater-or-equal (left >= right) | as 0x1c | CONFIRMED |
| 0x1e | Less-than (left < right) | as 0x1c | CONFIRMED |
| 0x1f | Greater-than (left > right) | as 0x1c | CONFIRMED |
| 0x20 | Add. Integer: wrap-around add. Real: soft-float add. String (type 6): concatenation through the string table. | left = byte 1, right = byte 0, dest byte 2 | CONFIRMED (int, real), LIKELY (string concat) |
| 0x21 | Subtract. Integer and real (type 5) only. | left = byte 1, right = byte 0 | CONFIRMED |
| 0x22 | Multiply. Integer: wrap-around. Real: soft-float multiply. Other types yield 0. | as 0x21 | CONFIRMED |
| 0x23 | Divide. Integer: truncating divide; zero divisor aborts with status 7. Real: zero divisor (exact equality with 0.0) aborts with status 7; otherwise divide. | dividend = byte 1, divisor = byte 0 | CONFIRMED |
| 0x24 | Modulo (integer only). Zero divisor yields result 0 with no error. Otherwise C-style remainder (sign follows dividend). | as 0x23 | CONFIRMED |
| 0x25 | Unary negate. Integer: two's-complement negate. Real: flip the sign bit. | byte 1 source, byte 2 dest | CONFIRMED |
| 0x26 | Boolean not (writes 1 if the source value is less than 1, else 0) | byte 1 source, byte 2 dest | CONFIRMED (code), LIKELY (intended for booleans only) |
| 0x27 | Return. If the current frame is the entry frame: copies slot 0 value to the caller's output and finishes with status 1. Otherwise pops the frame, restores pc from the saved return index, and continues. | none | CONFIRMED |
| 0x29 | Jump if condition slot (byte 2) is nonzero to label imm | byte 2 = condition | CONFIRMED |
| 0x2a | Jump if condition slot (byte 2) is zero to label imm | byte 2 = condition | CONFIRMED |

Operator-by-type dispatch uses the runtime type codes in section 2.

Operator operand order summary (CONFIRMED): for every binary op, byte 1 is the left operand and byte 0 is the right operand. The result goes to byte 2.

---

## 2. Value representation

### 2.1 Type codes (CONFIRMED from dispatch, the mapping of names is LIKELY)

| Code | Type | Payload |
|---|---|---|
| 0 | Null / unset | value ignored |
| 3 | Code (function reference) | function record pointer |
| 4 | Integer | 32-bit signed |
| 5 | Real | IEEE-754 binary32 bit pattern, arithmetic in software |
| 6 | String | index into string table (0 = null string) |
| 8 | Boolean | 0 or 1 |
| 7 (UNSURE) | Handle | index into handle table (see 2.3) |

Evidence: the equality, add, multiply, divide and compare cases branch on exactly these codes. Type 5 uses float helpers in every branch. Type 6 uses the string-table helpers. Type 8 uses truthiness compares. Type 4 uses plain integer ops.

### 2.2 Strings (CONFIRMED layout, LIKELY policy)

- The string table is an array of 16-byte entries located through the instance at +0x2874. Index 0 is the null string.
- Each entry has a reference count stored at entry offset +0xC. Addref increments it (CONFIRMED), and a release path decrements it (LIKELY, not read in full).
- Creating a string from a value first looks up an existing entry by content (hash lookup), and only creates a new entry when none is found. This is interning (LIKELY).
- Concatenation creates a new string from two entries (LIKELY).
- Strings are reference counted, so entries can be freed when unused (LIKELY).

### 2.3 Handles (CONFIRMED layout, UNSURE type code)

- Handles are stored as indices into a handle table at +0x2888 (4-byte entries). Handle identity comparisons use the index (CONFIRMED for the lookup, LIKELY for identity semantics).
- Native marshalling passes handle arguments as the raw stored word (CONFIRMED).

### 2.4 Booleans and null

- Booleans are integers 0 or 1 in type 8 (CONFIRMED).
- Null is the value 0 in any type (CONFIRMED: the executor tests value == 0 for null checks).

### 2.5 Reals

- Real values are stored as IEEE binary32 bit patterns (CONFIRMED: the software multiply manipulates sign, exponent and mantissa fields with 0x7f800000 and 0x7fffff masks).
- Arithmetic is done with integer-only routines, not the FPU, so results are deterministic across machines (CONFIRMED for multiply, LIKELY for the others).
- The multiply routine has these properties (CONFIRMED from its structure): a zero operand gives plus zero; the result sign is the XOR of operand signs; the mantissa product is truncated, not rounded; no NaN or infinity handling is visible in the routine. The reimplementation should expect truncation, not round-to-nearest.
- Division is implemented as a reciprocal followed by a multiply, with a shortcut that returns exactly 1.0 when the two raw operands are equal (CONFIRMED structure, LIKELY reciprocal).

---

## 3. Arithmetic semantics

| Case | Behaviour | Confidence |
|---|---|---|
| Integer add, subtract, multiply | Wrap-around 32-bit two's complement (no overflow check) | CONFIRMED |
| Integer divide, zero divisor | Aborts the thread with status 7; destination not written | CONFIRMED |
| Integer divide, INT_MIN / -1 | Not checked; on x86 this would raise a hardware exception. Behaviour UNSURE | UNSURE |
| Integer modulo, zero divisor | Result 0, no error | CONFIRMED |
| Real divide, zero divisor | Divisor compared to 0.0 by float equality (so +0 and -0 both count). Aborts with status 7 | CONFIRMED |
| Real arithmetic with NaN or infinity | No special handling visible in the routines inspected | UNSURE |
| Mixing integer and real in one op | The executor does not promote. Dispatch is on the left operand's type code. Mixed operands must be converted by an earlier explicit op or by the compiler. Exact behaviour when types differ UNSURE | LIKELY |
| Multiply or subtract with non-numeric type | Result 0 (multiply) or no store (subtract) | CONFIRMED (multiply), LIKELY (subtract) |
| Equality across types | Compares as the left operand's dispatch path; for mixed types the result is not defined by the executor | UNSURE |
| String concatenation with null | Null string is index 0 and is substituted with the empty string in the concatenation helper path | LIKELY |
| String compare | Content compare through the string helpers | LIKELY |
| Comparison of reals | Software float compares (less-than, less-or-equal) | CONFIRMED structure |

---

## 4. Operation limit and budget

### 4.1 Per-invocation instruction counter (CONFIRMED)

- The executor takes an instruction count as one of its arguments and stores it in the instance at +0x44.
- It runs a loop that executes instructions until the loop counter reaches that count, or until an opcode returns a status.
- So each call runs at most that many instructions and then returns status 2 (budget exhausted, the thread is still runnable).
- If the count is 0, the executor does no instructions and returns status 2.

### 4.2 Value of the limit (LIKELY)

- The constant 300000 appears as the budget or timeout argument at every top-level call site observed: the synchronous script call wrapper, the trigger-execute-wait helper, the thread-start helper and the run-current-thread helper.
- A string "Script function timeout!" is present near the wrapper that passes 300000 (FUN_6f3a8340).
- The most likely reading is a watchdog of 300000 time units (milliseconds = 5 minutes) for synchronous script calls, with an instruction budget passed separately. The decompile does not let me confirm which argument carries which meaning.
- The callers that pass 0 as the count run no instructions. They are used to set up or resume thread state rather than to execute.

### 4.3 What happens when the limit is exceeded

- Budget exhaustion returns status 2 and does not end the thread (CONFIRMED, from the status handling in the thread-start helper, which keeps the thread node for status 2).
- The thread is not silently killed by the instruction count. It is resumed on a later scheduling step (LIKELY).
- The "Script function timeout!" message path is a separate watchdog. Whether it kills the script or only reports it is UNSURE.
- The executor has no per-thread instruction counter across calls. Each invocation counts from zero (CONFIRMED).

---

## 5. Threads, waits and scheduling

### 5.1 Executor return codes (CONFIRMED)

| Status | Meaning | Thread node kept? |
|---|---|---|
| 1 | Entry frame returned normally; return value copied out | No (finished) |
| 2 | Instruction budget exhausted, or count 0 | Yes (runnable) |
| 3 | Suspended by a sleep request (suspension flag at +0x34 set during a native call) | Yes (sleeping) |
| 4 | Yielded or paused (flag at +0x3c set during a native call) | Yes (waiting) |
| 5 | Function not found for a start request (lookup returned null) | No (aborted) |
| 6 | Null or invalid reference during a ready check or a handle load | No (aborted) |
| 7 | Divide by zero (integer or real) | No (aborted) |

Evidence: the thread-start helper removes the thread node and frees its frames whenever the status is not 2, 3 or 4 (CONFIRMED).

### 5.2 Thread representation (CONFIRMED structure)

- A thread is a pooled object named AUFrame. The allocator takes objects from a free list at the owner's +0x14, or allocates a fresh one (FUN_6f45c0d0).
- Each thread object holds a pointer to its own value stack (a dynamic array of 40-byte slot records, count at +0x8c, capacity at +0x8) and a saved pc.
- Threads are linked into a circular list rooted at instance +0x2864. New threads are appended at the tail (instance +0x2868). This gives FIFO order of insertion (CONFIRMED for insertion, LIKELY for FIFO scheduling).

### 5.3 Starting a thread

- The thread-start helper (FUN_6f45fa30) creates a thread node when there is no current thread (pc is 0) or when a new-thread flag is set. Otherwise it reuses the current thread.
- For a new thread it marks the node and then runs the executor on it with the supplied count. After the executor returns, it removes the node unless the status is 2, 3 or 4 (CONFIRMED).
- The function-id lookup for a start request (FUN_6f45f8b0) returns status 5 if the function is unknown (CONFIRMED).

### 5.4 Sleep, yield and waits (LIKELY)

- Natives can request suspension by writing the thread's suspension record. The helper FUN_6f44b2f0 sets a suspend flag at +0x34, stores a payload at +0x38 and a value at +0x40. FUN_6f44b340 does the same but clears +0x38. FUN_6f44b3a0 sets the yield flag at +0x3c.
- After any native call, the executor checks these flags. Suspend flag set returns status 3. Yield flag set returns status 4 (CONFIRMED for the checks, LIKELY for the setters).
- The payload at +0x38 is read back by FUN_6f44b320 in the trigger execution state machine (FUN_6f447340). That machine treats it as a pointer to another trigger-execution object to wait on, and checks whether it equals the current object (LIKELY). So the wait target is possibly another trigger execution rather than a time value.
- Time-based sleep: the decompile does not show the wake-up time being computed. Unit of time and wake-up rule UNSURE.

### 5.5 TriggerSleepAction, TriggerWaitForSound, TriggerExecute, TriggerExecuteWait, TriggerEvaluate, ExecuteFunc, Sleep, StartThread

- Native names are present in the registration tables FUN_6f3d4020 and FUN_6f71d9e0. The index shows names and signature strings in one list, so the pairing of names to signatures is unreliable (CONFIRMED that the names exist, UNSURE for exact signatures).
- Names found: TriggerSleepAction, TriggerWaitForSound, TriggerExecute, TriggerExecuteWait, TriggerEvaluate, TriggerWaitOnSleeps, IsTriggerWaitOnSleeps, ExecuteFunc, Sleep, StartThread, PauseCompAI.
- "PolledWait" is not present in these tables (CONFIRMED by absence).
- TriggerExecute and TriggerExecuteWait map to the synchronous wrappers FUN_6f3b5470 and FUN_6f3b54b0 (LIKELY). FUN_6f3b54b0 returns true only when the run result is status 1 (CONFIRMED). FUN_6f3b5470 returns the trigger's result only when status 1 (CONFIRMED).
- TriggerEvaluate (condition evaluation) uses the same run path as trigger execution, with the condition's return value taken from slot 0 (LIKELY).
- TriggerSleepAction and the Sleep native set the suspension record and return; the thread resumes through the scheduler (LIKELY). The exact resume rule is UNSURE.

### 5.6 Scheduling order and resumption (LIKELY)

- The per-tick entry is FUN_6f44b400, called from the game-update function FUN_6f3af8d0. It iterates thread nodes in list order.
- It runs threads whose state field at +0x18 is zero (not currently running) and whose owner matches a filter. It also removes threads owned by a given id via FUN_6f45cf00 (LIKELY).
- Threads that were suspended (status 3 or 4) are kept at their list position and resumed in list order. Order of resumption among threads woken on the same tick is list order (LIKELY).
- Nested calls: a thread that starts another thread runs the new one synchronously through the start helper (CONFIRMED for the call structure).

### 5.7 Status codes from natives and nesting

- A native can set the suspend or yield flag while it runs. The executor then returns 3 or 4 after the native returns (CONFIRMED).

---

## 6. Locals, call stack, return values and depth

### 6.1 Registers and locals (CONFIRMED structure, LIKELY meaning)

- Operands address a fixed register file of 40-byte slots inside the instance. Slot 0 is the return-value slot (CONFIRMED: the return path reads slot 0). Slot 1 is used as a scratch or argument slot (LIKELY).
- Locals and temporaries beyond the fixed slots live on the per-thread value stack. Push saves a slot's three fields into the stack. Pop restores them (CONFIRMED).

### 6.2 Call stack

- Script-function calls (opcode 0x16) save the current pc and the caller's slot state onto the thread's value stack, then transfer control (CONFIRMED for the save, LIKELY for the transfer).
- Return (0x27) pops the frame and restores the saved pc. The saved return address is stored as an index scaled by 4 into the code base table at +0x2858. Byte units of the pc and jump targets are UNSURE.
- The entry frame is marked by a flag in the frame record (offset +0xb8). A return from the entry frame ends the thread with status 1 (CONFIRMED).

### 6.3 Return values

- The return value of a script function is slot 0 at return time (CONFIRMED).
- For a thread run to completion through the executor, the value is copied to the caller-supplied output pointer, but only when that pointer is nonzero (CONFIRMED).
- Native return values are stored into the destination slot with the declared type (CONFIRMED).

### 6.4 Maximum call depth

- No explicit call-depth limit or stack-overflow check was found in the executor or the call helpers (CONFIRMED by absence in the inspected code).
- The value stack and frame list grow on demand from the allocator (CONFIRMED).
- Whether a runaway recursion is caught by the watchdog or by memory exhaustion is UNSURE.

---

## 7. Native-call marshalling (CONFIRMED structure)

- Each native's function record provides: a native function pointer, an argument count, and a signature string. The signature is a string of type letters, read by the dispatcher (FUN_6f45d070).
- Signature letters:
  - I (and any letter not listed below): integer or boolean, passed as a raw 32-bit word.
  - R: real, passed as the raw float bit pattern.
  - S: string, converted from the string-table index to the string content before the call.
  - H ... ; (a handle type name in parentheses form, read up to the semicolon): handle, passed as the raw word.
  - C: code (function reference). Must be type 3, else null is passed.
- Arguments are read from the top of the value stack and placed in an argument array in reverse order. The dispatcher has fixed-size buffers for fifteen words each, which suggests a limit of 15 arguments (LIKELY).
- The native is called with the argument array and its return word is captured. For a real return, the bit pattern is used as-is (CONFIRMED).

---

## 8. Tables of constants observed

| Constant | Where | Meaning |
|---|---|---|
| 300000 | Budget or timeout argument at FUN_6f3a8340, FUN_6f3b5470, FUN_6f3b54b0, FUN_6f44b640 call sites, FUN_6f45f8b0, FUN_6f45fd00 | LIKELY: 5-minute watchdog in ms for synchronous script runs |
| 8 | Slot record size is 0x28 (40 bytes); instruction size 8 | CONFIRMED |
| 15 | Argument buffer size in native marshalling | LIKELY argument limit |
| 16 | String table entry size | CONFIRMED |
| 0x7f800000, 0x7fffff, 0x80000000 | Float field masks in software multiply | CONFIRMED |
| 0x3f800000 | Bit pattern of 1.0 (returned by the divide wrapper on equal operands) | CONFIRMED |
| 0x80274 | Event id passed to a trigger callback in FUN_6f446bb0 | CONFIRMED, meaning UNSURE |

---

## 9. Items not resolved

- The exact argument order of the executor call sites (the decompile mixes up the instance and count arguments). Marked UNSURE where it matters.
- The unit of the watchdog constant and of wait durations.
- The full string release path and the handle type code.
- How sleeping threads compute their wake-up time.
- Behaviour of the two extra opcode groups 0x0 to 0xa that are not in the executor switch.
- Whether INT_MIN divided by -1 faults.
