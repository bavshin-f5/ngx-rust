//! Collection types.
//!
//! This module provides common collection types, mostly implemented as wrappers over the
//! corresponding NGINX types.

#[cfg(feature = "alloc")]
pub use allocator_api2::{
    collections::{TryReserveError, TryReserveErrorKind},
    vec::{self, Vec},
};

pub use self::array::Array;
pub use self::queue::Queue;
pub use self::rbtree::RbTreeMap;

pub mod array;
pub mod queue;
pub mod rbtree;
