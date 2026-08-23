fn project_shape(v: &Value) -> Option<ShapeSchema> {
    let o = v.as_object()?;
    let allowed = [
        "$schema",
        "$id",
        "title",
        "description",
        "default",
        "examples",
        "type",
        "properties",
        "required",
        "items",
    ];
    if o.keys()
        .any(|k| !allowed.contains(&k.as_str()) && !k.starts_with("x-"))
    {
        return None;
    }
    match o.get("type")?.as_str()? {
        "string" => Some(ShapeSchema::String),
        "number" => Some(ShapeSchema::Number),
        "integer" => Some(ShapeSchema::Integer),
        "boolean" => Some(ShapeSchema::Boolean),
        "null" => Some(ShapeSchema::Null),
        "array" => Some(ShapeSchema::Array(Box::new(project_shape(
            o.get("items")?,
        )?))),
        "object" => {
            let properties = if let Some(m) = o.get("properties").and_then(Value::as_object) {
                m.iter()
                    .map(|(k, v)| Some((k.clone(), project_shape(v)?)))
                    .collect::<Option<Vec<_>>>()?
            } else {
                Vec::new()
            };
            let required = if let Some(a) = o.get("required").and_then(Value::as_array) {
                a.iter()
                    .map(|x| x.as_str().map(str::to_owned))
                    .collect::<Option<Vec<_>>>()?
            } else {
                Vec::new()
            };
            Some(ShapeSchema::Object(properties, required))
        }
        _ => None,
    }
}
fn escape(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}
fn pointer<'a>(root: &'a Value, p: &str) -> Option<&'a Value> {
    if p.is_empty() {
        Some(root)
    } else {
        root.pointer(p)
    }
}
fn semantic_digest(v: &Value) -> String {
    let canonical = canonical_semantic_json(v);
    let digest = Sha256::digest(canonical.as_bytes());
    format!("sha256:{digest:x}")
}

fn canonical_semantic_json(v: &Value) -> String {
    fn walk(v: &Value, out: &mut String) {
        match v {
            Value::Null => out.push('n'),
            Value::Bool(b) => out.push_str(if *b { "t" } else { "f" }),
            Value::Number(n) => out.push_str(&format!("#{n};")),
            Value::String(s) => out.push_str(&serde_json::to_string(s).unwrap_or_default()),
            Value::Array(a) => {
                out.push('[');
                for x in a {
                    walk(x, out);
                    out.push(',')
                }
                out.push(']')
            }
            Value::Object(o) => {
                out.push('{');
                let m: BTreeMap<_, _> = o.iter().collect();
                for (k, x) in m {
                    out.push_str(&serde_json::to_string(k).unwrap_or_default());
                    out.push(':');
                    walk(x, out);
                    out.push(',')
                }
                out.push('}')
            }
        }
    }
    let mut c = String::new();
    walk(v, &mut c);
    c
}
