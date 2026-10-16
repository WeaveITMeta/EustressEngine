//! Class methods, declared once: what an instance can be asked to do beyond
//! having its properties set (a road splitting at a station, a network
//! finding a route). Every method is declared here, with typed parameters,
//! what it returns and what it can change, so each surface that calls methods
//! (MCP's `call_method` today; Luau, Rune and the docs can read the same table)
//! agrees on what exists and on who may call it. The engine plugin that owns
//! the class implements the method under the same class and name.

/// What a method can do to the Space, which decides who may call it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodEffect {
    /// Reads; changes nothing.
    Read,
    /// Creates or changes data inside the Space.
    Write,
    /// Removes data, or changes it past an undo.
    Destructive,
}

/// The type of one method parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    Number,
    Integer,
    Bool,
    String,
    /// Three numbers, `[x, y, z]`.
    Vec3,
    /// Another instance, named as a tool names one: a Space-relative path or
    /// a uuid.
    Instance,
    /// One of a fixed set of names.
    Choice(&'static [&'static str]),
}

/// One method parameter.
#[derive(Debug, Clone, Copy)]
pub struct MethodParam {
    pub name: &'static str,
    pub kind: ParamKind,
    /// SI unit, empty when dimensionless (a station along a road is "m").
    pub unit: &'static str,
    pub required: bool,
    pub doc: &'static str,
}

/// One declared method.
#[derive(Debug, Clone, Copy)]
pub struct MethodDecl {
    /// The class name, as `ClassName::as_str` gives it.
    pub class: &'static str,
    pub name: &'static str,
    pub params: &'static [MethodParam],
    /// What it returns, in words.
    pub returns: &'static str,
    pub doc: &'static str,
    pub effect: MethodEffect,
}

/// Every declared class method. A class adds its methods here, and its
/// engine plugin implements each under the same class and name.
pub static METHODS: &[MethodDecl] = &[];

/// The method `name` of `class`, when declared.
pub fn declared(class: &str, name: &str) -> Option<&'static MethodDecl> {
    METHODS.iter().find(|m| m.class == class && m.name == name)
}

/// The methods declared for `class`.
pub fn methods_of(class: &str) -> impl Iterator<Item = &'static MethodDecl> + '_ {
    METHODS.iter().filter(move |m| m.class == class)
}

/// `"Road:Split"` as `("Road", "Split")`: how a caller names a method
/// together with its class, so who may call it is known from the call alone.
pub fn split_qualified(method: &str) -> Option<(&str, &str)> {
    let (class, name) = method.trim().split_once(':')?;
    let (class, name) = (class.trim(), name.trim());
    (!class.is_empty() && !name.is_empty() && !name.contains(':')).then_some((class, name))
}

/// The JSON Schema of a method's arguments object, built from its
/// declaration, so a tool schema and the docs never disagree with it.
pub fn params_schema(decl: &MethodDecl) -> serde_json::Value {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    for p in decl.params {
        let mut schema = match p.kind {
            ParamKind::Number => serde_json::json!({ "type": "number" }),
            ParamKind::Integer => serde_json::json!({ "type": "integer" }),
            ParamKind::Bool => serde_json::json!({ "type": "boolean" }),
            ParamKind::String => serde_json::json!({ "type": "string" }),
            ParamKind::Vec3 => serde_json::json!({ "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 3 }),
            ParamKind::Instance => serde_json::json!({ "type": "string", "description": "An instance: its Space-relative path or its uuid." }),
            ParamKind::Choice(options) => serde_json::json!({ "type": "string", "enum": options }),
        };
        let doc = if p.unit.is_empty() { p.doc.to_string() } else { format!("{} ({})", p.doc, p.unit) };
        if let Some(map) = schema.as_object_mut() {
            map.entry("description").or_insert(serde_json::Value::String(doc));
        }
        properties.insert(p.name.to_string(), schema);
        if p.required {
            required.push(p.name);
        }
    }
    serde_json::json!({ "type": "object", "properties": properties, "required": required })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_method_is_named_with_its_class() {
        assert_eq!(split_qualified("Road:Split"), Some(("Road", "Split")));
        assert_eq!(split_qualified(" RoadNetwork : Route "), Some(("RoadNetwork", "Route")));
        for bad in ["Split", ":Split", "Road:", "Road:Split:Again", ""] {
            assert_eq!(split_qualified(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_declaration_gives_its_schema() {
        static PARAMS: &[MethodParam] = &[
            MethodParam { name: "station", kind: ParamKind::Number, unit: "m", required: true, doc: "Distance along the road" },
            MethodParam { name: "side", kind: ParamKind::Choice(&["Left", "Right"]), unit: "", required: false, doc: "Which side" },
        ];
        let decl = MethodDecl {
            class: "Road",
            name: "Split",
            params: PARAMS,
            returns: "the new road",
            doc: "Split the road",
            effect: MethodEffect::Write,
        };
        let schema = params_schema(&decl);
        assert_eq!(schema["properties"]["station"]["type"], "number");
        assert_eq!(schema["properties"]["station"]["description"], "Distance along the road (m)");
        assert_eq!(schema["properties"]["side"]["enum"][1], "Right");
        assert_eq!(schema["required"], serde_json::json!(["station"]));
    }

    #[test]
    fn nothing_is_declared_yet() {
        assert!(METHODS.is_empty());
        assert!(declared("Road", "Split").is_none());
        assert_eq!(methods_of("Road").count(), 0);
    }
}
