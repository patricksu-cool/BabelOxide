use serde::Deserialize;
use serde_json::Value;
use std::{fmt, sync::LazyLock};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
pub enum FieldType {
    Bool,
    Integer,
    Float,
    String,
    StringSet,
}

#[derive(Clone, Deserialize)]
pub struct FieldSpec {
    pub path: String,
    /// Long CLI spelling without the leading --; input-files is positional.
    pub flag: String,
    pub env: String,
    pub kind: FieldType,
    pub default: Value,
    pub nullable: bool,
    pub secret: bool,
    pub help: String,
}

impl fmt::Debug for FieldSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FieldSpec")
            .field("path", &self.path)
            .field("kind", &self.kind)
            .field("nullable", &self.nullable)
            .field("secret", &self.secret)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct EngineSpec {
    pub name: String,
    pub flag: String,
    pub detail_key: Option<String>,
    pub supports_llm: bool,
}

#[derive(Deserialize)]
struct Schema {
    fields: Vec<FieldSpec>,
    engines: Vec<EngineSpec>,
    defaults: Value,
}

static SCHEMA: LazyLock<Schema> = LazyLock::new(|| {
    serde_json::from_str(include_str!("schema_data.json"))
        .expect("source-derived embedded configuration schema is valid")
});

pub fn field_specs() -> &'static [FieldSpec] {
    &SCHEMA.fields
}
pub fn engine_specs() -> &'static [EngineSpec] {
    &SCHEMA.engines
}
pub(crate) fn defaults() -> Value {
    SCHEMA.defaults.clone()
}

pub(crate) fn lookup<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(value, |node, part| node.get(part))
}

pub(crate) fn put(value: &mut Value, path: &str, item: Value) -> bool {
    let parts: Vec<_> = path.split('.').collect();
    if parts.is_empty() || parts.iter().any(|part| part.is_empty()) {
        return false;
    }
    let mut node = value;
    for part in &parts[..parts.len() - 1] {
        let Some(object) = node.as_object_mut() else {
            return false;
        };
        node = object.entry(*part).or_insert_with(|| serde_json::json!({}));
    }
    let Some(object) = node.as_object_mut() else {
        return false;
    };
    object.insert(parts[parts.len() - 1].into(), item);
    true
}
