# JASS front end (scanner, parser, compiler entry): behaviour spec

Scope: Warcraft III 1.26a Game.dll, decompiled pseudo-C only (chunk_019.c and index.tsv). No PE image or table data is in the workspace, so the flex DFA tables, the yacc action/goto tables and the flex rule text are NOT available. Anything that depends on them is marked UNSURE.

Confidence key: CONFIRMED (seen clearly in code), LIKELY (strong inference from code shape), UNSURE (cannot be determined from this material).

## 0. Architecture and entry points

| Role | Address | Source tag | Confidence |
|---|---|---|---|
| Top-level compile entry (scanner init, parse, cleanup) | 0x6F458900 | Compile.cpp:143 | CONFIRMED |
| Compile driver: builds parser state, runs the parse, sets the result | 0x6F458510 | Compile.cpp:129 (assert) | CONFIRMED |
| Scanner (yy_scan) constructor, wraps input text | 0x6F458870 | | CONFIRMED |
| Scanner base init (buffers, sizes, state) | 0x6F460570 | scanner.cpp:0x194-0x196 | CONFIRMED |
| Scanner destructor | 0x6F460530 | scanner.cpp:0x1AE-0x1AF | CONFIRMED |
| Character source: next byte from input, -1 at end | 0x6F458790 | | CONFIRMED |
| Input segment advance (input is a chain of (pointer, length) pairs) | 0x6F4587D0 | | LIKELY |
| Token producer (generated flex-style lexer, ~1300 bytes) | 0x6F45FF30 | | CONFIRMED (structure), tables UNSURE |
| Parser state-stack allocation (capacity 600) | 0x6F460790 | parser.cpp:0x3C4-0x3C5 | CONFIRMED |
| Parser free | 0x6F4607F0 | parser.cpp:0x3CB-0x3CC | CONFIRMED |
| Parser engine (LALR table-driven, yacc-style, ~6.8 KB) | 0x6F464460 | Jass2/Nodes.h guards | CONFIRMED (engine), tables UNSURE |
| Symbol table (chained hash, 1024 buckets) | 0x6F4586D0 (ctor), 0x6F458440 (insert), 0x6F458320 (probe), 0x6F45FE70 (lookup) | | CONFIRMED / LIKELY |
| Signature (type descriptor) parser for natives/functions | 0x6F455110 (error text), 0x6F4572D0 (type matcher) | Jass.cpp, Compile.cpp | CONFIRMED |
| AST pretty-printer (writes JASS text back out) | 0x6F466860 to 0x6F467390 | Jass2/Nodes.h | CONFIRMED |

Pipeline: source text, then the character source, then the lexer (0x6F45FF30) returns token codes, then the LALR engine (0x6F464460) reduces by rule number, and each reduce builds a Node object (class names from the vtables). Accept is a success flag at a field of the compile context (offset 0x8C). The compile driver returns success only when the engine returns false (see section 6).

## 1. Tokenizer

### 1.1 Token codes returned by the lexer

Token values are yacc-style: named tokens start at 257 (0x101), and a single-character token returns its own character code.

| Rule (switch case) | Returned token | Meaning (from the action) | Confidence |
|---|---|---|---|
| 0x2A | 0x108 (264) | decimal integer, value computed in the action (see 1.3) | CONFIRMED (value), LIKELY (rule) |
| 0x2B | 0x108 | octal integer (leading 0 form) | CONFIRMED (action), LIKELY (regex) |
| 0x2C | 0x108 | hex integer, prefix of 1 character skipped (probably `$`) | CONFIRMED (skip count), LIKELY (`$`) |
| 0x2D | 0x108 | hex integer, prefix of 2 characters skipped (`0x`) | CONFIRMED (skip count), LIKELY (`0x`) |
| 0x2F | 0x108 | rawcode / character constant `'...'` | CONFIRMED |
| 0x2E | 0x109 (265) | real literal | CONFIRMED (action), LIKELY (rule) |
| 0x30 | 0x10A (266) | string literal | CONFIRMED |
| 0x29 | one of 0x130 to 0x133 | identifier, classified by symbol-table lookup (see 1.6) | CONFIRMED |
| 2 | 0x107 (263) | identifier-like token with interned text | LIKELY (purpose unclear, see 1.6) |
| 0x32, 0x33 | 0x10B (267) | not identified (may be newline or EOL) | UNSURE |
| 0x34 | 0x10C (268) | not identified | UNSURE |
| 0x23 | 0x10D (269) | not identified | UNSURE |
| 0x14 | 0x10E (270) | not identified | UNSURE |
| 8 to 0x13, 0x15 to 0x22, 0x24 to 0x28, 0x2A to 0x2C etc. | 0x10F to 0x12D | named keyword/operator tokens (numbers listed in 1.5) | CONFIRMED (codes), UNSURE (spelling) |
| 0x35 | the matched character itself | single-character token (`(`, `+`, `=` and so on) | CONFIRMED |
| 0 (default) | pushes the character back and rescans | unmatched input | LIKELY |
| end of input | 0 | end-of-input token | CONFIRMED |

Literal and value tokens: 0x108 INTEGER, 0x109 REAL, 0x10A STRING, 0x107 identifier-like, 0x130-0x133 identifier classes.

### 1.2 Lexer state

- The lexer keeps a line counter (incremented for each newline in consumed text). Each token records the current and previous line numbers. Line-based error reporting is therefore possible. CONFIRMED (counter), LIKELY (use).
- Token text is kept in a buffer; a fixed capacity of 2100 bytes (constant 0x834 passed to the scanner constructor). If a token grows past that capacity while the token state flag is set, the lexer returns -2, which the engine treats as abort (returns failure). CONFIRMED (constant and -2 path), LIKELY (meaning as token-length limit).

### 1.3 Integer literals

| Form | Rule | Value computation | Overflow |
|---|---|---|---|
| Decimal (no leading 0, not hex) | 0x2A | value = value*10 + digit, 32-bit signed int | Wraps silently (no check in code). CONFIRMED (no check), LIKELY (wraps) |
| Octal (leading 0) | 0x2B | value = value*8 + digit; digits are read as-is, no 8/9 validation in the action | Wraps. The regex decides which strings reach this rule: UNSURE. "09" would produce 9 if it reaches the rule |
| Hex with `0x` prefix | 0x2D | value = value*16 + digit, digits 0-9, a-f, A-F | Wraps (32-bit). An 8+ digit hex value wraps to a negative int |
| Hex with `$` prefix (probable) | 0x2C | same as above, skip 1 char | Wraps |

- Integers are stored as 32-bit signed values. CONFIRMED.
- There is no range check and no error for an oversized literal. CONFIRMED (no check in actions).

### 1.4 Rawcode and character constants

Action at 0x6F466450 (rule 0x2F). Token returned: 0x108 (INTEGER).

| Input form | Value |
|---|---|
| `'x'` (1 char) | the character's code (0-255) |
| `'abcd'` (4 chars) | packed big-endian into 32 bits: a*256^3 + b*256^2 + c*256 + d. No length check in the action; the regex must enforce length 1 or 4. CONFIRMED (packing), UNSURE (regex enforces 1 or 4) |
| `'\\'` escape, then one of `"` `'` `\` b f n r t | 0x22, 0x27, 0x5C, 8, 0xC, 0xA, 0xD, 9 |
| `'\\'` escape, then anything else | 0 (default case). CONFIRMED |
| `'\uXXXX'` (or capital U) | 4 hex digits, packed into 32 bits. CONFIRMED |

### 1.5 Real literals

Action at 0x6F466690 (rule 0x2E), returns 0x109 (REAL).

- The action locates a `.` in the text. If there is none, the whole text is converted. Conversion uses an external float routine (0x6F6EF350 family). The result is stored as a 32-bit float. CONFIRMED (value stored as 32-bit), LIKELY (float32 semantics).
- Forms `.5` and `5.` are both handled by the same action. Which exact regex forms reach it (for example exponent forms) is UNSURE.

### 1.6 Identifiers and the `handle` special case

- Identifier text is interned in the global hashed string table (0x6F454930). CONFIRMED.
- Rule 0x29 (general identifier path): the lexer looks the name up in the symbol table. If the name is new it creates a symbol. The token returned depends on that symbol's type code: 0x130 (type codes 12, 41, 42, 43), 0x133 (type codes 9, 6), 0x131 (type code 10, or the name equals `handle` when unknown), otherwise 0x132 (plain identifier). CONFIRMED (branch structure), UNSURE (exact meaning of each class).
- Rule 2 interns the literal `handle` and returns 0x107. The meaning is UNSURE (it may be the `handle` type keyword, or a Ghidra artifact).
- Identifier character classes, length limit, and case sensitivity: UNSURE (regex not available). The only visible limit is the 2100-byte token buffer (1.2).

### 1.7 Strings

- Action at 0x6F466770 (rule 0x30): the leading quote is skipped, the last character (closing quote) is overwritten with NUL, and the text is interned. CONFIRMED.
- Escapes are NOT decoded in this action. A backslash stays as a literal character in the string value. CONFIRMED (no decode loop in the action). Whether the regex allows `\"` inside a string: UNSURE.
- String length is limited by the 2100-byte token buffer. LIKELY.

### 1.8 Comments and whitespace

- No comment handling is visible in the decompile (the rule actions for comment rules are not identified). `//` comments are almost certainly skipped, but UNSURE.
- Whitespace: skipped by the lexer (not a token), LIKELY.

## 2. Expression grammar

Node classes and the rule numbers that build them (yacc rule numbers from the engine switch):

| Operator | Rule number(s) | Node class | Opcode (node field) | Confidence |
|---|---|---|---|---|
| and | 0x1E | NodeAnd | 0x11 | CONFIRMED |
| or | 0x1F | NodeOr | 0x12 | CONFIRMED |
| == | 0x20, 0x21 (0x21 has a validity check and error code 0x1B) | NodeEQ | 0x1D | CONFIRMED |
| != | 0x22 | NodeNE | 0x1C | CONFIRMED |
| <= | 0x23 | NodeLE | 0x1B | CONFIRMED |
| >= | 0x24 | NodeGE | 0x18 | CONFIRMED |
| < | 0x25 | NodeLT | 0x19 | CONFIRMED |
| > | 0x26 | NodeGT | 0x1A | CONFIRMED |
| + | 0x27 | NodeAdd | 0x13 | CONFIRMED |
| - (binary) | 0x28 | NodeSub | 0x14 | CONFIRMED |
| * | 0x49 | NodeMul | 0x15 | CONFIRMED |
| / | 0x47 | NodeDiv | 0x16 | CONFIRMED |
| % (modulo) | 0x45 | NodeMod | 0x17 | CONFIRMED (node exists), UNSURE (which token reaches it) |
| unary - | 0x3B | NodeUMinus | 0x1E | CONFIRMED |
| not | 0x3D | NodeNot | 0x1F | CONFIRMED |

Notes:
- Comparison node construction (0x6F462770) has a null-coercion rule: if one operand has type code 2 (null), it takes the type of the other operand. CONFIRMED.
- Printer names for the same operators (0x6F466A50) use a different numbering (0x18 " and ", 0x19 " or ", 0x1A "==", and so on), which does not match the node opcodes above. One of the two numberings is a different enumeration. UNSURE which.

### 2.1 Precedence and associativity

- The precedence and associativity declarations are yacc table data, which is not available. UNSURE for exact levels.
- Rule numbering (yacc numbers rules in source order) gives a weak hint only: and/or (0x1E, 0x1F) come first, then the comparisons (0x20-0x26), then + and - (0x27, 0x28), then unary minus and not (0x3B, 0x3D), then * / % (0x45-0x49). LIKELY that the grammar layers are: logical, comparison, additive, unary, multiplicative, consistent with ordinary expression grammars. This is a hint, not a confirmation.
- Whether `and` and `or` share a level, and whether they are left or right associative: UNSURE.
- Whether `not` binds tighter than comparisons: UNSURE.

## 3. Short-circuit evaluation of and / or

- The parser builds NodeAnd and NodeOr with two child pointers and no special handling at parse time. CONFIRMED.
- The code emitted for them is in the virtual methods of NodeAnd and NodeOr, whose vtables are data and not visible here. UNSURE whether the emitted code uses conditional jumps (short-circuit) or evaluates both sides.

## 4. Declarations, ordering and redeclaration

- Symbols live in a chained hash with 1024 buckets (type name TAllocatorHashTable<Symbol, HASHKEY_CONSTSTR, 1024>). CONFIRMED.
- Insert (0x6F458440) probes the table, and an insert with an existing key moves the existing node in the chain instead of creating a second entry (LIKELY). The table grows through a resize routine (0x6F457FE0). CONFIRMED (resize call), LIKELY (behaviour).
- Growth stops at a size threshold of 0x1FFE (8190) entries; beyond that the probe returns 0 (LIKELY meaning: no growth past that count).
- Use before declaration: the lookup in the lexer (rule 0x29) creates a symbol when a name is not yet known, so an identifier is always accepted by the lexer. Whether the semantic phase then rejects a call to a function declared later: UNSURE (no message seen).
- Globals before declaration: UNSURE.
- Redeclaration: no error string is visible in this region. UNSURE.
- The keyword `function` is needed to make a function known to the compiler (node class NodeFunction exists), but the rule that enforces it is UNSURE.

## 5. Limits

| Limit | Value | Source | Confidence |
|---|---|---|---|
| Parser state-stack capacity | 600 entries | 0x6F460790(600) call in 0x6F458510; overflow check in 0x6F464460 | CONFIRMED |
| Token/input buffer | 2100 bytes (0x834 + 1 per buffer) | 0x6F458870 call; 0x6F460570 allocation | CONFIRMED |
| Symbol hash buckets | 1024 | type name | CONFIRMED |
| Symbol table growth stop | 0x1FFE (8190) entries | 0x6F458320 | LIKELY |
| Identifier length | no separate limit visible; bounded by the 2100-byte token buffer | | LIKELY |
| String length | same as identifier | | LIKELY |
| Maximum parameters per function | no visible check; a parameter counter is kept in the signature parser (0x6F455110) | | UNSURE (no cap seen) |
| Array size limit 8192 | not found in this region | | UNSURE (not present in these functions) |
| Literal overflow | none (wraps) | | CONFIRMED |

## 6. Error handling

### 6.1 Result of a parse

- The engine (0x6F464460) returns false (0) on success only if the accept flag is set; returns true (failure) on stack overflow, on lexer error -2, and on a rejected accept. CONFIRMED.
- The compile driver (0x6F458510) returns the success path when the engine returns false, and otherwise clears the result and frees the scope. CONFIRMED.

### 6.2 Error callbacks

The engine reports semantic and syntax problems through a virtual method on the compile context (vtable slot +8), with a numeric error code. Codes seen: 0x1B, 0x20, 0x18, 0x1A, 0x2F, 0x30, 0x02, and one variable code. The mapping from code to message text is not in this region, so messages are UNSURE. CONFIRMED (callback and codes), UNSURE (texts).

### 6.3 Message strings that are present

| String | Where | Meaning | Confidence |
|---|---|---|---|
| `function '%s' has bad signature` | 0x6F455110 (Jass.cpp) | a function's signature descriptor is malformed (end of descriptor reached without the closing `)`) | CONFIRMED |
| `.\Compile.cpp` line 129 (0x81) | 0x6F458510 | assertion on parse result | CONFIRMED |
| `.\Compile.cpp` line 143 (0x8F) | 0x6F458900 | allocation check | CONFIRMED |
| `.\Compile.cpp` line 215 (0xD7) | 0x6F4572D0 area | allocation check | CONFIRMED |
| `.\scanner.cpp` lines 0x194-0x196, 0x1AE-0x1AF | 0x6F460570, 0x6F460530 | allocation and free checks | CONFIRMED |
| `.\parser.cpp` lines 0x3C4-0x3C5, 0x3CB-0x3CC | 0x6F460790, 0x6F4607F0 | allocation and free checks | CONFIRMED |
| `.\Jass.cpp` line 0x186 | 0x6F454FF0 | allocation of a table of 4-byte slots | CONFIRMED |

The lists of syntax error texts (for example "Syntax error", "expected", "undeclared") are NOT present in this region of the decompile. Their absence is CONFIRMED for this region, and their location is UNSURE.

### 6.4 Printer keyword spellings (for reference)

Keywords the AST printer emits, all confirmed as strings in the decompile: `function`, `takes`, `returns`, `nothing`, `endfunction`, `constant`, `native`, `globals`, `endglobals`, `type`, `extends`, `call`, `then`, `endif`, `endloop`, `exitwhen`, `return`, `array`, `false`, `code`, `integer`, `real`, `string`, `boolean`, `handle`, `null` (type), `set` (also present in the index, UNSURE which rule emits it), `local` (present). Not found as literal strings in the index: `elseif`, `else`, `loop` (standalone), `if`, `true`, `null` (keyword form). The keyword-to-token mapping for the 0x101-0x12D range is UNSURE.

## 7. Open items

1. The flex DFA tables and the yacc action/goto/precedence tables are required to pin down: token spellings, identifier character classes, comment rules, the precedence levels, associativity, and short-circuit codegen. These need the original binary or the table data (addresses DAT_6FA80320-series and DAT_6FA822A8-series), which are not in the workspace.
2. Confirm the 0x130-0x133 identifier classes against the symbol-kind enum.
3. Confirm whether `%` reaches NodeMod in the surface grammar.
4. Confirm the NodeAnd / NodeOr virtual codegen (conditional jumps or not) from the vtable.
