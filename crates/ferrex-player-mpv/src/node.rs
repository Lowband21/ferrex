//! Owned mpv node values and bounded raw-node conversion.

use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    ptr,
};

use crate::raw::{
    FORMAT_BYTE_ARRAY, FORMAT_DOUBLE, FORMAT_FLAG, FORMAT_INT64, FORMAT_NODE,
    FORMAT_NODE_ARRAY, FORMAT_NODE_MAP, FORMAT_NONE, FORMAT_OSD_STRING,
    FORMAT_STRING, RawMpvByteArray, RawMpvNode, RawMpvNodeList,
    RawMpvNodeValue,
};

/// Ferrex-owned representation of any value supported by `mpv_node`.
#[derive(Debug, Clone, PartialEq)]
pub enum MpvNode {
    /// `MPV_FORMAT_NONE`.
    Null,
    /// UTF-8 text. Invalid native bytes are copied with replacement characters.
    String(String),
    /// Boolean flag.
    Bool(bool),
    /// Signed integer.
    Int(i64),
    /// Floating-point value.
    Double(f64),
    /// Ordered node array.
    Array(Vec<Self>),
    /// Ordered key/value map. mpv does not guarantee source map order.
    Map(Vec<(String, Self)>),
    /// Untyped byte payload.
    Bytes(Vec<u8>),
}

impl From<String> for MpvNode {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for MpvNode {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl From<bool> for MpvNode {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<i64> for MpvNode {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}

impl From<f64> for MpvNode {
    fn from(value: f64) -> Self {
        Self::Double(value)
    }
}

/// Native property format requested from libmpv.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MpvFormat {
    /// Notify without a value.
    None,
    /// Raw string value.
    String,
    /// Human-readable OSD string.
    OsdString,
    /// Boolean flag.
    Flag,
    /// Signed 64-bit integer.
    Int64,
    /// Double-precision number.
    Double,
    /// Arbitrary `mpv_node` value.
    Node,
}

impl MpvFormat {
    pub(crate) const fn raw(self) -> u32 {
        match self {
            Self::None => FORMAT_NONE,
            Self::String => FORMAT_STRING,
            Self::OsdString => FORMAT_OSD_STRING,
            Self::Flag => FORMAT_FLAG,
            Self::Int64 => FORMAT_INT64,
            Self::Double => FORMAT_DOUBLE,
            Self::Node => FORMAT_NODE,
        }
    }
}

/// Bounds applied while copying pointer-backed native node trees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MpvNodeLimits {
    /// Maximum recursive array/map nesting.
    pub max_depth: usize,
    /// Maximum number of aggregate array/map entries.
    pub max_items: usize,
    /// Maximum aggregate string/key/byte payload size.
    pub max_bytes: usize,
}

impl Default for MpvNodeLimits {
    fn default() -> Self {
        Self {
            max_depth: 64,
            max_items: 100_000,
            max_bytes: 16 * 1024 * 1024,
        }
    }
}

/// Failure while validating or copying a raw mpv value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MpvNodeError {
    /// A Rust string contained an interior NUL byte.
    #[error("{context} contains an interior NUL byte")]
    InteriorNul {
        /// Value category, never the value itself.
        context: &'static str,
    },
    /// Native data violated a non-null pointer requirement.
    #[error("libmpv returned a null {0} pointer")]
    NullPointer(&'static str),
    /// Native list count was negative or not representable.
    #[error("libmpv returned invalid node-list count {0}")]
    InvalidListCount(i64),
    /// A configured copy limit was exceeded.
    #[error("libmpv node exceeded the {kind} limit ({limit})")]
    LimitExceeded {
        /// Limit category.
        kind: &'static str,
        /// Configured limit.
        limit: usize,
    },
    /// Native data used an unknown node format.
    #[error("libmpv returned unknown node format {0}")]
    UnknownFormat(u32),
}

#[derive(Debug)]
struct CopyBudget {
    limits: MpvNodeLimits,
    items: usize,
    bytes: usize,
}

impl CopyBudget {
    const fn new(limits: MpvNodeLimits) -> Self {
        Self {
            limits,
            items: 0,
            bytes: 0,
        }
    }

    fn add_items(&mut self, count: usize) -> Result<(), MpvNodeError> {
        self.items = self.items.checked_add(count).ok_or(
            MpvNodeError::LimitExceeded {
                kind: "item-count",
                limit: self.limits.max_items,
            },
        )?;
        if self.items > self.limits.max_items {
            return Err(MpvNodeError::LimitExceeded {
                kind: "item-count",
                limit: self.limits.max_items,
            });
        }
        Ok(())
    }

    fn add_bytes(&mut self, count: usize) -> Result<(), MpvNodeError> {
        self.bytes = self.bytes.checked_add(count).ok_or(
            MpvNodeError::LimitExceeded {
                kind: "byte-count",
                limit: self.limits.max_bytes,
            },
        )?;
        if self.bytes > self.limits.max_bytes {
            return Err(MpvNodeError::LimitExceeded {
                kind: "byte-count",
                limit: self.limits.max_bytes,
            });
        }
        Ok(())
    }

    fn check_depth(&self, depth: usize) -> Result<(), MpvNodeError> {
        if depth > self.limits.max_depth {
            return Err(MpvNodeError::LimitExceeded {
                kind: "depth",
                limit: self.limits.max_depth,
            });
        }
        Ok(())
    }
}

/// Copy a raw node while all pointers are still valid.
///
/// # Safety
///
/// `node` and every pointer reachable from it must obey libmpv's `mpv_node`
/// contract for the duration of this call.
pub(crate) unsafe fn copy_raw_node(
    node: &RawMpvNode,
    limits: MpvNodeLimits,
) -> Result<MpvNode, MpvNodeError> {
    let mut budget = CopyBudget::new(limits);
    // SAFETY: delegated to the caller and recursively preserved below.
    unsafe { copy_raw_node_inner(node, 0, &mut budget) }
}

unsafe fn copy_raw_node_inner(
    node: &RawMpvNode,
    depth: usize,
    budget: &mut CopyBudget,
) -> Result<MpvNode, MpvNodeError> {
    budget.check_depth(depth)?;

    match node.format {
        FORMAT_NONE => Ok(MpvNode::Null),
        FORMAT_STRING => {
            // SAFETY: active union member follows `format` and pointer validity
            // is part of the caller's raw-node contract.
            let pointer = unsafe { node.value.string };
            copy_c_string(pointer.cast_const(), "string", budget)
                .map(MpvNode::String)
        }
        FORMAT_FLAG => {
            // SAFETY: active union member follows `format`.
            Ok(MpvNode::Bool(unsafe { node.value.flag } != 0))
        }
        FORMAT_INT64 => {
            // SAFETY: active union member follows `format`.
            Ok(MpvNode::Int(unsafe { node.value.int64 }))
        }
        FORMAT_DOUBLE => {
            // SAFETY: active union member follows `format`.
            Ok(MpvNode::Double(unsafe { node.value.double_ }))
        }
        FORMAT_NODE_ARRAY | FORMAT_NODE_MAP => {
            // SAFETY: active union member follows `format`.
            let list = unsafe { node.value.list };
            let list = unsafe { list.as_ref() }
                .ok_or(MpvNodeError::NullPointer("node-list"))?;
            let count = usize::try_from(list.count).map_err(|_| {
                MpvNodeError::InvalidListCount(list.count.into())
            })?;
            budget.add_items(count)?;

            if count > 0 && list.values.is_null() {
                return Err(MpvNodeError::NullPointer("node-list values"));
            }

            if node.format == FORMAT_NODE_ARRAY {
                let mut values = Vec::with_capacity(count);
                for index in 0..count {
                    // SAFETY: `values` contains `count` entries by contract.
                    let value = unsafe { &*list.values.add(index) };
                    // SAFETY: nested pointers share the caller's validity.
                    values.push(unsafe {
                        copy_raw_node_inner(value, depth + 1, budget)?
                    });
                }
                Ok(MpvNode::Array(values))
            } else {
                if count > 0 && list.keys.is_null() {
                    return Err(MpvNodeError::NullPointer("node-map keys"));
                }
                let mut values = Vec::with_capacity(count);
                for index in 0..count {
                    // SAFETY: map key/value arrays contain `count` entries.
                    let key_pointer = unsafe { *list.keys.add(index) };
                    let key = copy_c_string(
                        key_pointer.cast_const(),
                        "node-map key",
                        budget,
                    )?;
                    // SAFETY: `values` contains `count` entries by contract.
                    let value = unsafe { &*list.values.add(index) };
                    // SAFETY: nested pointers share the caller's validity.
                    let value = unsafe {
                        copy_raw_node_inner(value, depth + 1, budget)?
                    };
                    values.push((key, value));
                }
                Ok(MpvNode::Map(values))
            }
        }
        FORMAT_BYTE_ARRAY => {
            // SAFETY: active union member follows `format`.
            let bytes = unsafe { node.value.bytes };
            let bytes = unsafe { bytes.as_ref() }
                .ok_or(MpvNodeError::NullPointer("byte-array"))?;
            budget.add_bytes(bytes.size)?;
            if bytes.size > 0 && bytes.data.is_null() {
                return Err(MpvNodeError::NullPointer("byte-array data"));
            }
            // SAFETY: byte-array data contains `size` initialized bytes.
            let slice = unsafe {
                std::slice::from_raw_parts(bytes.data.cast::<u8>(), bytes.size)
            };
            Ok(MpvNode::Bytes(slice.to_vec()))
        }
        format => Err(MpvNodeError::UnknownFormat(format)),
    }
}

fn copy_c_string(
    pointer: *const c_char,
    category: &'static str,
    budget: &mut CopyBudget,
) -> Result<String, MpvNodeError> {
    if pointer.is_null() {
        return Err(MpvNodeError::NullPointer(category));
    }
    // SAFETY: callers only provide NUL-terminated strings from a valid raw mpv
    // payload. The copy is completed before the next native event is fetched.
    let bytes = unsafe { CStr::from_ptr(pointer) }.to_bytes();
    budget.add_bytes(bytes.len())?;
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

pub(crate) unsafe fn copy_raw_event_value(
    format: u32,
    data: *mut c_void,
    limits: MpvNodeLimits,
) -> Result<Option<MpvNode>, MpvNodeError> {
    if format == FORMAT_NONE {
        return Ok(None);
    }
    if data.is_null() {
        return Err(MpvNodeError::NullPointer("event value"));
    }

    let mut budget = CopyBudget::new(limits);
    let value = match format {
        FORMAT_STRING | FORMAT_OSD_STRING => {
            // SAFETY: string event data points to a `char *` value.
            let pointer = unsafe { *data.cast::<*const c_char>() };
            MpvNode::String(copy_c_string(
                pointer,
                "event string",
                &mut budget,
            )?)
        }
        FORMAT_FLAG => {
            // SAFETY: format identifies an `int` event value.
            MpvNode::Bool(unsafe { *data.cast::<c_int>() } != 0)
        }
        FORMAT_INT64 => {
            // SAFETY: format identifies an `int64_t` event value.
            MpvNode::Int(unsafe { *data.cast::<i64>() })
        }
        FORMAT_DOUBLE => {
            // SAFETY: format identifies a `double` event value.
            MpvNode::Double(unsafe { *data.cast::<f64>() })
        }
        FORMAT_NODE => {
            // SAFETY: format identifies an `mpv_node` event value.
            let node = unsafe { &*data.cast::<RawMpvNode>() };
            // SAFETY: node pointers share the event payload lifetime.
            unsafe { copy_raw_node_inner(node, 0, &mut budget)? }
        }
        FORMAT_BYTE_ARRAY => {
            // A direct byte-array property is unusual but has the same payload
            // shape as the node union member.
            // SAFETY: format identifies an `mpv_byte_array` event value.
            let bytes = unsafe { &*data.cast::<RawMpvByteArray>() };
            budget.add_bytes(bytes.size)?;
            if bytes.size > 0 && bytes.data.is_null() {
                return Err(MpvNodeError::NullPointer("event byte-array data"));
            }
            // SAFETY: native data contains `size` initialized bytes.
            let bytes = unsafe {
                std::slice::from_raw_parts(bytes.data.cast::<u8>(), bytes.size)
            };
            MpvNode::Bytes(bytes.to_vec())
        }
        unknown => return Err(MpvNodeError::UnknownFormat(unknown)),
    };
    Ok(Some(value))
}

/// Pointer-stable storage for one outbound node. libmpv copies this data before
/// every asynchronous submission function returns.
// Boxed list/byte descriptors are intentional: their addresses are embedded
// in parent nodes and must survive growth of the owning vectors.
#[allow(clippy::vec_box)]
pub(crate) struct RawNodeArena {
    root: RawMpvNode,
    strings: Vec<CString>,
    node_blocks: Vec<Box<[RawMpvNode]>>,
    key_blocks: Vec<Box<[*mut c_char]>>,
    lists: Vec<Box<RawMpvNodeList>>,
    byte_blocks: Vec<Box<[u8]>>,
    byte_arrays: Vec<Box<RawMpvByteArray>>,
    budget: CopyBudget,
}

impl RawNodeArena {
    pub(crate) fn new(value: &MpvNode) -> Result<Self, MpvNodeError> {
        let mut arena = Self {
            root: RawMpvNode {
                value: RawMpvNodeValue { int64: 0 },
                format: FORMAT_NONE,
            },
            strings: Vec::new(),
            node_blocks: Vec::new(),
            key_blocks: Vec::new(),
            lists: Vec::new(),
            byte_blocks: Vec::new(),
            byte_arrays: Vec::new(),
            budget: CopyBudget::new(MpvNodeLimits::default()),
        };
        arena.root = arena.build(value, 0)?;
        Ok(arena)
    }

    pub(crate) fn root_mut(&mut self) -> *mut RawMpvNode {
        &mut self.root
    }

    fn build(
        &mut self,
        value: &MpvNode,
        depth: usize,
    ) -> Result<RawMpvNode, MpvNodeError> {
        self.budget.check_depth(depth)?;
        let node = match value {
            MpvNode::Null => RawMpvNode {
                value: RawMpvNodeValue { int64: 0 },
                format: FORMAT_NONE,
            },
            MpvNode::String(value) => {
                self.budget.add_bytes(value.len())?;
                let value = CString::new(value.as_bytes()).map_err(|_| {
                    MpvNodeError::InteriorNul {
                        context: "node string",
                    }
                })?;
                let pointer = value.as_ptr().cast_mut();
                self.strings.push(value);
                RawMpvNode {
                    value: RawMpvNodeValue { string: pointer },
                    format: FORMAT_STRING,
                }
            }
            MpvNode::Bool(value) => RawMpvNode {
                value: RawMpvNodeValue {
                    flag: c_int::from(*value),
                },
                format: FORMAT_FLAG,
            },
            MpvNode::Int(value) => RawMpvNode {
                value: RawMpvNodeValue { int64: *value },
                format: FORMAT_INT64,
            },
            MpvNode::Double(value) => RawMpvNode {
                value: RawMpvNodeValue { double_: *value },
                format: FORMAT_DOUBLE,
            },
            MpvNode::Array(values) => {
                self.budget.add_items(values.len())?;
                let mut raw_values = Vec::with_capacity(values.len());
                for value in values {
                    raw_values.push(self.build(value, depth + 1)?);
                }
                self.list_node(raw_values, None, FORMAT_NODE_ARRAY)?
            }
            MpvNode::Map(values) => {
                self.budget.add_items(values.len())?;
                let mut raw_values = Vec::with_capacity(values.len());
                let mut keys = Vec::with_capacity(values.len());
                for (key, value) in values {
                    self.budget.add_bytes(key.len())?;
                    let key = CString::new(key.as_bytes()).map_err(|_| {
                        MpvNodeError::InteriorNul {
                            context: "node-map key",
                        }
                    })?;
                    keys.push(key.as_ptr().cast_mut());
                    self.strings.push(key);
                    raw_values.push(self.build(value, depth + 1)?);
                }
                self.list_node(raw_values, Some(keys), FORMAT_NODE_MAP)?
            }
            MpvNode::Bytes(bytes) => {
                self.budget.add_bytes(bytes.len())?;
                let mut block = bytes.clone().into_boxed_slice();
                let data = if block.is_empty() {
                    ptr::null_mut()
                } else {
                    block.as_mut_ptr().cast()
                };
                let mut byte_array = Box::new(RawMpvByteArray {
                    data,
                    size: block.len(),
                });
                let pointer = byte_array.as_mut() as *mut RawMpvByteArray;
                self.byte_blocks.push(block);
                self.byte_arrays.push(byte_array);
                RawMpvNode {
                    value: RawMpvNodeValue { bytes: pointer },
                    format: FORMAT_BYTE_ARRAY,
                }
            }
        };
        Ok(node)
    }

    fn list_node(
        &mut self,
        values: Vec<RawMpvNode>,
        keys: Option<Vec<*mut c_char>>,
        format: u32,
    ) -> Result<RawMpvNode, MpvNodeError> {
        let count = c_int::try_from(values.len()).map_err(|_| {
            MpvNodeError::InvalidListCount(
                i64::try_from(values.len()).unwrap_or(i64::MAX),
            )
        })?;
        let mut values = values.into_boxed_slice();
        let values_pointer = if values.is_empty() {
            ptr::null_mut()
        } else {
            values.as_mut_ptr()
        };

        let mut keys = keys.map(Vec::into_boxed_slice);
        let keys_pointer = keys.as_mut().map_or(ptr::null_mut(), |keys| {
            if keys.is_empty() {
                ptr::null_mut()
            } else {
                keys.as_mut_ptr()
            }
        });

        let mut list = Box::new(RawMpvNodeList {
            count,
            values: values_pointer,
            keys: keys_pointer,
        });
        let list_pointer = list.as_mut() as *mut RawMpvNodeList;
        self.node_blocks.push(values);
        if let Some(keys) = keys {
            self.key_blocks.push(keys);
        }
        self.lists.push(list);

        Ok(RawMpvNode {
            value: RawMpvNodeValue { list: list_pointer },
            format,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_nodes_round_trip_through_raw_layout() {
        let original = MpvNode::Map(vec![
            ("null".into(), MpvNode::Null),
            (
                "array".into(),
                MpvNode::Array(vec![
                    MpvNode::Bool(true),
                    MpvNode::Int(-7),
                    MpvNode::Double(2.5),
                    MpvNode::String("hello".into()),
                    MpvNode::Bytes(vec![0, 1, 2, 255]),
                ]),
            ),
        ]);
        let mut arena = RawNodeArena::new(&original).unwrap();
        // SAFETY: the arena owns every pointer reachable from its root.
        let copied = unsafe {
            copy_raw_node(&*arena.root_mut(), MpvNodeLimits::default()).unwrap()
        };
        assert_eq!(copied, original);
    }

    #[test]
    fn conversion_rejects_null_nested_storage_and_limits() {
        let raw = RawMpvNode {
            value: RawMpvNodeValue {
                list: ptr::null_mut(),
            },
            format: FORMAT_NODE_ARRAY,
        };
        // SAFETY: null is intentional and diagnosed before dereference.
        assert_eq!(
            unsafe { copy_raw_node(&raw, MpvNodeLimits::default()) },
            Err(MpvNodeError::NullPointer("node-list"))
        );

        let original =
            MpvNode::Array(vec![MpvNode::Array(vec![MpvNode::Null])]);
        let mut arena = RawNodeArena::new(&original).unwrap();
        // SAFETY: the arena owns every pointer reachable from its root.
        let error = unsafe {
            copy_raw_node(
                &*arena.root_mut(),
                MpvNodeLimits {
                    max_depth: 1,
                    ..MpvNodeLimits::default()
                },
            )
        }
        .unwrap_err();
        assert!(matches!(
            error,
            MpvNodeError::LimitExceeded { kind: "depth", .. }
        ));
    }

    #[test]
    fn outbound_nodes_reject_interior_nul_without_echoing_value() {
        let error = RawNodeArena::new(&MpvNode::String("secret\0tail".into()))
            .err()
            .unwrap();
        assert_eq!(
            error,
            MpvNodeError::InteriorNul {
                context: "node string"
            }
        );
        assert!(!error.to_string().contains("secret"));
    }
}
