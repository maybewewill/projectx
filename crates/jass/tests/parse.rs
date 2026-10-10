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
    assert!(matches!(&fib.body[0], Stmt::Loop { body, .. } if body.len() == 5));
    let main = &p.functions[1];
    assert!(matches!(&main.locals[0].init, Some(Expr::Int(0x68666F6F))));
    match &main.body[0] {
        Stmt::If {
            branches,
            otherwise,
            ..
        } => {
            assert_eq!(branches.len(), 2);
            assert!(matches!(otherwise[0], Stmt::Debug(_)));
            assert!(matches!(otherwise[1], Stmt::Return { value: None, .. }));
        }
        s => panic!("{s:?}"),
    }
}

/// Renders an expression fully parenthesised, to compare tree shapes compactly.
fn shape(e: &Expr) -> String {
    match e {
        Expr::Var(v) => v.clone(),
        Expr::Int(i) => i.to_string(),
        Expr::Not(x) => format!("(not {})", shape(x)),
        Expr::Neg(x) => format!("(-{})", shape(x)),
        Expr::Binary(op, a, b) => {
            let o = match op {
                BinOp::Or => "or",
                BinOp::And => "and",
                BinOp::Eq => "==",
                BinOp::Ne => "!=",
                BinOp::Lt => "<",
                BinOp::Le => "<=",
                BinOp::Gt => ">",
                BinOp::Ge => ">=",
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                BinOp::Div => "/",
            };
            format!("({} {o} {})", shape(a), shape(b))
        }
        other => format!("{other:?}"),
    }
}

fn parse_expr(src: &str) -> String {
    let p = jass::parse(&format!(
        "function F takes nothing returns boolean\nreturn {src}\nendfunction\n"
    ))
    .unwrap();
    let Stmt::Return { value: Some(e), .. } = &p.functions[0].body[0] else {
        panic!()
    };
    shape(e)
}

#[test]
fn precedence_matches_original_parser() {
    // Expected shapes from docs/spec/jass-precedence.md (decoded from Game.dll 1.26a yacc tables
    // and checked against the real parser in emulation).
    let cases = [
        ("a - b - c", "((a - b) - c)"),
        ("a / b / c", "((a / b) / c)"),
        ("a + b * c", "(a + (b * c))"),
        ("a or b and c", "((a or b) and c)"),
        ("a and b or c", "(a and (b or c))"),
        ("a and b and c", "(a and (b and c))"),
        ("a or b or c", "(a or (b or c))"),
        ("not a == b", "((not a) == b)"),
        ("a == b == c", "((a == b) == c)"),
        ("a < b == c < d", "((a < b) == (c < d))"),
        ("- a * b", "((-a) * b)"),
        ("a < b + c", "(a < (b + c))"),
    ];
    for (src, want) in cases {
        assert_eq!(parse_expr(src), want, "{src}");
    }
}

#[test]
fn reports_line_of_error() {
    let e = jass::parse("globals\ninteger x = \nendglobals\n").unwrap_err();
    assert!(matches!(e, jass::Error::Parse { line: 2, .. }), "{e:?}");
}
