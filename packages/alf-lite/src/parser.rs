//! Hand-written recursive-descent parser for `alf-lite`'s subset (no parser-generator
//! dependency, same clean-room approach as `sysml-textual`'s own grammar). No separate lexer —
//! a char-level `Cursor` (same shape as `sysml_textual::parser`'s) scans directly into the AST.
//!
//! ```text
//! program   := statement*
//! statement := let_stmt | if_stmt | send_stmt | assign_or_invoke_stmt
//! let_stmt  := "let" IDENT "=" expr ";"
//! if_stmt   := "if" "(" expr ")" "{" statement* "}" ("else" "{" statement* "}")?
//! send_stmt := "send" IDENT "(" arglist? ")" ";"
//! assign_or_invoke_stmt := IDENT ("." IDENT "=" expr ";" | "(" arglist? ")" ";")
//! arglist   := expr ("," expr)*
//! expr      := or_expr
//! or_expr   := and_expr ("||" and_expr)*
//! and_expr  := cmp_expr ("&&" cmp_expr)*
//! cmp_expr  := add_expr (("<"|"<="|">"|">="|"=="|"!=") add_expr)?
//! add_expr  := mul_expr (("+"|"-") mul_expr)*
//! mul_expr  := unary (("*"|"/") unary)*
//! unary     := "!" unary | primary
//! primary   := literal | "(" expr ")" | IDENT ("." IDENT)?
//! ```
//!
//! Deliberately excluded from this subset (no pilot fixture or test needs them — see the crate
//! README): loops, collection/sequence expressions, generics, extended multiplicity/typing. The
//! parser recognizes (rather than merely failing to parse) the `while`/`for` keywords and a
//! collection-literal keyword (`Sequence`/`Set`/`OrderedSet`/`Bag`) so it can raise a precise
//! [`CompileError`] naming the unsupported construct instead of a generic syntax error
//! (T-P1.4-03's literal ask).

use crate::ast::{BinaryOp, Expr, Literal, Program, Stmt, UnaryOp};
use crate::error::{CompileError, Span};

const COLLECTION_KEYWORDS: [&str; 4] = ["Sequence", "Set", "OrderedSet", "Bag"];

pub fn parse(source: &str) -> Result<Program, CompileError> {
    let mut cursor = Cursor::new(source);
    let mut statements = Vec::new();
    cursor.skip_ws();
    while !cursor.eof() {
        statements.push(parse_statement(&mut cursor)?);
        cursor.skip_ws();
    }
    Ok(Program(statements))
}

struct Cursor {
    chars: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
}

impl Cursor {
    fn new(source: &str) -> Self {
        Self {
            chars: source.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn mark(&self) -> Span {
        Span {
            start: self.pos,
            end: self.pos,
            line: self.line,
            col: self.col,
        }
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn eof(&self) -> bool {
        self.pos >= self.chars.len()
    }

    /// Consumes `s` if it appears next (after skipping no whitespace — caller's responsibility),
    /// as a fixed multi-char operator/punctuation token.
    fn try_consume(&mut self, s: &str) -> bool {
        for (i, expected) in s.chars().enumerate() {
            if self.peek_at(i) != Some(expected) {
                return false;
            }
        }
        for _ in 0..s.chars().count() {
            self.advance();
        }
        true
    }

    fn expect(&mut self, s: &str) -> Result<(), CompileError> {
        self.skip_ws();
        if self.try_consume(s) {
            Ok(())
        } else {
            Err(CompileError {
                message: format!("expected '{s}'"),
                span: self.mark(),
                construct: None,
            })
        }
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_continue(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Reads a bare identifier/keyword without consuming leading whitespace (caller does that).
fn parse_raw_ident(cursor: &mut Cursor) -> Option<String> {
    if !cursor.peek().map(is_ident_start).unwrap_or(false) {
        return None;
    }
    let mut s = String::new();
    while let Some(c) = cursor.peek() {
        if is_ident_continue(c) {
            s.push(c);
            cursor.advance();
        } else {
            break;
        }
    }
    Some(s)
}

fn parse_ident(cursor: &mut Cursor) -> Result<String, CompileError> {
    let start = cursor.mark();
    parse_raw_ident(cursor).ok_or_else(|| CompileError {
        message: "expected an identifier".to_string(),
        span: start,
        construct: None,
    })
}

fn unsupported(construct: &str, span: Span, hint: &str) -> CompileError {
    CompileError {
        message: format!("{construct} are not supported by alf-lite ({hint}, see the initial subset in packages/alf-lite/README.md)"),
        span,
        construct: Some(construct.to_string()),
    }
}

fn parse_statement(cursor: &mut Cursor) -> Result<Stmt, CompileError> {
    cursor.skip_ws();
    let start = cursor.mark();
    let word = parse_raw_ident(cursor).ok_or_else(|| CompileError {
        message: "expected a statement".to_string(),
        span: start,
        construct: None,
    })?;

    match word.as_str() {
        "while" | "for" => Err(unsupported(
            "loops",
            start,
            "no pilot fixture needs one yet — grown on demand per §9.6",
        )),
        "let" => parse_let(cursor),
        "if" => parse_if(cursor),
        "send" => parse_send(cursor),
        _ => parse_assign_or_invoke(cursor, word, start),
    }
}

fn parse_let(cursor: &mut Cursor) -> Result<Stmt, CompileError> {
    cursor.skip_ws();
    let name = parse_ident(cursor)?;
    cursor.expect("=")?;
    cursor.skip_ws();
    let value = parse_expr(cursor)?;
    cursor.expect(";")?;
    Ok(Stmt::Let { name, value })
}

fn parse_if(cursor: &mut Cursor) -> Result<Stmt, CompileError> {
    cursor.expect("(")?;
    cursor.skip_ws();
    let condition = parse_expr(cursor)?;
    cursor.expect(")")?;
    let then_branch = parse_block(cursor)?;

    cursor.skip_ws();
    let else_branch = if peek_keyword(cursor, "else") {
        consume_keyword(cursor, "else");
        parse_block(cursor)?
    } else {
        Vec::new()
    };

    Ok(Stmt::If {
        condition,
        then_branch,
        else_branch,
    })
}

fn parse_send(cursor: &mut Cursor) -> Result<Stmt, CompileError> {
    cursor.skip_ws();
    let signal_name = parse_ident(cursor)?;
    cursor.expect("(")?;
    let args = parse_arglist(cursor)?;
    cursor.expect(";")?;
    Ok(Stmt::SendSignal { signal_name, args })
}

/// Dispatches on what follows the already-consumed leading identifier: `.` starts an assignment
/// (`target.feature = expr;`), `(` starts a bare invocation (`name(args...);`).
fn parse_assign_or_invoke(
    cursor: &mut Cursor,
    leading_ident: String,
    start: Span,
) -> Result<Stmt, CompileError> {
    if cursor.peek() == Some('.') {
        cursor.advance();
        let feature = parse_ident(cursor)?;
        cursor.expect("=")?;
        cursor.skip_ws();
        let value = parse_expr(cursor)?;
        cursor.expect(";")?;
        return Ok(Stmt::Assign {
            target: leading_ident,
            feature,
            value,
        });
    }
    cursor.skip_ws();
    if cursor.peek() == Some('(') {
        cursor.advance();
        let args = parse_arglist(cursor)?;
        cursor.expect(";")?;
        return Ok(Stmt::Invoke {
            behavior_name: leading_ident,
            args,
        });
    }
    Err(CompileError {
        message: "expected '.' (property assignment) or '(' (behavior invocation) after identifier"
            .to_string(),
        span: start,
        construct: None,
    })
}

fn parse_arglist(cursor: &mut Cursor) -> Result<Vec<Expr>, CompileError> {
    let mut args = Vec::new();
    cursor.skip_ws();
    if cursor.peek() == Some(')') {
        cursor.advance();
        return Ok(args);
    }
    loop {
        cursor.skip_ws();
        args.push(parse_expr(cursor)?);
        cursor.skip_ws();
        if cursor.try_consume(",") {
            continue;
        }
        cursor.expect(")")?;
        return Ok(args);
    }
}

fn parse_block(cursor: &mut Cursor) -> Result<Vec<Stmt>, CompileError> {
    cursor.expect("{")?;
    let mut statements = Vec::new();
    loop {
        cursor.skip_ws();
        if cursor.peek() == Some('}') {
            cursor.advance();
            return Ok(statements);
        }
        if cursor.eof() {
            return Err(CompileError {
                message: "unterminated block, expected '}'".to_string(),
                span: cursor.mark(),
                construct: None,
            });
        }
        statements.push(parse_statement(cursor)?);
    }
}

/// Looks ahead for a bare keyword (not followed by an identifier-continuation character, so
/// `else` doesn't match a prefix of `elsewhere`) without consuming anything.
fn peek_keyword(cursor: &mut Cursor, keyword: &str) -> bool {
    for (i, expected) in keyword.chars().enumerate() {
        if cursor.peek_at(i) != Some(expected) {
            return false;
        }
    }
    !cursor
        .peek_at(keyword.chars().count())
        .map(is_ident_continue)
        .unwrap_or(false)
}

fn consume_keyword(cursor: &mut Cursor, keyword: &str) {
    for _ in 0..keyword.chars().count() {
        cursor.advance();
    }
}

// --- Expressions, by ascending precedence (lowest first) ---

fn parse_expr(cursor: &mut Cursor) -> Result<Expr, CompileError> {
    parse_or(cursor)
}

fn parse_or(cursor: &mut Cursor) -> Result<Expr, CompileError> {
    let mut left = parse_and(cursor)?;
    loop {
        cursor.skip_ws();
        if cursor.try_consume("||") {
            cursor.skip_ws();
            let right = parse_and(cursor)?;
            left = Expr::Binary {
                op: BinaryOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        } else {
            return Ok(left);
        }
    }
}

fn parse_and(cursor: &mut Cursor) -> Result<Expr, CompileError> {
    let mut left = parse_cmp(cursor)?;
    loop {
        cursor.skip_ws();
        if cursor.try_consume("&&") {
            cursor.skip_ws();
            let right = parse_cmp(cursor)?;
            left = Expr::Binary {
                op: BinaryOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        } else {
            return Ok(left);
        }
    }
}

fn parse_cmp(cursor: &mut Cursor) -> Result<Expr, CompileError> {
    let left = parse_add(cursor)?;
    cursor.skip_ws();
    let op = if cursor.try_consume("<=") {
        Some(BinaryOp::Le)
    } else if cursor.try_consume(">=") {
        Some(BinaryOp::Ge)
    } else if cursor.try_consume("==") {
        Some(BinaryOp::Eq)
    } else if cursor.try_consume("!=") {
        Some(BinaryOp::Ne)
    } else if cursor.try_consume("<") {
        Some(BinaryOp::Lt)
    } else if cursor.try_consume(">") {
        Some(BinaryOp::Gt)
    } else {
        None
    };
    match op {
        None => Ok(left),
        Some(op) => {
            cursor.skip_ws();
            let right = parse_add(cursor)?;
            Ok(Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            })
        }
    }
}

fn parse_add(cursor: &mut Cursor) -> Result<Expr, CompileError> {
    let mut left = parse_mul(cursor)?;
    loop {
        cursor.skip_ws();
        let op = if cursor.try_consume("+") {
            Some(BinaryOp::Add)
        } else if cursor.try_consume("-") {
            Some(BinaryOp::Sub)
        } else {
            None
        };
        match op {
            None => return Ok(left),
            Some(op) => {
                cursor.skip_ws();
                let right = parse_mul(cursor)?;
                left = Expr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            }
        }
    }
}

fn parse_mul(cursor: &mut Cursor) -> Result<Expr, CompileError> {
    let mut left = parse_unary(cursor)?;
    loop {
        cursor.skip_ws();
        let op = if cursor.try_consume("*") {
            Some(BinaryOp::Mul)
        } else if cursor.try_consume("/") {
            Some(BinaryOp::Div)
        } else {
            None
        };
        match op {
            None => return Ok(left),
            Some(op) => {
                cursor.skip_ws();
                let right = parse_unary(cursor)?;
                left = Expr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            }
        }
    }
}

fn parse_unary(cursor: &mut Cursor) -> Result<Expr, CompileError> {
    cursor.skip_ws();
    if cursor.try_consume("!") {
        cursor.skip_ws();
        let operand = parse_unary(cursor)?;
        return Ok(Expr::Unary {
            op: UnaryOp::Not,
            operand: Box::new(operand),
        });
    }
    parse_primary(cursor)
}

fn parse_primary(cursor: &mut Cursor) -> Result<Expr, CompileError> {
    cursor.skip_ws();
    let start = cursor.mark();

    if cursor.try_consume("(") {
        cursor.skip_ws();
        let inner = parse_expr(cursor)?;
        cursor.expect(")")?;
        return Ok(inner);
    }

    if cursor.peek() == Some('"') {
        return parse_string_literal(cursor).map(|s| Expr::Literal(Literal::Str(s)));
    }

    if cursor.peek().map(|c| c.is_ascii_digit()).unwrap_or(false) {
        return parse_number_literal(cursor);
    }

    let ident = parse_raw_ident(cursor).ok_or_else(|| CompileError {
        message: "expected an expression (literal, identifier, or '(')".to_string(),
        span: start,
        construct: None,
    })?;

    if COLLECTION_KEYWORDS.contains(&ident.as_str()) {
        cursor.skip_ws();
        if cursor.peek() == Some('{') {
            return Err(unsupported(
                "collection/sequence expressions",
                start,
                &format!("'{ident}' literal"),
            ));
        }
    }

    match ident.as_str() {
        "true" => return Ok(Expr::Literal(Literal::Bool(true))),
        "false" => return Ok(Expr::Literal(Literal::Bool(false))),
        _ => {}
    }

    if cursor.peek() == Some('.') {
        cursor.advance();
        let feature = parse_ident(cursor)?;
        return Ok(Expr::PropertyAccess {
            target: ident,
            feature,
        });
    }

    Ok(Expr::Var(ident))
}

fn parse_string_literal(cursor: &mut Cursor) -> Result<String, CompileError> {
    let start = cursor.mark();
    cursor.advance(); // consume opening '"'
    let mut s = String::new();
    loop {
        match cursor.advance() {
            Some('"') => return Ok(s),
            Some('\\') => match cursor.advance() {
                Some('"') => s.push('"'),
                Some('\\') => s.push('\\'),
                Some('n') => s.push('\n'),
                Some(other) => {
                    s.push('\\');
                    s.push(other);
                }
                None => {
                    return Err(CompileError {
                        message: "unterminated escape in string literal".to_string(),
                        span: cursor.mark(),
                        construct: None,
                    })
                }
            },
            Some(c) => s.push(c),
            None => {
                return Err(CompileError {
                    message: "unterminated string literal".to_string(),
                    span: start,
                    construct: None,
                })
            }
        }
    }
}

/// Integer if there's no `.`, real otherwise — no exponent notation, no suffixes (nothing in
/// the pilot fixture needs them).
fn parse_number_literal(cursor: &mut Cursor) -> Result<Expr, CompileError> {
    let start = cursor.mark();
    let mut s = String::new();
    while let Some(c) = cursor.peek() {
        if c.is_ascii_digit() {
            s.push(c);
            cursor.advance();
        } else {
            break;
        }
    }
    if cursor.peek() == Some('.')
        && cursor
            .peek_at(1)
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
    {
        s.push('.');
        cursor.advance();
        while let Some(c) = cursor.peek() {
            if c.is_ascii_digit() {
                s.push(c);
                cursor.advance();
            } else {
                break;
            }
        }
        let value: f64 = s.parse().map_err(|_| CompileError {
            message: format!("invalid real literal '{s}'"),
            span: start,
            construct: None,
        })?;
        return Ok(Expr::Literal(Literal::Real(value)));
    }
    let value: i64 = s.parse().map_err(|_| CompileError {
        message: format!("invalid integer literal '{s}'"),
        span: start,
        construct: None,
    })?;
    Ok(Expr::Literal(Literal::Int(value)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_let_with_literal() {
        let program = parse("let x = 42;").unwrap();
        assert_eq!(
            program.0,
            vec![Stmt::Let {
                name: "x".to_string(),
                value: Expr::Literal(Literal::Int(42)),
            }]
        );
    }

    #[test]
    fn parses_bool_and_string_literals() {
        let program = parse("let a = true; let b = \"hi\";").unwrap();
        assert_eq!(
            program.0[0],
            Stmt::Let {
                name: "a".to_string(),
                value: Expr::Literal(Literal::Bool(true)),
            }
        );
        assert_eq!(
            program.0[1],
            Stmt::Let {
                name: "b".to_string(),
                value: Expr::Literal(Literal::Str("hi".to_string())),
            }
        );
    }

    #[test]
    fn parses_real_literal() {
        let program = parse("let x = 3500.0;").unwrap();
        assert_eq!(
            program.0[0],
            Stmt::Let {
                name: "x".to_string(),
                value: Expr::Literal(Literal::Real(3500.0)),
            }
        );
    }

    #[test]
    fn parses_property_access() {
        let program = parse("let x = Turbine.rpm;").unwrap();
        assert_eq!(
            program.0[0],
            Stmt::Let {
                name: "x".to_string(),
                value: Expr::PropertyAccess {
                    target: "Turbine".to_string(),
                    feature: "rpm".to_string(),
                },
            }
        );
    }

    #[test]
    fn parses_property_assignment() {
        let program = parse("Turbine.rpm = 3500.0;").unwrap();
        assert_eq!(
            program.0[0],
            Stmt::Assign {
                target: "Turbine".to_string(),
                feature: "rpm".to_string(),
                value: Expr::Literal(Literal::Real(3500.0)),
            }
        );
    }

    #[test]
    fn parses_arithmetic_and_precedence() {
        // 1 + 2 * 3 must parse as 1 + (2 * 3), not (1 + 2) * 3.
        let program = parse("let x = 1 + 2 * 3;").unwrap();
        let Stmt::Let { value, .. } = &program.0[0] else {
            panic!("expected Let");
        };
        assert_eq!(
            *value,
            Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::Literal(Literal::Int(1))),
                right: Box::new(Expr::Binary {
                    op: BinaryOp::Mul,
                    left: Box::new(Expr::Literal(Literal::Int(2))),
                    right: Box::new(Expr::Literal(Literal::Int(3))),
                }),
            }
        );
    }

    #[test]
    fn parses_comparison_operators() {
        for (src, op) in [
            ("let x = 1 < 2;", BinaryOp::Lt),
            ("let x = 1 <= 2;", BinaryOp::Le),
            ("let x = 1 > 2;", BinaryOp::Gt),
            ("let x = 1 >= 2;", BinaryOp::Ge),
            ("let x = 1 == 2;", BinaryOp::Eq),
            ("let x = 1 != 2;", BinaryOp::Ne),
        ] {
            let program = parse(src).unwrap();
            let Stmt::Let { value, .. } = &program.0[0] else {
                panic!("expected Let");
            };
            assert_eq!(
                *value,
                Expr::Binary {
                    op,
                    left: Box::new(Expr::Literal(Literal::Int(1))),
                    right: Box::new(Expr::Literal(Literal::Int(2))),
                }
            );
        }
    }

    #[test]
    fn parses_boolean_operators_and_not() {
        let program = parse("let x = !true && false || true;").unwrap();
        // !true && false || true  =>  ((!true) && false) || true
        let Stmt::Let { value, .. } = &program.0[0] else {
            panic!("expected Let");
        };
        assert_eq!(
            *value,
            Expr::Binary {
                op: BinaryOp::Or,
                left: Box::new(Expr::Binary {
                    op: BinaryOp::And,
                    left: Box::new(Expr::Unary {
                        op: UnaryOp::Not,
                        operand: Box::new(Expr::Literal(Literal::Bool(true))),
                    }),
                    right: Box::new(Expr::Literal(Literal::Bool(false))),
                }),
                right: Box::new(Expr::Literal(Literal::Bool(true))),
            }
        );
    }

    #[test]
    fn parses_if_else() {
        let program =
            parse("if (Turbine.rpm < 3500) { SetTurbineRpm(3500.0); } else { let x = 1; }")
                .unwrap();
        let Stmt::If {
            then_branch,
            else_branch,
            ..
        } = &program.0[0]
        else {
            panic!("expected If");
        };
        assert_eq!(then_branch.len(), 1);
        assert_eq!(else_branch.len(), 1);
    }

    #[test]
    fn parses_if_without_else() {
        let program = parse("if (true) { let x = 1; }").unwrap();
        let Stmt::If { else_branch, .. } = &program.0[0] else {
            panic!("expected If");
        };
        assert!(else_branch.is_empty());
    }

    #[test]
    fn parses_bare_invocation_statement() {
        let program = parse("SetTurbineRpm(3500.0);").unwrap();
        assert_eq!(
            program.0[0],
            Stmt::Invoke {
                behavior_name: "SetTurbineRpm".to_string(),
                args: vec![Expr::Literal(Literal::Real(3500.0))],
            }
        );
    }

    #[test]
    fn parses_send_signal_statement() {
        let program = parse("send telemetryUpdate(Turbine.rpm);").unwrap();
        assert_eq!(
            program.0[0],
            Stmt::SendSignal {
                signal_name: "telemetryUpdate".to_string(),
                args: vec![Expr::PropertyAccess {
                    target: "Turbine".to_string(),
                    feature: "rpm".to_string(),
                }],
            }
        );
    }

    #[test]
    fn parses_the_golden_armed_to_running_action() {
        // The literal T-P1.4-02 scenario: a guard comparison + a behavior invocation setting
        // Turbine.rpm.
        let program = parse("if (Turbine.rpm < 3500.0) { SetTurbineRpm(3500.0); }").unwrap();
        assert_eq!(program.0.len(), 1);
    }

    #[test]
    fn rejects_collection_literal_by_name() {
        let err = parse("let x = Sequence{1, 2, 3};").unwrap_err();
        assert_eq!(
            err.construct,
            Some("collection/sequence expressions".to_string())
        );
    }

    #[test]
    fn rejects_while_loop_by_name() {
        let err = parse("while (true) { let x = 1; }").unwrap_err();
        assert_eq!(err.construct, Some("loops".to_string()));
    }

    #[test]
    fn rejects_for_loop_by_name() {
        let err = parse("for (let x = 1;) { let y = 2; }").unwrap_err();
        assert_eq!(err.construct, Some("loops".to_string()));
    }

    #[test]
    fn plain_syntax_error_has_no_construct_name() {
        let err = parse("let x = ;").unwrap_err();
        assert_eq!(err.construct, None);
    }
}
