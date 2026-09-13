use crate::backend::backend_for_dialect;
use crate::dialect::Dialect;
use crate::error::BuilderError;
use crate::types::{Primitive, SqlIdentifier, SqlQuery, SqlRaw, SqlValue};
use smallvec::SmallVec;

#[derive(Debug, Default)]
pub(crate) struct SqlQueryBuilder {
    text: String,
    raw: String,
    values: SmallVec<[Primitive; 4]>,
}

impl SqlQueryBuilder {
    pub(crate) fn with_capacity(text_capacity: usize, raw_capacity: usize) -> Self {
        Self {
            text: String::with_capacity(text_capacity),
            raw: String::with_capacity(raw_capacity),
            values: SmallVec::new(),
        }
    }

    pub(crate) fn push_text_raw(&mut self, text: &str, raw: &str) {
        self.text.push_str(text);
        self.raw.push_str(raw);
    }

    pub(crate) fn push_query(&mut self, query: &SqlQuery) {
        self.text.push_str(&query.text);
        self.raw.push_str(&query.raw);
        self.values.extend(query.values.iter().cloned());
    }

    pub(crate) fn push_value(&mut self, value: &SqlValue) -> Result<(), BuilderError> {
        let fragment = to_query_fragment_typed(value)?;
        self.push_query(&fragment);
        Ok(())
    }

    pub(crate) fn finish(self) -> SqlQuery {
        SqlQuery {
            text: self.text,
            raw: self.raw,
            values: self.values.into_vec(),
        }
    }
}

// ─── Identifier rendering ────────────────────────────────────────────────────

fn validate_identifier_part(part: &str) -> Result<(), BuilderError> {
    if part.is_empty() {
        return Err(BuilderError::Validation(
            "Identifier part cannot be empty".to_string(),
        ));
    }
    Ok(())
}

fn render_identifier_parts_impl(parts: &[String], escaped: bool) -> Result<String, BuilderError> {
    let mut rendered = String::new();

    for (index, part) in parts.iter().enumerate() {
        validate_identifier_part(part)?;
        if index > 0 {
            rendered.push('.');
        }
        if escaped {
            rendered.push('"');
            rendered.push_str(&part.replace('"', "\"\""));
            rendered.push('"');
        } else {
            rendered.push_str(part);
        }
    }

    Ok(rendered)
}

// ─── Value rendering (for raw SQL) ──────────────────────────────────────────

pub(crate) fn render_value(value: &Primitive) -> String {
    match value {
        Primitive::Null => "NULL".to_string(),
        Primitive::Bool(b) => {
            if *b {
                "TRUE".to_string()
            } else {
                "FALSE".to_string()
            }
        }
        Primitive::Number(n) => n.to_string(),
        Primitive::String(s) => format!("'{}'", s.replace('\'', "''")),
        Primitive::BigInt(bi) => bi.value.clone(),
        Primitive::Date(d) => format!("'{}'", d.value.replace('\'', "''")),
    }
}

// ─── SqlValue → SqlQuery conversion ─────────────────────────────────────────

/// Convert any SqlValue to a SqlQuery fragment (for interpolation inside sql!/join).
fn to_query_fragment_typed(value: &SqlValue) -> Result<SqlQuery, BuilderError> {
    match value {
        SqlValue::Query(q) => Ok(q.clone()),
        SqlValue::Identifier(id) => {
            let text = render_identifier_parts_impl(&id.parts, true)?;
            let raw = render_identifier_parts_impl(&id.parts, false)?;
            Ok(SqlQuery {
                text,
                raw,
                values: vec![],
            })
        }
        SqlValue::Raw(r) => Ok(SqlQuery {
            text: r.text.clone(),
            raw: r.text.clone(),
            values: vec![],
        }),
        SqlValue::Array(arr) => {
            if arr.is_empty() {
                return Err(BuilderError::Validation(
                    "Cannot interpolate an empty array".to_string(),
                ));
            }
            let mut builder = SqlQueryBuilder::with_capacity(
                arr.len().saturating_mul(3),
                arr.len().saturating_mul(8),
            );
            for (index, primitive) in arr.iter().enumerate() {
                if index > 0 {
                    builder.push_text_raw(", ", ", ");
                }
                builder.push_text_raw("?", &render_value(primitive));
                builder.values.push(primitive.clone());
            }
            Ok(builder.finish())
        }
        SqlValue::Primitive(p) => Ok(SqlQuery {
            text: "?".to_string(),
            raw: render_value(p),
            values: vec![p.clone()],
        }),
    }
}

/// Convert any SqlValue to a SqlQuery fragment (for interpolation inside sql!/join).
pub fn to_query_fragment(value: &SqlValue) -> Result<SqlQuery, String> {
    to_query_fragment_typed(value).map_err(|err| err.to_string())
}

// ─── Public SQL functions ────────────────────────────────────────────────────

/// Create a quoted SQL identifier.
pub fn identifier(parts: Vec<String>) -> Result<SqlIdentifier, String> {
    if parts.is_empty() {
        return Err("Identifier must have at least one part".to_string());
    }
    for part in &parts {
        validate_identifier_part(part).map_err(|err| err.to_string())?;
    }
    Ok(SqlIdentifier::new(parts))
}

/// Create a reference identifier (same as identifier — both quote parts).
pub fn ref_identifier(parts: Vec<String>) -> Result<SqlIdentifier, String> {
    identifier(parts)
}

/// Create a raw (unquoted, unparameterised) SQL fragment.
pub fn raw(text: String) -> SqlRaw {
    SqlRaw::new(text)
}

/// Join a list of SqlValues with a separator (default: ", ").
pub fn join(items: Vec<SqlValue>, separator: Option<String>) -> Result<SqlQuery, String> {
    let sep = separator.unwrap_or_else(|| ", ".to_string());
    let mut builder = SqlQueryBuilder::with_capacity(
        items.len().saturating_mul(8),
        items.len().saturating_mul(8),
    );

    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            builder.push_text_raw(&sep, &sep);
        }
        builder.push_value(item).map_err(|err| err.to_string())?;
    }

    Ok(builder.finish())
}

/// SQL template function: interleave string chunks with rendered SqlValue expressions.
pub fn sql(strings: Vec<String>, exprs: Vec<SqlValue>) -> Result<SqlQuery, String> {
    let string_capacity = strings.iter().map(String::len).sum::<usize>();
    let mut builder = SqlQueryBuilder::with_capacity(
        string_capacity + exprs.len().saturating_mul(2),
        string_capacity + exprs.len().saturating_mul(8),
    );

    for (i, chunk) in strings.iter().enumerate() {
        builder.push_text_raw(chunk, chunk);

        if i >= exprs.len() {
            continue;
        }
        builder
            .push_value(&exprs[i])
            .map_err(|err| err.to_string())?;
    }

    Ok(builder.finish())
}

pub fn compile_query(query: SqlQuery, dialect: &str) -> Result<SqlQuery, String> {
    compile_query_dialect(query, Dialect::parse(dialect))
}

pub fn compile_query_dialect(query: SqlQuery, dialect: Dialect) -> Result<SqlQuery, String> {
    backend_for_dialect(&dialect).compile_placeholders(query)
}

#[deprecated(
    note = "use compile_query(query, \"postgres\") or compile_query_dialect(query, Dialect::Postgres) instead"
)]
pub fn compile_postgres(query: SqlQuery) -> Result<SqlQuery, String> {
    compile_query_dialect(query, Dialect::Postgres)
}
