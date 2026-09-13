use serde::{Deserialize, Serialize};

// Tag enums for discriminated unions — only accept the exact string literal they represent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum IdentifierKindTag {
    #[serde(rename = "identifier")]
    Identifier,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum RawKindTag {
    #[serde(rename = "raw")]
    Raw,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum BigIntTag {
    #[serde(rename = "bigint")]
    BigInt,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum DateTag {
    #[serde(rename = "date")]
    Date,
}

/// Wire representation of a BigInt value: { "__kind": "bigint", "value": "123" }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BigIntWire {
    #[serde(rename = "__kind")]
    kind: BigIntTag,
    pub value: String,
}

impl BigIntWire {
    pub fn new(value: String) -> Self {
        Self {
            kind: BigIntTag::BigInt,
            value,
        }
    }
}

/// Wire representation of a Date value: { "__kind": "date", "value": "2023-01-01T00:00:00.000Z" }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DateWire {
    #[serde(rename = "__kind")]
    kind: DateTag,
    pub value: String,
}

impl DateWire {
    pub fn new(value: String) -> Self {
        Self {
            kind: DateTag::Date,
            value,
        }
    }
}

/// A SQL primitive value (a query parameter or literal). Matches the JS `Primitive` type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Primitive {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(String),
    BigInt(BigIntWire),
    Date(DateWire),
}

/// A compiled SQL query fragment: parameterised text, raw literal text, and bound values.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SqlQuery {
    pub text: String,
    pub raw: String,
    pub values: Vec<Primitive>,
}

/// A quoted SQL identifier (table or column name).
/// Wire: { "__kind": "identifier", "parts": ["schema", "table"] }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqlIdentifier {
    #[serde(rename = "__kind")]
    kind: IdentifierKindTag,
    pub parts: Vec<String>,
}

impl SqlIdentifier {
    pub fn new(parts: Vec<String>) -> Self {
        Self {
            kind: IdentifierKindTag::Identifier,
            parts,
        }
    }
}

/// A raw SQL fragment (not parameterised, injected verbatim).
/// Wire: { "__kind": "raw", "text": "..." }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqlRaw {
    #[serde(rename = "__kind")]
    kind: RawKindTag,
    pub text: String,
}

impl SqlRaw {
    pub fn new(text: String) -> Self {
        Self {
            kind: RawKindTag::Raw,
            text,
        }
    }
}

/// Any value that can appear in a SQL expression.
/// The untagged order ensures correct deserialization: arrays before objects, specific
/// objects before generic primitives.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SqlValue {
    Array(Vec<Primitive>),
    Query(SqlQuery),
    Identifier(SqlIdentifier),
    Raw(SqlRaw),
    Primitive(Primitive),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowOrderItem {
    pub expression: SqlQuery,
    pub direction: Option<String>,
    pub nulls: Option<String>,
}

/// The output of a successful SQL render pass.
///
/// In addition to the compiled [`SqlQuery`], the result carries any
/// [`DialectWarning`](crate::dialect::DialectWarning)s that were produced
/// during rendering — for example, when a fallback rewrite was applied
/// because the active dialect lacks native support for a feature.
///
/// Callers should inspect `warnings` to discover rewrites, or set
/// `dialect_strict` on the builder to promote rewrites to hard errors.
#[derive(Debug, Clone)]
pub struct RenderResult {
    /// The rendered SQL query with bound parameters and raw text.
    pub query: SqlQuery,
    /// Warnings emitted for any fallback rewrites applied during rendering.
    pub warnings: Vec<crate::dialect::DialectWarning>,
}
