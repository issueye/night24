use crate::ast::{Expr, InfixExpr, PrefixExpr};
use crate::object::Object;

use super::chunk::Chunk;
use super::compiler::unsupported;
use super::compiler_assign::compile_update_operator;
use super::compiler_conditionals::{compile_and, compile_nullish_coalescing, compile_or};
use super::opcode::Opcode;
use super::resolve::ResolutionMap;

type CompileExprFn = fn(&Expr, &mut Chunk, &ResolutionMap) -> Result<(), Object>;

pub(super) fn compile_prefix_expr(
    p: &PrefixExpr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    compile_expr: CompileExprFn,
) -> Result<(), Object> {
    // `++x` / `--x` result is the new value.
    match p.op.as_str() {
        "++" => {
            return compile_update_operator(
                &p.right,
                true,
                Opcode::Add,
                p.pos.clone(),
                chunk,
                resolutions,
                compile_expr,
            );
        }
        "--" => {
            return compile_update_operator(
                &p.right,
                true,
                Opcode::Sub,
                p.pos.clone(),
                chunk,
                resolutions,
                compile_expr,
            );
        }
        "delete" => {
            // `delete x` evaluates its operand for side effects and returns true.
            compile_expr(&p.right, chunk, resolutions)?;
            chunk.write_op(Opcode::Pop, p.pos.clone());
            let true_idx = chunk.add_constant(Object::Boolean(true));
            chunk.write_op(Opcode::Const, p.pos.clone());
            chunk.write_u16(true_idx, p.pos.clone());
            return Ok(());
        }
        _ => {}
    }

    compile_expr(&p.right, chunk, resolutions)?;
    let op = match p.op.as_str() {
        "!" => Opcode::Not,
        "-" => Opcode::Neg,
        "~" => Opcode::BitNot,
        "typeof" => Opcode::TypeOf,
        "+" => Opcode::Identity,
        "void" => {
            // `void x` evaluates its operand for side effects and returns undefined.
            chunk.write_op(Opcode::Pop, p.pos.clone());
            let und_idx = chunk.add_constant(Object::Undefined);
            chunk.write_op(Opcode::Const, p.pos.clone());
            chunk.write_u16(und_idx, p.pos.clone());
            return Ok(());
        }
        _ => {
            return Err(unsupported(
                p.pos.clone(),
                &format!("prefix operator `{}`", p.op),
            ));
        }
    };
    chunk.write_op(op, p.pos.clone());
    Ok(())
}

pub(super) fn compile_infix_expr(
    i: &InfixExpr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    compile_expr: CompileExprFn,
) -> Result<(), Object> {
    // `x++` / `x--` result is the old value.
    if i.right.is_none() && (i.op == "++" || i.op == "--") {
        let delta_op = if i.op == "++" {
            Opcode::Add
        } else {
            Opcode::Sub
        };
        return compile_update_operator(
            &i.left,
            false,
            delta_op,
            i.pos.clone(),
            chunk,
            resolutions,
            compile_expr,
        );
    }
    if i.right.is_none() {
        return Err(unsupported(
            i.pos.clone(),
            "postfix update operator (++/--)",
        ));
    }

    match i.op.as_str() {
        "&&" => {
            compile_expr(&i.left, chunk, resolutions)?;
            compile_and(i, chunk, resolutions, compile_expr)
        }
        "||" => {
            compile_expr(&i.left, chunk, resolutions)?;
            compile_or(i, chunk, resolutions, compile_expr)
        }
        "??" => {
            compile_expr(&i.left, chunk, resolutions)?;
            compile_nullish_coalescing(i, chunk, resolutions, compile_expr)
        }
        _ => {
            compile_expr(&i.left, chunk, resolutions)?;
            compile_expr(i.right.as_ref().unwrap(), chunk, resolutions)?;
            let op = binary_opcode(&i.op)
                .ok_or_else(|| unsupported(i.pos.clone(), &format!("infix operator `{}`", i.op)))?;
            chunk.write_op(op, i.pos.clone());
            Ok(())
        }
    }
}

/// Map a GTS infix operator string to its VM opcode.
pub(super) fn binary_opcode(op: &str) -> Option<Opcode> {
    Some(match op {
        "+" => Opcode::Add,
        "-" => Opcode::Sub,
        "*" => Opcode::Mul,
        "/" => Opcode::Div,
        "%" => Opcode::Mod,
        "**" => Opcode::Pow,
        "&" => Opcode::BitAnd,
        "|" => Opcode::BitOr,
        "^" => Opcode::BitXor,
        "<<" => Opcode::Shl,
        ">>" => Opcode::Shr,
        ">>>" => Opcode::UShr,
        "===" => Opcode::Eq,
        "!==" => Opcode::Neq,
        "<" => Opcode::Lt,
        "<=" => Opcode::Le,
        ">" => Opcode::Gt,
        ">=" => Opcode::Ge,
        "instanceof" => Opcode::InstanceOf,
        "in" => Opcode::In,
        _ => return None,
    })
}
