use core::cell::{OnceCell, RefCell};
use core::future::Future;
use core::ptr::NonNull;
use std::collections::vec_deque::VecDeque;

pub use async_task::Task;
use async_task::{Runnable, ScheduleInfo, WithInfo};
use nginx_sys::{ngx_cycle, ngx_event_t, ngx_post_event, ngx_posted_next_events};

use crate::ngx_log_debug;

// Ready-list of runnables. Flume provides a queue capable of sending and recieving on
// different threads, but since this is thread-local, it could be simplified.
static QUEUE: Queue = Queue::new();

struct Queue(RefCell<VecDeque<Runnable>>);
unsafe impl Send for Queue {}
unsafe impl Sync for Queue {}

impl Queue {
    pub const fn new() -> Self {
        Self(RefCell::new(VecDeque::new()))
    }
    pub fn send(&self, runnable: Runnable) {
        let mut queue = self.0.borrow_mut();
        queue.push_back(runnable)
    }
    pub fn recv(&self) -> Option<Runnable> {
        let mut queue = self.0.borrow_mut();
        queue.pop_front()
    }
}

// Nginx event which is posted when a task is made runnable inside the scheduler.
// This event's handler will run everything in the ready-list.
static EVENT: SchedulerEvent = SchedulerEvent::new();

struct SchedulerEvent(OnceCell<*mut ngx_event_t>);
// Safety: single threaded embedding
unsafe impl Send for SchedulerEvent {}
unsafe impl Sync for SchedulerEvent {}
impl SchedulerEvent {
    const fn new() -> Self {
        Self(OnceCell::new())
    }

    /// Post the event, so the handler is run next time ngx_process_events_and_timers comes around.
    /// This can be called multiple times before the queue is processed - the QUEUE will build the
    /// set of ready runnables, and the handler will run them all.
    fn post(&self) {
        let event: *mut ngx_event_t = *self.0.get_or_init(|| {
            let mut inner: NonNull<SchedulerEventInner> =
                crate::allocator::allocate(Default::default(), &crate::allocator::Global)
                    .expect("alloc SchedulerEvent");

            let i = unsafe { inner.as_mut() };
            i.ident = 0; // this integer may be in debug logs from ngx_event_expire_timers
            i.event.handler = Some(Self::handler);
            i.event.data = inner.as_ptr().cast();
            i.event.log = unsafe { (*ngx_cycle).log };
            core::ptr::addr_of_mut!(i.event)
        });

        let queue = core::ptr::addr_of_mut!(ngx_posted_next_events);
        unsafe { (*event).log = (*ngx_cycle).log };
        unsafe { ngx_post_event(event, queue) }
    }

    /// This event handler is called by ngx_event_process_posted at the end of
    /// ngx_process_events_and_timers.
    extern "C" fn handler(ev: *mut ngx_event_t) {
        let ev = unsafe { &mut *ev };

        ngx_log_debug!(ev.log, "runtime::step enter");
        while let Some(runnable) = QUEUE.recv() {
            ngx_log_debug!(ev.log, "runtime::step iter");
            runnable.run();
        }
        ngx_log_debug!(ev.log, "runtime::step exit");
    }
}

// Strictly speaking, this isnt required, because the scheduler event does not get used with a
// timer at the moment, its only posted to the ngx_posted_next_events queue. However, if we ever
// did use a timer, ngx_event_expire_timers will cast event->data to an ngx_connection_t and
// dereference ->fd, the fourth word in the struct. So, in order for the use of ngx_event_t in an
// ASAN-safe manner ASAN-safe, we must create this 4-word struct as the target for the event->data
// pointer, even though we do not use the data field in event for anything.
#[repr(C)]
struct SchedulerEventInner {
    _pad: [usize; 3],
    ident: usize,
    event: ngx_event_t,
}

impl Default for SchedulerEventInner {
    fn default() -> Self {
        Self {
            ..unsafe { core::mem::zeroed() }
        }
    }
}

fn schedule(runnable: Runnable, info: ScheduleInfo) {
    if info.woken_while_running {
        QUEUE.send(runnable);
        EVENT.post();
        ngx_log_debug!(unsafe { (*ngx_cycle).log }, "task woken while running");
    } else {
        runnable.run();
    }
}

pub fn spawn<F, T>(future: F) -> Task<T>
where
    F: Future<Output = T> + 'static,
    T: 'static,
{
    ngx_log_debug!(unsafe { (*ngx_cycle).log }, "runtime::spawn enter");
    let scheduler = WithInfo(schedule);
    // Safety: single threaded embedding takes care of send/sync requirements for future and
    // scheduler. Future and scheduler are both 'static.
    let (runnable, task) = unsafe { async_task::spawn_unchecked(future, scheduler) };

    runnable.schedule();

    ngx_log_debug!(unsafe { (*ngx_cycle).log }, "runtime::spawn exit");
    task
}
