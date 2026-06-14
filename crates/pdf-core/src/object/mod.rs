//! The PDF object model — ISO 32000-1 §7.3, PRD §9.2.
//!
//! The value graph is **flat**: an [`Object`] stores child references as
//! [`Object::Reference`] rather than boxing resolved children, so the graph is
//! cheap to `Clone` and FFI-friendly (PRD §8.1). Stream payloads live
//! out-of-line in [`bytes::Bytes`] (see [`stream`]).

mod name;
mod stream;
mod string;

pub mod parse;

pub use name::Name;
pub use stream::{StreamData, StreamObj};
pub use string::{PdfString, StringKind};

/// An ordered map of [`Name`] → [`Object`]. `BTreeMap` keeps keys sorted so
/// serialization is deterministic (PRD §9.2).
pub type Dict = std::collections::BTreeMap<Name, Object>;

/// An indirect-object reference `num gen R` (ISO 32000-1 §7.3.10).
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct ObjRef {
    /// Object number.
    pub num: u32,
    /// Generation number.
    pub gen: u16,
}

impl ObjRef {
    /// Creates a reference.
    #[must_use]
    pub const fn new(num: u32, gen: u16) -> Self {
        ObjRef { num, gen }
    }
}

/// A PDF object (the 8 base types plus indirect reference). PRD §9.2.
#[derive(Clone, Debug, PartialEq)]
pub enum Object {
    /// The null object.
    Null,
    /// A boolean.
    Boolean(bool),
    /// An integer (`i64`).
    Integer(i64),
    /// A real number (`f64`).
    Real(f64),
    /// A string (literal or hex).
    String(PdfString),
    /// A name.
    Name(Name),
    /// An array (heterogeneous, may contain references).
    Array(Vec<Object>),
    /// A dictionary.
    Dictionary(Dict),
    /// A stream (dict + out-of-line payload).
    Stream(StreamObj),
    /// An indirect reference.
    Reference(ObjRef),
}

impl Object {
    // --- predicates -------------------------------------------------------

    /// `true` when this is [`Object::Null`].
    #[must_use]
    pub fn is_null(&self) -> bool {
        matches!(self, Object::Null)
    }

    // --- accessors --------------------------------------------------------

    /// The boolean value, if this is a boolean.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Object::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    /// The integer value, if this is an integer.
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Object::Integer(i) => Some(*i),
            _ => None,
        }
    }

    /// The numeric value as `f64`, accepting both integers and reals.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Object::Integer(i) => Some(*i as f64),
            Object::Real(r) => Some(*r),
            _ => None,
        }
    }

    /// The string, if this is a string.
    #[must_use]
    pub fn as_string(&self) -> Option<&PdfString> {
        match self {
            Object::String(s) => Some(s),
            _ => None,
        }
    }

    /// The name, if this is a name.
    #[must_use]
    pub fn as_name(&self) -> Option<&Name> {
        match self {
            Object::Name(n) => Some(n),
            _ => None,
        }
    }

    /// The array, if this is an array.
    #[must_use]
    pub fn as_array(&self) -> Option<&[Object]> {
        match self {
            Object::Array(a) => Some(a),
            _ => None,
        }
    }

    /// The dictionary, if this is a dictionary **or** a stream (a stream's dict
    /// is conventionally addressable as a dictionary).
    #[must_use]
    pub fn as_dict(&self) -> Option<&Dict> {
        match self {
            Object::Dictionary(d) => Some(d),
            Object::Stream(s) => Some(&s.dict),
            _ => None,
        }
    }

    /// The stream, if this is a stream.
    #[must_use]
    pub fn as_stream(&self) -> Option<&StreamObj> {
        match self {
            Object::Stream(s) => Some(s),
            _ => None,
        }
    }

    /// The reference, if this is an indirect reference.
    #[must_use]
    pub fn as_reference(&self) -> Option<ObjRef> {
        match self {
            Object::Reference(r) => Some(*r),
            _ => None,
        }
    }
}

// --- ergonomic `From` impls (used by tests & callers) ---------------------

impl From<bool> for Object {
    fn from(b: bool) -> Self {
        Object::Boolean(b)
    }
}

impl From<i64> for Object {
    fn from(i: i64) -> Self {
        Object::Integer(i)
    }
}

impl From<i32> for Object {
    fn from(i: i32) -> Self {
        Object::Integer(i as i64)
    }
}

impl From<f64> for Object {
    fn from(r: f64) -> Self {
        Object::Real(r)
    }
}

impl From<PdfString> for Object {
    fn from(s: PdfString) -> Self {
        Object::String(s)
    }
}

impl From<Name> for Object {
    fn from(n: Name) -> Self {
        Object::Name(n)
    }
}

impl From<Vec<Object>> for Object {
    fn from(a: Vec<Object>) -> Self {
        Object::Array(a)
    }
}

impl From<Dict> for Object {
    fn from(d: Dict) -> Self {
        Object::Dictionary(d)
    }
}

impl From<StreamObj> for Object {
    fn from(s: StreamObj) -> Self {
        Object::Stream(s)
    }
}

impl From<ObjRef> for Object {
    fn from(r: ObjRef) -> Self {
        Object::Reference(r)
    }
}
