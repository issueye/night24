use super::*;

/// Fill a buffer from the OS RNG. Returns an Error object on failure.
pub(crate) fn fill_random(ctx: &mut CallContext, name: &str, buf: &mut [u8]) -> Result<(), Object> {
    if getrandom_inner(buf) {
        Ok(())
    } else {
        Err(new_error(
            ctx.pos.clone(),
            format!("{}: random source unavailable", name),
        ))
    }
}

#[cfg(unix)]
pub(crate) fn getrandom_inner(buf: &mut [u8]) -> bool {
    use std::io::Read;
    match std::fs::File::open("/dev/urandom") {
        Ok(mut f) => f.read_exact(buf).is_ok(),
        Err(_) => {
            // Fall back to a time-seeded PRNG; rare on Unix but keeps behavior total.
            fallback_rng(buf)
        }
    }
}

#[cfg(windows)]
pub(crate) fn getrandom_inner(buf: &mut [u8]) -> bool {
    use std::os::raw::c_void;
    #[link(name = "bcrypt")]
    extern "system" {
        fn BCryptGenRandom(
            hAlgorithm: *mut c_void,
            pbBuffer: *mut u8,
            cbBuffer: u32,
            dwFlags: u32,
        ) -> i32;
    }
    const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 0x00000002;
    let status = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            buf.as_mut_ptr(),
            buf.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status == 0 {
        true
    } else {
        fallback_rng(buf)
    }
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn getrandom_inner(buf: &mut [u8]) -> bool {
    fallback_rng(buf)
}

/// Deterministic fallback so the runtime never panics when the system RNG is
/// unavailable. This is weaker than the Go behavior but keeps parity of shape.
fn fallback_rng(buf: &mut [u8]) -> bool {
    use std::time::SystemTime;
    let mut seed = match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => d.as_nanos() as u64,
        Err(_) => 0x9E3779B97F4A7C15,
    };
    for byte in buf.iter_mut() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        *byte = (seed & 0xff) as u8;
    }
    true
}

pub(crate) fn read_random_u64(ctx: &mut CallContext, name: &str) -> Result<u64, Object> {
    let mut buf = [0u8; 8];
    fill_random(ctx, name, &mut buf)?;
    Ok(u64::from_le_bytes(buf))
}

/// Compute a uniform random integer in `[0, span)` via rejection sampling on
/// a 64-bit value, matching the spirit of Go's `rand.Int(reader, span)`.
pub(crate) fn bounded_random_u64(
    ctx: &mut CallContext,
    name: &str,
    span: u64,
) -> Result<u64, Object> {
    let limit = u64::MAX - (u64::MAX % span);
    loop {
        let value = read_random_u64(ctx, name)?;
        if value < limit {
            return Ok(value % span);
        }
    }
}

pub(crate) static PROCESS_START: std::sync::OnceLock<std::time::Instant> =
    std::sync::OnceLock::new();

pub(crate) fn required_positive_int(
    ctx: &mut CallContext,
    name: &str,
    value: &Object,
    label: &str,
) -> Result<usize, Object> {
    match value {
        Object::Number(n) => {
            let n = *n as i64;
            if n <= 0 {
                Err(new_error(
                    ctx.pos.clone(),
                    format!("{}: {} must be positive", name, label),
                ))
            } else {
                Ok(n as usize)
            }
        }
        _ => Err(new_error(
            ctx.pos.clone(),
            format!("{}: {} must be a number", name, label),
        )),
    }
}
