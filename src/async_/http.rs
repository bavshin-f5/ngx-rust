#![allow(missing_docs)]
use core::pin::Pin;
use core::ptr::{self, NonNull};
use core::slice;
use core::task::{self, Poll};
use std::io;

use http_body::Frame;
use nginx_sys::{ngx_buf_t, ngx_chain_t, ngx_http_request_body_t, ngx_http_request_t};
use pin_project_lite::pin_project;

pub struct Buf(NonNull<ngx_buf_t>);

impl Buf {
    /// Creates a new [Buf] from a pointer to [ngx_buf_t].
    ///
    /// # Safety
    ///
    /// ptr must be a valid, well-aligned pointer to [ngx_buf_t].
    pub unsafe fn from_ptr(buf: *mut ngx_buf_t) -> Self {
        Self(NonNull::new(buf).expect("valid pointer"))
    }
}

unsafe impl Send for Buf {}

impl AsRef<ngx_buf_t> for Buf {
    fn as_ref(&self) -> &ngx_buf_t {
        unsafe { self.0.as_ref() }
    }
}

impl AsMut<ngx_buf_t> for Buf {
    fn as_mut(&mut self) -> &mut ngx_buf_t {
        unsafe { self.0.as_mut() }
    }
}

impl bytes::Buf for Buf {
    fn remaining(&self) -> usize {
        let buf: &ngx_buf_t = self.as_ref();
        unsafe { buf.last.offset_from(buf.pos) as usize }
    }

    fn chunk(&self) -> &[u8] {
        let buf: &ngx_buf_t = self.as_ref();
        unsafe { slice::from_raw_parts(buf.pos, self.remaining()) }
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
    waker: Option<task::Waker>,
}
}

impl Default for RequestBody {
    fn default() -> Self {
        RequestBody {
            request_body: ptr::null_mut(),
            chain: ptr::null_mut(),
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
    type Error = io::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
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
