use std::ptr::addr_of_mut;
use std::time::{Duration, Instant};

use ngx::ffi::{ngx_current_msec, ngx_event_t, ngx_msec_int_t, ngx_msec_t};
use pin_project_lite::pin_project;

/// Waits until `duration` has elapsed
pub fn sleep(duration: Duration) -> Sleep {
    let deadline = Instant::now() + duration;

    Sleep::new_timeout(deadline)
}

#[inline]
pub fn ngx_add_timer(event: &mut ngx_event_t, timer: ngx_msec_t) {
    let key = unsafe { ngx_current_msec as ngx_msec_int_t } + timer as ngx_msec_int_t;

    if event.timer_set() != 0 {
        /*
         * Use a previous timer value if difference between it and a new
         * value is less than NGX_TIMER_LAZY_DELAY milliseconds: this allows
         * to minimize the rbtree operations for fast connections.
         */

        let diff = key - event.timer.key as ngx_msec_int_t;

        if diff.abs() < ngx::ffi::NGX_TIMER_LAZY_DELAY as ngx_msec_int_t {
            return;
        }

        ngx_del_timer(event);
    }

    event.timer.key = key as ngx_msec_t;

    unsafe {
        ngx::ffi::ngx_rbtree_insert(
            addr_of_mut!(ngx::ffi::ngx_event_timer_rbtree),
            &mut event.timer,
        );
    }

    event.set_timer_set(1);
}

#[inline]
pub fn ngx_del_timer(event: &mut ngx_event_t) {
    unsafe {
        ngx::ffi::ngx_rbtree_delete(
            addr_of_mut!(ngx::ffi::ngx_event_timer_rbtree),
            &mut event.timer,
        )
    };
    #[cfg(debug_assertions)]
    {
        event.timer.left = std::ptr::null_mut();
        event.timer.right = std::ptr::null_mut();
        event.timer.parent = std::ptr::null_mut();
    }

    event.set_timer_set(0);
}

pin_project! {
    pub struct Sleep {
        #[pin]
        event: ngx_event_t,
    }

    impl PinnedDrop for Sleep {
        fn drop(this: Pin<&mut Self>) {
            let event = this.project().event;
            if event.timer_set() != 0 {
                ngx_del_timer(unsafe { event.get_unchecked_mut() });
            }
        }
    }
}

impl Sleep {
    unsafe extern "C" fn handler(ev: *mut ngx_event_t) {
        assert!((*ev).timedout() > 0);
    }

    // pub fn new_timeout(deadline: Instant) -> Sleep {}
}
