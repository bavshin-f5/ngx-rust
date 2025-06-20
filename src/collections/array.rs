//! Wrapper over the [ngx_array_t].
use core::marker::PhantomData;
use core::mem;
use core::ops;
use core::ptr::{self, NonNull};
use core::slice;

use nginx_sys::{ngx_array_push, ngx_array_push_n, ngx_array_t, ngx_palloc, ngx_pool_t};

use crate::allocator::AllocError;

/// Wrapper over the [ngx_array_t].
///
/// See <https://nginx.org/en/docs/dev/development_guide.html#array>.
#[derive(Debug)]
#[repr(transparent)]
pub struct Array<T>(ngx_array_t, PhantomData<T>);

impl<T> Array<T> {
    /// Creates a new owned array, using the specified pool.
    pub fn new(pool: &mut ngx_pool_t, nalloc: usize) -> Result<Self, AllocError> {
        let size = mem::size_of::<T>();

        let elts = unsafe { ngx_palloc(pool, nalloc * size) };
        if elts.is_null() {
            return Err(AllocError);
        }

        let arr = ngx_array_t {
            elts,
            size,
            nalloc,
            pool,

            ..unsafe { mem::zeroed() }
        };

        Ok(Self(arr, Default::default()))
    }

    /// Returns `true` if the array contains no elements.
    pub fn is_empty(&self) -> bool {
        self.0.nelts == 0
    }

    /// Returns the number of elements in the array.
    pub fn len(&self) -> usize {
        self.0.nelts
    }

    /// Clears the array, removing all elements.
    pub fn clear(&mut self) {
        if self.is_empty() {
            return;
        }

        // TODO: drop elements for owned array

        let allocated = self.0.size * self.0.nalloc;
        let last = unsafe { self.0.elts.byte_add(allocated) };
        let pool = unsafe { &mut *self.0.pool };

        // Special case if the array data is on the top of the pool
        if ptr::addr_eq(last, pool.d.last) {
            pool.d.last = pool.d.last.wrapping_byte_sub(allocated);
        }

        self.0.nelts = 0;
    }

    /// Clones and appends all elements in a slice to `Array`.
    pub fn extend_from_slice(&mut self, other: &[T]) -> Result<(), AllocError>
    where
        T: Clone,
    {
        let dst = self.reserve(other.len())?;

        for (dst, src) in dst.iter_mut().zip(other) {
            dst.write(src.clone());
        }

        Ok(())
    }

    /// Attempts to reserve capacity for at least `n` more elements.
    pub fn reserve(&mut self, n: usize) -> Result<&mut [mem::MaybeUninit<T>], AllocError> {
        let p: *mut mem::MaybeUninit<T> = unsafe { ngx_array_push_n(&mut self.0, n).cast() };
        let p = NonNull::new(p).ok_or(AllocError)?;
        Ok(unsafe { NonNull::slice_from_raw_parts(p, n).as_mut() })
    }

    /// Attempts to add an element to the `Array`.
    pub fn push(&mut self, elem: T) -> Result<&T, AllocError> {
        let p: *mut mem::MaybeUninit<T> = unsafe { ngx_array_push(&mut self.0).cast() };
        let mut p = NonNull::new(p).ok_or(AllocError)?;
        Ok(unsafe { p.as_mut() }.write(elem))
    }

    /// Creates an `Array` reference from a pointer to [ngx_array_t].
    ///
    /// # Safety
    ///
    /// ptr must be a valid, well-aligned pointer to [ngx_array_t].
    pub unsafe fn from_ptr<'a>(ptr: *const ngx_array_t) -> &'a Self {
        debug_assert!(!ptr.is_null());
        debug_assert_eq!(mem::size_of::<T>(), (*ptr).size);
        &*ptr.cast()
    }

    /// Creates a mutable `Array` reference from a pointer to [ngx_array_t].
    ///
    /// # Safety
    ///
    /// ptr must be a valid, well-aligned pointer to [ngx_array_t].
    pub unsafe fn from_ptr_mut<'a>(ptr: *mut ngx_array_t) -> &'a mut Self {
        debug_assert!(!ptr.is_null());
        debug_assert_eq!(mem::size_of::<T>(), (*ptr).size);
        &mut *ptr.cast()
    }
}

impl<T> AsRef<ngx_array_t> for Array<T> {
    fn as_ref(&self) -> &ngx_array_t {
        &self.0
    }
}

impl<T> AsMut<ngx_array_t> for Array<T> {
    fn as_mut(&mut self) -> &mut ngx_array_t {
        &mut self.0
    }
}

impl<T> AsRef<[T]> for Array<T> {
    fn as_ref(&self) -> &[T] {
        if self.0.nelts == 0 {
            &[]
        } else {
            unsafe { slice::from_raw_parts(self.0.elts as *const T, self.0.nelts) }
        }
    }
}

impl<T> AsMut<[T]> for Array<T> {
    fn as_mut(&mut self) -> &mut [T] {
        if self.0.nelts == 0 {
            &mut []
        } else {
            unsafe { slice::from_raw_parts_mut(self.0.elts as *mut T, self.0.nelts) }
        }
    }
}

impl<T> ops::Deref for Array<T> {
    type Target = [T];

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl<T> ops::DerefMut for Array<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut()
    }
}

impl<T> Drop for Array<T> {
    fn drop(&mut self) {
        self.clear()
    }
}
