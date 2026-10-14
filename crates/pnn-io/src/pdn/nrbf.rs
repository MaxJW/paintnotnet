//! A reader for .NET's binary serialization format (what `BinaryFormatter` writes), as
//! specified by Microsoft's open specification [MS-NRBF] ".NET Remoting: Binary Format Data
//! Structure" (https://learn.microsoft.com/openspecs/windows_protocols/ms-nrbf). Only reading:
//! paint.net's `.pdn` files keep their document in it.
//!
//! The stream is a list of records; objects refer to each other by id. [`read`] returns every
//! object by id, the root's id and the order classes appeared in (paint.net's pixel blocks
//! follow the stream in that order).

use std::collections::HashMap;

/// A value inside an object.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    String(String),
    /// A reference to the object with this id.
    Ref(i32),
    /// An object or array written inline (it also has an id; see [`Graph::objects`]).
    Inline(i32),
    /// Another primitive (char, decimal, time span, date time) kept as raw text or ticks.
    Other(String),
}

/// An object in the stream.
#[derive(Clone, Debug, PartialEq)]
pub enum Object {
    Class {
        name: String,
        members: Vec<(String, Value)>,
    },
    String(String),
    Array(Vec<Value>),
    /// A single-dimension array of bytes (`byte[]`).
    Bytes(Vec<u8>),
}

#[derive(Debug, Default)]
pub struct Graph {
    pub root: i32,
    pub objects: HashMap<i32, Object>,
    /// Ids of class objects in the order their records appeared.
    pub class_order: Vec<i32>,
}

impl Graph {
    /// Follow references and inline ids to the object.
    pub fn get(&self, v: &Value) -> Option<&Object> {
        match v {
            Value::Ref(id) | Value::Inline(id) => self.objects.get(id),
            _ => None,
        }
    }

    pub fn class(&self, id: i32) -> Option<(&str, &[(String, Value)])> {
        match self.objects.get(&id)? {
            Object::Class { name, members } => Some((name, members)),
            _ => None,
        }
    }
}

/// Look a member up by name; also accepts a base class's `Base+name` form.
pub fn member<'a>(members: &'a [(String, Value)], name: &str) -> Option<&'a Value> {
    members
        .iter()
        .find(|(n, _)| n == name || n.rsplit('+').next() == Some(name))
        .map(|(_, v)| v)
}

#[derive(Debug, thiserror::Error)]
pub enum NrbfError {
    #[error("unexpected end of data")]
    Eof,
    #[error("unsupported record type {0}")]
    Record(u8),
    #[error("unsupported type {0}")]
    Type(u8),
    #[error("invalid data: {0}")]
    Invalid(&'static str),
}

type Result<T> = std::result::Result<T, NrbfError>;

#[derive(Clone, Debug)]
enum MemberType {
    Primitive(u8),
    /// Any record (strings, objects, arrays).
    Record,
}

#[derive(Clone, Debug)]
struct ClassMeta {
    name: String,
    members: Vec<String>,
    /// `None`: members untyped (each value is a record).
    types: Option<Vec<MemberType>>,
}

pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    meta: HashMap<i32, ClassMeta>,
    graph: Graph,
}

/// Read one serialized stream from the start of `data`. Returns the graph and the number of
/// bytes it took (paint.net's pixel data follows).
pub fn read(data: &[u8]) -> Result<(Graph, usize)> {
    let mut r = Reader {
        data,
        pos: 0,
        meta: HashMap::new(),
        graph: Graph::default(),
    };
    loop {
        match r.record()? {
            Step::Value(_) => {}
            Step::End => break,
            Step::Nulls(_) => return Err(NrbfError::Invalid("nulls outside an array")),
        }
    }
    Ok((r.graph, r.pos))
}

enum Step {
    Value(Value),
    Nulls(usize),
    End,
}

impl Reader<'_> {
    fn bytes(&mut self, n: usize) -> Result<&[u8]> {
        let end = self.pos.checked_add(n).ok_or(NrbfError::Eof)?;
        let b = self.data.get(self.pos..end).ok_or(NrbfError::Eof)?;
        self.pos = end;
        Ok(b)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }

    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(
            self.bytes(4)?.try_into().expect("4 bytes"),
        ))
    }

    fn count(&mut self) -> Result<usize> {
        let n = self.i32()?;
        // Each element takes at least a byte: a larger count can't be honest.
        usize::try_from(n)
            .ok()
            .filter(|&n| n <= self.data.len())
            .ok_or(NrbfError::Invalid("count"))
    }

    /// LengthPrefixedString: a 7-bit varint byte length, then UTF-8.
    fn string(&mut self) -> Result<String> {
        let mut len = 0usize;
        for shift in (0..35).step_by(7) {
            let b = self.u8()?;
            len |= ((b & 0x7f) as usize) << shift;
            if b & 0x80 == 0 {
                let bytes = self.bytes(len)?;
                return String::from_utf8(bytes.to_vec()).map_err(|_| NrbfError::Invalid("utf-8"));
            }
        }
        Err(NrbfError::Invalid("string length"))
    }

    fn primitive(&mut self, t: u8) -> Result<Value> {
        let le = |b: &[u8]| {
            let mut a = [0u8; 8];
            a[..b.len()].copy_from_slice(b);
            u64::from_le_bytes(a)
        };
        Ok(match t {
            1 => Value::Bool(self.u8()? != 0),
            2 => Value::UInt(self.u8()? as u64),
            3 => {
                // A UTF-8 encoded char: 1–4 bytes.
                let first = self.u8()?;
                let extra = match first {
                    0..=0x7f => 0,
                    0xc0..=0xdf => 1,
                    0xe0..=0xef => 2,
                    _ => 3,
                };
                let rest = self.bytes(extra)?.to_vec();
                let mut s = vec![first];
                s.extend(rest);
                Value::Other(String::from_utf8_lossy(&s).into_owned())
            }
            5 => Value::Other(self.string()?),
            6 => Value::Float(f64::from_le_bytes(self.bytes(8)?.try_into().expect("8"))),
            7 => Value::Int(i16::from_le_bytes(self.bytes(2)?.try_into().expect("2")) as i64),
            8 => Value::Int(self.i32()? as i64),
            9 => Value::Int(i64::from_le_bytes(self.bytes(8)?.try_into().expect("8"))),
            10 => Value::Int(self.u8()? as i8 as i64),
            11 => Value::Float(f32::from_le_bytes(self.bytes(4)?.try_into().expect("4")) as f64),
            12 | 13 => Value::Other(le(self.bytes(8)?).to_string()),
            14 => Value::UInt(le(self.bytes(2)?)),
            15 => Value::UInt(le(self.bytes(4)?)),
            16 => Value::UInt(le(self.bytes(8)?)),
            17 => Value::Null,
            18 => Value::String(self.string()?),
            t => return Err(NrbfError::Type(t)),
        })
    }

    /// ClassInfo: object id, name, member names.
    fn class_info(&mut self) -> Result<(i32, String, Vec<String>)> {
        let id = self.i32()?;
        let name = self.string()?;
        let n = self.count()?;
        let members = (0..n).map(|_| self.string()).collect::<Result<_>>()?;
        Ok((id, name, members))
    }

    /// MemberTypeInfo for `n` members.
    fn member_types(&mut self, n: usize) -> Result<Vec<MemberType>> {
        let kinds: Vec<u8> = self.bytes(n)?.to_vec();
        kinds
            .into_iter()
            .map(|k| {
                Ok(match k {
                    0 => MemberType::Primitive(self.u8()?),
                    1 | 2 | 5 | 6 => MemberType::Record,
                    3 => {
                        self.string()?;
                        MemberType::Record
                    }
                    4 => {
                        self.string()?;
                        self.i32()?;
                        MemberType::Record
                    }
                    7 => {
                        self.u8()?;
                        MemberType::Record
                    }
                    k => return Err(NrbfError::Type(k)),
                })
            })
            .collect()
    }

    fn class_values(&mut self, id: i32, meta: &ClassMeta) -> Result<Value> {
        let mut members = Vec::with_capacity(meta.members.len());
        for (i, name) in meta.members.iter().enumerate() {
            let v = match meta.types.as_ref().map(|t| &t[i]) {
                Some(MemberType::Primitive(t)) => self.primitive(*t)?,
                _ => match self.record()? {
                    Step::Value(v) => v,
                    _ => return Err(NrbfError::Invalid("member")),
                },
            };
            members.push((name.clone(), v));
        }
        self.graph.class_order.push(id);
        self.graph.objects.insert(
            id,
            Object::Class {
                name: meta.name.clone(),
                members,
            },
        );
        Ok(Value::Inline(id))
    }

    /// Array elements that are records (with null runs).
    fn record_elements(&mut self, n: usize) -> Result<Vec<Value>> {
        let mut out = Vec::with_capacity(n.min(1 << 16));
        while out.len() < n {
            match self.record()? {
                Step::Value(v) => out.push(v),
                Step::Nulls(k) => out.extend(std::iter::repeat_n(Value::Null, k)),
                Step::End => return Err(NrbfError::Invalid("array")),
            }
        }
        out.truncate(n);
        Ok(out)
    }

    fn record(&mut self) -> Result<Step> {
        let kind = self.u8()?;
        Ok(Step::Value(match kind {
            0 => {
                // SerializationHeader.
                self.graph.root = self.i32()?;
                self.bytes(12)?;
                return self.record();
            }
            1 => {
                let id = self.i32()?;
                let meta_id = self.i32()?;
                let meta = self
                    .meta
                    .get(&meta_id)
                    .cloned()
                    .ok_or(NrbfError::Invalid("class id"))?;
                self.class_values(id, &meta)?
            }
            2 | 3 => {
                let (id, name, members) = self.class_info()?;
                if kind == 3 {
                    self.i32()?; // library
                }
                let meta = ClassMeta {
                    name,
                    members,
                    types: None,
                };
                self.meta.insert(id, meta.clone());
                self.class_values(id, &meta)?
            }
            4 | 5 => {
                let (id, name, members) = self.class_info()?;
                let types = self.member_types(members.len())?;
                if kind == 5 {
                    self.i32()?; // library
                }
                let meta = ClassMeta {
                    name,
                    members,
                    types: Some(types),
                };
                self.meta.insert(id, meta.clone());
                self.class_values(id, &meta)?
            }
            6 => {
                let id = self.i32()?;
                let s = self.string()?;
                self.graph.objects.insert(id, Object::String(s));
                Value::Inline(id)
            }
            7 => {
                // BinaryArray.
                let id = self.i32()?;
                let shape = self.u8()?;
                let rank = self.count()?;
                let mut n = 1usize;
                for _ in 0..rank {
                    n = n
                        .checked_mul(self.count()?)
                        .ok_or(NrbfError::Invalid("array size"))?;
                }
                if matches!(shape, 3..=5) {
                    self.bytes(4 * rank)?; // lower bounds
                }
                let t = self.member_types(1)?.remove(0);
                let elements = match t {
                    MemberType::Primitive(p) => (0..n)
                        .map(|_| self.primitive(p))
                        .collect::<Result<Vec<_>>>()?,
                    MemberType::Record => self.record_elements(n)?,
                };
                self.graph.objects.insert(id, Object::Array(elements));
                Value::Inline(id)
            }
            8 => {
                let t = self.u8()?;
                self.primitive(t)?
            }
            9 => Value::Ref(self.i32()?),
            10 => Value::Null,
            11 => return Ok(Step::End),
            12 => {
                self.i32()?;
                self.string()?;
                return self.record();
            }
            13 => return Ok(Step::Nulls(self.u8()? as usize)),
            14 => return Ok(Step::Nulls(self.count()?)),
            15 => {
                let id = self.i32()?;
                let n = self.count()?;
                let t = self.u8()?;
                let object = if t == 2 {
                    Object::Bytes(self.bytes(n)?.to_vec())
                } else {
                    Object::Array((0..n).map(|_| self.primitive(t)).collect::<Result<_>>()?)
                };
                self.graph.objects.insert(id, object);
                Value::Inline(id)
            }
            16 | 17 => {
                let id = self.i32()?;
                let n = self.count()?;
                let elements = self.record_elements(n)?;
                self.graph.objects.insert(id, Object::Array(elements));
                Value::Inline(id)
            }
            k => return Err(NrbfError::Record(k)),
        }))
    }
}
