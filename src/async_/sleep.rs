use core::future;
use core::mem;
use core::pin::{pin, Pin};
use core::ptr;
use core::task::{self, Poll};
use core::time::Duration;

use nginx_sys::{ngx_add_timer, ngx_del_timer, ngx_event_t, ngx_msec_int_t, ngx_msec_t};

use crate::{ngx_container_of, ngx_log_debug};

/// Maximum duration that can be achieved using [ngx_add_timer].
const NGX_TIMER_DURATION_MAX: Duration = Duration::from_millis(ngx_msec_int_t::MAX as _);

/// Puts the current task to sleep for at least the specified amount of time.
#[cfg(not(target_pointer_width = "32"))]
pub async fn sleep(duration: Duration) {
    let mut timer = pin!(Timer::new());
    ngx_log_debug!(timer.event.log, "async: sleep for {duration:?}");

    let msec = duration.min(NGX_TIMER_DURATION_MAX).as_millis() as ngx_msec_t;
    future::poll_fn(|cx| timer.as_mut().poll_sleep(msec, cx)).await
}

/// Puts the current task to sleep for at least the specified amount of time.
#[cfg(target_pointer_width = "32")]
pub async fn sleep(mut duration: Duration) {
    let mut timer = pin!(Timer::new());
    ngx_log_debug!(timer.event.log, "async: sleep for {duration:?}");

    // Handle ngx_msec_t overflow on 32-bit platforms.
    while !duration.is_zero() {
        let msec = duration.min(NGX_TIMER_DURATION_MAX);
        duration = duration.saturating_sub(msec);

        let msec = msec.as_millis() as ngx_msec_t;
        timer.event.set_timedout(0); // rearm
        future::poll_fn(|cx| timer.as_mut().poll_sleep(msec, cx)).await
    }
}

struct Timer {
    event: ngx_event_t,
    waker: Option<task::Waker>,
}

// SAFETY: Timer will only be used in a single-threaded environment
unsafe impl Send for Timer {}
unsafe impl Sync for Timer {}

impl Timer {
    pub fn new() -> Self {
        static IDENT: [usize; 4] = [
            0, 0, 0, 0x4153594e, // ASYN
        ];

        let mut ev: ngx_event_t = unsafe { mem::zeroed() };
        // The data is only used for `ngx_event_ident` and will not be mutated.
        ev.data = ptr::addr_of!(IDENT).cast_mut().cast();
        ev.handler = Some(Self::timer_handler);
        ev.log = unsafe { *nginx_sys::ngx_cycle }.log;
        ev.set_cancelable(1);

        Self {
            event: ev,
            waker: None,
        }
    }

    pub fn poll_sleep(
        mut self: Pin<&mut Self>,
        duration: ngx_msec_t,
        context: &mut task::Context<'_>,
    ) -> Poll<()> {
        if self.event.timedout() != 0 {
            Poll::Ready(())
        } else if self.event.timer_set() != 0 {
            self.waker = Some(context.waker().clone());
            Poll::Pending
        } else {
            unsafe { ngx_add_timer(ptr::addr_of_mut!(self.event), duration) };
            self.waker = Some(context.waker().clone());
            Poll::Pending
        }
    }

    unsafe extern "C" fn timer_handler(ev: *mut ngx_event_t) {
        let timer = ngx_container_of!(ev, Self, event);

        if let Some(waker) = (*timer).waker.take() {
            waker.wake();
        }
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        if self.event.timer_set() != 0 {
            unsafe { ngx_del_timer(ptr::addr_of_mut!(self.event)) };
        }
    }
}
