//! A small JSON Schema subset for describing shapes as standard JSON Schema.
//!
//! [`ShapeSchema`] is a closed, intentionally minimal description of the data
//! shapes that interop surfaces (LLM tool parameters, structured outputs) need.
//! [`shape_to_json_schema`] lowers it to a standard JSON Schema document.

use serde_json::{Map, Value as JsonValue};

/// A minimal, closed schema vocabulary that lowers to standard JSON Schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShapeSchema {
    /// Matches any value. Lowers to the empty schema `{}`.
    Any,
    /// A JSON object with named properties and a list of required property
    /// names. Lowers to `{"type":"object","properties":{...},"required":[...]}`.
    Object(Vec<(String, ShapeSchema)>, Vec<String>),
    /// A JSON array whose items all match the inner schema. Lowers to
    /// `{"type":"array","items":...}`.
    Array(Box<ShapeSchema>),
    /// A JSON string. Lowers to `{"type":"string"}`.
    String,
    /// A JSON number. Lowers to `{"type":"number"}`.
    Number,
    /// A JSON integer. Lowers to `{"type":"integer"}`.
    Integer,
    /// A JSON boolean. Lowers to `{"type":"boolean"}`.
    Boolean,
    /// JSON null. Lowers to `{"type":"null"}`.
    Null,
}

/// Lowers a [`ShapeSchema`] to a standard JSON Schema document.
pub fn shape_to_json_schema(schema: &ShapeSchema) -> JsonValue {
    match schema {
        ShapeSchema::Any => JsonValue::Object(Map::new()),
        ShapeSchema::Object(properties, required) => {
            let mut props = Map::new();
            for (name, property) in properties {
                props.insert(name.clone(), shape_to_json_schema(property));
            }
            let mut object = Map::new();
            object.insert("type".to_owned(), JsonValue::String("object".to_owned()));
            object.insert("properties".to_owned(), JsonValue::Object(props));
            object.insert(
                "required".to_owned(),
                JsonValue::Array(
                    required
                        .iter()
                        .map(|name| JsonValue::String(name.clone()))
                        .collect(),
                ),
            );
            JsonValue::Object(object)
        }
        ShapeSchema::Array(items) => {
            let mut object = Map::new();
            object.insert("type".to_owned(), JsonValue::String("array".to_owned()));
            object.insert("items".to_owned(), shape_to_json_schema(items));
            JsonValue::Object(object)
        }
        ShapeSchema::String => typed("string"),
        ShapeSchema::Number => typed("number"),
        ShapeSchema::Integer => typed("integer"),
        ShapeSchema::Boolean => typed("boolean"),
        ShapeSchema::Null => typed("null"),
    }
}

fn typed(kind: &str) -> JsonValue {
    let mut object = Map::new();
    object.insert("type".to_owned(), JsonValue::String(kind.to_owned()));
    JsonValue::Object(object)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn any_is_empty_schema() {
        assert_eq!(shape_to_json_schema(&ShapeSchema::Any), json!({}));
    }

    #[test]
    fn scalar_types() {
        assert_eq!(
            shape_to_json_schema(&ShapeSchema::String),
            json!({"type": "string"})
        );
        assert_eq!(
            shape_to_json_schema(&ShapeSchema::Number),
            json!({"type": "number"})
        );
        assert_eq!(
            shape_to_json_schema(&ShapeSchema::Integer),
            json!({"type": "integer"})
        );
        assert_eq!(
            shape_to_json_schema(&ShapeSchema::Boolean),
            json!({"type": "boolean"})
        );
        assert_eq!(
            shape_to_json_schema(&ShapeSchema::Null),
            json!({"type": "null"})
        );
    }

    #[test]
    fn array_wraps_items() {
        let schema = ShapeSchema::Array(Box::new(ShapeSchema::String));
        assert_eq!(
            shape_to_json_schema(&schema),
            json!({"type": "array", "items": {"type": "string"}})
        );
    }

    #[test]
    fn object_with_properties_and_required() {
        let schema = ShapeSchema::Object(
            vec![
                ("name".to_owned(), ShapeSchema::String),
                ("age".to_owned(), ShapeSchema::Integer),
            ],
            vec!["name".to_owned()],
        );
        assert_eq!(
            shape_to_json_schema(&schema),
            json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "age": {"type": "integer"},
                },
                "required": ["name"],
            })
        );
    }

    #[test]
    fn nested_object_and_array() {
        let schema = ShapeSchema::Object(
            vec![(
                "tags".to_owned(),
                ShapeSchema::Array(Box::new(ShapeSchema::String)),
            )],
            vec![],
        );
        assert_eq!(
            shape_to_json_schema(&schema),
            json!({
                "type": "object",
                "properties": {
                    "tags": {"type": "array", "items": {"type": "string"}},
                },
                "required": [],
            })
        );
    }

    #[test]
    fn any_nested_in_object() {
        let schema = ShapeSchema::Object(vec![("data".to_owned(), ShapeSchema::Any)], vec![]);
        assert_eq!(
            shape_to_json_schema(&schema),
            json!({
                "type": "object",
                "properties": {"data": {}},
                "required": [],
            })
        );
    }
}
