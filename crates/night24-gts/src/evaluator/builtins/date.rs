use std::rc::Rc;

use crate::object::*;

pub fn date_method(name: &str) -> Option<BuiltinFn> {
    let f: Option<fn(&mut CallContext, &[Object]) -> Object> = match name {
        "getTime" | "valueOf" => Some(date_get_time),
        "getFullYear" => Some(date_get_full_year),
        "getMonth" => Some(date_get_month),
        "getDate" => Some(date_get_date),
        "getDay" => Some(date_get_day),
        "getHours" => Some(date_get_hours),
        "getMinutes" => Some(date_get_minutes),
        "getSeconds" => Some(date_get_seconds),
        "getMilliseconds" => Some(date_get_milliseconds),
        "toISOString" => Some(date_to_iso_string),
        "toDateString" => Some(date_to_date_string),
        "toTimeString" => Some(date_to_time_string),
        "toString" => Some(date_to_string),
        _ => None,
    };
    f.map(|f| Rc::new(f) as BuiltinFn)
}

fn active_date(ctx: &CallContext) -> Option<i64> {
    ctx.receiver.as_ref().and_then(|t| {
        if let Object::Date(ms) = t {
            Some(*ms)
        } else {
            None
        }
    })
}

fn utc_parts(ms: i64) -> (i32, u32, u32, u32, u32, u32, u32) {
    crate::stdlib::utc_parts_from_ms(ms)
}

fn date_get_time(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(ms) = active_date(ctx) {
        return Object::Number(ms as f64);
    }
    Object::Undefined
}

fn date_get_full_year(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(ms) = active_date(ctx) {
        let (year, _, _, _, _, _, _) = utc_parts(ms);
        return Object::Number(year as f64);
    }
    Object::Undefined
}

fn date_get_month(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(ms) = active_date(ctx) {
        let (_, month, _, _, _, _, _) = utc_parts(ms);
        return Object::Number((month - 1) as f64);
    }
    Object::Undefined
}

fn date_get_date(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(ms) = active_date(ctx) {
        let (_, _, day, _, _, _, _) = utc_parts(ms);
        return Object::Number(day as f64);
    }
    Object::Undefined
}

fn date_get_day(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(ms) = active_date(ctx) {
        let days = ms / 86400000;
        let day_of_week = ((days + 4) % 7 + 7) % 7;
        return Object::Number(day_of_week as f64);
    }
    Object::Undefined
}

fn date_get_hours(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(ms) = active_date(ctx) {
        let (_, _, _, hour, _, _, _) = utc_parts(ms);
        return Object::Number(hour as f64);
    }
    Object::Undefined
}

fn date_get_minutes(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(ms) = active_date(ctx) {
        let (_, _, _, _, minute, _, _) = utc_parts(ms);
        return Object::Number(minute as f64);
    }
    Object::Undefined
}

fn date_get_seconds(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(ms) = active_date(ctx) {
        let (_, _, _, _, _, second, _) = utc_parts(ms);
        return Object::Number(second as f64);
    }
    Object::Undefined
}

fn date_get_milliseconds(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(ms) = active_date(ctx) {
        let (_, _, _, _, _, _, millisecond) = utc_parts(ms);
        return Object::Number(millisecond as f64);
    }
    Object::Undefined
}

fn date_to_iso_string(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(ms) = active_date(ctx) {
        return str_obj(crate::stdlib::format_epoch_ms_utc(ms));
    }
    str_obj("")
}

fn date_to_date_string(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(ms) = active_date(ctx) {
        let (year, month, day, _, _, _, _) = utc_parts(ms);
        let day_names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        let month_names = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        let days = ms / 86400000;
        let day_of_week = ((days + 4) % 7 + 7) % 7;
        return str_obj(format!(
            "{} {} {:02} {}",
            day_names[day_of_week as usize],
            month_names[(month - 1) as usize],
            day,
            year
        ));
    }
    str_obj("")
}

fn date_to_time_string(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(ms) = active_date(ctx) {
        let (_, _, _, hour, minute, second, _) = utc_parts(ms);
        return str_obj(format!("{:02}:{:02}:{:02} GMT", hour, minute, second));
    }
    str_obj("")
}

fn date_to_string(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(ms) = active_date(ctx) {
        return str_obj(crate::stdlib::format_epoch_ms_utc(ms));
    }
    str_obj("")
}
