//! The one JSON style for `rules.json` and rules files, so every tool that writes them (Rule
//! Editor, scripts, the checkers) writes the same bytes, and a change shows in git as the lines it
//! changed.
//!
//! - Two-space indents, and a newline at the end.
//! - Whatever fits in 100 columns, counting its indent and key, stays on one line, so a rules file
//!   reads as a table: `"texture.max_size": {"severity": "warning", "params": {"max": 2048}},`.
//! - A list of strings too long for one line is wrapped, several to a line.
//! - Object keys stay in the order they were read or added: [`Node`] keeps them, where
//!   `serde_json::Value` would sort them.
//! - Text is written as it is (UTF-8), not as `\u` escapes.

use std::fmt;

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};

/// The widest a line gets, unless one value alone is wider.
const WIDTH: usize = 100;

/// A JSON value that keeps its object keys in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(String),
    Array(Vec<Node>),
    /// Keys in order. A key appears once: [`Node::parse`] keeps the last of duplicates, as
    /// `serde_json` does.
    Object(Vec<(String, Node)>),
}

impl Node {
    /// Parses JSON text, keeping object keys in the order written.
    pub fn parse(text: &str) -> Result<Node, serde_json::Error> {
        serde_json::from_str(text)
    }

    /// The value at `key`, for an object.
    pub fn get(&self, key: &str) -> Option<&Node> {
        match self {
            Node::Object(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The value at `key`, for an object, to change in place.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Node> {
        match self {
            Node::Object(entries) => entries.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Sets `key` in an object: in its place if it's there, else at the end. Returns the old
    /// value. Does nothing to a node that isn't an object.
    pub fn insert(&mut self, key: &str, value: Node) -> Option<Node> {
        let Node::Object(entries) = self else {
            return None;
        };
        match entries.iter_mut().find(|(k, _)| k == key) {
            Some((_, old)) => Some(std::mem::replace(old, value)),
            None => {
                entries.push((key.to_owned(), value));
                None
            }
        }
    }

    /// Removes `key` from an object, returning its value.
    pub fn remove(&mut self, key: &str) -> Option<Node> {
        let Node::Object(entries) = self else {
            return None;
        };
        let index = entries.iter().position(|(k, _)| k == key)?;
        Some(entries.remove(index).1)
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Node::String(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Node]> {
        match self {
            Node::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&[(String, Node)]> {
        match self {
            Node::Object(entries) => Some(entries),
            _ => None,
        }
    }

    /// The value in the house style, with a final newline.
    pub fn to_json(&self) -> String {
        let mut text = layout(self, 0, 0);
        text.push('\n');
        text
    }
}

impl From<serde_json::Value> for Node {
    /// Object keys come in the `Value`'s order, which is sorted unless `serde_json`'s
    /// `preserve_order` feature is on.
    fn from(value: serde_json::Value) -> Node {
        use serde_json::Value;
        match value {
            Value::Null => Node::Null,
            Value::Bool(b) => Node::Bool(b),
            Value::Number(n) => Node::Number(n),
            Value::String(s) => Node::String(s),
            Value::Array(items) => Node::Array(items.into_iter().map(Node::from).collect()),
            Value::Object(map) => {
                Node::Object(map.into_iter().map(|(k, v)| (k, Node::from(v))).collect())
            }
        }
    }
}

impl From<&Node> for serde_json::Value {
    fn from(node: &Node) -> serde_json::Value {
        use serde_json::Value;
        match node {
            Node::Null => Value::Null,
            Node::Bool(b) => Value::Bool(*b),
            Node::Number(n) => Value::Number(n.clone()),
            Node::String(s) => Value::String(s.clone()),
            Node::Array(items) => Value::Array(items.iter().map(Value::from).collect()),
            Node::Object(entries) => Value::Object(
                entries
                    .iter()
                    .map(|(k, v)| (k.clone(), Value::from(v)))
                    .collect(),
            ),
        }
    }
}

impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Node, D::Error> {
        deserializer.deserialize_any(NodeVisitor)
    }
}

struct NodeVisitor;

impl<'de> Visitor<'de> for NodeVisitor {
    type Value = Node;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON value")
    }

    fn visit_bool<E>(self, b: bool) -> Result<Node, E> {
        Ok(Node::Bool(b))
    }

    fn visit_i64<E>(self, n: i64) -> Result<Node, E> {
        Ok(Node::Number(n.into()))
    }

    fn visit_u64<E>(self, n: u64) -> Result<Node, E> {
        Ok(Node::Number(n.into()))
    }

    fn visit_f64<E: de::Error>(self, n: f64) -> Result<Node, E> {
        serde_json::Number::from_f64(n)
            .map(Node::Number)
            .ok_or_else(|| E::custom("a number JSON can't hold"))
    }

    fn visit_str<E>(self, s: &str) -> Result<Node, E> {
        Ok(Node::String(s.to_owned()))
    }

    fn visit_string<E>(self, s: String) -> Result<Node, E> {
        Ok(Node::String(s))
    }

    fn visit_unit<E>(self) -> Result<Node, E> {
        Ok(Node::Null)
    }

    fn visit_none<E>(self) -> Result<Node, E> {
        Ok(Node::Null)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Node, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(Node::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Node, A::Error> {
        let mut entries: Vec<(String, Node)> = Vec::new();
        while let Some((key, value)) = map.next_entry::<String, Node>()? {
            match entries.iter_mut().find(|(k, _)| *k == key) {
                Some((_, old)) => *old = value,
                None => entries.push((key, value)),
            }
        }
        Ok(Node::Object(entries))
    }
}

/// `node` on one line, with `", "` and `": "` between items.
fn one_line(node: &Node) -> String {
    match node {
        Node::Null => "null".to_owned(),
        Node::Bool(b) => b.to_string(),
        Node::Number(n) => n.to_string(),
        Node::String(s) => quoted(s),
        Node::Array(items) => {
            let items: Vec<String> = items.iter().map(one_line).collect();
            format!("[{}]", items.join(", "))
        }
        Node::Object(entries) => {
            let entries: Vec<String> = entries
                .iter()
                .map(|(k, v)| format!("{}: {}", quoted(k), one_line(v)))
                .collect();
            format!("{{{}}}", entries.join(", "))
        }
    }
}

/// A JSON string literal.
fn quoted(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_owned())
}

fn width(text: &str) -> usize {
    text.chars().count()
}

/// `node` at `indent`, after `lead` columns of key on its first line.
fn layout(node: &Node, indent: usize, lead: usize) -> String {
    let line = one_line(node);
    let container = matches!(node, Node::Array(_) | Node::Object(_));
    if !container || indent + lead + width(&line) <= WIDTH {
        return line;
    }
    let pad = " ".repeat(indent + 2);
    let end = format!("\n{}", " ".repeat(indent));
    match node {
        Node::Array(items) if items.iter().all(|item| matches!(item, Node::String(_))) => {
            let mut lines: Vec<String> = Vec::new();
            let mut current = String::new();
            for item in items {
                let word = one_line(item);
                if !current.is_empty() && width(&pad) + width(&current) + 2 + width(&word) > WIDTH {
                    current.push(',');
                    lines.push(std::mem::replace(&mut current, word));
                } else if current.is_empty() {
                    current = word;
                } else {
                    current.push_str(", ");
                    current.push_str(&word);
                }
            }
            lines.push(current);
            let body: Vec<String> = lines.iter().map(|l| format!("{pad}{l}")).collect();
            format!("[\n{}{end}]", body.join("\n"))
        }
        Node::Array(items) => {
            let body: Vec<String> = items
                .iter()
                .map(|item| format!("{pad}{}", layout(item, indent + 2, 0)))
                .collect();
            format!("[\n{}{end}]", body.join(",\n"))
        }
        Node::Object(entries) => {
            let body: Vec<String> = entries
                .iter()
                .map(|(k, v)| {
                    let key = format!("{}: ", quoted(k));
                    let value = layout(v, indent + 2, width(&key));
                    format!("{pad}{key}{value}")
                })
                .collect();
            format!("{{\n{}{end}}}", body.join(",\n"))
        }
        _ => line,
    }
}
