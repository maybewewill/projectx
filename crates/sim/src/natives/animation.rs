//! Animation and look natives, logged for a renderer: `QueueUnitAnimation`, `AddUnitAnimationProperties`,
//! `ResetUnitLookAt`, `SetDestructableAnimation`, `SetDestructableAnimationSpeed`, `QueueDestructableAnimation`,
//! `SetDoodadAnimation`, `SetDoodadAnimationRect` and `AddIndicator`.
//!
//! Addresses are the 1.26a `Game.dll` functions in `docs/spec/natives-1.26a.tsv`. Labels as in `unit.rs`.
//!
//! These natives change the visual state of an object only: an animation name, the animation properties of a unit, a
//! look-at target, a minimap indicator. No simulation state reads them, so the model records each call as a
//! [`PresentationEvent::Command`] for the renderer (`presentation.rs`), with the handle and the arguments as given.
//!
//! | native | address | support | notes |
//! |---|---|---|---|
//! | QueueUnitAnimation | 0x6F3C6180 | UNSURE | the unit must be live (the body's own checks are not all read); queue kind 2 |
//! | AddUnitAnimationProperties | 0x6F3C6220 | UNSURE | the property names are passed as text; the original splits them on spaces and adds or removes each (not split here) |
//! | ResetUnitLookAt | 0x6F3C65F0 | UNSURE | the unit must have its look-at object (the test is not read) |
//! | SetDestructableAnimation / QueueDestructableAnimation | 0x6F3C4A20 / 0x6F3C4A00 | UNSURE | the destructable must be live; queue kind 2 |
//! | SetDestructableAnimationSpeed | 0x6F3C4A40 | UNSURE | the destructable must be live |
//! | SetDoodadAnimation | 0x6F3B4570 | UNSURE | the doodad lookup is by position and id, the doodads are not modelled |
//! | SetDoodadAnimationRect | 0x6F3CD6F0 | UNSURE | the rect must be live |
//! | AddIndicator | 0x6F3C6C60 | UNSURE | a live unit widget only; the local-player test of the original is not read |

use std::cell::RefCell;
use std::rc::Rc;

use jass::vm::{NativeCtx, NativeRegistry, NativeResult, Value};

use super::game::{Game, HandleKind, ObjKey, Object};
use super::hashtable::{int_at, real_at};
use super::presentation::{PresentationEvent, PropValue};
use super::unit_attr::{NativeEntry, Support};

/// The support table of this module.
pub const NATIVES: &[NativeEntry] = &[
    NativeEntry {
        name: "QueueUnitAnimation",
        support: Support::Unsure,
    },
    NativeEntry {
        name: "AddUnitAnimationProperties",
        support: Support::Unsure,
    },
    NativeEntry {
        name: "ResetUnitLookAt",
        support: Support::Unsure,
    },
    NativeEntry {
        name: "SetDestructableAnimation",
        support: Support::Unsure,
    },
    NativeEntry {
        name: "QueueDestructableAnimation",
        support: Support::Unsure,
    },
    NativeEntry {
        name: "SetDestructableAnimationSpeed",
        support: Support::Unsure,
    },
    NativeEntry {
        name: "SetDoodadAnimation",
        support: Support::Unsure,
    },
    NativeEntry {
        name: "SetDoodadAnimationRect",
        support: Support::Unsure,
    },
    NativeEntry {
        name: "AddIndicator",
        support: Support::Unsure,
    },
];

/// Registers the animation natives on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("QueueUnitAnimation", move |ctx, a| {
        unit_command(&g, a, "QueueUnitAnimation", |_| {
            vec![PropValue::Text(string_arg(ctx, a, 1))]
        });
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("AddUnitAnimationProperties", move |ctx, a| {
        let text = string_arg(ctx, a, 1);
        let add = a.get(2).is_some_and(bool_of);
        unit_command(&g, a, "AddUnitAnimationProperties", |_| {
            vec![PropValue::Text(text), PropValue::Bool(add)]
        });
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("ResetUnitLookAt", move |_, a| {
        unit_command(&g, a, "ResetUnitLookAt", |_| Vec::new());
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("SetDestructableAnimation", move |ctx, a| {
        destructable_command(&g, a, "SetDestructableAnimation", |_| {
            vec![PropValue::Text(string_arg(ctx, a, 1))]
        });
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("QueueDestructableAnimation", move |ctx, a| {
        destructable_command(&g, a, "QueueDestructableAnimation", |_| {
            vec![PropValue::Text(string_arg(ctx, a, 1))]
        });
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("SetDestructableAnimationSpeed", move |_, a| {
        let speed = real_at(a, 1).to_f32();
        destructable_command(&g, a, "SetDestructableAnimationSpeed", |_| {
            vec![PropValue::Real(speed)]
        });
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("SetDoodadAnimation", move |ctx, a| {
        let args = vec![
            PropValue::Real(real_at(a, 0).to_f32()),
            PropValue::Real(real_at(a, 1).to_f32()),
            PropValue::Real(real_at(a, 2).to_f32()),
            PropValue::Int(int_at(a, 3)),
            PropValue::Bool(a.get(4).is_some_and(bool_of)),
            PropValue::Text(string_arg(ctx, a, 5)),
            PropValue::Bool(a.get(6).is_some_and(bool_of)),
        ];
        g.borrow_mut()
            .presentation_mut()
            .log
            .push(PresentationEvent::Command {
                name: "SetDoodadAnimation",
                args,
            });
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("SetDoodadAnimationRect", move |ctx, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = rect_at(&gm, a) {
            let rect = match gm.object(k) {
                Object::Rect(r) => [r.min_x, r.min_y, r.max_x, r.max_y],
                _ => unreachable!("rect_at checked the kind"),
            };
            let args = vec![
                PropValue::Reals(rect.iter().map(|v| v.to_f32()).collect()),
                PropValue::Int(int_at(a, 1)),
                PropValue::Text(string_arg(ctx, a, 2)),
                PropValue::Bool(a.get(3).is_some_and(bool_of)),
            ];
            gm.presentation_mut().log.push(PresentationEvent::Command {
                name: "SetDoodadAnimationRect",
                args,
            });
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("AddIndicator", move |_, a| {
        let mut gm = g.borrow_mut();
        let id = a.first().and_then(|v| v.as_handle()).unwrap_or(0);
        if gm.unit_of_handle(id).is_some() {
            let args = vec![
                PropValue::Handle(id),
                PropValue::Int(int_at(a, 1)),
                PropValue::Int(int_at(a, 2)),
                PropValue::Int(int_at(a, 3)),
                PropValue::Int(int_at(a, 4)),
            ];
            gm.presentation_mut().log.push(PresentationEvent::Command {
                name: "AddIndicator",
                args,
            });
        }
        NativeResult::Return(Value::Null)
    });
}

/// Logs a command for the unit in handle argument 0, when it is live.
fn unit_command(
    game: &RefCell<Game>,
    a: &[Value],
    name: &'static str,
    args: impl FnOnce(ObjKey) -> Vec<PropValue>,
) {
    let mut gm = game.borrow_mut();
    let id = handle_arg(a, 0);
    if let Some(k) = gm.unit_of_handle(id) {
        let mut all = vec![PropValue::Handle(id)];
        all.extend(args(k));
        gm.presentation_mut()
            .log
            .push(PresentationEvent::Command { name, args: all });
    }
}

/// Logs a command for the destructable in handle argument 0, when it is live.
fn destructable_command(
    game: &RefCell<Game>,
    a: &[Value],
    name: &'static str,
    args: impl FnOnce(ObjKey) -> Vec<PropValue>,
) {
    let mut gm = game.borrow_mut();
    let id = handle_arg(a, 0);
    if let Some(k) = gm.destructable_of_handle(id) {
        let mut all = vec![PropValue::Handle(id)];
        all.extend(args(k));
        gm.presentation_mut()
            .log
            .push(PresentationEvent::Command { name, args: all });
    }
}

/// The rect in handle argument 0, if it is a live rect.
fn rect_at(gm: &Game, a: &[Value]) -> Option<ObjKey> {
    gm.handle_of_kind(handle_arg(a, 0), HandleKind::Rect)
}

/// The handle id in argument `i`, or 0.
fn handle_arg(a: &[Value], i: usize) -> u32 {
    a.get(i).and_then(|v| v.as_handle()).unwrap_or(0)
}

/// The string in argument `i`, or the empty string.
fn string_arg(ctx: &NativeCtx<'_>, a: &[Value], i: usize) -> String {
    match a.get(i) {
        Some(Value::String(id)) => ctx.string(*id).to_string(),
        _ => String::new(),
    }
}

/// A boolean value (a JASS boolean, or an integer where nonzero is true).
fn bool_of(v: &Value) -> bool {
    match v {
        Value::Boolean(b) => *b,
        Value::Integer(i) => *i != 0,
        _ => false,
    }
}
