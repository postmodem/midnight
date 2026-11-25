//! Small helpers: name suggestions, counting words, integer arithmetic on
//! constants, and how a place is named in a message.

use crate::ast;

/// The number of edits that turn `a` into `b`. An edit inserts, deletes or
/// replaces one character, or swaps two adjacent ones.
pub(super) fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    // rows[i][j] is the distance between the first i characters of `a`
    // and the first j of `b`.
    let mut rows: Vec<Vec<usize>> = Vec::with_capacity(a.len() + 1);
    rows.push((0..=b.len()).collect());
    for i in 1..=a.len() {
        let mut row = vec![i];
        for j in 1..=b.len() {
            let replace = rows[i - 1][j - 1] + usize::from(a[i - 1] != b[j - 1]);
            let mut best = replace.min(rows[i - 1][j] + 1).min(row[j - 1] + 1);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(rows[i - 2][j - 2] + 1);
            }
            row.push(best);
        }
        rows.push(row);
    }
    rows[a.len()][b.len()]
}

/// The candidate closest to `name`, if one is within a third of its length
/// (and at least one edit) away.
pub(super) fn closest<'s>(
    name: &str,
    candidates: impl IntoIterator<Item = &'s str>,
) -> Option<&'s str> {
    let limit = (name.chars().count() / 3).max(1);
    candidates
        .into_iter()
        .filter(|candidate| *candidate != name)
        .map(|candidate| (edit_distance(name, candidate), candidate))
        .filter(|(distance, _)| *distance <= limit)
        .min()
        .map(|(_, candidate)| candidate)
}

/// "no arguments", "1 argument", "3 arguments".
pub(super) fn count(n: usize, noun: &str) -> String {
    match n {
        0 => format!("no {noun}s"),
        1 => format!("1 {noun}"),
        _ => format!("{n} {noun}s"),
    }
}

/// "none were given", "1 was given", "3 were given".
pub(super) fn were_given(n: usize) -> String {
    match n {
        0 => "none were given".to_string(),
        1 => "1 was given".to_string(),
        _ => format!("{n} were given"),
    }
}

/// "`a`", "`a` and `b`", "`a`, `b` and `c`".
pub(super) fn name_list(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => format!("`{one}`"),
        [init @ .., last] => {
            let init: Vec<String> = init.iter().map(|n| format!("`{n}`")).collect();
            format!("{} and `{last}`", init.join(", "))
        }
    }
}

/// `a ** b` on `Int`, wrapping. `None` when the exponent is negative.
pub(super) fn wrapping_pow(a: i64, b: i64) -> Option<i64> {
    if b < 0 {
        return None;
    }
    let mut result: i64 = 1;
    let mut factor = a;
    let mut left = b;
    while left != 0 {
        if left & 1 != 0 {
            result = result.wrapping_mul(factor);
        }
        factor = factor.wrapping_mul(factor);
        left >>= 1;
    }
    Some(result)
}

/// Integer division rounding toward negative infinity. `None` when the
/// divisor is zero.
pub(super) fn floor_div(a: i64, b: i64) -> Option<i64> {
    if b == 0 {
        return None;
    }
    let quotient = a.wrapping_div(b);
    let remainder = a.wrapping_rem(b);
    if remainder != 0 && ((remainder < 0) != (b < 0)) {
        Some(quotient.wrapping_sub(1))
    } else {
        Some(quotient)
    }
}

/// Remainder with the sign of the divisor. `None` when the divisor is zero.
pub(super) fn floor_mod(a: i64, b: i64) -> Option<i64> {
    if b == 0 {
        return None;
    }
    let remainder = a.wrapping_rem(b);
    if remainder != 0 && ((remainder < 0) != (b < 0)) {
        Some(remainder.wrapping_add(b))
    } else {
        Some(remainder)
    }
}

/// How an assignment target is written, for messages: `pos.x`, `xs[i]`,
/// `e[Health].hp`. `None` for an expression that can't be a place.
pub(super) fn describe(expr: &ast::Expr) -> Option<String> {
    match &expr.kind {
        ast::ExprKind::Var(name) | ast::ExprKind::Const(name) => Some(name.clone()),
        ast::ExprKind::SelfRef => Some("self".to_string()),
        ast::ExprKind::Member {
            recv,
            name,
            args: None,
            block: None,
        } => Some(format!("{}.{}", describe(recv)?, name.name)),
        ast::ExprKind::Index { recv, index } => {
            let index = match &index.kind {
                ast::ExprKind::Int(value) => value.to_string(),
                ast::ExprKind::Var(name) | ast::ExprKind::Const(name) => name.clone(),
                _ => "…".to_string(),
            };
            Some(format!("{}[{index}]", describe(recv)?))
        }
        _ => None,
    }
}

/// The names of the methods that are not operators, for suggestions.
pub(super) fn named_methods<T>(methods: &[(String, T)]) -> impl Iterator<Item = String> {
    methods
        .iter()
        .map(|(name, _)| name)
        .filter(|name| name.starts_with(|c: char| c.is_alphabetic() || c == '_'))
        .cloned()
}

pub(super) fn binary_symbol(op: ast::BinaryOp) -> &'static str {
    match op {
        ast::BinaryOp::Add => "+",
        ast::BinaryOp::Sub => "-",
        ast::BinaryOp::Mul => "*",
        ast::BinaryOp::Div => "/",
        ast::BinaryOp::Mod => "%",
        ast::BinaryOp::Pow => "**",
        ast::BinaryOp::Eq => "==",
        ast::BinaryOp::Ne => "!=",
        ast::BinaryOp::Lt => "<",
        ast::BinaryOp::Le => "<=",
        ast::BinaryOp::Gt => ">",
        ast::BinaryOp::Ge => ">=",
        ast::BinaryOp::And => "&&",
        ast::BinaryOp::Or => "||",
    }
}
