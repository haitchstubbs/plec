//! Bounded primitives for decoding browser-owned JSON values without first
//! materializing a `serde_json::Value` tree.

use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;

pub struct ObjectDecoder<'a> {
    value: &'a JsValue,
    fields: Vec<String>,
}

impl<'a> ObjectDecoder<'a> {
    pub fn new(value: &'a JsValue) -> Result<Self, JsValue> {
        if !value.is_object() || value.is_null() || value.is_instance_of::<js_sys::Array>() {
            return Err(type_error("object"));
        }
        let keys = js_sys::Object::keys(value.unchecked_ref::<js_sys::Object>());
        let mut fields = Vec::with_capacity(keys.length() as usize);
        for key in keys.iter() {
            fields.push(key.as_string().ok_or_else(|| type_error("string key"))?);
        }
        Ok(Self { value, fields })
    }

    pub fn has(&self, name: &str) -> bool {
        self.fields.iter().any(|field| field == name)
    }

    pub fn get(&mut self, name: &str) -> Result<JsValue, JsValue> {
        if !self.has(name) {
            return Err(missing(name));
        }
        self.fields.retain(|field| field != name);
        js_sys::Reflect::get(self.value, &JsValue::from_str(name))
    }

    pub fn get_alias(&mut self, name: &str, alias: &str) -> Result<JsValue, JsValue> {
        match (self.has(name), self.has(alias)) {
            (true, true) => Err(JsValue::from_str(&format!("duplicate field `{name}`"))),
            (true, false) => self.get(name),
            (false, true) => self.get(alias),
            (false, false) => Err(missing(name)),
        }
    }

    pub fn optional_alias(&mut self, name: &str, alias: &str) -> Result<Option<JsValue>, JsValue> {
        match (self.has(name), self.has(alias)) {
            (true, true) => Err(JsValue::from_str(&format!("duplicate field `{name}`"))),
            (true, false) => self.optional(name),
            (false, true) => self.optional(alias),
            (false, false) => Ok(None),
        }
    }

    pub fn optional(&mut self, name: &str) -> Result<Option<JsValue>, JsValue> {
        if !self.has(name) {
            return Ok(None);
        }
        let value = self.get(name)?;
        Ok(if value.is_null() || value.is_undefined() {
            None
        } else {
            Some(value)
        })
    }

    pub fn defaulted(&mut self, name: &str, default: JsValue) -> Result<JsValue, JsValue> {
        if self.has(name) {
            self.get(name)
        } else {
            Ok(default)
        }
    }

    pub fn reject_unknown(self) -> Result<(), JsValue> {
        match self.fields.first() {
            Some(field) => Err(JsValue::from_str(&format!("unknown field `{field}`"))),
            None => Ok(()),
        }
    }
}

pub fn array(value: &JsValue) -> Result<Vec<JsValue>, JsValue> {
    if !value.is_instance_of::<js_sys::Array>() {
        return Err(type_error("sequence"));
    }
    Ok(js_sys::Array::from(value).iter().collect())
}

pub fn string(value: &JsValue) -> Result<String, JsValue> {
    value.as_string().ok_or_else(|| type_error("string"))
}

pub fn boolean(value: &JsValue) -> Result<bool, JsValue> {
    value.as_bool().ok_or_else(|| type_error("boolean"))
}

pub fn number(value: &JsValue) -> Result<f64, JsValue> {
    value.as_f64().ok_or_else(|| type_error("number"))
}

pub fn usize(value: &JsValue) -> Result<usize, JsValue> {
    let value = number(value)?;
    let out_of_range = if usize::BITS == 64 {
        // `usize::MAX as f64` rounds to 2^64, which itself is out of range.
        value >= 18_446_744_073_709_551_616.0
    } else {
        value > usize::MAX as f64
    };
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || out_of_range {
        return Err(type_error("unsigned integer"));
    }
    Ok(value as usize)
}

pub fn i64(value: &JsValue) -> Result<i64, JsValue> {
    let value = number(value)?;
    // `i64::MAX as f64` rounds up to 2^63, which is already outside the
    // representable integer range. Use a strict upper bound to avoid a
    // saturating float-to-int cast silently accepting it.
    if !value.is_finite()
        || value.fract() != 0.0
        || value < -9_223_372_036_854_775_808.0
        || value >= 9_223_372_036_854_775_808.0
    {
        return Err(type_error("integer"));
    }
    Ok(value as i64)
}

pub fn u32(value: &JsValue) -> Result<u32, JsValue> {
    let value = number(value)?;
    if !value.is_finite() || value.fract() != 0.0 || !(0.0..=u32::MAX as f64).contains(&value) {
        return Err(type_error("u32"));
    }
    Ok(value as u32)
}

pub fn tagged<'a>(value: &'a JsValue, field: &str) -> Result<(ObjectDecoder<'a>, String), JsValue> {
    let mut object = ObjectDecoder::new(value)?;
    let tag = string(&object.get(field)?)?;
    Ok((object, tag))
}

/// Decodes the recursive JSON value representation iteratively bounded by the
/// same global JS depth/node ceilings used during normalization.
pub fn runtime_value(value: &JsValue) -> Result<crate::RuntimeValue, JsValue> {
    fn visit(
        value: &JsValue,
        depth: usize,
        nodes: &mut usize,
    ) -> Result<crate::RuntimeValue, JsValue> {
        if depth > plec_ir::limits::MAX_DECODE_JS_DEPTH {
            return Err(JsValue::from_str(
                "payload nesting exceeds the decode depth limit",
            ));
        }
        *nodes += 1;
        if *nodes > plec_ir::limits::MAX_DECODE_JS_NODES {
            return Err(JsValue::from_str(
                "payload property or element count exceeds the decode width limit",
            ));
        }
        if value.is_null() || value.is_undefined() {
            Ok(crate::RuntimeValue::Null)
        } else if let Some(value) = value.as_bool() {
            Ok(crate::RuntimeValue::Bool(value))
        } else if let Some(value) = value.as_f64() {
            Ok(crate::RuntimeValue::Number(value))
        } else if let Some(value) = value.as_string() {
            Ok(crate::RuntimeValue::String(value))
        } else if value.is_instance_of::<js_sys::Array>() {
            let values = array(value)?;
            let decoded = values
                .iter()
                .map(|value| visit(value, depth + 1, nodes))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(crate::RuntimeValue::Array(decoded))
        } else if value.is_object() {
            let keys = js_sys::Object::keys(value.unchecked_ref::<js_sys::Object>());
            let mut record = std::collections::HashMap::with_capacity(keys.length() as usize);
            for key in keys.iter() {
                let name = string(&key)?;
                let child = js_sys::Reflect::get(value, &key)?;
                record.insert(name, visit(&child, depth + 1, nodes)?);
            }
            Ok(crate::RuntimeValue::Record(record))
        } else {
            Err(type_error("JSON value"))
        }
    }

    visit(value, 0, &mut 0)
}

pub fn runtime_record(
    value: &JsValue,
) -> Result<std::collections::HashMap<String, crate::RuntimeValue>, JsValue> {
    let object = ObjectDecoder::new(value)?;
    let keys = js_sys::Object::keys(object.value.unchecked_ref::<js_sys::Object>());
    let mut record = std::collections::HashMap::with_capacity(keys.length() as usize);
    for key in keys.iter() {
        let name = string(&key)?;
        let child = js_sys::Reflect::get(value, &key)?;
        record.insert(name, runtime_value(&child)?);
    }
    Ok(record)
}

pub fn type_error(expected: &str) -> JsValue {
    JsValue::from_str(&format!("expected {expected}"))
}

pub fn missing(name: &str) -> JsValue {
    JsValue::from_str(&format!("missing field `{name}`"))
}
