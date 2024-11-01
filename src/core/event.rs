use std::ops::{Deref, DerefMut};

use foreign_types::{ForeignType, ForeignTypeRef, Opaque};

use crate::ffi::{self, ngx_event_t, ngx_int_t, ngx_msec_t, ngx_queue_t, ngx_uint_t, NGX_TIMER_LAZY_DELAY};
use crate::log::DebugMask;
use crate::ngx_log_debug_mask;

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
    /// Returns an identifier of the event.
    pub fn ident(&self) -> usize {
        // nginx_event_ident is implemented as `(*self.data.cast::<ngx_connection_t>()).fd`
        self.data as usize
    }

    /// Adds event to the timer tree.
    pub fn add_timer(&mut self, timer: ngx_msec_t) {
        let key = unsafe { ffi::ngx_current_msec }.wrapping_add(timer);

        if self.timer_set() != 0 {
            let diff = self.timer.key as isize - key as isize;
            /*
             * Use a previous timer value if difference between it and a new
             * value is less than NGX_TIMER_LAZY_DELAY milliseconds: this allows
             * to minimize the rbtree operations for fast connections.
             */
            if diff.abs() < NGX_TIMER_LAZY_DELAY as isize {
                ngx_log_debug_mask!(
                    DebugMask::Event,
                    self.log,
                    "event: {:x} old: {:?} new: {:?}",
                    self.ident(),
                    self.timer.key,
                    key
                );
                return;
            }

            self.del_timer();
        }

        self.timer.key = key;

        ngx_log_debug_mask!(
            DebugMask::Event,
            self.log,
            "event timer add: {}: {:?}:{:?}",
            self.ident(),
            timer,
            self.timer.key
        );

        unsafe { ffi::ngx_rbtree_insert(std::ptr::addr_of_mut!(ffi::ngx_event_timer_rbtree), &mut self.timer) };

        self.set_timer_set(1);
    }

    /// Removes event from the timer tree.
    pub fn del_timer(&mut self) {
        ngx_log_debug_mask!(
            DebugMask::Event,
            self.log,
            "event timer del: {}: {:?}",
            self.ident(),
            self.timer.key
        );

        unsafe { ffi::ngx_rbtree_delete(std::ptr::addr_of_mut!(ffi::ngx_event_timer_rbtree), &mut self.timer) };

        #[cfg(debug_assertions)]
        {
            self.timer.left = std::ptr::null_mut();
            self.timer.right = std::ptr::null_mut();
            self.timer.parent = std::ptr::null_mut();
        }

        self.set_timer_set(0);
    }

    /// Adds event to the specified posted events queue.
    ///
    /// # Safety
    /// `queue` must be a valid pointer to a posted events queue.
    pub unsafe fn post_event(&mut self, queue: *mut ngx_queue_t) {
        if self.posted() == 0 {
            self.set_posted(1);

            // ngx_queue_insert_tail
            self.queue.prev = unsafe { *queue }.prev;
            unsafe { *self.queue.prev }.next = &mut self.queue;
            self.queue.next = queue;
            unsafe { *queue }.prev = &mut self.queue;

            ngx_log_debug_mask!(DebugMask::Event, self.log, "post event {:x}", self.as_ptr() as usize);
        } else {
            ngx_log_debug_mask!(
                DebugMask::Event,
                self.log,
                "update posted event {:x}",
                self.as_ptr() as usize
            );
        }
    }

    /// Removes event from the posted events queue.
    pub fn delete_posted_event(&mut self) {
        if self.posted() != 0 {
            self.set_posted(0);

            // ngx_queue_remove
            unsafe { *self.queue.next }.prev = self.queue.prev;
            unsafe { *self.queue.prev }.next = self.queue.next;

            #[cfg(debug_assertions)]
            {
                self.queue.next = std::ptr::null_mut();
                self.queue.prev = std::ptr::null_mut();
            }

            ngx_log_debug_mask!(
                DebugMask::Event,
                self.log,
                "delete posted event {:x}",
                self.as_ptr() as usize
            );
        }
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
