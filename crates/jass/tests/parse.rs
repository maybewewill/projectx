use jass::ast::*;

const SRC: &str = r#"
// Hand-written sample exercising the grammar.
type agent extends handle
type unit extends agent

globals
    constant integer MAX_PLAYERS = 12
    unit array heroes
    real speed = -.5 * 2.0
    string greeting = "hi\n"
endglobals

constant native GetPlayerId takes player whichPlayer returns integer
native DoNothing takes nothing returns nothing

function Fib takes integer n returns integer
    local integer a = 0
    local integer b = 1
    local integer t
    loop
        exitwhen n <= 0
        set t = a + b
        set a = b
        set b = t
        set n = n - 1
    endloop
    return a
endfunction

function Main takes nothing returns nothing
    local integer i = 'hfoo'
    if Fib(10) == 55 and not false then
        call DoNothing()
    elseif i != 0 then
        set heroes[i / 2] = null
    else
        debug call DoNothing()
        return
    endif
    call ExecuteFunc("Main")
    set speed = function Fib == null or 1 < 2 and 3 >= 4
endfunction
"#;

#[test]
fn parses_sample() {
    let p = jass::parse(SRC).unwrap();
    assert_eq!(p.types.len(), 2);
    assert_eq!(p.globals.len(), 4);
    assert!(p.globals[0].constant && p.globals[1].array);
    assert_eq!(p.natives.len(), 2);
    assert!(p.natives[0].constant);
    assert_eq!(p.natives[1].sig.params, vec![]);
    assert_eq!(p.functions.len(), 2);
    let fib = &p.functions[0];
    assert_eq!(fib.locals.len(), 3);
    assert!(matches!(&fib.body[0], Stmt::Loop(b) if b.len() == 5));
    let main = &p.functions[1];
    assert!(matches!(&main.locals[0].init, Some(Expr::Int(0x68666F6F))));
    match &main.body[0] {
        Stmt::If {
            branches,
            otherwise,
        } => {
            assert_eq!(branches.len(), 2);
            assert!(matches!(otherwise[0], Stmt::Debug(_)));
            assert!(matches!(otherwise[1], Stmt::Return(None)));
        }
        s => panic!("{s:?}"),
    }
}

#[test]
fn and_or_share_precedence_left_assoc() {
    // `a or b and c` parses as `(a or b) and c` under pjass rules.
    let p = jass::parse(
        "function F takes nothing returns boolean\nreturn true or false and false\nendfunction\n",
    )
    .unwrap();
    let Stmt::Return(Some(Expr::Binary(BinOp::And, lhs, _))) = &p.functions[0].body[0] else {
        panic!()
    };
    assert!(matches!(**lhs, Expr::Binary(BinOp::Or, _, _)));
}

#[test]
fn reports_line_of_error() {
    let e = jass::parse("globals\ninteger x = \nendglobals\n").unwrap_err();
    assert!(matches!(e, jass::Error::Parse { line: 2, .. }), "{e:?}");
}
