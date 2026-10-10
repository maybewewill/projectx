# JASS expression precedence and associativity (as implemented by the Game.dll 1.26a parser)

Scope: derived by reading the yacc/LALR tables from `7b3e801f-game.dll` (ImageBase 0x6F000000) and simulating the driver. No file under /home/user/projectx was touched. Scripts are in `scratchpad/yacc/`.

Confidence key: CONFIRMED (shown by disassembly and/or matched against the real engine), LIKELY (strong inference), UNSURE (not determined).

## 1. Driver and tables

| Item | Value | Confidence |
|---|---|---|
| Parser engine (yyparse equivalent) | VA 0x6F464460, `__thiscall`, ctx arg on stack, returns `ctx[0x8C] != 0` on accept | CONFIRMED |
| Lexer called by engine | VA 0x6F45FF30 (flex-style), engine reads token at 0x6F46457F, text at ctx[0x26] | CONFIRMED |
| Parser struct fields (byte offsets from the parser struct) | 0x00 capacity (600), 0x0C state-stack base, 0x10 value-stack base, 0x14 state SP, 0x18 value SP, 0x0A current state (short), 0x08 rule/temp (short), 0x20 yyval, 0x28 lookahead, 0x2C error flag, 0x44 lexer/compile ctx | CONFIRMED |
| yypact (row offset per state) | DAT_6FA825C0, short[156], VA 0x6FA825C0..0x6FA826F8. State limit 0x9C (CONFIRMED from code); size from next table | CONFIRMED (limit), LIKELY (span) |
| Shift/reduce rows (packed) | base DAT_6FA822A8, short, VA 0x6FA822A8..0x6FA825C0 (396 shorts). Row = off from yypact; keys are scanned in row order until key <= lookahead, a match gives the action of key k at row[-(k+1)] | CONFIRMED |
| Default-action table | DAT_6FA820E8, short, VA 0x6FA820E8..0x6FA82158. Checked for state < 0x36 (code) | CONFIRMED (use), LIKELY (span) |
| Exception pairs | DAT_6FA82158, (tok, act) short pairs, tok<0 terminator, act after terminator = default. Entry for state s with negative defact value at index (-s-1) | CONFIRMED |
| yyr2 (RHS length per rule) | DAT_6FA82B50, short | CONFIRMED (148,327 reductions, all lengths equal the RHS observed in the trace) |
| yyr1 (LHS per rule, goto row index) | DAT_6FA82988, short, VA 0x6FA82988..0x6FA82B50 | CONFIRMED (goto use), LIKELY (span) |
| Goto table | DAT_6FA826F8, rows at base+2*LHS: default at row[0], keys at row[1..], value of key k at row[-(k+1)] | CONFIRMED |
| State encoding | shift/goto value v gives state -1-v. A state >= 0x9C is a reduce marker: rule = state - 0x6C (only used for rules >= 48) | CONFIRMED |
| Start | initial marker state 0x149 (rule 221, RHS length 0) goto LHS 190 default gives state 0 | CONFIRMED |
| Rule 0 | accept (returns ctx[0x8C] != 0) | CONFIRMED |
| Rule 1 | error-recovery entry (error token 0x100 search). Exception lists use rule 1 as the default for "no action" | CONFIRMED (trace), LIKELY (meaning) |

Lookup order in the engine (per step): state >= 0x9C is a reduce marker; else the shift row (if row[0] >= 0) is scanned, lexing only if needed; else the default: states < 0x36 use defact (>= 0 means reduce that rule without reading a token; < 0 means the exception list); states >= 0x36 go to error recovery.

## 2. Token numbers (from the real lexer, CONFIRMED)

Obtained by emulating the flex driver under unicorn (`lexer_probe.py`) on each spelling.

| Token | Code | Token | Code |
|---|---|---|---|
| EOF (end of input) | 0 | `or` | 0x129 (297) |
| integer literal | 0x108 | `and` | 0x128 (296) |
| real literal | 0x109 | `not` | 0x12F (303) |
| newline | 0x10B | `==` | 0x12A (298) |
| Ctrl-Z (0x1A, end-of-file marker of a script) | 0x10C | `!=` | 0x12B (299) |
| identifier, variable reference (used by the grammar in expressions and `set` target) | 0x130 (304) | `<=` | 0x12C (300) |
| identifier, new function name in `function` header | 0x132 (306) | `>=` | 0x12D (301) |
| `function` / `endfunction` / `takes` / `returns` / `nothing` / `set` / `local` | 0x11B / 0x115 / 0x125 / 0x123 / 0x121 / 0x124 / 0x11E | `<` `>` `+` `-` `*` `/` `%` `=` `(` `)` | single-char codes 0x3C 0x3E 0x2B 0x2D 0x2A 0x2F 0x25 0x3D 0x28 0x29 |

Note: the identifier class is chosen by the symbol table in the real lexer (action helpers at 0x6F4667B0 and the class results 0x130..0x133). The simulator and the harness take the class as an input. For expressions 0x130 is accepted, and the grammar rejects 0x132 at the `set` target (LIKELY: 0x132 is the "new name" class, 0x130 the "known name" class).

## 3. Operator rule numbers (CONFIRMED from traces)

| Operator | Binary rule | Mid-rule (empty) before the right operand | Unary rule |
|---|---|---|---|
| or | 31 | 84 | - |
| and | 30 | 85 | - |
| == | 32 | 83 | - |
| != | 34 | 81 | - |
| <= | 35 | not listed | - |
| >= | 36 | not listed | - |
| < | 37 | 78 | - |
| > | 38 | not listed | - |
| + | 39 | 76 | - |
| - | 40 | 75 | 59 (prefix minus, mid 60) |
| / | 71 | 72 | - |
| * | 73 | 74 | - |
| % | 69 | 70 | - |
| not | - | - | 61 (prefix, mid 62) |

Rule 41 is `expr := identifier`. The expression nonterminal is N125; mid-rule nonterminals are reduced by the empty rules above. Rules 26, 221, 223 are statement-level. These numbers match the rule numbers in the earlier notes (and=0x1E, or=0x1F, ==0x20, !=0x22, <=0x23, >=0x24, <0x25, >0x26, +0x27, -0x28, /0x47, *0x49, %0x45, unary minus 0x3B, not 0x3D).

## 4. Simulator and method

- `jass_lalr.py`: a Python port of the engine's control flow. Tables are read from the PE image with pefile (no game code is run). It takes a token list (0 = EOF, the Ctrl-Z token 0x10C ends a script), returns (accepted, rules, shifts) and optionally a trace of shifts, reductions (with RHS symbols) and gotos.
- Driver context: script `function f takes nothing returns nothing` / `set x = <expr>` / `endfunction` / Ctrl-Z. Identifiers: function name 0x132, every expression name 0x130. Without the Ctrl-Z token the parser enters error recovery at EOF (state 5 accepts only 0x10C and 0x11B after a function), so the Ctrl-Z terminator is required.
- Cross-check: `realparse.py` runs the real engine FUN_6F464460 under unicorn, driving the real lexer from text, with the symbol-table identifier helper stubbed to a fixed class and node-allocation FUN_6F4C1AB0 stubbed to return 0 (no AST, the control flow is unchanged). It logs tokens, pushes and reductions.
- Results: simulator and real engine give identical reduction sequences for the 21 traced expressions and for 60 random expressions (`crosscheck_real.py`). Across 500 random expressions (`validate_prec.py`) the reductions' RHS lengths equal yyr2 for all reductions, and the simulator's tree equals a precedence-climbing parser built from the table in section 6 (0 mismatches over 3000 expressions).
- Derivation (`derive_prec.py`): for each ordered pair (P, Q) parse `a P b Q c`. If the P rule is reduced before the Q token is shifted, P binds at least as tightly; the reverse test gives the other direction; both directions give the same level. Same operator twice gives associativity (reduce before the second shift = left).

## 5. Raw reduction traces

Full traces (shift tokens S, reduce rules R, rule-level detail) are in `scratchpad/yacc/traces.txt`. Selected:

- `a - b - c`: R187 SID R41 S- R75 SID R41 S- ... The first R40 (a-b) is reduced after the second `-` is seen as lookahead and before it is shifted. Left-assoc.
- `a / b / c`: R71 reduced before the second `/` is shifted. Left-assoc.
- `a + b * c`: S* shifted before R39 (+). `*` binds tighter.
- `a * b + c`: R73 reduced before S+. `*` binds tighter.
- `a or b and c`: R31 (or) reduced before S and. `or` binds tighter than `and`.
- `a and b or c`: S or shifted before R30 (and). `or` binds tighter.
- `a and b and c`: two shifts then R30 R30: right-nested.
- `a or b or c`: right-nested (R31 R31).
- `not a == b`: R61 (not) reduced before S== . `not` binds tighter.
- `a == b == c`: R32 before the second S==. Left-assoc.
- `- a * b`: R59 (unary minus) reduced before S*. Unary minus binds tighter.
- `a < b + c`: S+ before R37 (<). `+` binds tighter than `<`.
- `a < b < c`, `a != b != c`: reduced before the second operator. Left-assoc.
- `a == b < c`: R37 before R32. `<` binds tighter than `==`.
- `a + b == c`: R39 before S==. `+` binds tighter than `==`.
- `(a or b) and c`: parentheses override as expected.

## 6. Precedence and associativity table (tightest first)

Derived relation (12x12 binary plus `%`, unary checks) is written to `scratchpad/yacc/prec_relation.json`.

| Level | Operators | Associativity (tree shape) | Confidence |
|---|---|---|---|
| 1 (tightest) | prefix `not`, prefix unary `-` | prefix (nesting forced by grammar) | CONFIRMED vs all 12 binary operators (U > Q). Relative order of `not` and unary `-` is not decided by any conflict (UNSURE, does not matter for parsing) |
| 2 | `*`, `/`, `%` | left | CONFIRMED (`%` is accepted, JASS has no `%`) |
| 3 | `+`, `-` | left | CONFIRMED |
| 4 | `<`, `<=`, `>`, `>=` | left | CONFIRMED (same level among themselves by both-direction test) |
| 5 | `==`, `!=` | left | CONFIRMED |
| 6 | `or` | right (tree shape) | CONFIRMED tree shape; semantically harmless for boolean operators |
| 7 (loosest) | `and` | right (tree shape) | CONFIRMED tree shape |

So `a or b and c` parses as `(a or b) and c`, and `a and b or c` parses as `a and (b or c)`. This is the reverse of the usual C/JASS convention (where `and` binds tighter than `or`). CONFIRMED as implemented by these tables and the real engine. UNSURE whether that is intended behaviour of the shipped game or a documented JASS rule: check against the JASS manual before depending on it.

Other CONFIRMED facts: `-`, `/`, `*`, `+` are left-associative; `a - b * c` = `a - (b * c)`; `- a - b` = `(-a) - b`; `not` and unary `-` are accepted after the binary operators tested (`a + not b`, `a * not b`, `a or not b`, `a and not b`, `a == - b`; all parsed with no error recovery).

## 7. Limits and open points

- Identifier token classes come from the symbol table in the real lexer. This analysis fixed them (0x130 for expression names, 0x132 for a new function name). The precedence result does not depend on the class, as long as the grammar accepts it at that position (verified for 0x130 in expressions).
- `%` is in the grammar (rule 69, same level as `*` and `/`) although JASS has no `%`. Its lexer code is the raw character 0x25.
- Array spans (sections 1) are inferred from neighbouring base addresses, not from declared sizes. All accesses observed stayed inside those spans.
