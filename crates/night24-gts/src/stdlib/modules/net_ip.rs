use super::super::helpers::*;
use crate::object::{bool_obj, new_error, num_obj, str_obj, CallContext, Object};

pub(crate) fn net_ip_module() -> Object {
    module(vec![
        ("parseIP", native("netip.parseIP", net_ip_parse)),
        ("parseCIDR", native("netip.parseCIDR", net_ip_parse_cidr)),
        ("contains", native("netip.contains", net_ip_contains)),
        (
            "splitHostPort",
            native("netip.splitHostPort", net_ip_split_host_port),
        ),
        (
            "joinHostPort",
            native("netip.joinHostPort", net_ip_join_host_port),
        ),
        ("lookupHost", native("netip.lookupHost", net_ip_lookup_host)),
    ])
}

pub(crate) fn net_ip_parse(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "netip.parseIP", args);
    let text = match reader.required_string(0, "ip") {
        Ok(v) => v,
        Err(e) => return e,
    };
    match parse_ip_addr(&text) {
        Some(addr) => ip_addr_to_object(&addr),
        None => Object::Undefined,
    }
}

pub(crate) fn net_ip_parse_cidr(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "netip.parseCIDR", args);
    let text = match reader.required_string(0, "cidr") {
        Ok(v) => v,
        Err(e) => return e,
    };
    match parse_ip_cidr(&text) {
        Some(prefix) => ip_prefix_to_object(&prefix),
        None => Object::Undefined,
    }
}

pub(crate) fn net_ip_contains(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "netip.contains", args);
    let cidr = match reader.required_string(0, "cidr") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let ip = match reader.required_string(1, "ip") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let prefix = match parse_ip_cidr(&cidr) {
        Some(p) => p,
        None => return new_error(ctx.pos.clone(), "netip.contains: invalid cidr"),
    };
    let addr = match parse_ip_addr(&ip) {
        Some(a) => a,
        None => return new_error(ctx.pos.clone(), "netip.contains: invalid ip"),
    };
    bool_obj(ip_cidr_contains(&prefix, &addr))
}

pub(crate) fn net_ip_split_host_port(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "netip.splitHostPort", args);
    let address = match reader.required_string(0, "address") {
        Ok(v) => v,
        Err(e) => return e,
    };
    match split_host_port(&address) {
        Ok((host, port)) => ObjectBuilder::new()
            .set("host", str_obj(host))
            .set("port", str_obj(port))
            .build(),
        Err(e) => new_error(ctx.pos.clone(), format!("netip.splitHostPort: {}", e)),
    }
}

pub(crate) fn net_ip_join_host_port(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "netip.joinHostPort", args);
    let host = match reader.required_string(0, "host") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let port = match reader.required_string(1, "port") {
        Ok(v) => v,
        Err(e) => return e,
    };
    str_obj(join_host_port(&host, &port))
}

pub(crate) fn net_ip_lookup_host(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "netip.lookupHost", args);
    let host = match reader.required_string(0, "host") {
        Ok(v) => v,
        Err(e) => return e,
    };
    match std::net::ToSocketAddrs::to_socket_addrs(&(host.as_str(), 0u16)) {
        Ok(iter) => {
            let addrs: Vec<Object> = iter.map(|sa| str_obj(sa.ip().to_string())).collect();
            array(addrs)
        }
        Err(e) => new_error(ctx.pos.clone(), format!("netip.lookupHost: {}", e)),
    }
}

pub(crate) struct IpAddr {
    bytes: Vec<u8>,
    is_v6: bool,
}

impl IpAddr {
    fn is_loopback(&self) -> bool {
        if self.is_v6 {
            self.bytes == vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]
        } else {
            self.bytes == vec![127, 0, 0, 1]
        }
    }
}

pub(crate) fn parse_ip_addr(text: &str) -> Option<IpAddr> {
    if let Ok(v4) = std::net::Ipv4Addr::from_str(text) {
        return Some(IpAddr {
            bytes: v4.octets().to_vec(),
            is_v6: false,
        });
    }
    if let Ok(v6) = std::net::Ipv6Addr::from_str(text) {
        return Some(IpAddr {
            bytes: v6.octets().to_vec(),
            is_v6: true,
        });
    }
    None
}

pub(crate) struct IpPrefix {
    addr: IpAddr,
    bits: u32,
}

pub(crate) fn parse_ip_cidr(text: &str) -> Option<IpPrefix> {
    let (addr_part, bits_part) = text.split_once('/')?;
    let addr = parse_ip_addr(addr_part)?;
    let bits: u32 = bits_part.parse().ok()?;
    let max = if addr.is_v6 { 128 } else { 32 };
    if bits > max {
        return None;
    }
    Some(IpPrefix { addr, bits })
}

pub(crate) fn ip_cidr_contains(prefix: &IpPrefix, addr: &IpAddr) -> bool {
    if prefix.addr.is_v6 != addr.is_v6 {
        return false;
    }
    let mut bits_left = prefix.bits;
    for i in 0..prefix.addr.bytes.len() {
        if bits_left == 0 {
            return true;
        }
        let byte_bits = bits_left.min(8) as usize;
        let mask = if byte_bits == 8 {
            0xff
        } else {
            0xff << (8 - byte_bits)
        };
        if (prefix.addr.bytes[i] & mask) != (addr.bytes[i] & mask) {
            return false;
        }
        bits_left -= byte_bits as u32;
    }
    true
}

pub(crate) fn ip_addr_to_object(addr: &IpAddr) -> Object {
    let display = format_ip(addr);
    ObjectBuilder::new()
        .set("value", str_obj(display))
        .set("is4", bool_obj(!addr.is_v6))
        .set("is6", bool_obj(addr.is_v6))
        .set("isLoopback", bool_obj(addr.is_loopback()))
        .set("isPrivate", bool_obj(is_private_ip(addr)))
        .set("isMulticast", bool_obj(is_multicast_ip(addr)))
        .build()
}

pub(crate) fn ip_prefix_to_object(prefix: &IpPrefix) -> Object {
    ObjectBuilder::new()
        .set(
            "value",
            str_obj(format!("{}/{}", format_ip(&prefix.addr), prefix.bits)),
        )
        .set("addr", str_obj(format_ip(&prefix.addr)))
        .set("bits", num_obj(prefix.bits as f64))
        .set("is4", bool_obj(!prefix.addr.is_v6))
        .set("is6", bool_obj(prefix.addr.is_v6))
        .build()
}

pub(crate) fn format_ip(addr: &IpAddr) -> String {
    if addr.is_v6 {
        let octets: [u8; 16] = addr.bytes[..16].try_into().unwrap_or([0; 16]);
        std::net::Ipv6Addr::from(octets).to_string()
    } else {
        let octets: [u8; 4] = addr.bytes[..4].try_into().unwrap_or([0; 4]);
        std::net::Ipv4Addr::from(octets).to_string()
    }
}

pub(crate) fn is_private_ip(addr: &IpAddr) -> bool {
    if addr.is_v6 {
        addr.bytes[0] & 0xfe == 0xfc
    } else {
        let b = &addr.bytes;
        b[0] == 10 || (b[0] == 172 && (b[1] & 0xf0) == 16) || (b[0] == 192 && b[1] == 168)
    }
}

pub(crate) fn is_multicast_ip(addr: &IpAddr) -> bool {
    if addr.is_v6 {
        addr.bytes[0] == 0xff
    } else {
        (addr.bytes[0] & 0xf0) == 0xe0
    }
}

pub(crate) fn split_host_port(address: &str) -> Result<(String, String), String> {
    if let Some(end) = address.rfind(':') {
        Ok((address[..end].to_string(), address[end + 1..].to_string()))
    } else {
        Err("missing port in address".to_string())
    }
}

pub(crate) fn join_host_port(host: &str, port: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{}]:{}", host, port)
    } else {
        format!("{}:{}", host, port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cidr_contains_matches_ipv4_prefix_bits() {
        let prefix = parse_ip_cidr("192.168.1.0/24").expect("valid cidr");
        let inside = parse_ip_addr("192.168.1.42").expect("valid ip");
        let outside = parse_ip_addr("192.168.2.42").expect("valid ip");

        assert!(ip_cidr_contains(&prefix, &inside));
        assert!(!ip_cidr_contains(&prefix, &outside));
    }

    #[test]
    fn join_host_port_wraps_bare_ipv6_host() {
        assert_eq!(join_host_port("2001:db8::1", "443"), "[2001:db8::1]:443");
        assert_eq!(join_host_port("[2001:db8::1]", "443"), "[2001:db8::1]:443");
    }
}

use std::str::FromStr;

// ---------------------------------------------------------------------------
// retry: synchronous run with configurable backoff.
// ---------------------------------------------------------------------------
