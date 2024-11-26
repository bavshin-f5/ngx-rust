use core::mem::MaybeUninit;
use core::ops::{Deref, DerefMut};

use crate::ffi::{self, ngx_connection_t, ngx_err_t};

/// Wrapper struct for an [`ngx_connection_t`] pointer
///
/// [`ngx_connection_t`]: http://nginx.org/en/docs/dev/development_guide.html#connection
#[repr(transparent)]
pub struct Connection(ngx_connection_t);

impl AsRef<ngx_connection_t> for Connection {
    fn as_ref(&self) -> &ngx_connection_t {
        &self.0
    }
}

impl AsMut<ngx_connection_t> for Connection {
    fn as_mut(&mut self) -> &mut ngx_connection_t {
        &mut self.0
    }
}

impl Deref for Connection {
    type Target = ngx_connection_t;

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl DerefMut for Connection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut()
    }
}

impl Connection {
    /// Creates a new connection wrapper.
    pub fn from_ptr_mut<'a>(c: *mut ngx_connection_t) -> &'a mut Self {
        unsafe { &mut *c.cast::<Self>() }
    }

    /// Returns a connection pool reference
    pub fn pool(&mut self) -> &mut ffi::ngx_pool_t {
        debug_assert!(!self.0.pool.is_null());
        unsafe { &mut *self.0.pool }
    }

    /// Returns a read event reference
    pub fn read(&mut self) -> &mut ffi::ngx_event_t {
        debug_assert!(!self.0.read.is_null());
        unsafe { &mut *self.0.read }
    }

    /// Returns a write event reference
    pub fn write(&mut self) -> &mut ffi::ngx_event_t {
        debug_assert!(!self.0.write.is_null());
        unsafe { &mut *self.0.write }
    }

    /// Check `connect` result
    pub fn test_connect(&mut self) -> Result<(), ngx_err_t> {
        #[cfg(ngx_feature = "have_kqueue")]
        if unsafe { ffi::ngx_event_flags } & (ffi::NGX_USE_KQUEUE_EVENT as usize) != 0 {
            if self.write().pending_eof() != 0 || self.read().pending_eof() != 0 {
                let err = if self.write().pending_eof() != 0 {
                    self.write().kq_errno
                } else {
                    self.read().kq_errno
                };

                self.error(err, c"kevent() reported that connect() failed");
                return Err(err);
            } else {
                return Ok(());
            }
        }

        let mut err: std::ffi::c_int = 0;
        let mut len: libc::socklen_t = std::mem::size_of_val(&err) as _;

        // BSDs and Linux return 0 and set a pending error in err
        // Solaris returns -1 and sets errno
        if unsafe {
            libc::getsockopt(
                self.0.fd,
                libc::SOL_SOCKET as _,
                libc::SO_ERROR as _,
                std::ptr::addr_of_mut!(err).cast(),
                &mut len,
            ) == -1
        } {
            err = ffi::ngx_socket_errno() as _;
        }

        if err != 0 {
            self.error(err, c"connect() failed");
            Err(err)
        } else {
            Ok(())
        }
    }

    /// Handle OS errors
    pub fn error(&mut self, err: ngx_err_t, msg: &std::ffi::CStr) -> ffi::ngx_int_t {
        unsafe { ffi::ngx_connection_error(self.as_mut(), err, msg.as_ptr().cast_mut()) }
    }

    /// Receive data from the connection
    pub fn recv(&mut self, buf: &mut [MaybeUninit<u8>]) -> isize {
        // send and recv are always set
        unsafe {
            self.as_ref().recv.unwrap_unchecked()(self.as_mut(), buf.as_mut_ptr().cast(), buf.len())
        }
    }

    /// Send data to the connection
    pub fn send(&mut self, buf: &[u8]) -> isize {
        // send and recv are always set
        unsafe {
            self.as_ref().send.unwrap_unchecked()(self.as_mut(), buf.as_ptr().cast_mut(), buf.len())
        }
    }

    /// Shutdown the connection
    pub fn shutdown(&mut self, how: std::ffi::c_int) -> Result<(), ngx_err_t> {
        if unsafe { libc::shutdown(self.0.fd, how) } == -1 {
            return Err(ffi::ngx_socket_errno());
        }
        Ok(())
    }

    /// Close the connection
    pub fn close(&mut self) {
        unsafe { ffi::ngx_close_connection(self.as_mut()) }
    }
}
