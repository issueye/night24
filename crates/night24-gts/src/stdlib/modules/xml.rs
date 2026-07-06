use std::fs;
use std::ops::Deref;

use super::super::helpers::*;
use crate::object::{new_error, str_obj, CallContext, Object};

pub(crate) fn xml_module() -> Object {
    module(vec![
        ("parse", native("xml.parse", xml_parse)),
        ("stringify", native("xml.stringify", xml_stringify)),
        ("readFileSync", native("xml.readFileSync", xml_read_file)),
        ("writeFileSync", native("xml.writeFileSync", xml_write_file)),
    ])
}

pub(crate) fn xml_parse(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "xml.parse", args);
    let text = match reader.required_string(0, "text") {
        Ok(v) => v,
        Err(e) => return e,
    };
    match parse_xml_dom(&text) {
        Ok(node) => xml_node_to_object(&node),
        Err(e) => new_error(ctx.pos.clone(), format!("xml.parse: {}", e)),
    }
}

pub(crate) fn xml_stringify(ctx: &mut CallContext, args: &[Object]) -> Object {
    let value = match args.first() {
        Some(v) => v,
        None => return new_error(ctx.pos.clone(), "xml.stringify requires a node"),
    };
    match object_to_xml_node(value) {
        Ok(node) => str_obj(serialize_xml_node(&node)),
        Err(e) => new_error(ctx.pos.clone(), format!("xml.stringify: {}", e)),
    }
}

pub(crate) fn xml_read_file(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "xml.readFileSync", args);
    let path = match reader.required_string(0, "path") {
        Ok(v) => v,
        Err(e) => return e,
    };
    match fs::read_to_string(&path) {
        Ok(text) => match parse_xml_dom(&text) {
            Ok(node) => xml_node_to_object(&node),
            Err(e) => new_error(ctx.pos.clone(), format!("xml.parse: {}", e)),
        },
        Err(e) => new_error(ctx.pos.clone(), format!("xml.readFileSync: {}", e)),
    }
}

pub(crate) fn xml_write_file(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "xml.writeFileSync", args);
    let path = match reader.required_string(0, "path") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let value = match args.get(1) {
        Some(v) => v,
        None => return new_error(ctx.pos.clone(), "xml.writeFileSync requires node"),
    };
    match object_to_xml_node(value) {
        Ok(node) => match fs::write(&path, serialize_xml_node(&node)) {
            Ok(()) => Object::Undefined,
            Err(e) => new_error(ctx.pos.clone(), format!("xml.writeFileSync: {}", e)),
        },
        Err(e) => new_error(ctx.pos.clone(), format!("xml.stringify: {}", e)),
    }
}

pub(crate) struct XmlNode {
    name: String,
    attributes: Vec<(String, String)>,
    children: Vec<XmlNode>,
    text: String,
}

pub(crate) fn parse_xml_dom(input: &str) -> Result<XmlNode, String> {
    use quick_xml::events::Event;
    use quick_xml::Reader;
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut stack: Vec<XmlNode> = Vec::new();
    let mut root: Option<XmlNode> = None;
    let mut text_buf = String::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let mut attributes = Vec::new();
                for attr in e.attributes().flatten() {
                    let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                    let val = attr.unescape_value().map_err(|e| e.to_string())?;
                    attributes.push((key, val.to_string()));
                }
                stack.push(XmlNode {
                    name,
                    attributes,
                    children: Vec::new(),
                    text: String::new(),
                });
                text_buf.clear();
            }
            Ok(Event::End(_)) => {
                if let Some(mut node) = stack.pop() {
                    node.text = text_buf.trim().to_string();
                    text_buf.clear();
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(node);
                    } else {
                        root = Some(node);
                    }
                }
            }
            Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let mut attributes = Vec::new();
                for attr in e.attributes().flatten() {
                    let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                    let val = attr.unescape_value().map_err(|e| e.to_string())?;
                    attributes.push((key, val.to_string()));
                }
                let node = XmlNode {
                    name,
                    attributes,
                    children: Vec::new(),
                    text: String::new(),
                };
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                } else {
                    root = Some(node);
                }
            }
            Ok(Event::Text(e)) => {
                let t = e.unescape().map_err(|e| e.to_string())?;
                text_buf.push_str(&t);
            }
            Ok(Event::CData(e)) => {
                text_buf.push_str(&String::from_utf8_lossy(e.deref()));
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(e.to_string()),
        }
        buf.clear();
    }
    root.ok_or_else(|| "empty XML document".to_string())
}

pub(crate) fn xml_node_to_object(node: &XmlNode) -> Object {
    // Attributes sorted by key for determinism.
    let mut attrs = node.attributes.clone();
    attrs.sort_by(|a, b| a.0.cmp(&b.0));
    let mut attr_builder = ObjectBuilder::new();
    for (k, v) in &attrs {
        attr_builder.insert(k.clone(), str_obj(v.clone()));
    }
    let children: Vec<Object> = node.children.iter().map(xml_node_to_object).collect();
    ObjectBuilder::new()
        .set("name", str_obj(node.name.clone()))
        .set("attributes", attr_builder.build())
        .set("children", array(children))
        .set("text", str_obj(node.text.clone()))
        .build()
}

pub(crate) fn object_to_xml_node(value: &Object) -> Result<XmlNode, String> {
    let hash = match value {
        Object::Hash(h) => h.clone(),
        _ => return Err("node must be an object".to_string()),
    };
    let h = hash.borrow();
    let name = match h.get("name") {
        Some(Object::String(s)) if !s.is_empty() => s.as_str().to_string(),
        _ => return Err("node.name must be a string".to_string()),
    };
    let mut attributes = Vec::new();
    if let Some(Object::Hash(attr_hash)) = h.get("attributes") {
        for (k, v) in &attr_hash.borrow().entries {
            if let Object::String(s) = v {
                attributes.push((k.clone(), s.as_str().to_string()));
            }
        }
    }
    let text = match h.get("text") {
        Some(Object::String(s)) => s.as_str().to_string(),
        _ => String::new(),
    };
    let mut children = Vec::new();
    if let Some(Object::Array(arr)) = h.get("children") {
        for elem in &arr.borrow().elements {
            children.push(object_to_xml_node(elem)?);
        }
    }
    Ok(XmlNode {
        name,
        attributes,
        children,
        text,
    })
}

pub(crate) fn serialize_xml_node(node: &XmlNode) -> String {
    let mut out = String::new();
    serialize_xml_node_into(node, &mut out);
    out
}

pub(crate) fn serialize_xml_node_into(node: &XmlNode, out: &mut String) {
    out.push('<');
    out.push_str(&node.name);
    let mut attrs = node.attributes.clone();
    attrs.sort_by(|a, b| a.0.cmp(&b.0));
    for (k, v) in &attrs {
        out.push(' ');
        out.push_str(k);
        out.push_str("=\"");
        escape_xml_text(v, out);
        out.push('"');
    }
    if node.children.is_empty() && node.text.is_empty() {
        out.push_str("/>");
        return;
    }
    out.push('>');
    escape_xml_text(&node.text, out);
    for child in &node.children {
        serialize_xml_node_into(child, out);
    }
    out.push_str("</");
    out.push_str(&node.name);
    out.push('>');
}

pub(crate) fn escape_xml_text(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn string_field(object: &Object, key: &str) -> String {
        let Object::Hash(hash) = object else {
            panic!("expected XML node object");
        };
        match hash.borrow().get(key) {
            Some(Object::String(value)) => value.to_string(),
            _ => panic!("expected string field {key}"),
        }
    }

    #[test]
    fn xml_node_object_round_trips_to_stringified_xml() {
        let node =
            parse_xml_dom(r#"<root b="2" a="1"><child>Night &amp; day</child></root>"#).unwrap();
        let object = xml_node_to_object(&node);

        assert_eq!(string_field(&object, "name"), "root");

        let round_tripped = object_to_xml_node(&object).unwrap();

        assert_eq!(
            serialize_xml_node(&round_tripped),
            r#"<root a="1" b="2"><child>Night &amp; day</child></root>"#
        );
    }

    #[test]
    fn serialize_xml_node_escapes_text_and_attributes() {
        let object = ObjectBuilder::new()
            .set("name", str_obj("note"))
            .set(
                "attributes",
                ObjectBuilder::new()
                    .set("title", str_obj(r#"a "quoted" & tagged value"#))
                    .build(),
            )
            .set("children", array(Vec::new()))
            .set("text", str_obj("<hello> & 'bye'"))
            .build();
        let node = object_to_xml_node(&object).unwrap();

        assert_eq!(
            serialize_xml_node(&node),
            r#"<note title="a &quot;quoted&quot; &amp; tagged value">&lt;hello&gt; &amp; &apos;bye&apos;</note>"#
        );
    }
}

// ---------------------------------------------------------------------------
// markdown: parse (AST) + renderTerminal + fromHTML. The Go original has no
// markdown->HTML render; we mirror that surface.
// ---------------------------------------------------------------------------
