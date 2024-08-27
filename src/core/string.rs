use core::fmt;
use core::ops::{Deref, DerefMut};
use core::str::{self, Utf8Error};

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{borrow::Cow, string::String};
#[cfg(feature = "std")]
use std::{borrow::Cow, string::String};

use foreign_types::{ForeignType, ForeignTypeRef, Opaque};

use crate::ffi::ngx_str_t;

/// Static string initializer for [`ngx_str_t`].
///
/// The resulting byte string is always nul-terminated (just like a C string).
///
/// [`ngx_str_t`]: https://nginx.org/en/docs/dev/development_guide.html#string_overview
#[macro_export]
macro_rules! ngx_string {
    ($s:expr) => {{
        $crate::ffi::ngx_str_t {
            len: $s.len() as _,
            data: concat!($s, "\0").as_ptr() as *mut u8,
        }
    }};
}

/// Representation of a borrowed [Nginx string].
///
/// [Nginx string]: https://nginx.org/en/docs/dev/development_guide.html#string_overview
#[repr(transparent)]
pub struct NgxStr(ngx_str_t);

impl Default for NgxStr {
    fn default() -> Self {
        NgxStr::from_ngx_str(ngx_str_t::default())
    }
}

impl Deref for NgxStr {
    type Target = NgxStrRef;

    fn deref(&self) -> &Self::Target {
        unsafe { NgxStrRef::from_ptr(&self.0 as *const _ as *mut _) }
    }
}

impl DerefMut for NgxStr {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { NgxStrRef::from_ptr_mut(&mut self.0) }
    }
}

impl fmt::Display for NgxStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let str = String::from_utf8_lossy(self.as_bytes());
        f.write_str(&str)
    }
}

unsafe impl ForeignType for NgxStr {
    type CType = ngx_str_t;
    type Ref = NgxStrRef;

    unsafe fn from_ptr(ptr: *mut Self::CType) -> Self {
        assert!(!ptr.is_null());
        Self(*ptr)
    }

    fn as_ptr(&self) -> *mut Self::CType {
        &self.0 as *const _ as *mut _
    }
}

impl NgxStr {
    /// Create an [`NgxStr`] from an [`ngx_str_t`].
    ///
    /// [`ngx_str_t`]: https://nginx.org/en/docs/dev/development_guide.html#string_overview
    ///
    /// # Safety
    ///
    /// The caller has provided a valid `ngx_str_t` with a `data` pointer that points
    /// to range of bytes of at least `len` bytes, whose content remains valid and doesn't
    /// change for the lifetime of the returned `NgxStr`.
    pub fn from_ngx_str(str: ngx_str_t) -> Self {
        Self(str)
    }
}

/// Representation of a borrowed [Nginx string].
///
/// [Nginx string]: https://nginx.org/en/docs/dev/development_guide.html#string_overview
pub struct NgxStrRef(Opaque);

impl AsRef<ngx_str_t> for NgxStrRef {
    fn as_ref(&self) -> &ngx_str_t {
        unsafe { &*self.as_ptr() }
    }
}

impl AsMut<ngx_str_t> for NgxStrRef {
    fn as_mut(&mut self) -> &mut ngx_str_t {
        unsafe { &mut *self.as_ptr() }
    }
}

impl AsRef<[u8]> for NgxStrRef {
    fn as_ref(&self) -> &[u8] {
        AsRef::<ngx_str_t>::as_ref(self).as_bytes()
    }
}

impl AsMut<[u8]> for NgxStrRef {
    fn as_mut(&mut self) -> &mut [u8] {
        AsMut::<ngx_str_t>::as_mut(self).as_bytes_mut()
    }
}

impl Deref for NgxStrRef {
    type Target = ngx_str_t;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.as_ptr() }
    }
}

impl DerefMut for NgxStrRef {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.as_ptr() }
    }
}

impl fmt::Display for NgxStrRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let str = String::from_utf8_lossy(self.as_bytes());
        f.write_str(&str)
    }
}

unsafe impl ForeignTypeRef for NgxStrRef {
    type CType = ngx_str_t;
}

impl NgxStrRef {
    /// Creates a new `NgxStrRef` from an `ngx_str_t` pointer.
    ///
    /// # Safety
    /// The caller must ensure that a valid `ngx_str_t` pointer is provided, pointing to valid memory and non-null.
    /// A null argument will cause an assertion failure and panic.
    pub unsafe fn from_ngx_str<'a>(str: &ngx_str_t) -> &'a NgxStrRef {
        NgxStrRef::from_ptr(str as *const _ as *mut _)
    }

    /// Yields a `&str` slice if the [`NgxStr`] contains valid UTF-8.
    pub fn to_str(&self) -> Result<&str, Utf8Error> {
        str::from_utf8(self.as_bytes())
    }

    /// Converts an [`NgxStr`] into a [`Cow<str>`], replacing invalid UTF-8 sequences.
    ///
    /// See [`String::from_utf8_lossy`].
    #[cfg(feature = "alloc")]
    pub fn to_string_lossy(&self) -> Cow<str> {
        String::from_utf8_lossy(self.as_bytes())
    }
}
