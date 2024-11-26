use std::mem::MaybeUninit;
use std::ops::{Deref, DerefMut};

use foreign_types::{ForeignTypeRef, Opaque};

use crate::core::{EventRef, PoolRef};
use crate::ffi::{self, ngx_connection_t, ngx_err_t};

/// Wrapper struct for an [`ngx_connection_t`] pointer
///
/// There's no owned counterpart, as modules should never create or own `ngx_connection_t`
///
/// [`ngx_connection_t`]: http://nginx.org/en/docs/dev/development_guide.html#connection
pub struct Connection(Opaque);

unsafe impl ForeignTypeRef for Connection {
    type CType = ngx_connection_t;
}

impl AsRef<ngx_connection_t> for Connection {
    fn as_ref(&self) -> &ngx_connection_t {
        self.deref()
    }
}

impl AsMut<ngx_connection_t> for Connection {
    fn as_mut(&mut self) -> &mut ngx_connection_t {
        self.deref_mut()
    }
}

impl Deref for Connection {
    type Target = ngx_connection_t;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.as_ptr() }
    }
}

impl DerefMut for Connection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.as_ptr() }
    }
}

impl Connection {
    /// Returns a configuration pool reference
    pub fn pool(&mut self) -> &mut PoolRef {
        debug_assert!(!self.pool.is_null());
        unsafe { PoolRef::from_ptr_mut(self.pool) }
    }

    /// Returns a read event reference
    pub fn read(&mut self) -> &mut EventRef {
        debug_assert!(!self.read.is_null());
        unsafe { EventRef::from_ptr_mut(self.read) }
    }

    /// Returns a write event reference
    pub fn write(&mut self) -> &mut EventRef {
        debug_assert!(!self.write.is_null());
        unsafe { EventRef::from_ptr_mut(self.write) }
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
                self.fd,
                libc::SOL_SOCKET as _,
                libc::SO_ERROR as _,
                std::ptr::addr_of_mut!(err).cast(),
                &mut len,
            ) == -1
        } {
            err = errno::errno().0;
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
        unsafe { ffi::ngx_connection_error(self.as_ptr(), err, msg.as_ptr().cast_mut()) }
    }

    /// Receive data from the connection
    pub fn recv(&mut self, buf: &mut [MaybeUninit<u8>]) -> isize {
        // send and recv are always set
        unsafe { self.recv.unwrap_unchecked()(self.as_ptr(), buf.as_mut_ptr().cast(), buf.len()) }
    }

    /// Send data to the connection
    pub fn send(&mut self, buf: &[u8]) -> isize {
        // send and recv are always set
        unsafe { self.send.unwrap_unchecked()(self.as_ptr(), buf.as_ptr().cast_mut(), buf.len()) }
    }

    /// Shutdown the connection
    pub fn shutdown(&mut self, how: std::ffi::c_int) -> Result<(), ngx_err_t> {
        if unsafe { libc::shutdown(self.fd, how) } == -1 {
            return Err(errno::errno().0);
        }
        Ok(())
    }

    /// Close the connection
    pub fn close(&mut self) {
        unsafe { ffi::ngx_close_connection(self.as_ptr()) }
    }
}
