//! Bytecode parity vectors: instruction streams produced by the ORIGINAL JASS compiler
//! (Game.dll 1.26a, run in emulation with `tools/emu/jasscomp`). Each vector is the
//! source text and the 32-bit words of the stream (`w0 w1` per instruction).
//! Regenerate with `tools/emu/jasscomp/gen_vectors.py`. Vectors with real literals
//! need the soft-float tables: they run only when GAME_DLL is set.

use jass::compile::compile;

struct Vector {
    name: &'static str,
    src: &'static str,
    /// Words of the original's stream, two per instruction.
    words: &'static [u32],
}

const VECTORS: &[Vector] = &[
    Vector {
        name: "00_empty",
        src: r#"function f takes nothing returns nothing
endfunction
"#,
        words: &[
            50331648, 2, 201326592, 0, 654311424, 0, 67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "01_int_add",
        src: r#"function f takes nothing returns nothing
local integer a = 1 + 2
endfunction
"#,
        words: &[
            50331648, 2, 84148224, 3, 201393152, 1, 318832640, 0, 201458688, 2, 335740928, 0,
            537068290, 0, 285409280, 3, 201326592, 0, 654311424, 0, 67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "02_int_prec",
        src: r#"function f takes nothing returns nothing
local integer a = 1 + 2 * 3
endfunction
"#,
        words: &[
            50331648, 2, 84148224, 3, 201393152, 1, 318832640, 0, 201458688, 2, 318898176, 0,
            201524224, 3, 335806464, 0, 570688515, 0, 335872000, 0, 537199876, 0, 285540352, 3,
            201326592, 0, 654311424, 0, 67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "05_neg_literal",
        src: r#"function f takes nothing returns nothing
local integer a = -5
endfunction
"#,
        words: &[
            50331648, 2, 84148224, 3, 201393152, 5, 620822528, 0, 285278208, 3, 201326592, 0,
            654311424, 0, 67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "08_lt",
        src: r#"function f takes nothing returns nothing
local boolean a = 1 < 2
endfunction
"#,
        words: &[
            50331648, 2, 84410368, 3, 201393152, 1, 318832640, 0, 201458688, 2, 335740928, 0,
            503513858, 0, 285409280, 3, 201326592, 0, 654311424, 0, 67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "10_and_or",
        src: r#"function f takes nothing returns nothing
local boolean a = true and false
local boolean b = true or false
endfunction
"#,
        words: &[
            50331648, 2, 84410368, 3, 201394176, 1, 704708608, 1, 201459712, 0, 721420288, 2,
            671088640, 1, 201459712, 0, 671088640, 2, 285343744, 3, 84410368, 4, 201525248, 1,
            688062464, 3, 201590784, 0, 721420288, 4, 671088640, 3, 201590784, 1, 671088640, 4,
            285474816, 4, 201326592, 0, 654311424, 0, 67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "11_not",
        src: r#"function f takes nothing returns nothing
local boolean a = not true
endfunction
"#,
        words: &[
            50331648, 2, 84410368, 3, 201394176, 1, 637599744, 0, 285278208, 3, 201326592, 0,
            654311424, 0, 67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "12_if",
        src: r#"function f takes nothing returns nothing
local integer a = 1
if a == 1 then
set a = 2
endif
endfunction
"#,
        words: &[
            50331648, 2, 84148224, 3, 201393152, 1, 285278208, 3, 235013120, 3, 318898176, 0,
            201524224, 1, 335806464, 0, 436470787, 0, 704905216, 1, 201655296, 2, 285540352, 3,
            721420288, 2, 671088640, 1, 671088640, 2, 201326592, 0, 654311424, 0, 67108864, 0,
            16777216, 0,
        ],
    },
    Vector {
        name: "13_if_elseif_else",
        src: r#"function f takes nothing returns nothing
local integer a = 1
if a == 1 then
set a = 2
elseif a == 3 then
set a = 4
else
set a = 5
endif
endfunction
"#,
        words: &[
            50331648, 2, 84148224, 3, 201393152, 1, 285278208, 3, 235013120, 3, 318898176, 0,
            201524224, 1, 335806464, 0, 436470787, 0, 704905216, 1, 201655296, 2, 285540352, 3,
            721420288, 2, 671088640, 1, 235275264, 3, 319160320, 0, 201786368, 3, 336068608, 0,
            436733959, 0, 705167360, 3, 201917440, 4, 285802496, 3, 721420288, 4, 671088640, 3,
            201982976, 5, 285868032, 3, 671088640, 4, 671088640, 2, 201326592, 0, 654311424, 0,
            67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "14_loop_exitwhen",
        src: r#"function f takes nothing returns nothing
local integer i = 0
loop
set i = i + 1
exitwhen i == 3
endloop
endfunction
"#,
        words: &[
            50331648, 2, 84148224, 3, 201393152, 0, 285278208, 3, 671088640, 1, 235013120, 3,
            318898176, 0, 201524224, 1, 335806464, 0, 537134083, 0, 285474816, 3, 235209728, 3,
            319094784, 0, 201720832, 3, 336003072, 0, 436668166, 0, 688324608, 2, 721420288, 1,
            671088640, 2, 201326592, 0, 654311424, 0, 67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "15_array_store",
        src: r#"function f takes nothing returns nothing
local integer array arr
set arr[2] = 5
endfunction
"#,
        words: &[
            50331648, 2, 84475904, 3, 201393152, 2, 318832640, 0, 201458688, 5, 335740928, 0,
            302187008, 3, 201326592, 0, 654311424, 0, 67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "16_globals_store",
        src: r#"globals
integer g = 3
endglobals
function f takes nothing returns nothing
set g = 4
endfunction
"#,
        words: &[
            50331648, 4, 100925440, 2, 201393152, 3, 285278208, 2, 654311424, 0, 67108864, 0,
            50331648, 3, 201458688, 4, 285343744, 2, 201326592, 0, 654311424, 0, 67108864, 0,
            16777216, 0,
        ],
    },
    Vector {
        name: "17_call_args",
        src: r#"function g takes integer x, integer y returns nothing
endfunction
function f takes nothing returns nothing
call g(1, 2)
endfunction
"#,
        words: &[
            50331648, 2, 134480384, 3, 134480128, 4, 201326592, 0, 654311424, 0, 67108864, 0,
            50331648, 5, 201393152, 1, 318832640, 0, 201458688, 2, 318898176, 0, 369098752, 2,
            184680448, 0, 201326592, 0, 654311424, 0, 67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "18_return_literal",
        src: r#"function r takes nothing returns integer
return 7
endfunction
"#,
        words: &[
            50593792, 2, 201393152, 7, 218104064, 0, 654311424, 0, 201326592, 0, 654311424, 0,
            67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "19_string_lit",
        src: r#"function f takes nothing returns nothing
local string s = "hi"
endfunction
"#,
        words: &[
            50331648, 2, 84279296, 3, 201393664, 4, 285278208, 3, 201326592, 0, 654311424, 0,
            67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "20_funcref",
        src: r#"function g takes nothing returns nothing
endfunction
function f takes nothing returns nothing
local code c = function g
endfunction
"#,
        words: &[
            50331648, 2, 201326592, 0, 654311424, 0, 67108864, 0, 50331648, 3, 84082688, 4,
            251724544, 2, 285278208, 4, 201326592, 0, 654311424, 0, 67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "21_call_value",
        src: r#"function g takes integer x returns integer
return x
endfunction
function f takes nothing returns nothing
local integer r = g(3)
endfunction
"#,
        words: &[
            50593792, 2, 134480128, 3, 234947584, 3, 218104064, 0, 654311424, 0, 201326592, 0,
            654311424, 0, 67108864, 0, 50331648, 4, 84148224, 5, 201458688, 3, 318898176, 0,
            369098752, 2, 184614912, 0, 285212672, 5, 201326592, 0, 654311424, 0, 67108864, 0,
            16777216, 0,
        ],
    },
    Vector {
        name: "24_string_concat",
        src: r#"function f takes nothing returns nothing
local string s = "a" + "b"
endfunction
"#,
        words: &[
            50331648, 2, 84279296, 3, 201393664, 4, 318832640, 0, 201459200, 5, 335740928, 0,
            537068290, 0, 285409280, 3, 201326592, 0, 654311424, 0, 67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "29_array_read",
        src: r#"function f takes nothing returns nothing
local integer array arr
local integer x = arr[1]
endfunction
"#,
        words: &[
            50331648, 2, 84475904, 3, 84148224, 4, 201393152, 1, 268566788, 3, 285343744, 4,
            201326592, 0, 654311424, 0, 67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "35_params_return",
        src: r#"function add takes integer a, integer b returns integer
return a + b
endfunction
"#,
        words: &[
            50593792, 2, 134480384, 3, 134480128, 4, 234947584, 3, 318832640, 0, 235013120, 4,
            335740928, 0, 537068290, 0, 218104576, 0, 654311424, 0, 201326592, 0, 654311424, 0,
            67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "36_return_nothing",
        src: r#"function r takes nothing returns nothing
return
endfunction
"#,
        words: &[
            50331648, 2, 654311424, 0, 201326592, 0, 654311424, 0, 67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "38_call_noargs",
        src: r#"function g takes nothing returns nothing
endfunction
function f takes nothing returns nothing
call g()
endfunction
"#,
        words: &[
            50331648, 2, 201326592, 0, 654311424, 0, 67108864, 0, 50331648, 3, 369098752, 2,
            201326592, 0, 654311424, 0, 67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "39_call_in_expr",
        src: r#"function g takes nothing returns integer
return 1
endfunction
function f takes nothing returns nothing
local integer r = g() + 2
endfunction
"#,
        words: &[
            50593792, 2, 201393152, 1, 218104064, 0, 654311424, 0, 201326592, 0, 654311424, 0,
            67108864, 0, 50331648, 3, 84148224, 4, 369098752, 2, 318767104, 0, 201458688, 2,
            335740928, 0, 537068290, 0, 285409280, 4, 201326592, 0, 654311424, 0, 67108864, 0,
            16777216, 0,
        ],
    },
    Vector {
        name: "40_if_call",
        src: r#"function N takes nothing returns nothing
endfunction
function f takes nothing returns nothing
local boolean b = true
if b then
call N()
endif
endfunction
"#,
        words: &[
            50331648, 2, 201326592, 0, 654311424, 0, 67108864, 0, 50331648, 3, 84410368, 4,
            201394176, 1, 285278208, 4, 235014144, 4, 704774144, 1, 369098752, 2, 721420288, 2,
            671088640, 1, 671088640, 2, 201326592, 0, 654311424, 0, 67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "41_string_dedup",
        src: r#"function f takes nothing returns nothing
local string s = "hi"
local string t = "yo"
local string u = "hi"
endfunction
"#,
        words: &[
            50331648, 2, 84279296, 3, 201393664, 4, 285278208, 3, 84279296, 5, 201459200, 6,
            285343744, 5, 84279296, 7, 201524736, 4, 285409280, 7, 201326592, 0, 654311424, 0,
            67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "47_labels_two_fns",
        src: r#"function f takes nothing returns nothing
local integer a = 1
if a == 1 then
set a = 2
endif
endfunction
function g takes nothing returns nothing
local integer a = 1
if a == 1 then
set a = 2
endif
endfunction
"#,
        words: &[
            50331648, 2, 84148224, 3, 201393152, 1, 285278208, 3, 235013120, 3, 318898176, 0,
            201524224, 1, 335806464, 0, 436470787, 0, 704905216, 1, 201655296, 2, 285540352, 3,
            721420288, 2, 671088640, 1, 671088640, 2, 201326592, 0, 654311424, 0, 67108864, 0,
            50331648, 4, 84148224, 3, 201720832, 1, 285605888, 3, 235340800, 3, 319225856, 0,
            201851904, 1, 336134144, 0, 436799752, 0, 705232896, 3, 201982976, 2, 285868032, 3,
            721420288, 4, 671088640, 3, 671088640, 4, 201326592, 0, 654311424, 0, 67108864, 0,
            16777216, 0,
        ],
    },
    Vector {
        name: "debug_block",
        src: r#"function f takes nothing returns nothing
local integer a = 1
debug if a == 1 then
set a = 2
endif
endfunction
"#,
        words: &[
            50331648, 2, 84148224, 3, 201393152, 1, 285278208, 3, 33554432, 1, 235013120, 3,
            318898176, 0, 201524224, 1, 335806464, 0, 436470787, 0, 704905216, 2, 201655296, 2,
            285540352, 3, 721420288, 3, 671088640, 2, 671088640, 3, 671088640, 1, 201326592, 0,
            654311424, 0, 67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "nested_loop",
        src: r#"function f takes nothing returns nothing
loop
loop
exitwhen true
endloop
exitwhen true
endloop
endfunction
"#,
        words: &[
            50331648, 2, 671088640, 1, 671088640, 3, 201394176, 1, 687931392, 4, 721420288, 3,
            671088640, 4, 201459712, 1, 687996928, 2, 721420288, 1, 671088640, 2, 201326592, 0,
            654311424, 0, 67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "and_vars",
        src: r#"function f takes nothing returns nothing
local boolean a = true
local boolean b = false
local boolean c = a and b
endfunction
"#,
        words: &[
            50331648, 2, 84410368, 3, 201394176, 1, 285278208, 3, 84410368, 4, 201459712, 0,
            285343744, 4, 84410368, 5, 235079680, 3, 704839680, 1, 235145216, 4, 721420288, 2,
            671088640, 1, 201590784, 0, 671088640, 2, 285474816, 5, 201326592, 0, 654311424, 0,
            67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "if_and_or_nested",
        src: r#"function f takes nothing returns nothing
local boolean a = true
local boolean b = true
local boolean c = true
local integer x = 0
if a and (b or c) then
set x = 1
endif
endfunction
"#,
        words: &[
            50331648, 2, 84410368, 3, 201394176, 1, 285278208, 3, 84410368, 4, 201459712, 1,
            285343744, 4, 84410368, 5, 201525248, 1, 285409280, 5, 84148224, 6, 201589760, 0,
            285474816, 6, 235210752, 3, 704970752, 1, 235276288, 4, 688259072, 2, 235341824, 5,
            721420288, 3, 671088640, 2, 201787392, 1, 671088640, 3, 721420288, 4, 671088640, 1,
            201787392, 0, 671088640, 4, 705101824, 5, 201851904, 1, 285736960, 6, 721420288, 6,
            671088640, 5, 671088640, 6, 201326592, 0, 654311424, 0, 67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "wrap300",
        src: r#"function f takes nothing returns nothing
local integer a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
set a = 1
endfunction
"#,
        words: &[
            50331648, 2, 84148224, 3, 201393152, 1, 285278208, 3, 201458688, 1, 285343744, 3,
            201524224, 1, 285409280, 3, 201589760, 1, 285474816, 3, 201655296, 1, 285540352, 3,
            201720832, 1, 285605888, 3, 201786368, 1, 285671424, 3, 201851904, 1, 285736960, 3,
            201917440, 1, 285802496, 3, 201982976, 1, 285868032, 3, 202048512, 1, 285933568, 3,
            202114048, 1, 285999104, 3, 202179584, 1, 286064640, 3, 202245120, 1, 286130176, 3,
            202310656, 1, 286195712, 3, 202376192, 1, 286261248, 3, 202441728, 1, 286326784, 3,
            202507264, 1, 286392320, 3, 202572800, 1, 286457856, 3, 202638336, 1, 286523392, 3,
            202703872, 1, 286588928, 3, 202769408, 1, 286654464, 3, 202834944, 1, 286720000, 3,
            202900480, 1, 286785536, 3, 202966016, 1, 286851072, 3, 203031552, 1, 286916608, 3,
            203097088, 1, 286982144, 3, 203162624, 1, 287047680, 3, 203228160, 1, 287113216, 3,
            203293696, 1, 287178752, 3, 203359232, 1, 287244288, 3, 203424768, 1, 287309824, 3,
            203490304, 1, 287375360, 3, 203555840, 1, 287440896, 3, 203621376, 1, 287506432, 3,
            203686912, 1, 287571968, 3, 203752448, 1, 287637504, 3, 203817984, 1, 287703040, 3,
            203883520, 1, 287768576, 3, 203949056, 1, 287834112, 3, 204014592, 1, 287899648, 3,
            204080128, 1, 287965184, 3, 204145664, 1, 288030720, 3, 204211200, 1, 288096256, 3,
            204276736, 1, 288161792, 3, 204342272, 1, 288227328, 3, 204407808, 1, 288292864, 3,
            204473344, 1, 288358400, 3, 204538880, 1, 288423936, 3, 204604416, 1, 288489472, 3,
            204669952, 1, 288555008, 3, 204735488, 1, 288620544, 3, 204801024, 1, 288686080, 3,
            204866560, 1, 288751616, 3, 204932096, 1, 288817152, 3, 204997632, 1, 288882688, 3,
            205063168, 1, 288948224, 3, 205128704, 1, 289013760, 3, 205194240, 1, 289079296, 3,
            205259776, 1, 289144832, 3, 205325312, 1, 289210368, 3, 205390848, 1, 289275904, 3,
            205456384, 1, 289341440, 3, 205521920, 1, 289406976, 3, 205587456, 1, 289472512, 3,
            205652992, 1, 289538048, 3, 205718528, 1, 289603584, 3, 205784064, 1, 289669120, 3,
            205849600, 1, 289734656, 3, 205915136, 1, 289800192, 3, 205980672, 1, 289865728, 3,
            206046208, 1, 289931264, 3, 206111744, 1, 289996800, 3, 206177280, 1, 290062336, 3,
            206242816, 1, 290127872, 3, 206308352, 1, 290193408, 3, 206373888, 1, 290258944, 3,
            206439424, 1, 290324480, 3, 206504960, 1, 290390016, 3, 206570496, 1, 290455552, 3,
            206636032, 1, 290521088, 3, 206701568, 1, 290586624, 3, 206767104, 1, 290652160, 3,
            206832640, 1, 290717696, 3, 206898176, 1, 290783232, 3, 206963712, 1, 290848768, 3,
            207029248, 1, 290914304, 3, 207094784, 1, 290979840, 3, 207160320, 1, 291045376, 3,
            207225856, 1, 291110912, 3, 207291392, 1, 291176448, 3, 207356928, 1, 291241984, 3,
            207422464, 1, 291307520, 3, 207488000, 1, 291373056, 3, 207553536, 1, 291438592, 3,
            207619072, 1, 291504128, 3, 207684608, 1, 291569664, 3, 207750144, 1, 291635200, 3,
            207815680, 1, 291700736, 3, 207881216, 1, 291766272, 3, 207946752, 1, 291831808, 3,
            208012288, 1, 291897344, 3, 208077824, 1, 291962880, 3, 208143360, 1, 292028416, 3,
            208208896, 1, 292093952, 3, 208274432, 1, 292159488, 3, 208339968, 1, 292225024, 3,
            208405504, 1, 292290560, 3, 208471040, 1, 292356096, 3, 208536576, 1, 292421632, 3,
            208602112, 1, 292487168, 3, 208667648, 1, 292552704, 3, 208733184, 1, 292618240, 3,
            208798720, 1, 292683776, 3, 208864256, 1, 292749312, 3, 208929792, 1, 292814848, 3,
            208995328, 1, 292880384, 3, 209060864, 1, 292945920, 3, 209126400, 1, 293011456, 3,
            209191936, 1, 293076992, 3, 209257472, 1, 293142528, 3, 209323008, 1, 293208064, 3,
            209388544, 1, 293273600, 3, 209454080, 1, 293339136, 3, 209519616, 1, 293404672, 3,
            209585152, 1, 293470208, 3, 209650688, 1, 293535744, 3, 209716224, 1, 293601280, 3,
            209781760, 1, 293666816, 3, 209847296, 1, 293732352, 3, 209912832, 1, 293797888, 3,
            209978368, 1, 293863424, 3, 210043904, 1, 293928960, 3, 210109440, 1, 293994496, 3,
            210174976, 1, 294060032, 3, 210240512, 1, 294125568, 3, 210306048, 1, 294191104, 3,
            210371584, 1, 294256640, 3, 210437120, 1, 294322176, 3, 210502656, 1, 294387712, 3,
            210568192, 1, 294453248, 3, 210633728, 1, 294518784, 3, 210699264, 1, 294584320, 3,
            210764800, 1, 294649856, 3, 210830336, 1, 294715392, 3, 210895872, 1, 294780928, 3,
            210961408, 1, 294846464, 3, 211026944, 1, 294912000, 3, 211092480, 1, 294977536, 3,
            211158016, 1, 295043072, 3, 211223552, 1, 295108608, 3, 211289088, 1, 295174144, 3,
            211354624, 1, 295239680, 3, 211420160, 1, 295305216, 3, 211485696, 1, 295370752, 3,
            211551232, 1, 295436288, 3, 211616768, 1, 295501824, 3, 211682304, 1, 295567360, 3,
            211747840, 1, 295632896, 3, 211813376, 1, 295698432, 3, 211878912, 1, 295763968, 3,
            211944448, 1, 295829504, 3, 212009984, 1, 295895040, 3, 212075520, 1, 295960576, 3,
            212141056, 1, 296026112, 3, 212206592, 1, 296091648, 3, 212272128, 1, 296157184, 3,
            212337664, 1, 296222720, 3, 212403200, 1, 296288256, 3, 212468736, 1, 296353792, 3,
            212534272, 1, 296419328, 3, 212599808, 1, 296484864, 3, 212665344, 1, 296550400, 3,
            212730880, 1, 296615936, 3, 212796416, 1, 296681472, 3, 212861952, 1, 296747008, 3,
            212927488, 1, 296812544, 3, 212993024, 1, 296878080, 3, 213058560, 1, 296943616, 3,
            213124096, 1, 297009152, 3, 213189632, 1, 297074688, 3, 213255168, 1, 297140224, 3,
            213320704, 1, 297205760, 3, 213386240, 1, 297271296, 3, 213451776, 1, 297336832, 3,
            213517312, 1, 297402368, 3, 213582848, 1, 297467904, 3, 213648384, 1, 297533440, 3,
            213713920, 1, 297598976, 3, 213779456, 1, 297664512, 3, 213844992, 1, 297730048, 3,
            213910528, 1, 297795584, 3, 213976064, 1, 297861120, 3, 214041600, 1, 297926656, 3,
            214107136, 1, 297992192, 3, 214172672, 1, 298057728, 3, 214238208, 1, 298123264, 3,
            214303744, 1, 298188800, 3, 214369280, 1, 298254336, 3, 214434816, 1, 298319872, 3,
            214500352, 1, 298385408, 3, 214565888, 1, 298450944, 3, 214631424, 1, 298516480, 3,
            214696960, 1, 298582016, 3, 214762496, 1, 298647552, 3, 214828032, 1, 298713088, 3,
            214893568, 1, 298778624, 3, 214959104, 1, 298844160, 3, 215024640, 1, 298909696, 3,
            215090176, 1, 298975232, 3, 215155712, 1, 299040768, 3, 215221248, 1, 299106304, 3,
            215286784, 1, 299171840, 3, 215352320, 1, 299237376, 3, 215417856, 1, 299302912, 3,
            215483392, 1, 299368448, 3, 215548928, 1, 299433984, 3, 215614464, 1, 299499520, 3,
            215680000, 1, 299565056, 3, 215745536, 1, 299630592, 3, 215811072, 1, 299696128, 3,
            215876608, 1, 299761664, 3, 215942144, 1, 299827200, 3, 216007680, 1, 299892736, 3,
            216073216, 1, 299958272, 3, 216138752, 1, 300023808, 3, 216204288, 1, 300089344, 3,
            216269824, 1, 300154880, 3, 216335360, 1, 300220416, 3, 216400896, 1, 300285952, 3,
            216466432, 1, 300351488, 3, 216531968, 1, 300417024, 3, 216597504, 1, 300482560, 3,
            201326592, 0, 654311424, 0, 67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "and_calls",
        src: r#"function g takes nothing returns boolean
return true
endfunction
function f takes nothing returns nothing
local boolean x = g() and g()
endfunction
"#,
        words: &[
            50855936, 2, 201394176, 1, 218104064, 0, 654311424, 0, 201326592, 0, 654311424, 0,
            67108864, 0, 50331648, 3, 84410368, 4, 369098752, 2, 704643072, 1, 369098752, 2,
            721420288, 2, 671088640, 1, 201328640, 0, 671088640, 2, 285212672, 4, 201326592, 0,
            654311424, 0, 67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "if_empty_then_elseif",
        src: r#"function f takes nothing returns nothing
local boolean a = true
local integer x = 0
if a then
elseif a then
set x = 1
endif
endfunction
"#,
        words: &[
            50331648, 2, 84410368, 3, 201394176, 1, 285278208, 3, 84148224, 4, 201458688, 0,
            285343744, 4, 235079680, 3, 704839680, 1, 721420288, 2, 671088640, 1, 235145216, 3,
            704905216, 3, 201655296, 1, 285540352, 4, 721420288, 4, 671088640, 3, 671088640, 4,
            671088640, 2, 201326592, 0, 654311424, 0, 67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "int_to_real_add",
        src: r#"function f takes nothing returns nothing
local real r = 1 + 2.5
endfunction
"#,
        words: &[
            50331648, 2, 84213760, 3, 201393152, 1, 385941504, 0, 318832640, 0, 201458944,
            1075838976, 335740928, 0, 537068290, 0, 285409280, 3, 201326592, 0, 654311424, 0,
            67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "call_nested_args",
        src: r#"function g takes integer x returns integer
return x
endfunction
function f takes nothing returns nothing
local integer r = g(g(1))
endfunction
"#,
        words: &[
            50593792, 2, 134480128, 3, 234947584, 3, 218104064, 0, 654311424, 0, 201326592, 0,
            654311424, 0, 67108864, 0, 50331648, 4, 84148224, 5, 201458688, 1, 318898176, 0,
            369098752, 2, 184614912, 0, 318767104, 0, 369098752, 2, 184614912, 0, 285212672, 5,
            201326592, 0, 654311424, 0, 67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "globals_string_init",
        src: r#"globals
string s = "Q"
endglobals
function f takes nothing returns nothing
local string t = "Q"
endfunction
"#,
        words: &[
            50331648, 6, 101056512, 2, 201393664, 3, 285278208, 2, 654311424, 0, 67108864, 0,
            50331648, 4, 84279296, 5, 201459200, 3, 285343744, 5, 201326592, 0, 654311424, 0,
            67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "type_two",
        src: r#"type foo extends handle
type bar extends foo
function f takes nothing returns nothing
local bar x
endfunction
"#,
        words: &[
            150994944, 6, 167772160, 2, 150994944, 2, 167772160, 3, 50331648, 4, 84344832, 5,
            201326592, 0, 654311424, 0, 67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "ret_codes_bool",
        src: r#"function r takes nothing returns boolean
return true
endfunction
function s takes nothing returns string
return "x"
endfunction
function c takes nothing returns code
return null
endfunction
"#,
        words: &[
            50855936, 2, 201394176, 1, 218104064, 0, 654311424, 0, 201326592, 0, 654311424, 0,
            67108864, 0, 50724864, 3, 201459200, 4, 218104320, 0, 654311424, 0, 201326592, 0,
            654311424, 0, 67108864, 0, 50528256, 5, 201523712, 0, 218104576, 0, 654311424, 0,
            201326592, 0, 654311424, 0, 67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "empty_if",
        src: r#"function f takes nothing returns nothing
local boolean a = true
if a then
endif
endfunction
"#,
        words: &[
            50331648, 2, 84410368, 3, 201394176, 1, 285278208, 3, 235014144, 3, 201326592, 0,
            654311424, 0, 67108864, 0, 16777216, 0,
        ],
    },
    Vector {
        name: "param_only",
        src: r#"function N takes integer a, real b, handle c, string d, boolean e returns nothing
endfunction
"#,
        words: &[
            50331648, 2, 134481152, 3, 134546432, 4, 134677248, 5, 134611456, 6, 134742272, 7,
            201326592, 0, 654311424, 0, 67108864, 0, 16777216, 0,
        ],
    },
];

fn tables() -> Option<softfloat::Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).expect("reading GAME_DLL");
    Some(softfloat::Tables::from_game_dll(&bytes).expect("lookup tables in GAME_DLL"))
}

/// Whether a source has a real literal (`1.5`): those need the tables to get the original's bits.
fn has_real_literal(src: &str) -> bool {
    src.as_bytes()
        .windows(3)
        .any(|w| w[0].is_ascii_digit() && w[1] == b'.' && w[2].is_ascii_digit())
}

#[test]
fn streams_match_the_original_compiler() {
    let tables = tables();
    let mut checked = 0;
    for v in VECTORS {
        if has_real_literal(v.src) && tables.is_none() {
            continue;
        }
        let prog = jass::parse_with(v.src, tables.as_ref())
            .unwrap_or_else(|e| panic!("{}: parse failed: {e}", v.name));
        let m = compile(&[prog]).unwrap_or_else(|e| panic!("{}: compile failed: {e}", v.name));
        let mut words = Vec::new();
        for ins in m.stream() {
            let b = ins.to_bytes();
            words.push(u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
            words.push(u32::from_le_bytes([b[4], b[5], b[6], b[7]]));
        }
        assert_eq!(
            words, v.words,
            "{}: stream differs from the original",
            v.name
        );
        checked += 1;
    }
    assert!(checked > 0, "no vector was checked");
}
