use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;

use foreign_types::ForeignTypeRef;

use crate::core::{NgxStr, PoolRef};
use crate::ffi::ngx_conf_t;

/// Wrapper struct for an `ngx_conf_t` pointer
///
/// There's no owned counterpart, as modules should never create or own `ngx_conf_t`
#[repr(transparent)]
pub struct NgxConfRef(NonNull<ngx_conf_t>);

unsafe impl ForeignTypeRef for NgxConfRef {
    type CType = ngx_conf_t;
}

impl AsRef<ngx_conf_t> for NgxConfRef {
    fn as_ref(&self) -> &ngx_conf_t {
        self.deref()
    }
}

impl AsMut<ngx_conf_t> for NgxConfRef {
    fn as_mut(&mut self) -> &mut ngx_conf_t {
        self.deref_mut()
    }
}

impl Deref for NgxConfRef {
    type Target = ngx_conf_t;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.as_ptr() }
    }
}

impl DerefMut for NgxConfRef {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.as_ptr() }
    }
}

impl NgxConfRef {
    /// Returns the list of arguments for the current configuration directive
    pub fn args(&self) -> &[NgxStr] {
        if let Some(args) = unsafe { self.args.as_ref() } {
            unsafe { std::slice::from_raw_parts(args.elts.cast(), args.nelts) }
        } else {
            &[]
        }
    }

    /// Returns a configuration pool reference
    pub fn pool(&mut self) -> &mut PoolRef {
        unsafe { PoolRef::from_ptr_mut(self.pool) }
    }
}
