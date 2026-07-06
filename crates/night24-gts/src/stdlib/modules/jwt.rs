use super::super::helpers::*;
use crate::object::{bool_obj, new_error, str_obj, CallContext, Object};

pub(crate) fn jwt_module() -> Object {
    module(vec![
        ("sign", native("jwt.sign", jwt_sign)),
        ("verify", native("jwt.verify", jwt_verify)),
        ("decode", native("jwt.decode", jwt_decode)),
    ])
}

pub(crate) fn jwt_sign(ctx: &mut CallContext, args: &[Object]) -> Object {
    if args.len() < 2 {
        return new_error(ctx.pos.clone(), "jwt.sign requires payload and secret");
    }
    let (payload, secret) = (&args[0], &args[1]);
    let secret = match secret {
        Object::String(s) => s.as_bytes().to_vec(),
        _ => return new_error(ctx.pos.clone(), "jwt.sign expects string secret"),
    };
    let header = serde_json::json!({"alg": "HS256", "typ": "JWT"});
    let mut payload_value = object_to_value(payload);
    if let serde_json::Value::Object(ref mut map) = payload_value {
        if !map.contains_key("iat") {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            map.insert("iat".to_string(), serde_json::Value::Number(now.into()));
        }
    } else {
        return new_error(ctx.pos.clone(), "jwt.sign expects hash payload");
    }
    let header_b64 = base64url_encode_string(
        serde_json::to_string(&header)
            .unwrap_or_default()
            .as_bytes(),
    );
    let payload_b64 = base64url_encode_string(
        serde_json::to_string(&payload_value)
            .unwrap_or_default()
            .as_bytes(),
    );
    let message = format!("{}.{}", header_b64, payload_b64);
    let sig = hmac(HashKind::Sha256, &secret, message.as_bytes());
    let sig_b64 = base64url_encode_string(&sig);
    str_obj(format!("{}.{}.{}", header_b64, payload_b64, sig_b64))
}

pub(crate) fn jwt_verify(ctx: &mut CallContext, args: &[Object]) -> Object {
    if args.len() < 2 {
        return new_error(ctx.pos.clone(), "jwt.verify requires token and secret");
    }
    let (token, secret) = match (&args[0], &args[1]) {
        (Object::String(t), Object::String(s)) => (t.as_str().to_string(), s.as_bytes().to_vec()),
        _ => {
            return new_error(
                ctx.pos.clone(),
                "jwt.verify expects string token and secret",
            )
        }
    };
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return bool_obj(false);
    }
    let message = format!("{}.{}", parts[0], parts[1]);
    let expected = hmac(HashKind::Sha256, &secret, message.as_bytes());
    let expected_b64 = base64url_encode_string(&expected);
    if expected_b64 != parts[2] {
        return bool_obj(false);
    }
    let table = base64_url_table();
    let payload_bytes = match base64_decode_into(&table, "jwt.verify", parts[1], true) {
        Ok(b) => b,
        Err(_) => return bool_obj(false),
    };
    let payload: serde_json::Value = match serde_json::from_slice(&payload_bytes) {
        Ok(v) => v,
        Err(_) => return bool_obj(false),
    };
    if let Some(exp) = payload.get("exp").and_then(|v| v.as_f64()) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as f64)
            .unwrap_or(0.0);
        if now > exp {
            return bool_obj(false);
        }
    }
    bool_obj(true)
}

pub(crate) fn jwt_decode(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "jwt.decode", args);
    let token = match reader.required_string(0, "token") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return new_error(ctx.pos.clone(), "jwt.decode: invalid token format");
    }
    let table = base64_url_table();
    let payload_bytes = match base64_decode_into(&table, "jwt.decode", parts[1], true) {
        Ok(b) => b,
        Err(e) => return new_error(ctx.pos.clone(), format!("jwt.decode: {}", e)),
    };
    match serde_json::from_slice::<serde_json::Value>(&payload_bytes) {
        Ok(v) => value_to_object(&v),
        Err(e) => new_error(ctx.pos.clone(), format!("jwt.decode: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Position;
    use crate::object::{Environment, VirtualMachine};

    fn test_context(env: &crate::object::EnvRef) -> CallContext<'_> {
        CallContext::new(env, Position::default())
    }

    fn error_message(object: Object) -> String {
        match object {
            Object::Error(error) => error.borrow().message.clone(),
            _ => panic!("expected error"),
        }
    }

    #[test]
    fn decode_returns_payload_object() {
        let env = Environment::new_root(VirtualMachine::new());
        let mut ctx = test_context(&env);
        let header = base64url_encode_string(br#"{"alg":"none","typ":"JWT"}"#);
        let payload = base64url_encode_string(br#"{"sub":"night24","admin":true,"exp":123}"#);
        let token = format!("{}.{}.signature", header, payload);

        let decoded = jwt_decode(&mut ctx, &[str_obj(token)]);

        let Object::Hash(hash) = decoded else {
            panic!("expected decoded payload hash");
        };
        let hash = hash.borrow();
        assert!(matches!(hash.get("admin"), Some(Object::Boolean(true))));
        assert!(matches!(hash.get("exp"), Some(Object::Number(value)) if *value == 123.0));
        assert!(
            matches!(hash.get("sub"), Some(Object::String(value)) if value.as_str() == "night24")
        );
    }

    #[test]
    fn decode_rejects_invalid_token_format() {
        let env = Environment::new_root(VirtualMachine::new());
        let mut ctx = test_context(&env);

        let decoded = jwt_decode(&mut ctx, &[str_obj("not-a-jwt")]);

        assert_eq!(error_message(decoded), "jwt.decode: invalid token format");
    }
}
