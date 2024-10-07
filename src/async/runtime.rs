use std::future::Future;
use std::panic::catch_unwind;

use async_task::{Runnable, ScheduleInfo, WithInfo};
use flume::{Receiver, Sender};

use crate::core::Event;
use crate::ffi::{ngx_cycle, ngx_event_t, ngx_posted_next_events};
use crate::ngx_log_debug;

pub use async_task::Task;

#[derive(Debug)]
pub struct RuntimeError;

impl std::error::Error for RuntimeError {}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Error: task panicked")
    }
}

thread_local! {
    static QUEUE: (Sender<Runnable>, Receiver<Runnable>) = flume::unbounded();
    static EVENT: std::cell::UnsafeCell<Event> = Event::default().into();
}

fn schedule(runnable: Runnable, info: ScheduleInfo) {
    if info.woken_while_running {
        QUEUE.with(|(s, _)| s.send(runnable).unwrap());
        // FIXME: attach pinned ngx_event_t to the task to avoid using flume
        EVENT.with(|ev| unsafe {
            let ev = &mut *ev.get();
            ev.handler = Some(ngx_async_posted_event_handler);
            ev.post_event(std::ptr::addr_of_mut!(ngx_posted_next_events));
        });
        ngx_log_debug!((*ngx_cycle).log, "task woken while running");
    } else if let Err(err) = catch_unwind(|| runnable.run()) {
        ngx_log_debug!((*ngx_cycle).log, "runtime::run failed {:?}", err);
    }
}

pub fn spawn<F, T>(future: F) -> Result<Task<T>, RuntimeError>
where
    F: Future<Output = T> + 'static,
    T: 'static,
{
    ngx_log_debug!((*ngx_cycle).log, "runtime::spawn enter");
    let scheduler = WithInfo(schedule);
    let (runnable, task) = async_task::spawn_local(future, scheduler);

    runnable.schedule();

    ngx_log_debug!((*ngx_cycle).log, "runtime::spawn exit");
    Ok(task)
}

extern "C" fn ngx_async_posted_event_handler(_ev: *mut ngx_event_t) {
    ngx_log_debug!((*ngx_cycle).log, "runtime::step enter");
    QUEUE.with(|(_, receiver)| {
        while let Ok(runnable) = receiver.try_recv() {
            ngx_log_debug!((*ngx_cycle).log, "runtime::step iter");
            let _ignore = catch_unwind(|| runnable.run());
        }
    });
    ngx_log_debug!((*ngx_cycle).log, "runtime::step exit");
}
