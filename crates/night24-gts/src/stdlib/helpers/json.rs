use super::*;

#[derive(Clone)]
pub(crate) enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}

pub(crate) fn simple_json_parse(source: &str) -> Result<JsonValue, String> {
    let mut parser = JsonParser::new(source);
    let value = parser.parse_value()?;
    parser.skip_ws();
    if parser.is_eof() {
        Ok(value)
    } else {
        Err("trailing characters".into())
    }
}

pub(crate) fn normalize_json5(text: &str) -> String {
    let mut out = text.to_string();
    if let Ok(re) = Regex::new(r"//[^\n]*") {
        out = re.replace_all(&out, "").to_string();
    }
    if let Ok(re) = Regex::new(r"/\*[\s\S]*?\*/") {
        out = re.replace_all(&out, "").to_string();
    }
    out = out.replace('\'', "\"");
    if let Ok(re) = Regex::new(r",(\s*[}\]])") {
        out = re.replace_all(&out, "$1").to_string();
    }
    if let Ok(re) = Regex::new(r"([A-Za-z_][A-Za-z0-9_]*):") {
        out = re.replace_all(&out, "\"$1\":").to_string();
    }
    out
}

pub(crate) fn stringify_options(value: Option<&Object>) -> (Option<String>, bool) {
    let Some(Object::Hash(hash)) = value else {
        return (None, false);
    };
    let hash = hash.borrow();
    let space = match hash.get("space") {
        Some(Object::Number(n)) if *n > 0.0 => Some(" ".repeat(*n as usize)),
        _ => None,
    };
    let single_quote =
        matches!(hash.get("quote"), Some(Object::String(s)) if s.as_str() == "single");
    (space, single_quote)
}

pub(crate) fn object_to_json(value: &Object, indent: usize, space: Option<&str>) -> String {
    match value {
        Object::Number(n) => crate::object::format_number(*n),
        Object::String(s) => quote_json_string(s),
        Object::Boolean(value) => value.to_string(),
        Object::Null | Object::Undefined => "null".into(),
        Object::Array(array) => {
            let elements = array.borrow();
            if let Some(space) = space {
                if elements.elements.is_empty() {
                    return "[]".into();
                }
                let child_indent = space.repeat(indent + 1);
                let current_indent = space.repeat(indent);
                let items: Vec<String> = elements
                    .elements
                    .iter()
                    .map(|value| {
                        format!(
                            "{}{}",
                            child_indent,
                            object_to_json(value, indent + 1, Some(space))
                        )
                    })
                    .collect();
                format!("[\n{}\n{}]", items.join(",\n"), current_indent)
            } else {
                let items: Vec<String> = elements
                    .elements
                    .iter()
                    .map(|value| object_to_json(value, indent, None))
                    .collect();
                format!("[{}]", items.join(","))
            }
        }
        Object::Hash(hash) => {
            let hash = hash.borrow();
            if let Some(space) = space {
                if hash.entries.is_empty() {
                    return "{}".into();
                }
                let child_indent = space.repeat(indent + 1);
                let current_indent = space.repeat(indent);
                let items: Vec<String> = hash
                    .entries
                    .iter()
                    .map(|(key, value)| {
                        format!(
                            "{}{}: {}",
                            child_indent,
                            quote_json_string(key),
                            object_to_json(value, indent + 1, Some(space))
                        )
                    })
                    .collect();
                format!("{{\n{}\n{}}}", items.join(",\n"), current_indent)
            } else {
                let items: Vec<String> = hash
                    .entries
                    .iter()
                    .map(|(key, value)| {
                        format!(
                            "{}:{}",
                            quote_json_string(key),
                            object_to_json(value, indent, None)
                        )
                    })
                    .collect();
                format!("{{{}}}", items.join(","))
            }
        }
        _ => "null".into(),
    }
}

pub(crate) fn quote_json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub(crate) fn validate_json_value(
    value: &Object,
    schema: &HashData,
    path: &str,
    errors: &mut Vec<String>,
) {
    if let Some(Object::String(type_value)) = schema.get("type") {
        let valid = match type_value.as_str() {
            "string" => matches!(value, Object::String(_)),
            "number" => matches!(value, Object::Number(_)),
            "boolean" => matches!(value, Object::Boolean(_)),
            "array" => matches!(value, Object::Array(_)),
            "object" => matches!(value, Object::Hash(_)),
            "null" => matches!(value, Object::Null),
            _ => true,
        };
        if !valid {
            errors.push(format!("{}: expected type {}", path, type_value));
        }
    }

    if let Object::String(text) = value {
        if let Some(Object::Number(min)) = schema.get("minLength") {
            if text.len() < *min as usize {
                errors.push(format!("{}: string too short", path));
            }
        }
        if let Some(Object::Number(max)) = schema.get("maxLength") {
            if text.len() > *max as usize {
                errors.push(format!("{}: string too long", path));
            }
        }
        if let Some(Object::String(pattern)) = schema.get("pattern") {
            if Regex::new(pattern)
                .map(|re| !re.is_match(text))
                .unwrap_or(false)
            {
                errors.push(format!("{}: string does not match pattern", path));
            }
        }
    }

    if let Object::Number(number) = value {
        if let Some(Object::Number(min)) = schema.get("minimum") {
            if number < min {
                errors.push(format!("{}: number too small", path));
            }
        }
        if let Some(Object::Number(max)) = schema.get("maximum") {
            if number > max {
                errors.push(format!("{}: number too large", path));
            }
        }
    }

    if let Object::Array(array) = value {
        let len = array.borrow().elements.len();
        if let Some(Object::Number(min)) = schema.get("minItems") {
            if len < *min as usize {
                errors.push(format!("{}: array too short", path));
            }
        }
        if let Some(Object::Number(max)) = schema.get("maxItems") {
            if len > *max as usize {
                errors.push(format!("{}: array too long", path));
            }
        }
    }

    if let Object::Hash(hash) = value {
        let hash = hash.borrow();
        if let Some(Object::Array(required)) = schema.get("required") {
            for item in &required.borrow().elements {
                if let Object::String(key) = item {
                    if !hash.contains(key) {
                        errors.push(format!("{}: missing required field {}", path, key));
                    }
                }
            }
        }
        if let Some(Object::Hash(properties)) = schema.get("properties") {
            for (key, prop_schema) in &properties.borrow().entries {
                if let (Some(value), Object::Hash(prop_schema)) = (hash.get(key), prop_schema) {
                    let sub_path = format!("{}/{}", path, key);
                    validate_json_value(value, &prop_schema.borrow(), &sub_path, errors);
                }
            }
        }
    }
}

pub(crate) fn pointer_get(doc: &Object, path: &str) -> Option<Object> {
    if path.is_empty() {
        return Some(doc.clone());
    }
    let mut current = doc.clone();
    for part in pointer_parts(path) {
        current = match current {
            Object::Hash(hash) => hash.borrow().get(&part).cloned()?,
            Object::Array(array) => {
                let index = part.parse::<usize>().ok()?;
                array.borrow().elements.get(index).cloned()?
            }
            _ => return None,
        };
    }
    Some(current)
}

pub(crate) fn pointer_set(doc: &Object, path: &str, value: Object) {
    if path.is_empty() {
        return;
    }
    let parts = pointer_parts(path);
    if parts.is_empty() {
        return;
    }
    let mut current = doc.clone();
    for part in &parts[..parts.len() - 1] {
        match current {
            Object::Hash(hash) => {
                let next = hash.borrow().get(part).cloned().unwrap_or_else(|| {
                    let created = Object::Hash(Rc::new(RefCell::new(HashData::default())));
                    hash.borrow_mut().set(part.clone(), created.clone());
                    created
                });
                current = next;
            }
            Object::Array(array) => {
                let Ok(index) = part.parse::<usize>() else {
                    return;
                };
                let Some(next) = array.borrow().elements.get(index).cloned() else {
                    return;
                };
                current = next;
            }
            _ => return,
        }
    }
    let last = parts.last().cloned().unwrap_or_default();
    match current {
        Object::Hash(hash) => hash.borrow_mut().set(last, value),
        Object::Array(array) => {
            if last == "-" {
                array.borrow_mut().elements.push(value);
            } else if let Ok(index) = last.parse::<usize>() {
                let mut array = array.borrow_mut();
                if index < array.elements.len() {
                    array.elements[index] = value;
                } else if index == array.elements.len() {
                    array.elements.push(value);
                }
            }
        }
        _ => {}
    }
}

pub(crate) fn pointer_remove(doc: &Object, path: &str) {
    if path.is_empty() {
        return;
    }
    let parts = pointer_parts(path);
    if parts.is_empty() {
        return;
    }
    let mut current = doc.clone();
    for part in &parts[..parts.len() - 1] {
        current = match current {
            Object::Hash(hash) => match hash.borrow().get(part).cloned() {
                Some(value) => value,
                None => return,
            },
            Object::Array(array) => match part.parse::<usize>() {
                Ok(index) => match array.borrow().elements.get(index).cloned() {
                    Some(value) => value,
                    None => return,
                },
                Err(_) => return,
            },
            _ => return,
        };
    }
    let last = parts.last().cloned().unwrap_or_default();
    match current {
        Object::Hash(hash) => {
            hash.borrow_mut().remove(&last);
        }
        Object::Array(array) => {
            if let Ok(index) = last.parse::<usize>() {
                let mut array = array.borrow_mut();
                if index < array.elements.len() {
                    array.elements.remove(index);
                }
            }
        }
        _ => {}
    }
}

pub(crate) fn pointer_parts(path: &str) -> Vec<String> {
    path.trim_start_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
        .map(unescape_pointer)
        .collect()
}

pub(crate) fn unescape_pointer(value: &str) -> String {
    value.replace("~1", "/").replace("~0", "~")
}

pub(crate) fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

pub(crate) fn deep_clone_object(value: &Object) -> Object {
    match value {
        Object::Array(values) => array(
            values
                .borrow()
                .elements
                .iter()
                .map(deep_clone_object)
                .collect(),
        ),
        Object::Hash(hash) => {
            let cloned = Rc::new(RefCell::new(HashData::default()));
            for (key, value) in &hash.borrow().entries {
                cloned
                    .borrow_mut()
                    .set(key.clone(), deep_clone_object(value));
            }
            Object::Hash(cloned)
        }
        other => other.clone(),
    }
}

pub(crate) fn objects_deep_equal(left: &Object, right: &Object) -> bool {
    match (left, right) {
        (Object::Number(a), Object::Number(b)) => a == b,
        (Object::String(a), Object::String(b)) => a == b,
        (Object::Boolean(a), Object::Boolean(b)) => a == b,
        (Object::Null, Object::Null) | (Object::Undefined, Object::Undefined) => true,
        (Object::Array(a), Object::Array(b)) => {
            let a = a.borrow();
            let b = b.borrow();
            a.elements.len() == b.elements.len()
                && a.elements
                    .iter()
                    .zip(b.elements.iter())
                    .all(|(a, b)| objects_deep_equal(a, b))
        }
        (Object::Hash(a), Object::Hash(b)) => {
            let a = a.borrow();
            let b = b.borrow();
            a.entries.len() == b.entries.len()
                && a.entries.iter().all(|(key, value)| {
                    b.get(key)
                        .map(|other| objects_deep_equal(value, other))
                        .unwrap_or(false)
                })
        }
        _ => false,
    }
}

pub(crate) fn diff_objects(old: &Object, new: &Object, path: &str, patches: &mut Vec<Object>) {
    if objects_deep_equal(old, new) {
        return;
    }
    let (Object::Hash(old_hash), Object::Hash(new_hash)) = (old, new) else {
        patches.push(module(vec![
            ("op", str_obj("replace")),
            ("path", str_obj(path)),
            ("value", deep_clone_object(new)),
        ]));
        return;
    };
    let old_hash = old_hash.borrow();
    let new_hash = new_hash.borrow();
    for (key, new_value) in &new_hash.entries {
        let sub_path = format!("{}/{}", path, escape_pointer(key));
        if let Some(old_value) = old_hash.get(key) {
            diff_objects(old_value, new_value, &sub_path, patches);
        } else {
            patches.push(module(vec![
                ("op", str_obj("add")),
                ("path", str_obj(sub_path)),
                ("value", deep_clone_object(new_value)),
            ]));
        }
    }
    for (key, _) in &old_hash.entries {
        if !new_hash.contains(key) {
            patches.push(module(vec![
                ("op", str_obj("remove")),
                ("path", str_obj(format!("{}/{}", path, escape_pointer(key)))),
            ]));
        }
    }
}

pub(crate) fn json_to_object(value: JsonValue) -> Object {
    match value {
        JsonValue::Null => Object::Null,
        JsonValue::Bool(value) => bool_obj(value),
        JsonValue::Number(value) => num_obj(value),
        JsonValue::String(value) => str_obj(value),
        JsonValue::Array(values) => array(values.into_iter().map(json_to_object).collect()),
        JsonValue::Object(entries) => {
            let hash = Rc::new(RefCell::new(HashData::default()));
            for (key, value) in entries {
                hash.borrow_mut().set(key, json_to_object(value));
            }
            Object::Hash(hash)
        }
    }
}

pub(crate) struct JsonParser<'a> {
    source: &'a [u8],
    pos: usize,
}

impl<'a> JsonParser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source: source.as_bytes(),
            pos: 0,
        }
    }

    fn parse_value(&mut self) -> Result<JsonValue, String> {
        self.skip_ws();
        match self.peek() {
            Some(b'n') => self.parse_literal(b"null", JsonValue::Null),
            Some(b't') => self.parse_literal(b"true", JsonValue::Bool(true)),
            Some(b'f') => self.parse_literal(b"false", JsonValue::Bool(false)),
            Some(b'"') => self.parse_string().map(JsonValue::String),
            Some(b'[') => self.parse_array(),
            Some(b'{') => self.parse_object(),
            Some(b'-' | b'0'..=b'9') => self.parse_number().map(JsonValue::Number),
            Some(_) => Err("unexpected token".into()),
            None => Err("unexpected end of input".into()),
        }
    }

    fn parse_literal(&mut self, literal: &[u8], value: JsonValue) -> Result<JsonValue, String> {
        if self.source.get(self.pos..self.pos + literal.len()) == Some(literal) {
            self.pos += literal.len();
            Ok(value)
        } else {
            Err("invalid literal".into())
        }
    }

    fn parse_string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out = String::new();
        while let Some(byte) = self.next() {
            match byte {
                b'"' => return Ok(out),
                b'\\' => {
                    let escaped = self
                        .next()
                        .ok_or_else(|| "unterminated escape".to_string())?;
                    match escaped {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.parse_unicode_escape()?),
                        _ => return Err("invalid escape".into()),
                    }
                }
                0x00..=0x1f => return Err("control character in string".into()),
                other => out.push(other as char),
            }
        }
        Err("unterminated string".into())
    }

    fn parse_unicode_escape(&mut self) -> Result<char, String> {
        let mut value = 0u32;
        for _ in 0..4 {
            let byte = self
                .next()
                .ok_or_else(|| "invalid unicode escape".to_string())?;
            value = value * 16
                + match byte {
                    b'0'..=b'9' => (byte - b'0') as u32,
                    b'a'..=b'f' => (byte - b'a' + 10) as u32,
                    b'A'..=b'F' => (byte - b'A' + 10) as u32,
                    _ => return Err("invalid unicode escape".into()),
                };
        }
        char::from_u32(value).ok_or_else(|| "invalid unicode scalar".into())
    }

    fn parse_array(&mut self) -> Result<JsonValue, String> {
        self.expect(b'[')?;
        let mut values = Vec::new();
        loop {
            self.skip_ws();
            if self.consume(b']') {
                break;
            }
            values.push(self.parse_value()?);
            self.skip_ws();
            if self.consume(b']') {
                break;
            }
            self.expect(b',')?;
        }
        Ok(JsonValue::Array(values))
    }

    fn parse_object(&mut self) -> Result<JsonValue, String> {
        self.expect(b'{')?;
        let mut entries = Vec::new();
        loop {
            self.skip_ws();
            if self.consume(b'}') {
                break;
            }
            let key = self.parse_string()?;
            self.skip_ws();
            self.expect(b':')?;
            let value = self.parse_value()?;
            entries.push((key, value));
            self.skip_ws();
            if self.consume(b'}') {
                break;
            }
            self.expect(b',')?;
        }
        Ok(JsonValue::Object(entries))
    }

    fn parse_number(&mut self) -> Result<f64, String> {
        let start = self.pos;
        self.consume(b'-');
        match self.peek() {
            Some(b'0') => {
                self.pos += 1;
            }
            Some(b'1'..=b'9') => {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.pos += 1;
                }
            }
            _ => return Err("invalid number".into()),
        }
        if self.consume(b'.') {
            let digit_start = self.pos;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
            if self.pos == digit_start {
                return Err("invalid number".into());
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            let _ = self.consume(b'+') || self.consume(b'-');
            let digit_start = self.pos;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
            if self.pos == digit_start {
                return Err("invalid number".into());
            }
        }
        std::str::from_utf8(&self.source[start..self.pos])
            .map_err(|_| "invalid number".to_string())?
            .parse::<f64>()
            .map_err(|_| "invalid number".to_string())
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.pos += 1;
        }
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.source.len()
    }

    fn peek(&self) -> Option<u8> {
        self.source.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.pos += 1;
        Some(byte)
    }

    fn consume(&mut self, expected: u8) -> bool {
        if self.peek() == Some(expected) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: u8) -> Result<(), String> {
        if self.consume(expected) {
            Ok(())
        } else {
            Err(format!("expected '{}'", expected as char))
        }
    }
}
