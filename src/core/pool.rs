use core::ffi::c_void;
use core::{mem, ptr};

use foreign_types::{foreign_type, ForeignType, ForeignTypeRef};

use crate::core::buffer::{Buffer, MemoryBuffer, TemporaryBuffer};
use crate::ffi::{
    ngx_buf_t, ngx_create_pool, ngx_create_temp_buf, ngx_destroy_pool, ngx_log_t, ngx_palloc, ngx_pcalloc, ngx_pnalloc,
    ngx_pool_cleanup_add, ngx_pool_t,
};

foreign_type! {
    /// Wrapper struct for an `ngx_pool_t` pointer, providing methods for working with memory pools.
    ///
    /// See <https://nginx.org/en/docs/dev/development_guide.html#pool>
    pub unsafe type Pool {
        type CType = ngx_pool_t;

        fn drop = ngx_destroy_pool;
    }
}

impl AsRef<ngx_pool_t> for PoolRef {
    fn as_ref(&self) -> &ngx_pool_t {
        // SAFETY: PoolRef must contain a valid pointer to the pool
        unsafe { &*self.as_ptr() }
    }
}

impl AsMut<ngx_pool_t> for PoolRef {
    fn as_mut(&mut self) -> &mut ngx_pool_t {
        // SAFETY: PoolRef must contain a valid pointer to the pool
        unsafe { &mut *self.as_ptr() }
    }
}

impl Pool {
    /// Creates a pool with the specified size and log.
    ///
    /// # Safety
    /// The caller must pass a valid pointer to the `ngx_log_t`
    pub unsafe fn create(size: usize, log: *mut ngx_log_t) -> Option<Self> {
        debug_assert!(!log.is_null());
        let pool = unsafe { ngx_create_pool(size, log) };
        if pool.is_null() {
            None
        } else {
            // SAFETY: already checked that the pointer is valid
            Some(unsafe { Self::from_ptr(pool) })
        }
    }
}

impl PoolRef {
    /// Creates a buffer of the specified size in the memory pool.
    ///
    /// Returns `Some(TemporaryBuffer)` if the buffer is successfully created, or `None` if allocation fails.
    pub fn create_buffer(&mut self, size: usize) -> Option<TemporaryBuffer> {
        let buf = unsafe { ngx_create_temp_buf(self.as_ptr(), size) };
        if buf.is_null() {
            return None;
        }

        Some(TemporaryBuffer::from_ngx_buf(buf))
    }

    /// Creates a buffer from a string in the memory pool.
    ///
    /// Returns `Some(TemporaryBuffer)` if the buffer is successfully created, or `None` if allocation fails.
    pub fn create_buffer_from_str(&mut self, str: &str) -> Option<TemporaryBuffer> {
        let mut buffer = self.create_buffer(str.len())?;
        unsafe {
            let buf = buffer.as_ngx_buf_mut();
            ptr::copy_nonoverlapping(str.as_ptr(), (*buf).pos, str.len());
            (*buf).last = (*buf).pos.add(str.len());
        }
        Some(buffer)
    }

    /// Creates a buffer from a static string in the memory pool.
    ///
    /// Returns `Some(MemoryBuffer)` if the buffer is successfully created, or `None` if allocation fails.
    pub fn create_buffer_from_static_str(&mut self, str: &'static str) -> Option<MemoryBuffer> {
        let buf = self.calloc_type::<ngx_buf_t>();
        if buf.is_null() {
            return None;
        }

        // We cast away const, but buffers with the memory flag are read-only
        let start = str.as_ptr() as *mut u8;
        let end = unsafe { start.add(str.len()) };

        unsafe {
            (*buf).start = start;
            (*buf).pos = start;
            (*buf).last = end;
            (*buf).end = end;
            (*buf).set_memory(1);
        }

        Some(MemoryBuffer::from_ngx_buf(buf))
    }

    /// Adds a cleanup handler for a value in the memory pool.
    ///
    /// Returns `Ok(())` if the cleanup handler is successfully added, or `Err(())` if the cleanup handler cannot be added.
    ///
    /// # Safety
    /// This function is marked as unsafe because it involves raw pointer manipulation.
    unsafe fn add_cleanup_for_value<T>(&mut self, value: *mut T) -> Result<(), ()> {
        let cln = ngx_pool_cleanup_add(self.as_ptr(), 0);
        if cln.is_null() {
            return Err(());
        }
        (*cln).handler = Some(cleanup_type::<T>);
        (*cln).data = value as *mut c_void;

        Ok(())
    }

    /// Allocates memory from the pool of the specified size.
    /// The resulting pointer is aligned to a platform word size.
    ///
    /// Returns a raw pointer to the allocated memory.
    pub fn alloc(&mut self, size: usize) -> *mut c_void {
        unsafe { ngx_palloc(self.as_ptr(), size) }
    }

    /// Allocates memory for a type from the pool.
    /// The resulting pointer is aligned to a platform word size.
    ///
    /// Returns a typed pointer to the allocated memory.
    pub fn alloc_type<T: Copy>(&mut self) -> *mut T {
        self.alloc(mem::size_of::<T>()) as *mut T
    }

    /// Allocates zeroed memory from the pool of the specified size.
    /// The resulting pointer is aligned to a platform word size.
    ///
    /// Returns a raw pointer to the allocated memory.
    pub fn calloc(&mut self, size: usize) -> *mut c_void {
        unsafe { ngx_pcalloc(self.as_ptr(), size) }
    }

    /// Allocates zeroed memory for a type from the pool.
    /// The resulting pointer is aligned to a platform word size.
    ///
    /// Returns a typed pointer to the allocated memory.
    pub fn calloc_type<T: Copy>(&mut self) -> *mut T {
        self.calloc(mem::size_of::<T>()) as *mut T
    }

    /// Allocates unaligned memory from the pool of the specified size.
    ///
    /// Returns a raw pointer to the allocated memory.
    pub fn alloc_unaligned(&mut self, size: usize) -> *mut c_void {
        unsafe { ngx_pnalloc(self.as_ptr(), size) }
    }

    /// Allocates unaligned memory for a type from the pool.
    ///
    /// Returns a typed pointer to the allocated memory.
    pub fn alloc_type_unaligned<T: Copy>(&mut self) -> *mut T {
        self.alloc_unaligned(mem::size_of::<T>()) as *mut T
    }

    /// Allocates memory for a value of a specified type and adds a cleanup handler to the memory pool.
    ///
    /// Returns a typed pointer to the allocated memory if successful, or a null pointer if allocation or cleanup handler addition fails.
    pub fn allocate<T>(&mut self, value: T) -> *mut T {
        unsafe {
            let p = self.alloc(mem::size_of::<T>()) as *mut T;
            ptr::write(p, value);
            if self.add_cleanup_for_value(p).is_err() {
                ptr::drop_in_place(p);
                return ptr::null_mut();
            };
            p
        }
    }
}

/// Cleanup handler for a specific type `T`.
///
/// This function is called when cleaning up a value of type `T` in an FFI context.
///
/// # Safety
/// This function is marked as unsafe due to the raw pointer manipulation and the assumption that `data` is a valid pointer to `T`.
///
/// # Arguments
///
/// * `data` - A raw pointer to the value of type `T` to be cleaned up.
unsafe extern "C" fn cleanup_type<T>(data: *mut c_void) {
    ptr::drop_in_place(data as *mut T);
}
