//! The compiled subset's AST — deliberately plain Rust types, no protobuf/serde involved. This
//! crate is pure logic (same convention as `sysml-core`/`sysml-textual`); `apps/api` is where
//! this AST gets converted into the wire-format protobuf messages `fuml-runtime` consumes (see
//! `apps/api/src/alf_ir.rs`), not here.

#[derive(Debug, Clone, PartialEq)]
pub struct Program(pub Vec<Stmt>);

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// `let name = expr;`
    Let { name: String, value: Expr },
    /// `target.feature = expr;`
    Assign {
        target: String,
        feature: String,
        value: Expr,
    },
    /// `if (cond) { then } else { else_ }` — `else_` is empty when no `else` clause is present.
    If {
        condition: Expr,
        then_branch: Vec<Stmt>,
        else_branch: Vec<Stmt>,
    },
    /// `name(args...);` — a bare behavior-invocation statement.
    Invoke {
        behavior_name: String,
        args: Vec<Expr>,
    },
    /// `send name(args...);`
    SendSignal {
        signal_name: String,
        args: Vec<Expr>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Literal(Literal),
    Var(String),
    /// `target.feature` — one level of property access.
    PropertyAccess {
        target: String,
        feature: String,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Bool(bool),
    Int(i64),
    Real(f64),
    Str(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
}
