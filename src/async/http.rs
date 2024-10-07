use std::task::Poll;

use foreign_types::{ForeignType, ForeignTypeRef};
use http_body::Frame;
use pin_project_lite::pin_project;

use crate::ffi::{ngx_buf_t, ngx_chain_t, ngx_http_request_body_t, ngx_http_request_t};

foreign_types::foreign_type! {
    /// Wrapper struct for an [`ngx_buf_t`]
    pub unsafe type Buf: Send {
        type CType = ngx_buf_t;
        // No cleanup required for pool-allocated structs
        fn drop = |_|();
    }
}

impl AsRef<ngx_buf_t> for Buf {
    fn as_ref(&self) -> &ngx_buf_t {
        unsafe { &*self.as_ptr() }
    }
}

impl AsMut<ngx_buf_t> for Buf {
    fn as_mut(&mut self) -> &mut ngx_buf_t {
        unsafe { &mut *self.as_ptr() }
    }
}

impl AsRef<ngx_buf_t> for BufRef {
    fn as_ref(&self) -> &ngx_buf_t {
        unsafe { &*self.as_ptr() }
    }
}

impl AsMut<ngx_buf_t> for BufRef {
    fn as_mut(&mut self) -> &mut ngx_buf_t {
        unsafe { &mut *self.as_ptr() }
    }
}

impl bytes::Buf for Buf {
    fn remaining(&self) -> usize {
        let buf: &ngx_buf_t = self.as_ref();
        unsafe { buf.last.offset_from(buf.pos) as usize }
    }

    fn chunk(&self) -> &[u8] {
        let buf: &ngx_buf_t = self.as_ref();
        unsafe { std::slice::from_raw_parts(buf.pos, self.remaining()) }
    }

    fn advance(&mut self, cnt: usize) {
        let buf: &mut ngx_buf_t = self.as_mut();
        unsafe { buf.pos = buf.pos.add(cnt) };
    }
}

pin_project! {
pub struct RequestBody {
    request_body: *mut ngx_http_request_body_t,
    chain: *mut ngx_chain_t,
    waker: Option<std::task::Waker>,
}
}

impl Default for RequestBody {
    fn default() -> Self {
        RequestBody {
            request_body: std::ptr::null_mut(),
            chain: std::ptr::null_mut(),
            waker: None,
        }
    }
}

impl RequestBody {
    pub fn update(&mut self, r: &mut ngx_http_request_t) {
        self.request_body = r.request_body;
        self.chain = unsafe { (*self.request_body).bufs };

        if let Some(waker) = self.waker.take() {
            waker.wake()
        }
    }
}

impl http_body::Body for RequestBody {
    type Data = Buf;
    type Error = std::io::Error;

    fn poll_frame(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.project();

        if this.request_body.is_null() {
            *this.waker = Some(cx.waker().clone());
            return Poll::Pending;
        }

        if this.chain.is_null() {
            return Poll::Ready(None);
        }

        let cl = unsafe { **this.chain };
        *this.chain = cl.next;

        let buf = unsafe { Buf::from_ptr(cl.buf) };
        Poll::Ready(Some(Ok(Frame::data(buf))))
    }

    fn is_end_stream(&self) -> bool {
        !self.request_body.is_null() && self.chain.is_null()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        http_body::SizeHint::default()
    }
}
