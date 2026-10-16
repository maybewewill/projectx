//! The ability codes the simulation registers, gathered from the module registries (no hand list), so the host can load
//! the level records of every one of them from `AbilityData.slk`.

use std::collections::BTreeSet;

/// Every ability code some module of the simulation registers: the order table of the cast system (all spell modules),
/// the unit ability batches A and M, the creep alias parents, the detector and permanent-invisibility abilities and the
/// morph abilities.
pub fn registered_codes() -> BTreeSet<u32> {
    let mut set = BTreeSet::new();
    set.extend(crate::cast::SpellSystem::new().order_bases());
    set.extend(crate::unit_abilities_a::ABILITIES.iter().map(|i| i.code));
    set.extend(crate::unit_abilities_a::MORPH_ORDERS.iter().map(|m| m.0));
    set.extend(crate::unit_abilities_m::CODES.iter().map(|c| u32::from_be_bytes(*c.0)));
    set.extend(crate::neutral_spells::MODELLED.iter().map(|c| u32::from_be_bytes(*c)));
    for (derived, parent, _) in crate::neutral_spells::STATIC_PARENTS {
        set.insert(derived);
        set.insert(parent);
    }
    set.extend(crate::invisibility::DETECTORS);
    set.extend(crate::invisibility::PERMANENT);
    set.insert(crate::abilities::EVASION);
    set
}

/// The morph abilities as `(ability, primary order id, alternate order id)`.
pub fn morph_abilities() -> &'static [(u32, u32, u32)] {
    &crate::unit_abilities_a::MORPH_ORDERS
}
