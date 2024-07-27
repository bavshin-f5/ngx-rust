use std::ops::{Deref, DerefMut};

use foreign_types::{ForeignType, ForeignTypeRef};

use crate::ffi::{ngx_array_push, ngx_array_push_n, ngx_array_t};

foreign_types::foreign_type! {
    /// Wrapper struct for an [`ngx_array_t`]
    pub unsafe type NgxArray<T: ForeignType>: Send {
        type CType = ngx_array_t;
        type PhantomData = T;
        // No cleanup required for pool-allocated structs
        fn drop = |_|();
    }
}

impl<T: ForeignType> AsRef<ngx_array_t> for NgxArrayRef<T> {
    fn as_ref(&self) -> &ngx_array_t {
        // SAFETY: `NgxArrayRef` must contain a valid pointer to the `ngx_array_t`
        unsafe { &*self.as_ptr() }
    }
}

impl<T: ForeignType> AsMut<ngx_array_t> for NgxArrayRef<T> {
    fn as_mut(&mut self) -> &mut ngx_array_t {
        // SAFETY: `NgxArrayRef` must contain a valid pointer to the `ngx_array_t`
        unsafe { &mut *self.as_ptr() }
    }
}

impl<T: ForeignType> Deref for NgxArrayRef<T> {
    type Target = [T::Ref];

    fn deref(&self) -> &Self::Target {
        let inner: &ngx_array_t = self.as_ref();
        // SAFETY: valid `ngx_array_t` instance must contain `nelts` valid elements
        // ForeignTypeRef representation must match its CType
        unsafe { std::slice::from_raw_parts(inner.elts.cast::<T::Ref>(), inner.nelts) }
    }
}

impl<T: ForeignType> DerefMut for NgxArrayRef<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        let inner: &mut ngx_array_t = self.as_mut();
        // SAFETY: valid `ngx_array_t` instance must contain `nelts` valid elements
        // ForeignTypeRef representation must match its CType
        unsafe { std::slice::from_raw_parts_mut(inner.elts.cast::<T::Ref>(), inner.nelts) }
    }
}

impl<T: ForeignType> NgxArrayRef<T> {
    /// Appends an element to the back of the array.
    ///
    /// Returns a mutable reference to the new element if allocation succeeds.
    pub fn push(&mut self) -> Option<&mut T::Ref> {
        let elt = unsafe { ngx_array_push(self.as_ptr()).cast::<<T as ForeignType>::CType>() };

        if elt.is_null() {
            return None;
        }

        Some(unsafe { T::Ref::from_ptr_mut(elt) })
    }

    /// Appends `n` elements to the back of the array.
    ///
    /// Returns a mutable slice with all the added elements if allocation succeeds.
    pub fn push_n(&mut self, n: usize) -> Option<&mut [T::Ref]> {
        let elts = unsafe { ngx_array_push_n(self.as_ptr(), n).cast::<T::Ref>() };

        if elts.is_null() {
            return None;
        }

        Some(unsafe { std::slice::from_raw_parts_mut(elts, n) })
    }
}
