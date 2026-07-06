use super::super::helpers::*;
use crate::object::{new_error, num_obj, str_obj, CallContext, Object};

pub(crate) fn time_module() -> Object {
    module(vec![
        (
            "now",
            native("time.now", |_ctx, _args| Object::Date(now_ms())),
        ),
        (
            "nowMs",
            native("time.nowMs", |_ctx, _args| num_obj(now_ms() as f64)),
        ),
        ("unix", native("time.unix", time_unix)),
        ("unixMs", native("time.unixMs", time_unix_ms)),
        ("parse", native("time.parse", time_parse)),
        ("format", native("time.format", time_format)),
        ("add", native("time.add", time_add)),
        ("since", native("time.since", time_since)),
        ("until", native("time.until", time_until)),
        (
            "parseDuration",
            native("time.parseDuration", time_parse_duration),
        ),
        ("duration", native("time.duration", time_duration)),
        ("sleep", native("time.sleep", time_sleep)),
        ("RFC3339", str_obj("2006-01-02T15:04:05Z07:00")),
        (
            "RFC3339Nano",
            str_obj("2006-01-02T15:04:05.999999999Z07:00"),
        ),
        ("RFC1123", str_obj("Mon, 02 Jan 2006 15:04:05 MST")),
        ("RFC1123Z", str_obj("Mon, 02 Jan 2006 15:04:05 -0700")),
        ("UnixDate", str_obj("Mon Jan _2 15:04:05 MST 2006")),
        ("DateTime", str_obj("2006-01-02 15:04:05")),
        ("DateOnly", str_obj("2006-01-02")),
        ("TimeOnly", str_obj("15:04:05")),
        ("Kitchen", str_obj("3:04PM")),
    ])
}

pub(crate) fn time_unix(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "time.unix", args);
    let seconds = match reader.required_number(0, "seconds") {
        Ok(seconds) => seconds,
        Err(err) => return err,
    };
    let nanos = match args.get(1) {
        Some(Object::Number(value)) => *value,
        Some(_) => return new_error(ctx.pos.clone(), "time.unix: nanoseconds must be a number"),
        None => 0.0,
    };
    Object::Date((seconds * 1000.0 + nanos / 1_000_000.0) as i64)
}

pub(crate) fn time_unix_ms(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "time.unixMs", args);
    match reader.required_number(0, "milliseconds") {
        Ok(ms) => Object::Date(ms as i64),
        Err(err) => err,
    }
}

pub(crate) fn time_parse(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "time.parse", args);
    let value = match reader.required_string(0, "value") {
        Ok(value) => value,
        Err(err) => return err,
    };
    match parse_time_ms(&value) {
        Some(ms) => Object::Date(ms),
        None => new_error(
            ctx.pos.clone(),
            format!("time.parse: unsupported time {}", value),
        ),
    }
}

pub(crate) fn time_format(ctx: &mut CallContext, args: &[Object]) -> Object {
    let ms = match time_from_object(ctx, "time.format", args, 0) {
        Ok(ms) => ms,
        Err(err) => return err,
    };
    let layout = match args.get(1) {
        Some(Object::String(value)) => value.as_str(),
        Some(Object::Undefined | Object::Null) | None => "2006-01-02T15:04:05Z07:00",
        Some(_) => return new_error(ctx.pos.clone(), "time.format: layout must be a string"),
    };
    str_obj(format_time_layout(ms, layout))
}

pub(crate) fn time_add(ctx: &mut CallContext, args: &[Object]) -> Object {
    let ms = match time_from_object(ctx, "time.add", args, 0) {
        Ok(ms) => ms,
        Err(err) => return err,
    };
    let duration = match duration_from_object(ctx, "time.add", args, 1) {
        Ok(duration) => duration,
        Err(err) => return err,
    };
    Object::Date(ms + duration)
}

pub(crate) fn time_since(ctx: &mut CallContext, args: &[Object]) -> Object {
    match time_from_object(ctx, "time.since", args, 0) {
        Ok(ms) => num_obj((now_ms() - ms) as f64),
        Err(err) => err,
    }
}

pub(crate) fn time_until(ctx: &mut CallContext, args: &[Object]) -> Object {
    match time_from_object(ctx, "time.until", args, 0) {
        Ok(ms) => num_obj((ms - now_ms()) as f64),
        Err(err) => err,
    }
}

pub(crate) fn time_parse_duration(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "time.parseDuration", args);
    match reader.required_string(0, "duration") {
        Ok(value) => match parse_duration_ms(&value) {
            Some(ms) => duration_object(ms),
            None => new_error(
                ctx.pos.clone(),
                format!("time.parseDuration: invalid duration {}", value),
            ),
        },
        Err(err) => err,
    }
}

pub(crate) fn time_duration(ctx: &mut CallContext, args: &[Object]) -> Object {
    match duration_from_object(ctx, "time.duration", args, 0) {
        Ok(ms) => duration_object(ms),
        Err(err) => err,
    }
}

pub(crate) fn time_sleep(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "time.sleep", args);
    let ms = match reader.required_number(0, "milliseconds") {
        Ok(ms) => ms.max(0.0) as u64,
        Err(err) => return err,
    };
    std::thread::sleep(std::time::Duration::from_millis(ms));
    Object::Undefined
}

pub(crate) fn time_from_object(
    ctx: &CallContext,
    module: &str,
    args: &[Object],
    index: usize,
) -> Result<i64, Object> {
    match args.get(index) {
        Some(Object::Date(ms)) => Ok(*ms),
        Some(Object::Number(ms)) => Ok(*ms as i64),
        Some(Object::String(value)) => parse_time_ms(value).ok_or_else(|| {
            new_error(
                ctx.pos.clone(),
                format!("{}: unsupported time {}", module, value),
            )
        }),
        Some(_) => Err(new_error(
            ctx.pos.clone(),
            format!(
                "{}: time must be a Date, number milliseconds, or string",
                module
            ),
        )),
        None => Err(new_error(
            ctx.pos.clone(),
            format!("{} requires time", module),
        )),
    }
}

pub(crate) fn duration_from_object(
    ctx: &CallContext,
    module: &str,
    args: &[Object],
    index: usize,
) -> Result<i64, Object> {
    match args.get(index) {
        Some(Object::Number(ms)) => Ok(*ms as i64),
        Some(Object::String(value)) => parse_duration_ms(value).ok_or_else(|| {
            new_error(
                ctx.pos.clone(),
                format!("{}: invalid duration {}", module, value),
            )
        }),
        Some(_) => Err(new_error(
            ctx.pos.clone(),
            format!(
                "{}: duration must be a number of milliseconds or Go duration string",
                module
            ),
        )),
        None => Err(new_error(
            ctx.pos.clone(),
            format!("{} requires duration", module),
        )),
    }
}

pub(crate) fn parse_time_ms(value: &str) -> Option<i64> {
    parse_rfc3339_ms(value)
        .or_else(|| parse_datetime_ms(value))
        .or_else(|| parse_date_only_ms(value))
}

pub(crate) fn parse_rfc3339_ms(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.len() < 20 {
        return None;
    }
    let year = parse_i32(value.get(0..4)?)?;
    expect_byte(bytes, 4, b'-')?;
    let month = parse_u32(value.get(5..7)?)?;
    expect_byte(bytes, 7, b'-')?;
    let day = parse_u32(value.get(8..10)?)?;
    let sep = *bytes.get(10)?;
    if sep != b'T' && sep != b't' && sep != b' ' {
        return None;
    }
    let hour = parse_u32(value.get(11..13)?)?;
    expect_byte(bytes, 13, b':')?;
    let minute = parse_u32(value.get(14..16)?)?;
    expect_byte(bytes, 16, b':')?;
    let second = parse_u32(value.get(17..19)?)?;
    let mut pos = 19usize;
    let mut millis = 0i64;
    if bytes.get(pos) == Some(&b'.') {
        pos += 1;
        let start = pos;
        while pos < bytes.len() && bytes[pos].is_ascii_digit() {
            pos += 1;
        }
        let fraction = value.get(start..pos)?;
        let mut ms_text = fraction.chars().take(3).collect::<String>();
        while ms_text.len() < 3 {
            ms_text.push('0');
        }
        millis = ms_text.parse::<i64>().ok()?;
    }
    let offset_ms = if bytes.get(pos) == Some(&b'Z') || bytes.get(pos) == Some(&b'z') {
        0
    } else if matches!(bytes.get(pos), Some(b'+' | b'-')) {
        let sign = if bytes[pos] == b'+' { 1 } else { -1 };
        let off_hour = parse_i32(value.get(pos + 1..pos + 3)?)?;
        expect_byte(bytes, pos + 3, b':')?;
        let off_min = parse_i32(value.get(pos + 4..pos + 6)?)?;
        sign * ((off_hour * 60 + off_min) as i64) * 60_000
    } else {
        return None;
    };
    let base = utc_ms_from_parts(year, month, day, hour, minute, second, millis)?;
    Some(base - offset_ms)
}

pub(crate) fn parse_datetime_ms(value: &str) -> Option<i64> {
    if value.len() != 19 {
        return None;
    }
    let bytes = value.as_bytes();
    let year = parse_i32(value.get(0..4)?)?;
    expect_byte(bytes, 4, b'-')?;
    let month = parse_u32(value.get(5..7)?)?;
    expect_byte(bytes, 7, b'-')?;
    let day = parse_u32(value.get(8..10)?)?;
    expect_byte(bytes, 10, b' ')?;
    let hour = parse_u32(value.get(11..13)?)?;
    expect_byte(bytes, 13, b':')?;
    let minute = parse_u32(value.get(14..16)?)?;
    expect_byte(bytes, 16, b':')?;
    let second = parse_u32(value.get(17..19)?)?;
    utc_ms_from_parts(year, month, day, hour, minute, second, 0)
}

pub(crate) fn parse_date_only_ms(value: &str) -> Option<i64> {
    if value.len() != 10 {
        return None;
    }
    let bytes = value.as_bytes();
    let year = parse_i32(value.get(0..4)?)?;
    expect_byte(bytes, 4, b'-')?;
    let month = parse_u32(value.get(5..7)?)?;
    expect_byte(bytes, 7, b'-')?;
    let day = parse_u32(value.get(8..10)?)?;
    utc_ms_from_parts(year, month, day, 0, 0, 0, 0)
}

pub(crate) fn expect_byte(bytes: &[u8], index: usize, expected: u8) -> Option<()> {
    if bytes.get(index) == Some(&expected) {
        Some(())
    } else {
        None
    }
}

pub(crate) fn parse_i32(value: &str) -> Option<i32> {
    value.parse::<i32>().ok()
}

pub(crate) fn parse_u32(value: &str) -> Option<u32> {
    value.parse::<u32>().ok()
}

pub(crate) fn utc_ms_from_parts(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    millisecond: i64,
) -> Option<i64> {
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some(
        days * 86_400_000
            + hour as i64 * 3_600_000
            + minute as i64 * 60_000
            + second as i64 * 1_000
            + millisecond,
    )
}

pub(crate) fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = year - if month <= 2 { 1 } else { 0 };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month = month as i32;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day as i32 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146097 + doe - 719468) as i64
}

pub(crate) fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let days = days + 719468;
    let era = if days >= 0 { days } else { days - 146096 } / 146097;
    let doe = days - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = yoe as i32 + era as i32 * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };
    (year, month as u32, day as u32)
}

pub(crate) fn utc_parts_from_ms(ms: i64) -> (i32, u32, u32, u32, u32, u32, u32) {
    let days = ms.div_euclid(86_400_000);
    let day_ms = ms.rem_euclid(86_400_000);
    let (year, month, day) = civil_from_days(days);
    let hour = (day_ms / 3_600_000) as u32;
    let minute = ((day_ms % 3_600_000) / 60_000) as u32;
    let second = ((day_ms % 60_000) / 1_000) as u32;
    let millisecond = (day_ms % 1_000) as u32;
    (year, month, day, hour, minute, second, millisecond)
}

pub(crate) fn ms_from_utc_parts(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    millisecond: u32,
) -> i64 {
    // Calculate days since epoch
    let mut y = year as i64;
    let m = month as i64;

    // Adjust year and month (months are 1-12)
    y += (m - 1) / 12;
    let month_adj = ((m - 1) % 12 + 12) % 12 + 1;

    // Days in each month (non-leap year)
    let days_in_month = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

    // Calculate days since epoch (1970-01-01)
    let mut days = (y - 1970) * 365;

    // Add leap days
    if y > 1970 {
        days += (y - 1969) / 4;
        days -= (y - 1901) / 100;
        days += (y - 1601) / 400;
    } else if y < 1970 {
        days += (y - 1972) / 4;
        days -= (y - 2000) / 100;
        days += (y - 2000) / 400;
    }

    // Add days for months
    for i in 1..(month_adj as usize) {
        days += days_in_month[i - 1] as i64;
    }

    // Add extra day if leap year and month > February
    let is_leap = (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0);
    if is_leap && month_adj > 2 {
        days += 1;
    }

    // Add day of month (1-indexed, so subtract 1)
    days += day as i64 - 1;

    // Convert to milliseconds

    days * 86400000
        + (hour as i64) * 3600000
        + (minute as i64) * 60000
        + (second as i64) * 1000
        + (millisecond as i64)
}

pub(crate) fn format_epoch_ms_utc(ms: i64) -> String {
    let (year, month, day, hour, minute, second, millisecond) = utc_parts_from_ms(ms);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millisecond:03}Z")
}

pub(crate) fn format_time_layout(ms: i64, layout: &str) -> String {
    let (year, month, day, hour, minute, second, millisecond) = utc_parts_from_ms(ms);
    match layout {
        "2006-01-02T15:04:05Z07:00" => {
            format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
        }
        "2006-01-02T15:04:05.999999999Z07:00" => format_epoch_ms_utc(ms),
        "2006-01-02 15:04:05" => {
            format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}")
        }
        "2006-01-02" => format!("{year:04}-{month:02}-{day:02}"),
        "15:04:05" => format!("{hour:02}:{minute:02}:{second:02}"),
        "3:04PM" => {
            let suffix = if hour < 12 { "AM" } else { "PM" };
            let hour12 = match hour % 12 {
                0 => 12,
                value => value,
            };
            format!("{hour12}:{minute:02}{suffix}")
        }
        _ => {
            let _ = millisecond;
            format_epoch_ms_utc(ms)
        }
    }
}

pub(crate) fn parse_duration_ms(value: &str) -> Option<i64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(ms) = trimmed.parse::<f64>() {
        return Some(ms as i64);
    }

    let bytes = trimmed.as_bytes();
    let mut pos = 0usize;
    let mut total = 0.0f64;
    while pos < bytes.len() {
        let start = pos;
        if bytes[pos] == b'+' || bytes[pos] == b'-' {
            pos += 1;
        }
        while pos < bytes.len() && (bytes[pos].is_ascii_digit() || bytes[pos] == b'.') {
            pos += 1;
        }
        if pos == start || (pos == start + 1 && matches!(bytes[start], b'+' | b'-')) {
            return None;
        }
        let amount = trimmed[start..pos].parse::<f64>().ok()?;
        let unit_start = pos;
        while pos < bytes.len() && !bytes[pos].is_ascii_digit() && bytes[pos] != b'.' {
            pos += 1;
        }
        let unit = &trimmed[unit_start..pos];
        let factor = match unit {
            "ns" => 0.000001,
            "us" | "µs" => 0.001,
            "ms" => 1.0,
            "s" => 1_000.0,
            "m" => 60_000.0,
            "h" => 3_600_000.0,
            "d" => 86_400_000.0,
            _ => return None,
        };
        total += amount * factor;
    }
    Some(total as i64)
}

pub(crate) fn duration_object(ms: i64) -> Object {
    module(vec![
        ("nanoseconds", num_obj(ms as f64 * 1_000_000.0)),
        ("microseconds", num_obj(ms as f64 * 1_000.0)),
        ("milliseconds", num_obj(ms as f64)),
        ("ms", num_obj(ms as f64)),
        ("seconds", num_obj(ms as f64 / 1_000.0)),
        ("minutes", num_obj(ms as f64 / 60_000.0)),
        ("hours", num_obj(ms as f64 / 3_600_000.0)),
        ("string", str_obj(format_duration(ms))),
        (
            "toString",
            native("time.duration.toString", move |_ctx, _args| {
                str_obj(format_duration(ms))
            }),
        ),
    ])
}

pub(crate) fn format_duration(ms: i64) -> String {
    if ms % 3_600_000 == 0 {
        format!("{}h", ms / 3_600_000)
    } else if ms % 60_000 == 0 {
        format!("{}m", ms / 60_000)
    } else if ms % 1_000 == 0 {
        format!("{}s", ms / 1_000)
    } else {
        format!("{}ms", ms)
    }
}
