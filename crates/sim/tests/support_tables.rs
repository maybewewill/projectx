//! The support tables of the modules added for the common work list (`pool`, `defeat`, `economy`, `object`,
//! `animation`, `item`, `order::GROUP_NATIVES` and `pending`): every row is bound exactly when it is not
//! `NotImplemented`, every name is a native of the 1.26a table (`docs/spec/natives-1.26a.tsv`), and no name appears
//! twice across these tables.

use std::collections::BTreeSet;

use jass::vm::NativeRegistry;
use sim::natives::unit_attr::{NativeEntry, Support};
use sim::natives::{self, item, order, pending, pool};

const SPEC: &str = include_str!("../../../docs/spec/natives-1.26a.tsv");

fn tables() -> Vec<(&'static str, &'static [NativeEntry])> {
    vec![
        ("pool", pool::NATIVES),
        ("defeat", natives::defeat::NATIVES),
        ("economy", natives::economy::NATIVES),
        ("object", natives::object::NATIVES),
        ("animation", natives::animation::NATIVES),
        ("item", item::NATIVES),
        ("group orders", order::GROUP_NATIVES),
        ("pending", pending::NATIVES),
        ("melee start", natives::melee_start::NATIVES),
        ("lane e", natives::lane_e::NATIVES),
        ("lane f", natives::lane_f::NATIVES),
        ("slots", natives::gamestate::SLOT_NATIVES),
        ("shop", natives::shop::NATIVES),
    ]
}

fn spec_names() -> BTreeSet<&'static str> {
    SPEC.lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .filter_map(|l| l.split('\t').next())
        .collect()
}

/// Declares every row as a native in a script and asks the VM which ones have no binding. The bindings are the
/// simulation's natives, which `ScriptHost::new` registers.
#[test]
fn rows_are_bound_exactly_when_they_are_implemented() {
    let mut decls = String::new();
    let mut expected_unbound = BTreeSet::new();
    for (_, rows) in tables() {
        for row in rows {
            decls.push_str(&format!(
                "native {} takes nothing returns nothing\n",
                row.name
            ));
            if matches!(row.support, Support::NotImplemented(_)) {
                expected_unbound.insert(row.name);
            }
        }
    }
    let src = format!("{decls}\nfunction Main takes nothing returns nothing\nendfunction\n");
    let prog = jass::parse(&src).expect("parse");
    let module = jass::compile::compile(&[prog]).expect("compile");
    let host = sim::ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    let unbound: BTreeSet<&str> = host.vm().unbound_natives().into_iter().collect();
    for (table, rows) in tables() {
        for row in rows {
            let implemented = !matches!(row.support, Support::NotImplemented(_));
            assert_eq!(
                !unbound.contains(row.name),
                implemented,
                "{table}: {} has support {:?}",
                row.name,
                row.support
            );
        }
    }
    assert_eq!(unbound, expected_unbound);
}

#[test]
fn every_row_is_a_native_of_the_spec_and_appears_once() {
    let spec = spec_names();
    let mut seen = BTreeSet::new();
    for (table, rows) in tables() {
        for row in rows {
            assert!(
                spec.contains(row.name),
                "{table}: {} is not in the 1.26a table",
                row.name
            );
            assert!(seen.insert(row.name), "{} appears in two tables", row.name);
        }
    }
}

#[test]
fn not_implemented_rows_give_a_reason() {
    for (table, rows) in tables() {
        for row in rows {
            if let Support::NotImplemented(reason) = row.support {
                assert!(!reason.is_empty(), "{table}: {} has no reason", row.name);
            }
        }
    }
}
