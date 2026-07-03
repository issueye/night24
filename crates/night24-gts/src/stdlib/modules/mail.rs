use std::cell::RefCell;
use std::rc::Rc;

use super::super::helpers::*;
use super::time::{parse_time_ms, time_from_object, utc_parts_from_ms};
use crate::object::{new_error, str_obj, CallContext, HashData, Object};

pub(crate) fn mail_module() -> Object {
    module(vec![
        (
            "parseAddress",
            native("mail.parseAddress", mail_parse_address),
        ),
        (
            "parseAddressList",
            native("mail.parseAddressList", mail_parse_address_list),
        ),
        (
            "parseMessage",
            native("mail.parseMessage", mail_parse_message),
        ),
        (
            "formatAddress",
            native("mail.formatAddress", mail_format_address),
        ),
        (
            "formatAddressList",
            native("mail.formatAddressList", mail_format_address_list),
        ),
        ("parseDate", native("mail.parseDate", mail_parse_date)),
        ("formatDate", native("mail.formatDate", mail_format_date)),
        ("getHeader", native("mail.getHeader", mail_get_header)),
    ])
}

/// A parsed mailbox address: optional display name + `local@domain`.
#[derive(Clone)]
pub(crate) struct MailAddress {
    name: String,
    address: String,
}

/// Parse a single address. Accepts both `Name <addr>` and bare `addr` forms,
/// mirroring Go's `mail.ParseAddress`.
fn parse_one_address(value: &str) -> Result<MailAddress, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("empty address".to_string());
    }

    // Form: "Display Name" <addr@domain>  (or  Name <addr@domain>)
    if let (Some(lt), Some(gt)) = (trimmed.rfind('<'), trimmed.rfind('>')) {
        if lt < gt {
            let mut name = trimmed[..lt].trim().to_string();
            // Strip surrounding quotes from a quoted display name.
            if name.len() >= 2 && name.starts_with('"') && name.ends_with('"') {
                name = name[1..name.len() - 1].to_string();
            }
            let address = trimmed[lt + 1..gt].trim().to_string();
            if !is_valid_addr(&address) {
                return Err(format!("invalid address: {}", address));
            }
            return Ok(MailAddress { name, address });
        }
    }
    // Bare address form.
    if !is_valid_addr(trimmed) {
        return Err(format!("invalid address: {}", trimmed));
    }
    Ok(MailAddress {
        name: String::new(),
        address: trimmed.to_string(),
    })
}

pub(crate) fn is_valid_addr(addr: &str) -> bool {
    // Minimal local@domain check (one '@', non-empty local and domain).
    match addr.find('@') {
        Some(i) if i > 0 && i < addr.len() - 1 => !addr[i + 1..].is_empty(),
        _ => false,
    }
}

/// Format back into `Name <addr>` (or bare `addr` when no display name).
fn format_address(addr: &MailAddress) -> String {
    if addr.name.is_empty() {
        addr.address.clone()
    } else if addr.name.contains(',') || addr.name.contains('"') {
        // Quote when the display name would otherwise be ambiguous.
        format!("\"{}\" <{}>", addr.name.replace('"', "\\\""), addr.address)
    } else {
        format!("{} <{}>", addr.name, addr.address)
    }
}

/// Split an address list on top-level commas (commas inside quotes or
/// angle brackets are preserved).
fn split_address_list(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut buf = String::new();
    let mut in_quotes = false;
    let mut in_angle = false;
    for c in value.chars() {
        match c {
            '"' if !in_angle => {
                in_quotes = !in_quotes;
                buf.push(c);
            }
            '<' if !in_quotes => {
                in_angle = true;
                buf.push(c);
            }
            '>' if in_angle => {
                in_angle = false;
                buf.push(c);
            }
            ',' if !in_quotes && !in_angle => {
                let t = buf.trim().to_string();
                if !t.is_empty() {
                    parts.push(t);
                }
                buf.clear();
            }
            _ => buf.push(c),
        }
    }
    let last = buf.trim().to_string();
    if !last.is_empty() {
        parts.push(last);
    }
    parts
}

pub(crate) fn mail_address_object(addr: &MailAddress) -> Object {
    let hash = Rc::new(RefCell::new(HashData::default()));
    hash.borrow_mut().set("name", str_obj(addr.name.clone()));
    hash.borrow_mut()
        .set("address", str_obj(addr.address.clone()));
    Object::Hash(hash)
}

pub(crate) fn mail_address_from_value(
    ctx: &CallContext,
    name: &str,
    value: &Object,
) -> Result<MailAddress, Object> {
    match value {
        Object::String(s) => match parse_one_address(s) {
            Ok(a) => Ok(a),
            Err(e) => Err(new_error(ctx.pos.clone(), format!("{}: {}", name, e))),
        },
        Object::Hash(h) => {
            let address = match h.borrow().get("address") {
                Some(Object::String(s)) => s.to_string(),
                _ => {
                    return Err(new_error(
                        ctx.pos.clone(),
                        format!("{}: address.address is required", name),
                    ))
                }
            };
            if address.is_empty() {
                return Err(new_error(
                    ctx.pos.clone(),
                    format!("{}: address.address is required", name),
                ));
            }
            let display = match h.borrow().get("name") {
                Some(Object::String(s)) => s.to_string(),
                _ => String::new(),
            };
            Ok(MailAddress {
                name: display,
                address,
            })
        }
        _ => Err(new_error(
            ctx.pos.clone(),
            format!("{}: address must be a string or object", name),
        )),
    }
}

pub(crate) fn mail_parse_address(ctx: &mut CallContext, args: &[Object]) -> Object {
    let value = match required_string(ctx, "mail.parseAddress", args, 0, "address") {
        Ok(v) => v,
        Err(e) => return e,
    };
    match parse_one_address(&value) {
        Ok(a) => mail_address_object(&a),
        Err(e) => new_error(ctx.pos.clone(), format!("mail.parseAddress: {}", e)),
    }
}

pub(crate) fn mail_parse_address_list(ctx: &mut CallContext, args: &[Object]) -> Object {
    let value = match required_string(ctx, "mail.parseAddressList", args, 0, "addresses") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let mut out = Vec::new();
    for part in split_address_list(&value) {
        match parse_one_address(&part) {
            Ok(a) => out.push(mail_address_object(&a)),
            Err(e) => return new_error(ctx.pos.clone(), format!("mail.parseAddressList: {}", e)),
        }
    }
    array(out)
}

pub(crate) fn mail_parse_message(ctx: &mut CallContext, args: &[Object]) -> Object {
    let value = match required_string(ctx, "mail.parseMessage", args, 0, "message") {
        Ok(v) => v,
        Err(e) => return e,
    };
    // RFC 5322: headers then a blank line then the body.
    let (headers, body) = match value.find("\n\n") {
        Some(i) => (value[..i].to_string(), value[i + 2..].to_string()),
        None => match value.find("\r\n\r\n") {
            Some(i) => (value[..i].to_string(), value[i + 4..].to_string()),
            None => (value.clone(), String::new()),
        },
    };
    let headers_obj = parse_rfc5322_headers(&headers);
    let out = Rc::new(RefCell::new(HashData::default()));
    out.borrow_mut().set("headers", headers_obj);
    out.borrow_mut().set("body", str_obj(body));
    Object::Hash(out)
}

/// Parse a header block into a Hash mapping header name -> Array<string>.
/// Header unfolding (continuation lines starting with whitespace) is handled.
fn parse_rfc5322_headers(block: &str) -> Object {
    let hash = Rc::new(RefCell::new(HashData::default()));
    let mut current_name: Option<String> = None;
    let mut current_vals: Vec<String> = Vec::new();
    let flush =
        |name: &mut Option<String>, vals: &mut Vec<String>, hash: &Rc<RefCell<HashData>>| {
            if let Some(n) = name.take() {
                let arr: Vec<Object> = vals.drain(..).map(str_obj).collect();
                hash.borrow_mut().set(n, array(arr));
            }
        };
    for raw_line in block.lines() {
        if raw_line.starts_with(' ') || raw_line.starts_with('\t') {
            // Continuation of previous header value.
            if let Some(name) = &current_name {
                let _ = name; // suppress unused warnings
                if let Some(last) = current_vals.last_mut() {
                    last.push(' ');
                    last.push_str(raw_line.trim());
                }
            }
            continue;
        }
        flush(&mut current_name, &mut current_vals, &hash);
        match raw_line.find(':') {
            Some(i) => {
                current_name = Some(raw_line[..i].trim().to_string());
                current_vals.push(raw_line[i + 1..].trim().to_string());
            }
            None => {
                current_name = None;
                current_vals.clear();
            }
        }
    }
    flush(&mut current_name, &mut current_vals, &hash);
    Object::Hash(hash)
}

pub(crate) fn mail_format_address(ctx: &mut CallContext, args: &[Object]) -> Object {
    let addr = match args.first() {
        Some(v) => v,
        None => return new_error(ctx.pos.clone(), "mail.formatAddress requires address"),
    };
    match mail_address_from_value(ctx, "mail.formatAddress", addr) {
        Ok(a) => str_obj(format_address(&a)),
        Err(e) => e,
    }
}

pub(crate) fn mail_format_address_list(ctx: &mut CallContext, args: &[Object]) -> Object {
    let arr = match args.first() {
        Some(Object::Array(a)) => a.clone(),
        Some(_) => {
            return new_error(
                ctx.pos.clone(),
                "mail.formatAddressList: addresses must be an array",
            )
        }
        None => return new_error(ctx.pos.clone(), "mail.formatAddressList requires addresses"),
    };
    let mut formatted = Vec::new();
    for item in &arr.borrow().elements {
        match mail_address_from_value(ctx, "mail.formatAddressList", item) {
            Ok(a) => formatted.push(format_address(&a)),
            Err(e) => return e,
        }
    }
    str_obj(formatted.join(", "))
}

pub(crate) fn mail_parse_date(ctx: &mut CallContext, args: &[Object]) -> Object {
    let value = match required_string(ctx, "mail.parseDate", args, 0, "date") {
        Ok(v) => v,
        Err(e) => return e,
    };
    match parse_time_ms(&value) {
        Some(ms) => Object::Date(ms),
        None => new_error(
            ctx.pos.clone(),
            format!("mail.parseDate: unsupported date {}", value),
        ),
    }
}

pub(crate) fn mail_format_date(ctx: &mut CallContext, args: &[Object]) -> Object {
    let ms = match time_from_object(ctx, "mail.formatDate", args, 0) {
        Ok(ms) => ms,
        Err(_err) => now_ms(),
    };
    str_obj(format_rfc1123z(ms))
}

/// Format an epoch-millis instant as an RFC 1123 date with a numeric zone,
/// e.g. `Mon, 02 Jan 2006 15:04:05 -0700`. Uses UTC (+0000) because the GTS
/// time module renders in UTC throughout.
fn format_rfc1123z(ms: i64) -> String {
    let (year, month, day, hour, minute, second, _ms) = utc_parts_from_ms(ms);
    let days = ms.div_euclid(86_400_000);
    let weekday = weekday_short(days);
    let month_name = month_short(month);
    format!("{weekday}, {day:02} {month_name} {year:04} {hour:02}:{minute:02}:{second:02} +0000")
}

/// Return the 3-letter weekday for a `days-since-1970-01-01` count.
/// 1970-01-01 was a Thursday.
fn weekday_short(days: i64) -> &'static str {
    let names = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    let idx = days.rem_euclid(7) as usize;
    names[idx]
}

pub(crate) fn month_short(month: u32) -> &'static str {
    match month {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        _ => "Dec",
    }
}

pub(crate) fn mail_get_header(ctx: &mut CallContext, args: &[Object]) -> Object {
    let headers = match args.first() {
        Some(Object::Hash(h)) => h.clone(),
        Some(_) => return new_error(ctx.pos.clone(), "mail.getHeader: headers must be an object"),
        None => return new_error(ctx.pos.clone(), "mail.getHeader requires headers"),
    };
    let name = match required_string(ctx, "mail.getHeader", args, 1, "name") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let lower = name.to_ascii_lowercase();
    let hb = headers.borrow();
    for (k, v) in &hb.entries {
        if k.to_ascii_lowercase() == lower {
            if let Object::Array(arr) = v {
                let elems = &arr.borrow().elements;
                if elems.is_empty() {
                    return Object::Undefined;
                }
                return elems[0].clone();
            }
            return v.clone();
        }
    }
    Object::Undefined
}

// ---------------------------------------------------------------------------
// net/socket/client: synchronous TCP client (@std/net/socket/client)
// ---------------------------------------------------------------------------
