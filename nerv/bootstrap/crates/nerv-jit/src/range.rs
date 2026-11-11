//! Conservative inclusive i64 intervals. Never infer through wrapping overflow,
//! reassigned locals, calls, or array elements; those remain LLVM's responsibility.
use nerv_sema::ir::{BinaryOp as B, Expr, ExprKind as E, SymbolId, UnaryOp as U};
use std::collections::{HashMap, HashSet};

pub(super) type Range = (i64, i64);

/// Whole-function write set, including expressions nested in branches and calls.
/// Array element writes do not rebind the handle or change its fixed length.
pub(super) fn assigned_locals(body: &[nerv_sema::ir::Statement]) -> HashSet<SymbolId> {
    use nerv_sema::ir::{CallTarget, Statement, StatementKind as S};
    fn block(body: &[Statement], writes: &mut HashSet<SymbolId>) {
        for statement in body {
            match &statement.kind {
                S::Break | S::Continue | S::ReturnVoid => {}
                S::Assign { target, value } | S::AssignField { target, value, .. } => {
                    writes.insert(*target);
                    expr(value, writes);
                }
                S::Let { value, .. } | S::Return(value) | S::Expr(value) => expr(value, writes),
                S::AssignIndex { base, index, value } => {
                    expr(base, writes);
                    expr(index, writes);
                    expr(value, writes);
                }
                S::While { condition, body } => {
                    expr(condition, writes);
                    block(body, writes);
                }
                S::For {
                    start, end, body, ..
                } => {
                    expr(start, writes);
                    expr(end, writes);
                    block(body, writes);
                }
            }
        }
    }
    fn expr(value: &Expr, writes: &mut HashSet<SymbolId>) {
        match &value.kind {
            E::If {
                condition,
                then_body,
                else_body,
            } => {
                expr(condition, writes);
                block(then_body, writes);
                block(else_body, writes);
            }
            E::Match { value, arms, .. } => {
                expr(value, writes);
                for arm in arms {
                    expr(&arm.body, writes);
                }
            }
            E::Call { target, args } => {
                if let CallTarget::Indirect(callee) = target {
                    expr(callee, writes);
                }
                for arg in args {
                    expr(arg, writes);
                }
            }
            E::Closure { captures: args, .. } | E::Array(args) | E::Runtime { args, .. } => {
                for arg in args {
                    expr(arg, writes);
                }
            }
            E::ArrayRepeat { value, length } => {
                expr(value, writes);
                expr(length, writes);
            }
            E::Struct { fields, .. } => {
                for (_, field) in fields {
                    expr(field, writes);
                }
            }
            E::Enum { payload, .. } => {
                if let Some(value) = payload {
                    expr(value, writes);
                }
            }
            E::Try { value: base, .. }
            | E::Field { base, .. }
            | E::Unary { value: base, .. }
            | E::Convert { value: base, .. }
            | E::Assert(base) => expr(base, writes),
            E::Index { base, index } => {
                expr(base, writes);
                expr(index, writes);
            }
            E::Binary { left, right, .. } => {
                expr(left, writes);
                expr(right, writes);
            }
            E::Int(_) | E::Float(_) | E::String(_) | E::Bool(_) | E::Local(_) | E::Function(_) => {}
        }
    }
    let mut writes = HashSet::new();
    block(body, &mut writes);
    writes
}

pub(super) fn infer(expr: &Expr, locals: &HashMap<SymbolId, Range>) -> Option<Range> {
    match &expr.kind {
        E::Int(n) => Some((*n, *n)),
        E::Local(id) => locals.get(id).copied(),
        E::Unary { op: U::Neg, value } => {
            let (lo, hi) = infer(value, locals)?;
            Some((hi.checked_neg()?, lo.checked_neg()?))
        }
        E::Binary { left, op, right } => {
            let (a, b) = infer(left, locals)?;
            let (c, d) = infer(right, locals)?;
            match op {
                B::Add => Some((a.checked_add(c)?, b.checked_add(d)?)),
                B::Sub => Some((a.checked_sub(d)?, b.checked_sub(c)?)),
                B::Mul => {
                    let products = [
                        a.checked_mul(c)?,
                        a.checked_mul(d)?,
                        b.checked_mul(c)?,
                        b.checked_mul(d)?,
                    ];
                    Some((*products.iter().min()?, *products.iter().max()?))
                }
                B::Rem if a >= 0 && c > 0 => Some((0, b.min(d - 1))),
                B::Div if a >= 0 && c > 0 => Some((a / d, b / c)),
                _ => None,
            }
        }
        _ => None,
    }
}

pub(super) fn fits_u32(range: Option<Range>) -> bool {
    range.is_some_and(|(lo, hi)| lo >= 0 && hi <= u32::MAX as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nerv_sema::ir::StatementKind;

    fn expression(source: &str) -> Expr {
        let module = nerv_syntax::parse(&format!("fn main() -> i64 => {source}\n")).unwrap();
        let program = nerv_sema::check_modules(&[module]).unwrap();
        match &program.functions()[0].body[0].kind {
            StatementKind::Expr(expr) | StatementKind::Return(expr) => expr.clone(),
            _ => panic!("expected expression"),
        }
    }

    #[test]
    fn intervals_do_not_assume_wrapping_arithmetic_is_monotonic() {
        for source in [
            "9223372036854775807 + 1",
            "(-9223372036854775807 - 1) * -1",
            "1 / 0",
            "1 % -1",
        ] {
            assert_eq!(
                infer(&expression(source), &HashMap::new()),
                None,
                "{source}"
            );
        }
        assert_eq!(
            infer(&expression("(20 + 2) * 3 % 5"), &HashMap::new()),
            Some((0, 4))
        );
        assert!(fits_u32(Some((0, u32::MAX as i64))));
        assert!(!fits_u32(Some((-1, 22))));
        assert!(!fits_u32(Some((0, 1_i64 << 32))));
    }
}
