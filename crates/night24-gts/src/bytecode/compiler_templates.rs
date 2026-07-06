use crate::ast::{Expr, Position, Stmt, TemplateLit};
use crate::lexer::Lexer;
use crate::object::{new_error, str_obj, Object};
use crate::parser::Parser;

use super::chunk::Chunk;
use super::emit::emit_const;
use super::opcode::Opcode;
use super::resolve::ResolutionMap;

pub(super) type CompileTemplateExpr = fn(&Expr, &mut Chunk, &ResolutionMap) -> Result<(), Object>;

pub(super) fn compile_template_literal(
    t: &TemplateLit,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    compile_expr: CompileTemplateExpr,
) -> Result<(), Object> {
    if !t.literal.contains("${") {
        return compile_template_static(t, chunk);
    }
    compile_template_interpolated(t, chunk, resolutions, compile_expr)
}

fn compile_template_static(t: &TemplateLit, chunk: &mut Chunk) -> Result<(), Object> {
    let value = crate::evaluator::string_lit::eval_template_static(t);
    let idx = chunk.add_constant(value);
    emit_const(chunk, idx, t.pos.clone());
    Ok(())
}

/// Compile an interpolated template literal into a string concatenation.
///
/// Each `${expr}` segment is re-parsed as a sub-expression (matching the
/// tree-walker's `eval_template_expression`), evaluated, and converted to its
/// string form via TO_STRING. Literal text segments are CONST strings. All
/// parts are joined left-to-right with `+` (string concat).
fn compile_template_interpolated(
    t: &TemplateLit,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    compile_expr: CompileTemplateExpr,
) -> Result<(), Object> {
    let lit = &t.literal;
    if lit.len() < 2 || !lit.starts_with('`') {
        return compile_template_static(t, chunk);
    }
    let mut inner = &lit[1..];
    if inner.ends_with('`') {
        inner = &inner[..inner.len() - 1];
    }
    let bytes = inner.as_bytes();
    let mut segments_emitted = 0;
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'$' && bytes[i + 1] == b'{' {
            let end = match find_template_expr_end(inner, i + 2) {
                Some(end) => end,
                None => {
                    return Err(unsupported(
                        t.pos.clone(),
                        "unterminated template expression",
                    ));
                }
            };
            let expr_str = inner[i + 2..end].trim();
            if !expr_str.is_empty() {
                let sub_expr = parse_template_expr(expr_str, t.pos.clone())?;
                compile_expr(&sub_expr, chunk, resolutions)?;
                chunk.write_op(Opcode::ToString, t.pos.clone());
                finish_template_segment(chunk, &mut segments_emitted, t.pos.clone());
            }
            i = end + 1;
            continue;
        }

        let start = i;
        while i < bytes.len() && !(i + 1 < bytes.len() && bytes[i] == b'$' && bytes[i + 1] == b'{')
        {
            i += 1;
        }
        let text = crate::evaluator::string_lit::unescape_string(&inner[start..i]);
        emit_template_string_const(chunk, text, t.pos.clone());
        finish_template_segment(chunk, &mut segments_emitted, t.pos.clone());
    }

    if segments_emitted == 0 {
        emit_template_string_const(chunk, "", t.pos.clone());
    }
    Ok(())
}

fn emit_template_string_const(chunk: &mut Chunk, value: impl Into<String>, pos: Position) {
    let idx = chunk.add_constant(str_obj(value.into()));
    emit_const(chunk, idx, pos);
}

fn finish_template_segment(chunk: &mut Chunk, segments_emitted: &mut usize, pos: Position) {
    if *segments_emitted > 0 {
        chunk.write_op(Opcode::Concat, pos);
    }
    *segments_emitted += 1;
}

/// Re-parse a template `${...}` sub-expression string into an AST Expr, so the
/// compiler can emit bytecode for it (rather than deferring to a runtime
/// re-parse). Mirrors the tree-walker's `eval_template_expression` parse step.
fn parse_template_expr(src: &str, pos: Position) -> Result<Expr, Object> {
    let wrap = format!("let __gts_tpl = {};", src);
    let lex = Lexer::new(&wrap);
    let mut parser = Parser::new(lex, pos.file.as_ref());
    let prog = parser.parse_program();
    if !parser.errors().is_empty() || !prog.errors.is_empty() {
        return Err(unsupported(pos, "template expression parse error"));
    }

    for stmt in &prog.body {
        if let Stmt::Let(l) = stmt {
            if let Some(v) = &l.value {
                return Ok(v.clone());
            }
        }
    }
    Err(unsupported(pos, "template expression parse error"))
}

/// Find the matching `}` for a `${...}` template expression, accounting for
/// nested braces and quoted strings. Mirrors the tree-walker's helper.
fn find_template_expr_end(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut quote: u8 = 0;
    let mut escape = false;
    let mut i = start;
    while i < bytes.len() {
        let ch = bytes[i];
        if quote != 0 {
            if escape {
                escape = false;
            } else if ch == b'\\' {
                escape = true;
            } else if ch == quote {
                quote = 0;
            }
            i += 1;
            continue;
        }
        match ch {
            b'"' | b'\'' => quote = ch,
            b'{' => depth += 1,
            b'}' => {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn unsupported(pos: Position, what: &str) -> Object {
    new_error(
        pos,
        format!("CompileError: bytecode VM does not yet support {}", what),
    )
}
