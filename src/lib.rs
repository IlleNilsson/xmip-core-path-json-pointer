#![forbid(unsafe_code)]

//! The JSON Pointer path technology — a technology of `xmip-core-path`.
//!
//! Three things, because a path language is nothing without content to address:
//! [`JsonPointerEngine`], the [`PathEngine`] for the language `json-pointer`;
//! [`JsonStructure`], a [`StructureReader`] over a JSON Stream; and
//! [`JsonRewrite`], a [`StructureWriter`] that produces a new Stream with one or
//! more values replaced, as ADR-0013 asks of anything that changes content.
//! Promote reads through the first two; demote writes through the first and
//! third; route and process read. The JSON document itself — parsed once,
//! bridged to a scalar, written back as a Stream — is the capability's `json`,
//! shared with the other JSON languages (ADR-0044); what is this technology's
//! is the pointer.
//!
//! A pointer is RFC 6901: `/lines/0/qty`, with `~1` for `/` and `~0` for `~`.
//! A value read is a scalar — null, boolean, number, string — and an object or
//! array at the pointer is refused rather than stringified, because a promoted
//! property is one value, not a document.

use path::json::{self, Document, Rewrite};
use path::{Path, PathCost, PathEngine};
use sdk::contract::{
    ContractDescriptor, ContractError, StructureReader, StructureWriter, StructuredValue,
};
use serde_json::Value;
use stream::Stream;
use xcore::StreamId;

/// The `json-pointer` engine. The reader speaks pointers already, so the
/// engine adds no traversal of its own.
pub struct JsonPointerEngine;

impl PathEngine for JsonPointerEngine {
    fn language(&self) -> &'static str {
        "json-pointer"
    }

    fn read(
        &self,
        reader: &dyn StructureReader,
        path: &Path,
    ) -> Result<Option<StructuredValue>, ContractError> {
        reader.read(&path.expression)
    }

    fn write(
        &self,
        writer: &mut dyn StructureWriter,
        path: &Path,
        value: StructuredValue,
    ) -> Result<(), ContractError> {
        writer.write(&path.expression, value)
    }

    /// JSON has no prefix an answer can be read from; the document is parsed
    /// whole.
    fn cost(&self, _path: &Path) -> PathCost {
        PathCost::Materialized
    }
}

/// A JSON Stream, read by pointer.
pub struct JsonStructure {
    document: Document,
}

impl JsonStructure {
    /// Parse `stream` once; every read is a pointer walk after that.
    ///
    /// # Errors
    /// The Stream is not JSON.
    pub fn parse(stream: &Stream) -> Result<Self, ContractError> {
        Ok(Self {
            document: Document::parse(stream)?,
        })
    }
}

impl StructureReader for JsonStructure {
    fn contract(&self) -> &ContractDescriptor {
        &self.document.descriptor
    }

    fn read(&self, path: &str) -> Result<Option<StructuredValue>, ContractError> {
        self.document
            .value
            .pointer(path)
            .map(|found| json::scalar(found, path))
            .transpose()
    }
}

/// A JSON Stream being rewritten into a new one.
pub struct JsonRewrite {
    rewrite: Rewrite,
}

impl JsonRewrite {
    /// Start from `stream`; the Stream `finish` produces carries `id`.
    ///
    /// # Errors
    /// The Stream is not JSON.
    pub fn of(stream: &Stream, id: StreamId) -> Result<Self, ContractError> {
        Ok(Self {
            rewrite: Rewrite::of(stream, id)?,
        })
    }
}

impl StructureWriter for JsonRewrite {
    fn contract(&self) -> &ContractDescriptor {
        &self.rewrite.descriptor
    }

    /// Replace the value at `path`, or add it when its parent is an object.
    /// A pointer into nothing is refused: demote names a place, it does not
    /// invent structure.
    fn write(&mut self, path: &str, value: StructuredValue) -> Result<(), ContractError> {
        let replacement = json::from_scalar(value)?;
        if let Some(existing) = self.rewrite.value.pointer_mut(path) {
            *existing = replacement;
            return Ok(());
        }
        let (parent, key) = path
            .rsplit_once('/')
            .ok_or_else(|| ContractError::new(format!("{path:?} is not a JSON pointer")))?;
        let key = key.replace("~1", "/").replace("~0", "~");
        match self.rewrite.value.pointer_mut(parent) {
            Some(Value::Object(members)) => {
                members.insert(key, replacement);
                Ok(())
            }
            _ => Err(ContractError::new(format!(
                "{path:?} has no object to write into"
            ))),
        }
    }

    fn finish(self: Box<Self>) -> Result<Stream, ContractError> {
        self.rewrite.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use path::fixture::stream;

    const ORDER: &str = r#"{"id":"A1","paid":false,"lines":[{"sku":"X","qty":2,"price":9.5}]}"#;

    #[test]
    fn reads_scalars_by_pointer_and_refuses_structures() {
        let structure = JsonStructure::parse(&stream(ORDER)).expect("parses");
        let engine = JsonPointerEngine;
        let read = |p: &str| engine.read(&structure, &Path::new("json-pointer", p));
        assert_eq!(
            read("/id").expect("reads"),
            Some(StructuredValue::Text("A1".into()))
        );
        assert_eq!(
            read("/paid").expect("reads"),
            Some(StructuredValue::Bool(false))
        );
        assert_eq!(
            read("/lines/0/qty").expect("reads"),
            Some(StructuredValue::Integer(2))
        );
        assert_eq!(
            read("/lines/0/price").expect("reads"),
            Some(StructuredValue::Decimal(9.5))
        );
        assert_eq!(read("/nowhere").expect("reads"), None);
        assert!(read("/lines").is_err());
        assert_eq!(
            engine.cost(&Path::new("json-pointer", "/id")),
            PathCost::Materialized
        );
    }

    #[test]
    fn rewrites_into_a_new_stream_with_the_given_id() {
        let mut rewrite = JsonRewrite::of(&stream(ORDER), StreamId::new(2)).expect("parses");
        let engine = JsonPointerEngine;
        engine
            .write(
                &mut rewrite,
                &Path::new("json-pointer", "/paid"),
                StructuredValue::Bool(true),
            )
            .expect("writes");
        engine
            .write(
                &mut rewrite,
                &Path::new("json-pointer", "/ref"),
                StructuredValue::Text("R7".into()),
            )
            .expect("adds");
        assert!(
            rewrite
                .write("/nowhere/deep", StructuredValue::Null)
                .is_err()
        );
        let out = Box::new(rewrite).finish().expect("finishes");
        assert_eq!(out.id(), StreamId::new(2));
        assert_eq!(out.media_type(), Some("application/json"));
        let back = JsonStructure::parse(&out).expect("parses");
        assert_eq!(
            back.read("/paid").expect("reads"),
            Some(StructuredValue::Bool(true))
        );
        assert_eq!(
            back.read("/ref").expect("reads"),
            Some(StructuredValue::Text("R7".into()))
        );
    }

    #[test]
    fn a_stream_that_is_not_json_is_refused_up_front() {
        assert!(JsonStructure::parse(&stream("{nope")).is_err());
        assert!(JsonRewrite::of(&stream("{nope"), StreamId::new(1)).is_err());
    }
}
