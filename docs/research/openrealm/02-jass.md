# OpenRealm JASS implementation: read-only findings

Repo: `/home/user/corepunch/open-realm` (read only; `git status` clean). All statements below come from reading the source. Nothing was built or executed, so behaviour claims marked "by reading" are unverified at runtime.

Root: `games/warcraft-3/jass/`.

## 1. Architecture: lexer, parser, interpreter

| Stage | File | Notes |
|---|---|---|
| Lexer | `games/warcraft-3/jass/jlex.c` (184 lines) | `parse_token` at `jlex.c:16`; `peek_token` at `jlex.c:86`. Uses static buffers (`static char word[MAX_SEGMENT_SIZE]`), so it is not reentrant. Max token/segment is 16384 bytes, silently truncated. |
| Parser | `games/warcraft-3/jass/jparser.c` (866 lines) | Recursive descent producing a linked token AST (`token_t`, declared in `jparser.h`). Entry: `JASS_ParseTokens` at `jparser.c:491`. Galaxy front end: `GALAXY_ParseTokens` at `jparser.c:833`. |
| AST | `games/warcraft-3/jass/jparser.h` | `TOKENTYPE` enum (`jparser.h:9-29`) and `struct token` with `primary/secondary`, `init`, `body`, `next`, `args`, `condition`, `elseblock`, `index`. |
| Interpreter | `games/warcraft-3/jass/jdo.c` (3058 lines) | Tree-walking over the AST. Entry points: `jass_dobuffer_ex` (`jdo.c:2289`), `jass_evalprogram` (`jdo.c:2278`), `jass_dofile_ex` (`jdo.c:2924`). |
| Coroutine scheduler | `jdo.c:858-986` (`jass_resumecoroutine`), `jdo.c:992` (`jass_resume`), `jdo.c:1081` (`jass_runevents`) | Explicit heap-allocated frame stack, so a `wait` yields without holding the C stack. Frame layout is in `games/warcraft-3/jass/jstate.h` (`struct jass_coroutine_frame`). |
| Public API | `games/warcraft-3/jass/jass.h`, `jass_api.h` | `jass_sleep` (`jass.h:106`), `jass_evaluatetrigger` (`jass.h:138`), `jass_executetrigger` (`jass.h:142`), `jass_rterror` (`jass.h:156`). |

**Answer: it is an AST interpreter, not bytecode.** Two evaluation paths share the same tree:
- Synchronous calls recurse on the C stack: `eval_TOKENS` / `jass_dotoken` (`jdo.c:1903`) / `VM_EvalCall` (`jdo.c:1832`) / `jass_call_impl` (`jdo.c:2955`).
- Coroutines (every trigger action, `ExecuteFunc`, and any function that reaches `TriggerSleepAction`) step through `jass_resumecoroutine` (`jdo.c:858`), keeping per-frame `pc` and locals.

`games/warcraft-3/jass/jcode.c` (261 lines) looks like a bytecode emitter. It emits text such as `pushi`/`pushf`, but `VM_Compile` (`jcode.c:231`) is declared in `jvm.h` and never called. `jopcodes.h` holds only a placeholder enum. Treat that as dead code.

Operators are not special in the evaluator. The parser rewrites them to calls of native functions named `__add`, `__sub`, `__and`, etc. (`jparser.c:77-88`, `parse_operator_token` at `jparser.c:223`). The table is `jass_operators` at `jdo.c:299-321`. All arguments are evaluated before the call (`VM_EvalCall`, `jdo.c:1832-1851`).

Value stack: `MAX_JASS_STACK 256` slots (`jstate.h:121,130`). Overflow raises `"stack overflow"` (`jdo.c:45`). Because each active call holds at least one slot, nested synchronous recursion is bounded by this limit rather than unbounded C recursion. This is by reading.

Modes: JASS mode sets `c_operators = false` (`jparser.c:492`). Galaxy mode sets it true (`jparser.c:834`) and enables `!`, `&&`, `||`, `&`, `|`, `^`, `<<`, `>>`, and C-style declarations. `SYNTAX_C_OPERATORS` is defined at `jdo.c:23`.

## 2. Operator precedence and associativity

Grammar levels, lowest to highest (`games/warcraft-3/jass/jparser.c`):

| Level | Operators | Function | Lines |
|---|---|---|---|
| 1 (lowest) | `and`, `or` (same level) | `parse_logical_expression` | `jparser.c:326-343` |
| 2 | `==`, `!=`, `<`, `>`, `<=`, `>=` (one level) | `parse_comparison_expression` | `jparser.c:314-324`, operator set at `jparser.c:64-67` |
| 3 | `+`, `-` | `parse_additive_expression` | `jparser.c:302-312`, set at `jparser.c:56-62` |
| 4 | `*`, `/` | `parse_multiplicative_expression` | `jparser.c:290-300`, set at `jparser.c:51-54` |
| 5 (highest) | prefix `not`, unary `-`, `function name`, `( )`, literals, identifiers, calls, array access | `read_single_identifier` | `jparser.c:247-288` |

Facts from the code:

- **`and` and `or` share one precedence level.** There is no separate tier for `and`. `parse_logical_expression` consumes either operator, and the two are right-associative.
  - `a and b or c` parses as `a and (b or c)`.
  - `a or b and c` parses as `a or (b and c)`.
  - Standard precedence (and binds tighter than or) would give `(a and b) or c` for the first case. This disagrees with the usual reading of `and`/`or` and should be checked against retail.
- **All binary levels are right-associative**, because each level recurses on its right operand (`right = parse_X(p)` at `jparser.c:294, 306, 318, 330`). Consequences:
  - `10 - 3 - 2` evaluates to `10 - (3 - 2) = 9`. Left-associative gives 5.
  - `10 - 3 + 2` evaluates to `10 - (3 + 2) = 5`. Standard gives 9.
  - `12 / 3 / 2` evaluates to `12 / (3 / 2)`. Integer division gives `12 / 1 = 12`. Standard gives 2.
  - `a < b == c` nests the comparison on the right.
  - The repo has no test or doc covering chained arithmetic or mixed `and`/`or` (searched `games/warcraft-3/game/tests/*.c`). The bundled `tests/resources-src/Scripts/common.j` and `Blizzard.j` also contain no such unparenthesised chains (grep found none). So this would not have shown up in the bundled scripts, but it may affect real maps that write `a - b - c`. This is the most important finding in this report. It is by reading only.
- **Prefix operators bind to a single primary.** `read_single_identifier` (`jparser.c:257-261`) takes one primary as the operand of `not` or unary `-`. So `not a == b` is `(not a) == b`, and `-x * y` is `(-x) * y`. Retail precedence for `not` is not in the repo.
- **Parenthesised expressions** go through `parse_logical_expression` (`jparser.c:262`), which consumes the closing `)` at the tail (`jparser.c:339`).
- **`and`/`or` are not short-circuit.** `__and` and `__or` (`jdo.c:270-276`) receive both operands already evaluated (`VM_EvalCall`, `jdo.c:1832-1851`). The coroutine IF path (`jdo.c:800-836`) also uses ordinary expression evaluation for `and`/`or`. Retail short-circuit behaviour is not documented in this repo, so it is not confirmed here.
- Comparison operators return boolean via `JASS_CMPOP` (`jdo.c:65-68`), which compares as `jass_checknumber` values. Strings in `<` silently become `0`.
- Equality (`var_eq`, `jdo.c:240-260`) compares by type and value. Handles compare by pointer, and by `type_id` only for value-handle types (`jass_valuehandle`, `jdo.c:227-238`).

Retail notes: the repo has no operator-precedence documentation for JASS. The only retail-related comment in the parser is `jparser.c:450`, which says debug-prefixed statements are parsed but excluded from release execution.

## 3. Arithmetic, conversions, strings, handles

### Integers and reals

- `JASS_NUMOP` (`jdo.c:70-77`) computes `+ - * /` with C operators on `int32_t` when both operands are integer; otherwise it uses `float`. `__add` (`jdo.c:185-209`) special-cases string concatenation. `__unm` is at `jdo.c:211-217`.
- **Integer overflow:** signed 32-bit overflow is undefined behaviour in C. The Makefile does not pass `-fwrapv` (`Makefile:5, 11-16`). In practice it wraps like retail, but this is not guaranteed by the code. No wrap is applied explicitly. Retail 32-bit wrap behaviour is not documented here.
- **Integer division by zero:** `__div` (`jdo.c:221`) has no guard. For two integers, `x / 0` is a C division by zero and traps with SIGFPE on x86, which crashes the process. `INT32_MIN / -1` traps the same way. No guard was found (grep for "divide", "by zero", "FPE" found nothing). By reading.
- **Real division by zero** is IEEE `inf`/`nan` with no check. Retail behaviour is not documented here.
- **Real-to-integer:** there is no implicit conversion. `jass_checkinteger` (`jdo.c:1672-1676`) calls `assert_type` (`jdo.c:43`), which raises a runtime error if the argument is not integer. Passing a real to an integer parameter therefore aborts the current coroutine.
- `R2I` is a C cast `(int32_t)` (`games/warcraft-3/game/api/api_misc.h:102`). It truncates toward zero. Out-of-range or NaN is undefined behaviour in C. `I2R` (`api_misc.h:101`) is a plain cast.
- `S2R` uses `atoi` (`api_misc.h:116`, via `MATH_FUNC`), so `S2R("3.7")` returns `3.0`. This looks like a bug, since `S2R` is declared `returns real` in `games/warcraft-3/game/common.txt:2239`. By reading.
- `S2I` uses `atoi` (`api_misc.h:115`). No overflow handling.
- **Copy-into-integer hazard:** `jass_copy` (`jdo.c:1512-1560`) copies `sizeof(int32_t)` bytes into an integer variable regardless of the source's type (`jdo.c:1524-1526`). Assigning a boolean (1-byte allocation, boolean case inside the same function) or a short string into an integer reads past the source buffer. Well-typed JASS should not reach this, but it is a memory-safety gap. By reading.
- **Mixed `+` with a string and a number** silently produces a numeric result, because `jass_checknumber` returns `0` for strings (`jdo.c:1678-1693`). No runtime error is raised.

### Literals

- Integers: `VM_EvalInteger` (`jdo.c:1765-1769`) uses `strtol(s, NULL, 0)`, so hex (`0x`) and `$` hex are supported, with no range check. Values wrap through the `int32_t` cast.
- `is_integer` (`jdo.c:337-347`) uses `strtol` base 0. Leading-zero literals therefore parse as octal, so `"010"` is 8. `"08"` is not a valid octal, so it falls through to `is_float` and becomes a real. Retail behaviour for leading zeros is not documented here.
- `is_float` (`jdo.c:349-353`) uses `strtod`, which also accepts `inf`, `nan`, `infinity`, and hex floats. `read_single_identifier` checks `is_float` before `is_identifier` (`jparser.c:247-288`), so a variable named `inf` or `nan` would parse as a real literal. By reading.
- FourCC literals: `'xxxx'` (`jdo.c:355-357`), evaluated by `VM_EvalFourCC` (`jdo.c:1828-1830`) as a raw `uint32` cast to int.

### Strings

- Storage: every value push heap-copies its bytes (`jass_store_value`, `jdo.c:56-62`, and `jass_pushstringlen`, `jdo.c:1632-1638`). Concatenation (`__add`, `jdo.c:185-209`) allocates through `jass_alloc`. There is no interning.
- **Backslash collapsing:** `jass_pushstringlen` calls `removeDoubleBackslashes` (`jdo.c:325-335`) on every pushed string. This includes native results and concatenation results, so `\\` becomes `\` everywhere. Whether this matches retail is not documented. By reading.
- Null: `jass_checkstring` (`jdo.c:1717-1726`) maps a null handle to `""`. The comment at `jdo.c:1719-1721` says this matches Blizzard's cinematic helpers passing null through string parameters.
- Formatting natives: `I2S` uses `%d`, `R2S` uses `%f` (6 decimals), `R2SW` clamps width and precision (`api_misc.h:118-138`). Retail formatting is not verified in repo.
- Lexer truncation: string literals longer than 16384 bytes are silently truncated (`jlex.c:16-50`).

### Handles and agent allocation

- **VM-owned handles** (`jass_newhandle`, `jdo.c:1599-1608`) get `ref->id = ++jass_root(j)->next_handle_id`. The counter is monotonic per root state and never reused. However, `GetHandleId` does not read `ref->id`. The counter is used only by snapshot code (`jdo.c:2459`, `jdo.c:2538`).
- **Light handles** (`jass_pushlighthandle`, `jdo.c:1610-1618`) wrap an engine pointer with `refs = 1` and no id.
- **Reference counting:** `jass_copy` increments `refs` (`jdo.c:1534`). `jass_setnull` decrements it and frees on zero (`jdo.c:1454-1486`).
- **`GetHandleId`** (`api_hashtable.h:150`) calls `hashtable_handle_id` (`api_hashtable.h:70-92`):
  - Units and other edicts: edict index (`hashtable_is_edict`).
  - Hashtables: `HASHTABLE_HANDLE_ID_BASE + 0x3000 + index`.
  - Players: client index.
  - Groups: `+ 0x2000` on the slot index.
  - Regions and events: encoded slot and generation.
  - Event registrations: `+ 0x5000`.
  - **Everything else** (triggers, timers, dialogs, sounds, and so on) falls through to a pointer-derived hash: `HASHTABLE_HANDLE_ID_BASE + 0x1000 + ((p>>3) ^ (p>>32))` (`api_hashtable.h:88`). That contradicts the comment at `api_hashtable.h:70` ("stable registry ordinals"). Those ids are not guaranteed unique or stable across save and load. By reading.
- The `HASHTABLE_HANDLE_ID_BASE` comment (`games/warcraft-3/game/g_local.h:1042`) says ids are kept out of the low edict range.

### Agent slot reuse

| Agent | Allocator | Reuse | Lines |
|---|---|---|---|
| Trigger | bump allocator, `MAX_TRIGGERS 4096` | never reused. `DestroyTrigger` is a no-op. Overflow raises `"CreateTrigger: trigger registry is full"` | `g_local.h:1027`; `g_utils.c:543-547`; `api_trigger.h:59-62`, `api_trigger.h:64-66` |
| Timer | bump allocator, `MAX_TIMERS 1024` | never reused. `DestroyTimer` bumps `generation` and pauses | `g_local.h:1028`; `g_timer.c:30-34`, `g_timer.c:124-129`; `api_misc.h:330-339` |
| Group | free list scan from `first_free_group`, `handle_id` = slot index | reused on `DestroyGroup` | `g_utils.c:411-412`, `g_utils.c:417-435`; `api_group.h:35-39` |
| Unit / edict | `G_Spawn` scans from `max_clients` for `!inuse && freetime + 1000 < level.time` | reused after a 1-second quarantine | `g_spawn.c:386-401`; `g_utils.c:60`, `g_utils.c:102` |
| Dialog / button | fixed slots | freed slots reused | `g_local.h:27-28` comment |

Consequence: a unit handle that outlives its unit can alias a new unit after the quarantine. `jass_checkhandle` (`jdo.c:1734-1747`) checks only for null and emits a stderr warning on type mismatch. It has no generation check. Triggers and timers never alias, but they are capped at 4096 and 1024 per map.

Retail notes: `jdo.c:1255-1258` and `jdo.c:2079` contain Blizzard-specific comments. There are no comments in `jdo.c` on handle numbering or reuse versus retail.

## 4. Threads, coroutines, sleep, triggers

### Starting threads

- `jass_startcoroutine` (`jdo.c:505-555`) memcpy's the whole root `jass_t` into a new state. This includes `global_hash[4096]`, `function_hash[4096]`, and the 256-slot stack, roughly 75 KB per coroutine. It then inherits player, unit, enum-unit, and region context (`jdo.c:530-546`). Performance note, by reading.
- `ExecuteFunc` (`games/warcraft-3/game/api/api_misc.h:509-513`) calls `jass_startcoroutinebyname` (`jdo.c:557-568`). That queues a coroutine. It does not run until the next `jass_runevents`. Retail timing is not documented in repo, so "runs immediately until first wait" is unverified.
- Unknown function name in `ExecuteFunc` calls `jass_missingcall` (`jdo.c:676-695`). Inside a coroutine that longjmps and aborts the caller (`jdo.c:709-720`).
- `newthread` (`api_misc.h:514`) also starts a coroutine with a copied context.

### TriggerSleepAction and wait

- `TriggerSleepAction` (`games/warcraft-3/game/api/api_trigger.h:538-553`) computes `msec = (uint32_t)(timeout * 1000)`. If `G_SkipCutscene()`, the timeout is clamped to `0.001`. It then calls `jass_sleep`.
- `jass_sleep` (`jdo.c:648-656`) sets `wake_time = jass_gettime() + msec` and `yielded = true`. Outside a coroutine it returns silently (`jdo.c:650-652`). So a sleep reached from synchronous code is a no-op.
- Negative timeouts: `(uint32_t)` of a negative float is undefined behaviour. There is no clamp at 0. By reading.
- `TriggerWaitForSound` (`api_trigger.h:555`) uses the same sleep path, with duration from the sound.

### Scheduler

- `jass_resume` (`jdo.c:992-1079`) returns without running if `wake_time > now`. It also invalidates a timer-started coroutine if the timer was paused before its first resume (`jdo.c:1010-1025`). It saves and restores `currentplayer`, `currentunit`, and `currentenumunit`, and the `bj_forLoopAIndex` global.
- `jass_runevents` (`jdo.c:1081-1108`) resumes every coroutine once per call and frees finished ones. It stops early when `halt_events` is set. Callers: `games/warcraft-3/game/g_main.c:494, 1022, 1134`; `g_utils.c:130`; `g_events.c:441`; `g_bot.c:2323`.
- Granularity: a waiting action resumes on the first `runevents` after its `wake_time`, so the effective resolution is the server frame.

### Operation limit

- There is **no instruction or operation budget**. The only guard is `INF_LOOP_PROTECTION 1000000` (`jdo.c:22`), used in two `assert` calls:
  - Synchronous `LOOP` (`jdo.c:2122`): `assert(i < INF_LOOP_PROTECTION)` after each iteration.
  - Coroutine loop frame (`jdo.c:900-903`): `frame->loop_count++` followed by `assert(frame->loop_count <= INF_LOOP_PROTECTION)`.
- The in-tree Makefile does not define `NDEBUG` (`Makefile:5, 11-16`), so these asserts are active in release (`-O2`) builds too. The comments at `jdo.c:900` and `jdo.c:1944` show the authors deliberately kept the increment outside the assert.
- **The coroutine counter never resets across waits.** `loop_count` lives on the loop frame, which persists in `co->frames` while the coroutine sleeps. A `loop` that calls `TriggerSleepAction(0.01)` on each iteration accumulates about 1 million iterations after roughly 2.8 hours of game time, and then aborts the process. At `0.1` s per iteration that is about 28 hours. This is by reading and was not run. It is a plausible failure mode for long-running map scripts.
- Infinite loops with no wait are not limited at all in release builds except by the assert above, which fires only after 1 million iterations. A synchronous loop that stays under 1 million iterations per call does not trip it.

### TriggerEvaluate, TriggerExecute, TriggerExecuteWait

- **`TriggerEvaluate`** (`api_trigger.h:572-599`) calls `jass_evaluatetrigger` (`jdo.c:1141`), which calls `jass_evaluatetriggercontext` (`jdo.c:1110-1139`). For each condition it memcpy's the whole `jass_s` into a scratch state, sets the trigger, unit, and event context, and calls the condition. The trigger passes only if every condition returns exactly one true boolean. It ignores `trigger->disabled`, which matches `docs/games/warcraft-3/trigger-events.md` ("Explicit trigger execution" section).
- **`TriggerExecute`** (`api_trigger.h:601-628`) calls `jass_executetrigger` (`jdo.c:1249`), which calls `jass_executetriggercontext(..., immediate=true)` (`jdo.c:1204-1247`). That creates one coroutine per action, in order, and resumes each one synchronously until it finishes or hits its first wait (`jdo.c:1238-1246`). Waiting actions stay scheduled. The `last` marker stops the loop from running actions that were added by nested executes. This matches the trigger-events doc.
- **Event dispatch** (`jass_calltriggercontext`, `jdo.c:1253-1264`) returns false if the trigger is disabled, evaluates conditions, then queues actions with `immediate=false`. So registered events respect `DisableTrigger`, and explicit `TriggerExecute` does not.
- **`TriggerExecuteWait`** (`api_trigger.h:630-639`) is a **stub**. It validates the handle and returns. Its own TODO says "Execute and yield until the trigger finishes once the coroutine scheduler supports waits."
- **Null trigger:** `TriggerEvaluate(null)` and `TriggerExecute(null)` have no null check in `api_trigger.h`. `jass_evaluatetriggercontext` dereferences `params->trigger->conditions` (`jdo.c:1113`), and `jass_executetriggercontext` dereferences `params->trigger->actions` (`jdo.c:1206`). The result would be a null-pointer dereference. By reading.
- `And` and `Or` boolexpr natives (`api_misc.h`, near the end of the file) are stubs that return a null handle.

Retail notes: `jdo.c:1253-1258` documents that Blizzard's queue helpers evaluate disabled triggers before executing them. The repo's trigger-events doc gives the same rationale. `jdo.c:2079-2082` mentions Blizzard's melee reveal timer.

## 5. Native coverage

Sources: `games/warcraft-3/jass-native-coverage.md` (568 lines) and `docs/games/warcraft-3/trigger-events.md` (232 lines). The coverage doc also points to the bundled declarations in `games/warcraft-3/game/common.txt`.

### What the coverage doc says

- Registry: 919 callbacks. The "last conservative source audit" classified 836 of them. Headline split: **502 implemented, 334 placeholders, 60.0 % overall** at that snapshot. The doc notes that newer registrations are not yet folded into this split (`jass-native-coverage.md`, "Baseline" section).
- Per-module table (same doc): the rows sum to **527 implemented and 309 placeholders** (836 total). That does not match the headline 502/334. The doc is internally inconsistent here, so treat the headline as stale.
- The doc says the codebase "already exceeds 50 %" and sets a stretch target of 656 implemented (78.5 %).
- Status vocabulary: `implemented`, `partial`, `placeholder`, `unregistered`. A callback is implemented only if it consumes every argument, returns the declared type, owns its handle lifetime, and has a behaviour test.
- Named gaps in the doc include `GroupAddUnit`/`GroupRemoveUnit` (no boolean return), filter args ignored in several enumerators, `ForceEnumPlayers` filters not evaluated, `SyncStored*` not synchronised, `ReloadGameCachesFromDisk` unregistered, `TimerDialogSetSpeed` no-op, `TriggerRegisterGameStateEvent` integer limits not implemented, `TriggerExecuteWait` absent from the doc but stubbed in code, and several Blizzard-enum and dialog-quit gaps.
- The doc points at `common.txt` as the signature authority and says placeholders are counted only when they ignore args and return zero or null.

### What I counted from source (by reading, not from the doc)

- Registration table `games/warcraft-3/game/api/api_module.c` has **1235 unique registered names** (`{ "Name", fn }` lines; `ConvertRace` at `api_module.c:137`, `S2R` at `:195`).
- `games/warcraft-3/game/common.txt` has **1249 unique `native` declarations** (after filtering comment noise).
- Overlap: **827** names are both declared and registered.
- **Declared but not registered: about 420** (two extra tokens from comments removed). Of these, **297 are `Blz*`**, which is the UI frame, ability-field, and item-field API family. Only 3 of 300 declared `Blz*` natives are registered. Other notable missing ones: `MathRound` (declared at `common.txt:2229`), `GetSelectedUnit`, `GetFilterItem`, `GetPlayerScore`, `SetTerrainType`, `TriggerRegisterCommandEvent`, `TriggerRegisterUpgradeCommandEvent`, `TriggerSyncReady`/`TriggerSyncStart`, `SetTextTagPos`, `UnitDamagePoint`, `CripplePlayer`, `IssueInstantPointOrder`, the `Multiboard*` getters and setters, `Convert*` helpers (about 39 of them, for example `ConvertAbilityIntegerField` and `ConvertUnitCategory`), `Automation*` test hooks, and `Request*` extra-data natives.
- **Registered but not declared in common.txt: about 408.** These are mostly `Save*`/`Load*`/`Have*`/`Flush*`/`Store*` hashtable and gamecache helpers, `Captain*`/bot AI natives, and Warsmash-style extensions. These are expected because they are not in the retail `common.j`.
- Caveat: "registered" does not mean implemented. A registered name may still be a placeholder. Use the per-module doc numbers for the implementation split, and this count only for the registration gap.
- `MathRound` is declared, not registered, so calling it raises `"unimplemented native"` (`jdo.c:676-695`) and aborts the calling coroutine.

### Trigger-events doc (`docs/games/warcraft-3/trigger-events.md`)

- Event payload is `GAMEEVENT` (`edict`, `source`, `value`, `point`, `responseTo`), copied into `JASSCONTEXT` by `jass_calltriggerevent`. It survives sleeps.
- Only the high-confidence spell event (`SPELL_EFFECT`) is produced. `SPELL_CHANNEL`, `SPELL_CAST`, `SPELL_FINISH`, and `SPELL_ENDCAST` are explicitly unsupported.
- Attacked events fire at the start of a real swing, with `GetAttacker()` from `source`.
- Region enter/leave, range, order, item pickup/use, and Hero-skill events are documented as implemented.
- Diagnostics are gated by `wc3_event_debug`, with counters `registered`, `queued`, `matched`, `dispatched`, and `filtered`.
- The doc's verification list points to tests in `games/warcraft-3/game/tests/t_jass_map.c` (68 `TEST(` entries in `t_jass*.c`). I did not find a test for operator precedence or chained arithmetic (see section 2).

## 6. Retail-compatibility comments in the JASS code

- `jdo.c:22`: `INF_LOOP_PROTECTION` comment says SC2 Galaxy scripts have large but legitimate loops.
- `jdo.c:23`: `SYNTAX_C_OPERATORS` enables Galaxy operators.
- `jdo.c:1255-1258`: Blizzard queue helpers evaluate disabled triggers before executing them.
- `jdo.c:1719-1721`: null string is treated as `""` because Blizzard's cinematic helpers pass null through string parameters.
- `jdo.c:2079-2082`: melee reveal timer passes `GetLocalPlayer()` outside an IF, in Blizzard's script.
- `jparser.c:450`: retail parses debug statements but excludes them at runtime.
- `jparser.c:502`: retail AI scripts use empty `globals` blocks.
- `api_hashtable.h:70`: "stable registry ordinals" for `GetHandleId`. Contradicted for non-edict handles (see section 3).
- `g_local.h:1038`: the `MAX_TEXTTAGS` comment calls the pool "retail-ish".
- `games/warcraft-3/jass-native-coverage.md`: `BZ_COMPAT_GUESS` tags mark unverified policies in several subsystems.

No comment in the JASS core claims exact retail behaviour for precedence, overflow, division, string escaping, or handle numbering.

## 7. Risks and checks for follow-up (ranked)

1. **Right-associative arithmetic** (`jparser.c:290-312`): `a - b - c`, `a / b / c`, and `a - b + c` evaluate incorrectly versus standard left-associative JASS. Add a test with those expressions and compare with retail output. Fix would be left-folding in `parse_additive_expression` and `parse_multiplicative_expression`.
2. **`and`/`or` at one level, right-associative** (`jparser.c:326-343`), and **no short-circuit** (`jdo.c:270-276`, `VM_EvalCall` at `jdo.c:1832`). Confirm the retail rule before changing anything.
3. **Integer division by zero** (`jdo.c:221`) traps the process. Guard and define the retail result.
4. **`INT32_MIN / -1` and signed overflow** (`jdo.c:70-77`) rely on undefined behaviour. Add `-fwrapv` or explicit unsigned arithmetic.
5. **Loop assert across waits** (`jdo.c:900-903`): a loop that waits per iteration can abort the process after 1 million iterations. Reset `loop_count` on each wait, or count only iterations without a yield.
6. **Null `TriggerEvaluate` and `TriggerExecute`** (`api_trigger.h:572`, `601`) dereference null (`jdo.c:1113`, `1206`). Add a null check.
7. **`S2R` uses `atoi`** (`api_misc.h:116`). Use `atof`.
8. **`jass_copy` integer case** (`jdo.c:1524-1526`) reads 4 bytes from any source type. Check the source type first.
9. **`GetHandleId` for triggers and timers** falls to a pointer hash (`api_hashtable.h:88`). Not stable across save and load. Extend the domain table if scripts store these ids in hashtables.
10. **`TriggerExecuteWait` stub** (`api_trigger.h:630`) and missing natives: `MathRound` and about 420 other declared natives, 297 of them `Blz*`.
11. **Coverage doc inconsistency**: headline 502/334 versus table sum 527/309. Reconcile.
12. **`is_float` accepts `inf`/`nan`** (`jdo.c:349-353`). Check identifiers before floats, or restrict the real parser to digits.
13. **Octal literals and `08`** (`jdo.c:337-347`). Decide the retail rule and make the lexer agree.

## Method and limits

- Read-only. No files in `/home/user/corepunch/open-realm` or `/home/user/projectx` were modified. `git status` in the repo was clean before and after.
- Nothing was compiled or run. Everything marked "by reading" is a claim from the source, not from executing a JASS script.
- Line numbers were taken from the working tree at the time of reading.
