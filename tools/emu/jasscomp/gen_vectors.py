"""Writes crates/jass/tests/bytecode_parity.rs: original-compiler streams for representative snippets.

Run with the same environment as parity.py (GAME_DLL, PYTHONPATH with unicorn). The Rust test
compiles each source with our compiler and compares the instruction words.
"""
import os
import sys
import re

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import parity  # noqa: E402

NAMES = [
    "00_empty", "01_int_add", "02_int_prec", "05_neg_literal", "08_lt", "10_and_or", "11_not",
    "12_if", "13_if_elseif_else", "14_loop_exitwhen", "15_array_store", "16_globals_store",
    "17_call_args", "18_return_literal", "19_string_lit", "20_funcref", "21_call_value",
    "24_string_concat", "29_array_read", "35_params_return", "36_return_nothing", "38_call_noargs",
    "39_call_in_expr", "40_if_call", "41_string_dedup", "47_labels_two_fns", "47_labels_two_fns",
    "debug_block", "nested_loop", "and_vars", "if_and_or_nested", "wrap300", "and_calls",
    "if_empty_then_elseif", "int_to_real_add", "call_nested_args", "globals_string_init",
    "type_two", "ret_codes_bool", "empty_if", "param_only",
]


def main():
    cases = dict(parity.corpus())
    seen = set()
    vectors = []
    for name in NAMES:
        if name in seen:
            continue
        seen.add(name)
        src = cases[name]
        want = parity.original(src)
        if isinstance(want, tuple):
            continue
        words = [v for pair in want for v in pair]
        vectors.append((name, src, words))
    out = []
    out.append("//! Bytecode parity vectors: instruction streams produced by the ORIGINAL JASS compiler")
    out.append("//! (Game.dll 1.26a, run in emulation with `tools/emu/jasscomp`). Each vector is the")
    out.append("//! source text and the 32-bit words of the stream (`w0 w1` per instruction).")
    out.append("//! Regenerate with `tools/emu/jasscomp/gen_vectors.py`. Vectors with real literals")
    out.append("//! need the soft-float tables: they run only when GAME_DLL is set.")
    out.append("")
    out.append("use jass::compile::compile;")
    out.append("")
    out.append("struct Vector {")
    out.append("    name: &'static str,")
    out.append("    src: &'static str,")
    out.append("    /// Words of the original's stream, two per instruction.")
    out.append("    words: &'static [u32],")
    out.append("}")
    out.append("")
    out.append("const VECTORS: &[Vector] = &[")
    for name, src, words in vectors:
        out.append("    Vector {")
        out.append('        name: "%s",' % name)
        out.append('        src: r#"%s"#,' % src)
        out.append("        words: &[")
        for i in range(0, len(words), 8):
            out.append("            " + ", ".join(str(w) for w in words[i:i + 8]) + ",")
        out.append("        ],")
        out.append("    },")
    out.append("];")
    out.append("")
    out.append(TAIL)
    path = os.path.join(HERE, "..", "..", "..", "crates", "jass", "tests", "bytecode_parity.rs")
    with open(os.path.normpath(path), "w") as f:
        f.write("\n".join(out) + "\n")
    print("wrote %d vectors" % len(vectors))


TAIL = r'''fn tables() -> Option<softfloat::Tables> {
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
        assert_eq!(words, v.words, "{}: stream differs from the original", v.name);
        checked += 1;
    }
    assert!(checked > 0, "no vector was checked");
}
'''

if __name__ == "__main__":
    main()
