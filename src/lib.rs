#![forbid(unsafe_code)]

//! The JSON Pointer path technology — a technology of `xmip-core-path`.
//!
//! [`JsonPointerLanguage`] is the [`PathLanguage`] `json-pointer`: it compiles
//! a pointer once, and the compiled pointer reads one value from a Stream's
//! JSON and writes one into a rewrite of it (ADR-0013). Promote reads through
//! it; demote writes through it; route and process read. The JSON document
//! itself — parsed once per Message, bridged to a scalar, written back as a
//! Stream, written at a pointer — is the capability's `json`, shared with the
//! other JSON languages (ADR-0044); what is this technology's is the pointer.
//!
//! A pointer is RFC 6901: `/lines/0/qty`, with `~1` for `/` and `~0` for `~`.
//! A value read is a scalar — null, boolean, number, string — and an object or
//! array at the pointer is refused rather than stringified, because a promoted
//! property is one value, not a document. A write replaces the value at the
//! pointer, or adds it when its parent is an object.

use contract::ContractError;
use path::{CompiledExpression, Content, PathLanguage, Rewriting, json};
use serde_json::Value;
use xcore::ScalarValue;

/// The language `json-pointer`.
pub struct JsonPointerLanguage;

impl PathLanguage for JsonPointerLanguage {
    fn language(&self) -> &'static str {
        "json-pointer"
    }

    /// A pointer is empty, for the whole document, or starts with `/`.
    fn compile(&self, expression: &str) -> Result<Box<dyn CompiledExpression>, ContractError> {
        if !expression.is_empty() && !expression.starts_with('/') {
            return Err(ContractError::new(format!(
                "{expression:?} is not a JSON pointer"
            )));
        }
        Ok(Box::new(Pointer(expression.to_string())))
    }
}

/// A compiled pointer.
struct Pointer(String);

impl CompiledExpression for Pointer {
    fn read(&self, content: &Content<'_>) -> Result<Option<ScalarValue>, ContractError> {
        content
            .form::<Value>()?
            .pointer(&self.0)
            .map(|found| json::scalar(found, &self.0))
            .transpose()
    }

    fn write(&self, rewriting: &mut Rewriting, value: ScalarValue) -> Result<(), ContractError> {
        let replacement = json::from_scalar(value)?;
        json::set_at_pointer(rewriting.form_mut::<Value>()?, &self.0, replacement)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use contract::fixture::stream;
    use xcore::StreamId;

    const ORDER: &str = r#"{"id":"A1","paid":false,"lines":[{"sku":"X","qty":2,"price":9.5}]}"#;

    fn compiled(pointer: &str) -> Box<dyn CompiledExpression> {
        JsonPointerLanguage.compile(pointer).expect("compiles")
    }

    #[test]
    fn reads_scalars_by_pointer_and_refuses_structures() {
        let order = stream(ORDER);
        let content = Content::of(&order);
        let read = |p: &str| compiled(p).read(&content);
        assert_eq!(
            read("/id").expect("reads"),
            Some(ScalarValue::Text("A1".into()))
        );
        assert_eq!(
            read("/paid").expect("reads"),
            Some(ScalarValue::Bool(false))
        );
        assert_eq!(
            read("/lines/0/qty").expect("reads"),
            Some(ScalarValue::Integer(2))
        );
        assert_eq!(
            read("/lines/0/price").expect("reads"),
            Some(ScalarValue::Decimal(9.5))
        );
        assert_eq!(read("/nowhere").expect("reads"), None);
        assert!(read("/lines").is_err());
        assert!(JsonPointerLanguage.compile("id").is_err());
    }

    #[test]
    fn rewrites_into_a_new_stream_with_the_given_id() {
        let mut rewriting = Rewriting::of(&stream(ORDER), StreamId::new(2));
        compiled("/paid")
            .write(&mut rewriting, ScalarValue::Bool(true))
            .expect("writes");
        compiled("/ref")
            .write(&mut rewriting, ScalarValue::Text("R7".into()))
            .expect("adds");
        assert!(
            compiled("/nowhere/deep")
                .write(&mut rewriting, ScalarValue::Null)
                .is_err()
        );
        let out = rewriting.finish().expect("finishes");
        assert_eq!(out.id(), StreamId::new(2));
        assert_eq!(out.media_type(), Some("application/json"));
        let back = Content::of(&out);
        assert_eq!(
            compiled("/paid").read(&back).expect("reads"),
            Some(ScalarValue::Bool(true))
        );
        assert_eq!(
            compiled("/ref").read(&back).expect("reads"),
            Some(ScalarValue::Text("R7".into()))
        );
    }

    #[test]
    fn a_stream_that_is_not_json_is_refused() {
        let broken = stream("{nope");
        assert!(compiled("/id").read(&Content::of(&broken)).is_err());
        let mut rewriting = Rewriting::of(&broken, StreamId::new(1));
        assert!(
            compiled("/id")
                .write(&mut rewriting, ScalarValue::Null)
                .is_err()
        );
    }
}
