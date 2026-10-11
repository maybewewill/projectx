//! Defeat conditions and trackables: `CreateDefeatCondition`, `DefeatConditionSetDescription`,
//! `DestroyDefeatCondition` and `CreateTrackable`.
//!
//! Addresses are the 1.26a `Game.dll` functions in `docs/spec/natives-1.26a.tsv`. Labels as in `unit.rs`.
//!
//! | native | address | support | notes |
//! |---|---|---|---|
//! | CreateDefeatCondition | 0x6F3BC9E0 | LIKELY | the object is built by `FUN_6f3f6230`, its handle is allocated last with flag 0 |
//! | DefeatConditionSetDescription | 0x6F3CBE00 | CONFIRMED | the text is stored on a live condition; a null or destroyed one does nothing |
//! | DestroyDefeatCondition | 0x6F3CBDB0 | LIKELY | the condition leaves the game's list (`FUN_6f3f9520`) and its creation reference is released |
//! | CreateTrackable | 0x6F3D30B0 | LIKELY | built by `FUN_6f3dc690` / `FUN_6f3dc6e0`, the handle is allocated last with flag 0 |
//!
//! Not modelled: the victory and defeat checks that read the condition list, and the trackable events
//! (`TriggerRegisterTrackableHitEvent` and `TrackEvent` keep their null event in `event.rs`). The model path is stored
//! as given (UNSURE: the original may resolve it through the model table when the trackable is shown).

use std::cell::RefCell;
use std::rc::Rc;

use jass::vm::{NativeRegistry, NativeResult, Value};

use super::game::{DefeatObj, Game, HandleKind, ObjKey, Object, TrackableObj};
use super::hashtable::real_at;
use super::unit_attr::{NativeEntry, Support};

/// The support table of this module.
pub const NATIVES: &[NativeEntry] = &[
    NativeEntry {
        name: "CreateDefeatCondition",
        support: Support::Likely,
    },
    NativeEntry {
        name: "DefeatConditionSetDescription",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "DestroyDefeatCondition",
        support: Support::Likely,
    },
    NativeEntry {
        name: "CreateTrackable",
        support: Support::Likely,
    },
];

/// Registers the defeat condition and trackable natives on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("CreateDefeatCondition", move |_, _| {
        let (_, id) = g
            .borrow_mut()
            .add_object(Object::DefeatCondition(DefeatObj::default()), 0);
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("DefeatConditionSetDescription", move |ctx, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = condition_at(&gm, a) {
            if let Object::DefeatCondition(d) = gm.object_mut(k) {
                d.description = string_arg(ctx, a, 1);
            }
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("DestroyDefeatCondition", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = condition_at(&gm, a) {
            gm.kill(k);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("CreateTrackable", move |ctx, a| {
        let obj = Object::Trackable(TrackableObj {
            model: string_arg(ctx, a, 0),
            x: real_at(a, 1),
            y: real_at(a, 2),
            z: real_at(a, 3),
        });
        let (_, id) = g.borrow_mut().add_object(obj, 0);
        NativeResult::Return(Value::Handle(id))
    });
}

/// The defeat condition in handle argument 0, if it is a live one.
fn condition_at(gm: &Game, a: &[Value]) -> Option<ObjKey> {
    let id = a.first().and_then(|v| v.as_handle()).unwrap_or(0);
    gm.handle_of_kind(id, HandleKind::DefeatCondition)
}

/// The string in argument `i` (an empty string for a null or non-string value).
fn string_arg(ctx: &jass::vm::NativeCtx<'_>, a: &[Value], i: usize) -> String {
    match a.get(i) {
        Some(Value::String(id)) => ctx.string(*id).to_string(),
        _ => String::new(),
    }
}
