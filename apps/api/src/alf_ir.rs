//! Converts `alf_lite::ast` (a pure Rust AST, no wire-format knowledge) into the protobuf
//! `CompiledExpression`/`CompiledStatement` messages `fuml-runtime` consumes over gRPC. This is
//! the one place protobuf enters the picture for a compiled Alf action — `alf-lite` itself stays
//! a pure-logic package, matching this codebase's existing convention (`sysml-textual`'s
//! `GraphOp` is likewise a plain Rust enum that `apps/api` translates into Neo4j calls, not
//! something the package itself serializes).

use alf_lite::ast::{BinaryOp, Expr, Literal, Program, Stmt, UnaryOp};

use crate::fuml_client::proto;

pub(crate) fn compile_program(program: &Program) -> Vec<proto::CompiledStatement> {
    program.0.iter().map(compile_stmt).collect()
}

fn compile_stmt(stmt: &Stmt) -> proto::CompiledStatement {
    use proto::compiled_statement::Kind;
    let kind = match stmt {
        Stmt::Let { name, value } => Kind::LetStmt(proto::LetStatement {
            name: name.clone(),
            value: Some(compile_expr(value)),
        }),
        Stmt::Assign {
            target,
            feature,
            value,
        } => Kind::AssignStmt(proto::AssignStatement {
            target: target.clone(),
            feature: feature.clone(),
            value: Some(compile_expr(value)),
        }),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => Kind::IfStmt(proto::IfStatement {
            condition: Some(compile_expr(condition)),
            then_branch: then_branch.iter().map(compile_stmt).collect(),
            else_branch: else_branch.iter().map(compile_stmt).collect(),
        }),
        Stmt::Invoke {
            behavior_name,
            args,
        } => Kind::InvokeStmt(proto::InvokeStatement {
            behavior_name: behavior_name.clone(),
            args: args.iter().map(compile_expr).collect(),
        }),
        Stmt::SendSignal { signal_name, args } => {
            Kind::SendSignalStmt(proto::SendSignalStatement {
                signal_name: signal_name.clone(),
                args: args.iter().map(compile_expr).collect(),
            })
        }
    };
    proto::CompiledStatement { kind: Some(kind) }
}

fn compile_expr(expr: &Expr) -> proto::CompiledExpression {
    use proto::compiled_expression::Kind;
    let kind = match expr {
        Expr::Literal(Literal::Bool(b)) => Kind::BoolLiteral(*b),
        Expr::Literal(Literal::Int(i)) => Kind::IntLiteral(*i),
        Expr::Literal(Literal::Real(r)) => Kind::RealLiteral(*r),
        Expr::Literal(Literal::Str(s)) => Kind::StringLiteral(s.clone()),
        Expr::Var(name) => Kind::VarRef(name.clone()),
        Expr::PropertyAccess { target, feature } => Kind::PropertyAccess(proto::PropertyAccess {
            target: target.clone(),
            feature: feature.clone(),
        }),
        Expr::Unary { op, operand } => Kind::UnaryOp(Box::new(proto::UnaryOp {
            op: unary_op_str(*op).to_string(),
            operand: Some(Box::new(compile_expr(operand))),
        })),
        Expr::Binary { op, left, right } => Kind::BinaryOp(Box::new(proto::BinaryOp {
            op: binary_op_str(*op).to_string(),
            left: Some(Box::new(compile_expr(left))),
            right: Some(Box::new(compile_expr(right))),
        })),
    };
    proto::CompiledExpression { kind: Some(kind) }
}

fn unary_op_str(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Not => "not",
    }
}

fn binary_op_str(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "add",
        BinaryOp::Sub => "sub",
        BinaryOp::Mul => "mul",
        BinaryOp::Div => "div",
        BinaryOp::Lt => "lt",
        BinaryOp::Le => "le",
        BinaryOp::Gt => "gt",
        BinaryOp::Ge => "ge",
        BinaryOp::Eq => "eq",
        BinaryOp::Ne => "ne",
        BinaryOp::And => "and",
        BinaryOp::Or => "or",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_the_golden_armed_to_running_action() {
        let program =
            alf_lite::parse("if (Turbine.rpm < 3500.0) { SetTurbineRpm(3500.0); }").unwrap();
        let statements = compile_program(&program);
        assert_eq!(statements.len(), 1);
        let Some(proto::compiled_statement::Kind::IfStmt(if_stmt)) = &statements[0].kind else {
            panic!("expected an IfStmt");
        };
        let Some(proto::compiled_expression::Kind::BinaryOp(cmp)) =
            &if_stmt.condition.as_ref().unwrap().kind
        else {
            panic!("expected a BinaryOp condition");
        };
        assert_eq!(cmp.op, "lt");
        assert_eq!(if_stmt.then_branch.len(), 1);
        let Some(proto::compiled_statement::Kind::InvokeStmt(invoke)) =
            &if_stmt.then_branch[0].kind
        else {
            panic!("expected an InvokeStmt");
        };
        assert_eq!(invoke.behavior_name, "SetTurbineRpm");
    }
}
