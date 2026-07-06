use super::super::helpers::*;
use crate::object::{new_error, CallContext, Object};

pub(crate) fn retry_module() -> Object {
    module(vec![
        ("run", native("retry.run", retry_run)),
        (
            "exponential",
            native("retry.exponential", retry_exponential),
        ),
    ])
}

pub(crate) fn retry_run(ctx: &mut CallContext, args: &[Object]) -> Object {
    let func = match args.first() {
        Some(value) if is_callable(value) => value.clone(),
        Some(_) => return new_error(ctx.pos.clone(), "retry.run expects function"),
        None => return new_error(ctx.pos.clone(), "retry.run requires function"),
    };
    let (times, mut delay, backoff) = parse_retry_opts(args.get(1), 3, 0.0, 1.0);
    let mut last_err = Object::Undefined;
    for i in 0..times {
        let result = call_script_function(&func, ctx.env, &[]);
        if !result.is_runtime_error() {
            return result;
        }
        last_err = result;
        if i + 1 < times && delay > 0 {
            std::thread::sleep(std::time::Duration::from_millis(delay as u64));
            delay = (delay as f64 * backoff) as i64;
        }
    }
    last_err
}

pub(crate) fn retry_exponential(ctx: &mut CallContext, args: &[Object]) -> Object {
    let func = match args.first() {
        Some(value) if is_callable(value) => value.clone(),
        Some(_) => return new_error(ctx.pos.clone(), "retry.exponential expects function"),
        None => return new_error(ctx.pos.clone(), "retry.exponential requires function"),
    };
    let (times, mut delay) = parse_retry_opts_exp(args.get(1), 5, 1000);
    let mut last_err = Object::Undefined;
    for i in 0..times {
        let result = call_script_function(&func, ctx.env, &[]);
        if !result.is_runtime_error() {
            return result;
        }
        last_err = result;
        if i + 1 < times && delay > 0 {
            std::thread::sleep(std::time::Duration::from_millis(delay as u64));
            delay *= 2;
        }
    }
    last_err
}

pub(crate) fn parse_retry_opts(
    opts: Option<&Object>,
    default_times: usize,
    default_delay: f64,
    default_backoff: f64,
) -> (usize, i64, f64) {
    match opts {
        Some(Object::Hash(h)) => {
            let hash = h.borrow();
            let opts = ObjectView::new(&hash);
            let times = opts
                .number("times")
                .map(|value| value as usize)
                .unwrap_or(default_times);
            let delay = opts
                .number("delay")
                .map(|value| value as i64)
                .unwrap_or(default_delay as i64);
            let backoff = opts.number("backoff").unwrap_or(default_backoff);
            (times, delay, backoff)
        }
        _ => (default_times, default_delay as i64, default_backoff),
    }
}

pub(crate) fn parse_retry_opts_exp(
    opts: Option<&Object>,
    default_times: usize,
    default_delay: i64,
) -> (usize, i64) {
    match opts {
        Some(Object::Hash(h)) => {
            let hash = h.borrow();
            let opts = ObjectView::new(&hash);
            let times = opts
                .number("times")
                .map(|value| value as usize)
                .unwrap_or(default_times);
            let delay = opts
                .number("initialDelay")
                .map(|value| value as i64)
                .unwrap_or(default_delay);
            (times, delay)
        }
        _ => (default_times, default_delay),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_retry_opts_reads_numeric_object_fields() {
        let opts = ObjectBuilder::new()
            .set("times", Object::Number(7.0))
            .set("delay", Object::Number(250.0))
            .set("backoff", Object::Number(1.5))
            .build();

        assert_eq!(parse_retry_opts(Some(&opts), 3, 0.0, 1.0), (7, 250, 1.5));
    }

    #[test]
    fn parse_retry_opts_keeps_defaults_for_missing_or_non_object_fields() {
        let opts = ObjectBuilder::new()
            .set("times", Object::Boolean(true))
            .build();

        assert_eq!(parse_retry_opts(Some(&opts), 3, 10.0, 2.0), (3, 10, 2.0));
        assert_eq!(
            parse_retry_opts(Some(&Object::Undefined), 4, 20.0, 3.0),
            (4, 20, 3.0)
        );
    }

    #[test]
    fn parse_retry_opts_exp_reads_numeric_object_fields() {
        let opts = ObjectBuilder::new()
            .set("times", Object::Number(9.0))
            .set("initialDelay", Object::Number(500.0))
            .build();

        assert_eq!(parse_retry_opts_exp(Some(&opts), 5, 1000), (9, 500));
    }

    #[test]
    fn parse_retry_opts_exp_keeps_defaults_for_missing_or_non_object_fields() {
        let opts = ObjectBuilder::new()
            .set("initialDelay", Object::Boolean(true))
            .build();

        assert_eq!(parse_retry_opts_exp(Some(&opts), 5, 1000), (5, 1000));
        assert_eq!(
            parse_retry_opts_exp(Some(&Object::Undefined), 6, 2000),
            (6, 2000)
        );
    }
}

// ---------------------------------------------------------------------------
// stream: a synchronous readable stream over a string.
// ---------------------------------------------------------------------------
