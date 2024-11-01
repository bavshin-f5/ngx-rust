use std::ops::{Deref, DerefMut};

use foreign_types::{ForeignType, ForeignTypeRef, Opaque};

use crate::ffi::{self, ngx_event_t, ngx_int_t, ngx_msec_t, ngx_queue_t, ngx_uint_t};

/// Representation of an [Nginx event].
///
/// [Nginx event]: http://nginx.org/en/docs/dev/development_guide.html#events
pub struct Event(ngx_event_t);

impl Default for Event {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

impl Deref for Event {
    type Target = EventRef;

    fn deref(&self) -> &Self::Target {
        unsafe { EventRef::from_ptr(&self.0 as *const _ as *mut _) }
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        if self.timer_set() != 0 {
            self.del_timer();
        }

        if self.posted() != 0 {
            self.delete_posted_event();
        }
    }
}

impl DerefMut for Event {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { EventRef::from_ptr_mut(&mut self.0) }
    }
}

unsafe impl ForeignType for Event {
    type CType = ngx_event_t;
    type Ref = EventRef;

    unsafe fn from_ptr(ptr: *mut Self::CType) -> Self {
        assert!(!ptr.is_null());
        Self(*ptr)
    }

    fn as_ptr(&self) -> *mut Self::CType {
        &self.0 as *const _ as *mut _
    }
}

/// Representation of a borrowed [Nginx event].
///
/// [Nginx event]: http://nginx.org/en/docs/dev/development_guide.html#events
pub struct EventRef(Opaque);

impl AsRef<ngx_event_t> for EventRef {
    fn as_ref(&self) -> &ngx_event_t {
        unsafe { &*self.as_ptr() }
    }
}

impl AsMut<ngx_event_t> for EventRef {
    fn as_mut(&mut self) -> &mut ngx_event_t {
        unsafe { &mut *self.as_ptr() }
    }
}

impl Deref for EventRef {
    type Target = ngx_event_t;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.as_ptr() }
    }
}

impl DerefMut for EventRef {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.as_ptr() }
    }
}

unsafe impl ForeignTypeRef for EventRef {
    type CType = ngx_event_t;
}

impl EventRef {
    /// Adds event to the timer tree.
    pub fn add_timer(&mut self, timer: ngx_msec_t) {
        unsafe { ffi::ngx_add_timer(self.as_ptr(), timer) };
    }

    /// Removes event from the timer tree.
    pub fn del_timer(&mut self) {
        unsafe { ffi::ngx_del_timer(self.as_ptr()) };
    }

    /// Adds event to the specified posted events queue.
    ///
    /// # Safety
    /// `queue` must be a valid pointer to a posted events queue.
    pub unsafe fn post_event(&mut self, queue: *mut ngx_queue_t) {
        unsafe { ffi::ngx_post_event(self.as_ptr(), queue) };
    }

    /// Removes event from the posted events queue.
    pub fn delete_posted_event(&mut self) {
        unsafe { ffi::ngx_delete_posted_event(self.as_ptr()) };
    }

    /// Updates read event state
    pub fn handle_read(&mut self, flags: ngx_uint_t) -> Result<(), ngx_int_t> {
        let rc = unsafe { ffi::ngx_handle_read_event(self.as_ptr(), flags) };
        if rc != ffi::NGX_OK as _ {
            return Err(rc);
        }
        Ok(())
    }

    /// Updates write event state
    pub fn handle_write(&mut self, lowat: usize) -> Result<(), ngx_int_t> {
        let rc = unsafe { ffi::ngx_handle_write_event(self.as_ptr(), lowat) };
        if rc != ffi::NGX_OK as _ {
            return Err(rc);
        }
        Ok(())
    }
}
