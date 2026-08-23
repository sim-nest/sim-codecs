struct State<'a> {
    root: &'a SchemaDocument,
    retriever: Option<&'a dyn SchemaRetriever>,
    errors: Vec<SchemaError>,
    refs: usize,
    branches: usize,
    regex_work: usize,
    diag_bytes: usize,
}
impl<'a> State<'a> {
    fn new(root: &'a SchemaDocument, retriever: Option<&'a dyn SchemaRetriever>) -> Self {
        Self {
            root,
            retriever,
            errors: vec![],
            refs: 0,
            branches: 0,
            regex_work: 0,
            diag_bytes: 0,
        }
    }
    fn error(&mut self, ip: &str, sp: &str, kw: &str, msg: &str) {
        if self.errors.len() >= self.root.limits.errors {
            return;
        }
        let room = self
            .root
            .limits
            .diagnostic_bytes
            .saturating_sub(self.diag_bytes);
        let message: String = msg.chars().take(room.min(512)).collect();
        self.diag_bytes += message.len();
        self.errors.push(SchemaError {
            instance_pointer: ip.into(),
            schema_pointer: sp.into(),
            keyword: kw.into(),
            message,
            source: self.root.identity.source.clone(),
        });
    }
    fn fork_valid(&mut self, s: &Value, v: &Value, ip: &str, sp: &str, depth: usize) -> bool {
        let n = self.errors.len();
        self.eval(s, v, ip, sp, depth);
        let ok = self.errors.len() == n;
        self.errors.truncate(n);
        ok
    }
    fn eval(&mut self, s: &Value, v: &Value, ip: &str, sp: &str, depth: usize) {
        if self.errors.len() >= self.root.limits.errors {
            return;
        }
        if depth > self.root.limits.instance_depth {
            self.error(ip, sp, "budget", "instance depth budget exceeded");
            return;
        }
        if let Some(b) = s.as_bool() {
            if !b {
                self.error(ip, sp, "false", "boolean schema rejects value");
            }
            return;
        }
        let Some(o) = s.as_object() else {
            return;
        };
        let reference = o
            .get("$ref")
            .and_then(Value::as_str)
            .map(|uri| ("$ref", uri))
            .or_else(|| {
                o.get("$dynamicRef")
                    .and_then(Value::as_str)
                    .map(|uri| ("$dynamicRef", uri))
            });
        if let Some((keyword, r)) = reference {
            self.refs += 1;
            if self.refs > self.root.limits.refs {
                self.error(ip, sp, "budget", "reference budget exceeded");
                return;
            }
            if let Some(frag) = r.strip_prefix('#') {
                if let Some(target) = pointer(&self.root.value, frag).cloned() {
                    self.eval(&target, v, ip, &format!("#{frag}"), depth + 1);
                } else {
                    self.error(ip, sp, keyword, "unresolved local reference");
                }
            } else if let Some(retriever) = self.retriever {
                match retriever.retrieve(r) {
                    Ok(resource) => self.eval_retrieved(resource, v, ip, sp, keyword),
                    Err(error) => self.error(ip, sp, keyword, &error.message),
                }
            } else {
                self.error(
                    ip,
                    sp,
                    keyword,
                    "external reference requires an injected retriever",
                );
            }
            return;
        }
        if let Some(c) = o.get("const")
            && c != v
        {
            self.error(ip, sp, "const", "value does not equal const");
        }
        if let Some(e) = o.get("enum").and_then(Value::as_array)
            && !e.contains(v)
        {
            self.error(ip, sp, "enum", "value is not an enum member");
        }
        if let Some(t) = o.get("type") {
            let ok = t
                .as_str()
                .map(|x| type_ok(x, v))
                .or_else(|| {
                    t.as_array()
                        .map(|a| a.iter().filter_map(Value::as_str).any(|x| type_ok(x, v)))
                })
                .unwrap_or(false);
            if !ok {
                self.error(
                    ip,
                    &format!("{sp}/type"),
                    "type",
                    "instance has the wrong JSON type",
                );
                return;
            }
        }
        for (key, want) in [("minimum", true), ("exclusiveMinimum", false)] {
            if let (Some(a), Some(b)) = (v.as_f64(), o.get(key).and_then(Value::as_f64))
                && ((want && a < b) || (!want && a <= b))
            {
                self.error(
                    ip,
                    &format!("{sp}/{key}"),
                    key,
                    "number is below the allowed bound",
                );
            }
        }
        for (key, want) in [("maximum", true), ("exclusiveMaximum", false)] {
            if let (Some(a), Some(b)) = (v.as_f64(), o.get(key).and_then(Value::as_f64))
                && ((want && a > b) || (!want && a >= b))
            {
                self.error(
                    ip,
                    &format!("{sp}/{key}"),
                    key,
                    "number is above the allowed bound",
                );
            }
        }
        if let (Some(a), Some(step)) = (v.as_f64(), o.get("multipleOf").and_then(Value::as_f64)) {
            if step <= 0.0 {
                self.error(
                    ip,
                    &format!("{sp}/multipleOf"),
                    "multipleOf",
                    "multipleOf must be positive",
                );
            } else {
                let quotient = a / step;
                if (quotient - quotient.round()).abs() > f64::EPSILON * quotient.abs().max(1.0) {
                    self.error(
                        ip,
                        &format!("{sp}/multipleOf"),
                        "multipleOf",
                        "number is not a multiple of the required step",
                    );
                }
            }
        }
        if let Some(text) = v.as_str() {
            if let Some(n) = o.get("minLength").and_then(Value::as_u64)
                && text.chars().count() < n as usize
            {
                self.error(
                    ip,
                    &format!("{sp}/minLength"),
                    "minLength",
                    "string is too short",
                );
            }
            if let Some(n) = o.get("maxLength").and_then(Value::as_u64)
                && text.chars().count() > n as usize
            {
                self.error(
                    ip,
                    &format!("{sp}/maxLength"),
                    "maxLength",
                    "string is too long",
                );
            }
            if let Some(p) = o.get("pattern").and_then(Value::as_str) {
                match self.pattern_matches(p, text) {
                    Ok(false) => self.error(
                        ip,
                        &format!("{sp}/pattern"),
                        "pattern",
                        "string does not match pattern",
                    ),
                    Ok(true) => {}
                    Err(PatternError::Budget) => {
                        self.error(ip, sp, "budget", "regex work budget exceeded")
                    }
                    Err(error) => self.error(
                        ip,
                        &format!("{sp}/pattern"),
                        "pattern",
                        error.runtime_message(),
                    ),
                }
            }
        }
        if let Some(a) = v.as_array() {
            if let Some(n) = o.get("minItems").and_then(Value::as_u64)
                && a.len() < n as usize
            {
                self.error(
                    ip,
                    &format!("{sp}/minItems"),
                    "minItems",
                    "array has too few items",
                );
            }
            if let Some(n) = o.get("maxItems").and_then(Value::as_u64)
                && a.len() > n as usize
            {
                self.error(
                    ip,
                    &format!("{sp}/maxItems"),
                    "maxItems",
                    "array has too many items",
                );
            }
            if o.get("uniqueItems").and_then(Value::as_bool) == Some(true) {
                let mut seen = BTreeSet::new();
                for item in a {
                    if !seen.insert(canonical_semantic_json(item)) {
                        self.error(
                            ip,
                            &format!("{sp}/uniqueItems"),
                            "uniqueItems",
                            "array contains duplicate items",
                        );
                        break;
                    }
                }
            }
            if let Some(prefix) = o.get("prefixItems").and_then(Value::as_array) {
                for (i, subschema) in prefix.iter().enumerate() {
                    if let Some(item) = a.get(i) {
                        self.eval(
                            subschema,
                            item,
                            &format!("{ip}/{i}"),
                            &format!("{sp}/prefixItems/{i}"),
                            depth + 1,
                        );
                    }
                }
            }
            if let Some(items) = o.get("items") {
                let start = o
                    .get("prefixItems")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len);
                for (i, x) in a.iter().enumerate().skip(start) {
                    self.eval(
                        items,
                        x,
                        &format!("{ip}/{i}"),
                        &format!("{sp}/items"),
                        depth + 1,
                    );
                }
            }
            if let Some(contains) = o.get("contains") {
                let matches = a
                    .iter()
                    .enumerate()
                    .filter(|(i, item)| {
                        self.fork_valid(
                            contains,
                            item,
                            &format!("{ip}/{i}"),
                            &format!("{sp}/contains"),
                            depth + 1,
                        )
                    })
                    .count();
                let min = o.get("minContains").and_then(Value::as_u64).unwrap_or(1) as usize;
                let max = o
                    .get("maxContains")
                    .and_then(Value::as_u64)
                    .map(|n| n as usize);
                if matches < min || max.is_some_and(|max| matches > max) {
                    self.error(
                        ip,
                        &format!("{sp}/contains"),
                        "contains",
                        "array contains count is outside the allowed bounds",
                    );
                }
            }
        }
        let mut evaluated = BTreeSet::new();
        if let Some(m) = v.as_object() {
            if let Some(n) = o.get("minProperties").and_then(Value::as_u64)
                && m.len() < n as usize
            {
                self.error(
                    ip,
                    &format!("{sp}/minProperties"),
                    "minProperties",
                    "object has too few properties",
                );
            }
            if let Some(n) = o.get("maxProperties").and_then(Value::as_u64)
                && m.len() > n as usize
            {
                self.error(
                    ip,
                    &format!("{sp}/maxProperties"),
                    "maxProperties",
                    "object has too many properties",
                );
            }
            if let Some(req) = o.get("required").and_then(Value::as_array) {
                for name in req.iter().filter_map(Value::as_str) {
                    if !m.contains_key(name) {
                        self.error(
                            ip,
                            &format!("{sp}/required"),
                            "required",
                            &format!("required property {name} is missing"),
                        );
                    }
                }
            }
            if let Some(dependent) = o.get("dependentRequired").and_then(Value::as_object) {
                for (name, required) in dependent {
                    if !m.contains_key(name) {
                        continue;
                    }
                    for required_name in required
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                    {
                        if !m.contains_key(required_name) {
                            self.error(
                                ip,
                                &format!("{sp}/dependentRequired/{}", escape(name)),
                                "dependentRequired",
                                &format!("dependent property {required_name} is missing"),
                            );
                        }
                    }
                }
            }
            if let Some(props) = o.get("properties").and_then(Value::as_object) {
                for (name, sub) in props {
                    if let Some(x) = m.get(name) {
                        evaluated.insert(name.clone());
                        self.eval(
                            sub,
                            x,
                            &format!("{ip}/{}", escape(name)),
                            &format!("{sp}/properties/{}", escape(name)),
                            depth + 1,
                        );
                    }
                }
            }
            if let Some(patterns) = o.get("patternProperties").and_then(Value::as_object) {
                for (pattern, sub) in patterns {
                    for (name, value) in m {
                        match self.pattern_matches(pattern, name) {
                            Ok(true) => {
                                evaluated.insert(name.clone());
                                self.eval(
                                    sub,
                                    value,
                                    &format!("{ip}/{}", escape(name)),
                                    &format!("{sp}/patternProperties/{}", escape(pattern)),
                                    depth + 1,
                                );
                            }
                            Ok(false) => {}
                            Err(PatternError::Budget) => {
                                self.error(ip, sp, "budget", "regex work budget exceeded")
                            }
                            Err(error) => self.error(
                                ip,
                                &format!("{sp}/patternProperties/{}", escape(pattern)),
                                "patternProperties",
                                error.runtime_message(),
                            ),
                        }
                    }
                }
            }
            if let Some(property_names) = o.get("propertyNames") {
                for name in m.keys() {
                    self.eval(
                        property_names,
                        &Value::String(name.clone()),
                        &format!("{ip}/{}", escape(name)),
                        &format!("{sp}/propertyNames"),
                        depth + 1,
                    );
                }
            }
            if let Some(dependent) = o.get("dependentSchemas").and_then(Value::as_object) {
                for (name, sub) in dependent {
                    if m.contains_key(name) {
                        self.eval(
                            sub,
                            v,
                            ip,
                            &format!("{sp}/dependentSchemas/{}", escape(name)),
                            depth + 1,
                        );
                    }
                }
            }
            if let Some(additional) = o.get("additionalProperties") {
                match additional {
                    Value::Bool(false) => {
                        let names: Vec<String> = m
                            .keys()
                            .filter(|name| !evaluated.contains(*name))
                            .cloned()
                            .collect();
                        for name in names {
                            self.error(
                                &format!("{ip}/{}", escape(&name)),
                                &format!("{sp}/additionalProperties"),
                                "additionalProperties",
                                "additional property is not allowed",
                            );
                        }
                    }
                    Value::Object(_) | Value::Bool(true)
                        if !additional.as_bool().unwrap_or(false) =>
                    {
                        let names: Vec<String> = m
                            .keys()
                            .filter(|name| !evaluated.contains(*name))
                            .cloned()
                            .collect();
                        for name in names {
                            if let Some(value) = m.get(&name) {
                                evaluated.insert(name.clone());
                                self.eval(
                                    additional,
                                    value,
                                    &format!("{ip}/{}", escape(&name)),
                                    &format!("{sp}/additionalProperties"),
                                    depth + 1,
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }
            if let Some(unevaluated) = o.get("unevaluatedProperties") {
                match unevaluated {
                    Value::Bool(false) => {
                        let names: Vec<String> = m
                            .keys()
                            .filter(|name| !evaluated.contains(*name))
                            .cloned()
                            .collect();
                        for name in names {
                            self.error(
                                &format!("{ip}/{}", escape(&name)),
                                &format!("{sp}/unevaluatedProperties"),
                                "unevaluatedProperties",
                                "unevaluated property is not allowed",
                            );
                        }
                    }
                    Value::Object(_) => {
                        let names: Vec<String> = m
                            .keys()
                            .filter(|name| !evaluated.contains(*name))
                            .cloned()
                            .collect();
                        for name in names {
                            if let Some(value) = m.get(&name) {
                                self.eval(
                                    unevaluated,
                                    value,
                                    &format!("{ip}/{}", escape(&name)),
                                    &format!("{sp}/unevaluatedProperties"),
                                    depth + 1,
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        if let Some(condition) = o.get("if") {
            if self.fork_valid(condition, v, ip, &format!("{sp}/if"), depth + 1) {
                if let Some(then_schema) = o.get("then") {
                    self.eval(then_schema, v, ip, &format!("{sp}/then"), depth + 1);
                }
            } else if let Some(else_schema) = o.get("else") {
                self.eval(else_schema, v, ip, &format!("{sp}/else"), depth + 1);
            }
        }
        for key in ["allOf", "anyOf", "oneOf"] {
            if let Some(a) = o.get(key).and_then(Value::as_array) {
                self.branches += a.len();
                if self.branches > self.root.limits.evaluated_branches {
                    self.error(ip, sp, "budget", "branch budget exceeded");
                    continue;
                }
                let count = a
                    .iter()
                    .enumerate()
                    .filter(|(i, x)| {
                        self.fork_valid(x, v, ip, &format!("{sp}/{key}/{i}"), depth + 1)
                    })
                    .count();
                let ok = match key {
                    "allOf" => count == a.len(),
                    "anyOf" => count > 0,
                    _ => count == 1,
                };
                if !ok {
                    self.error(
                        ip,
                        &format!("{sp}/{key}"),
                        key,
                        "composition constraint failed",
                    );
                }
            }
        }
        if let Some(n) = o.get("not")
            && self.fork_valid(n, v, ip, &format!("{sp}/not"), depth + 1)
        {
            self.error(ip, &format!("{sp}/not"), "not", "negated schema matched");
        }
    }

    fn eval_retrieved(
        &mut self,
        resource: RetrievedResource,
        instance: &Value,
        ip: &str,
        sp: &str,
        keyword: &str,
    ) {
        if !matches!(
            resource.media_type.as_str(),
            "application/schema+json" | "application/json"
        ) {
            self.error(
                ip,
                sp,
                keyword,
                "retrieved schema has unsupported media type",
            );
            return;
        }
        let identity = ResourceIdentity {
            base_uri: resource.base_uri.clone(),
            source: resource.base_uri,
        };
        let document = match SchemaDocument::parse(&resource.bytes, identity, self.root.limits) {
            Ok(document) => document,
            Err(error) => {
                self.error(ip, sp, keyword, &error.message);
                return;
            }
        };
        if !resource.digest.is_empty()
            && resource.digest != document.semantic_digest()
            && resource.digest != document.semantic_digest().trim_start_matches("sha256:")
        {
            self.error(ip, sp, keyword, "retrieved schema digest mismatch");
            return;
        }
        if let Err(errors) = document.validate_with(instance, self.retriever) {
            for error in errors {
                self.error(
                    &error.instance_pointer,
                    &error.schema_pointer,
                    &error.keyword,
                    &error.message,
                );
            }
        }
    }

    fn pattern_matches(&mut self, pattern: &str, text: &str) -> Result<bool, PatternError> {
        let program = EcmaPattern::compile(pattern)?;
        let remaining = self.root.limits.regex_work.saturating_sub(self.regex_work);
        let (matched, used) = program.is_match(text, remaining)?;
        self.regex_work = self.regex_work.saturating_add(used);
        Ok(matched)
    }
}

fn type_ok(t: &str, v: &Value) -> bool {
    match t {
        "object" => v.is_object(),
        "array" => v.is_array(),
        "string" => v.is_string(),
        "number" => v.is_number(),
        "integer" => v.as_i64().is_some() || v.as_u64().is_some(),
        "boolean" => v.is_boolean(),
        "null" => v.is_null(),
        _ => false,
    }
}
