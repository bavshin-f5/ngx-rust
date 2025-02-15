mod array;
mod buffer;
mod conf_file;
mod hash;
mod list;
mod pool;
mod status;
mod string;

pub use array::*;
pub use buffer::*;
pub use conf_file::*;
pub use hash::*;
pub use list::*;
pub use pool::*;
pub use status::*;
pub use string::*;

/// Gets an outer object pointer from a pointer to one of its fields.
/// While there is no corresponding C macro, the pattern is common in the NGINX source.
///
/// # Safety
///
/// `$ptr` must be a valid pointer to the field `$field` of `$type`.
#[macro_export]
macro_rules! ngx_container_of {
    ($ptr:expr, $type:path, $field:ident) => {
        $ptr.byte_sub(::core::mem::offset_of!($type, $field)).cast::<$type>()
    };
}
