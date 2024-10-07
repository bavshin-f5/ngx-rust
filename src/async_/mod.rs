//! Async runtime and set of utilities on top of the NGINX event loop.
pub use self::sleep::sleep;
pub use self::spawn::{spawn, Task};

#[cfg(feature = "std")]
pub mod http;
mod sleep;
mod spawn;
