#![allow(unused)]

use std::os::raw::{c_char, c_void};
use std::pin::Pin;
use std::ptr::addr_of;
use std::task::Poll;

use anyhow::Result;

use ngx::core::{self, NgxConfRef};
use ngx::ffi::{
    self, nginx_version, ngx_chain_t, ngx_command_t, ngx_conf_t, ngx_http_core_module, ngx_http_module_t,
    ngx_http_request_t, ngx_int_t, ngx_module_t, ngx_uint_t, ngx_url_t, NGX_CONF_TAKE1, NGX_HTTP_LOC_CONF,
    NGX_HTTP_MODULE, NGX_RS_HTTP_LOC_CONF_OFFSET, NGX_RS_MODULE_SIGNATURE,
};
use ngx::http::{ngx_http_conf_get_module_loc_conf, HTTPModule, MergeConfigError};
use ngx::r#async::http::RequestBody;
use ngx::ForeignTypeRef;
use ngx::{ngx_log_debug, r#async as ngx_async};
use ngx::{ngx_log_debug_http, ngx_null_command, ngx_string};

struct Module;

impl ngx::http::HTTPModule for Module {
    type MainConf = ();
    type SrvConf = ();
    type LocConf = ModuleConfig;
}

struct ModuleConfig {
    endpoint: ngx_url_t,
}

impl Default for ModuleConfig {
    fn default() -> Self {
        ModuleConfig {
            endpoint: unsafe { std::mem::zeroed() },
        }
    }
}

impl ngx::http::Merge for ModuleConfig {
    fn merge(&mut self, prev: &ModuleConfig) -> Result<(), MergeConfigError> {
        if self.endpoint.url.is_empty() {
            self.endpoint = prev.endpoint;
        }
        Ok(())
    }
}

static mut NGX_HTTP_ASYNC_NGX_COMMANDS: [ngx_command_t; 2] = [
    ngx_command_t {
        name: ngx_string!("async_pass"),
        type_: (NGX_HTTP_LOC_CONF | NGX_CONF_TAKE1) as ngx_uint_t,
        set: Some(ngx_http_async_commands_async_pass),
        conf: NGX_RS_HTTP_LOC_CONF_OFFSET,
        offset: 0,
        post: std::ptr::null_mut(),
    },
    ngx_null_command!(),
];

static NGX_HTTP_ASYNC_NGX_MODULE_CTX: ngx_http_module_t = ngx_http_module_t {
    preconfiguration: Some(Module::preconfiguration),
    postconfiguration: Some(Module::postconfiguration),
    create_main_conf: Some(Module::create_main_conf),
    init_main_conf: Some(Module::init_main_conf),
    create_srv_conf: Some(Module::create_srv_conf),
    merge_srv_conf: Some(Module::merge_srv_conf),
    create_loc_conf: Some(Module::create_loc_conf),
    merge_loc_conf: Some(Module::merge_loc_conf),
};

// Generate the `ngx_modules` table with exported modules.
// This feature is required to build a 'cdylib' dynamic module outside of the NGINX buildsystem.
#[cfg(feature = "export-modules")]
ngx::ngx_modules!(ngx_http_async_ngx_module);

#[no_mangle]
#[used]
pub static mut ngx_http_async_ngx_module: ngx_module_t = ngx_module_t {
    ctx_index: ngx_uint_t::MAX,
    index: ngx_uint_t::MAX,
    name: std::ptr::null_mut(),
    spare0: 0,
    spare1: 0,
    version: nginx_version as ngx_uint_t,
    signature: NGX_RS_MODULE_SIGNATURE.as_ptr() as *const c_char,

    ctx: std::ptr::addr_of!(NGX_HTTP_ASYNC_NGX_MODULE_CTX).cast_mut().cast(),
    commands: unsafe { NGX_HTTP_ASYNC_NGX_COMMANDS.as_mut_ptr() },
    type_: NGX_HTTP_MODULE as ngx_uint_t,

    init_master: None,
    init_module: None,
    init_process: None,
    init_thread: None,
    exit_thread: None,
    exit_process: None,
    exit_master: None,

    spare_hook0: 0,
    spare_hook1: 0,
    spare_hook2: 0,
    spare_hook3: 0,
    spare_hook4: 0,
    spare_hook5: 0,
    spare_hook6: 0,
    spare_hook7: 0,
};

fn to_request_builder(request: &ngx::http::Request) -> Result<http::request::Builder> {
    let method = http::Method::try_from(request.method().as_str()).expect("supported method");

    // SAFETY: server config block for ngx_http_core_module always exists and is always ngx_http_core_srv_conf_t
    let cscf = unsafe {
        request
            .get_module_srv_conf::<ngx::ffi::ngx_http_core_srv_conf_t>(&*addr_of!(ngx::ffi::ngx_http_core_module))
            .unwrap()
    };

    let authority = if request.get_inner().headers_in.server.len > 0 {
        request.get_inner().headers_in.server
    } else {
        cscf.server_name
    };

    let scheme = if request.get_inner().schema.len > 0 {
        request.get_inner().schema.to_str()
    } else {
        "http"
    };

    ngx_log_debug_http!(request, "authority: {}", authority.to_str());

    let uri = request.unparsed_uri();
    let uri = http::Uri::builder()
        .scheme(scheme)
        .authority(authority.as_bytes())
        .path_and_query(uri.as_bytes())
        .build()?;

    let ver = match request.get_inner().http_version as u32 {
        ngx::ffi::NGX_HTTP_VERSION_9 => http::Version::HTTP_09,
        ngx::ffi::NGX_HTTP_VERSION_10 => http::Version::HTTP_10,
        ngx::ffi::NGX_HTTP_VERSION_11 => http::Version::HTTP_11,
        ngx::ffi::NGX_HTTP_VERSION_20 => http::Version::HTTP_2,
        ngx::ffi::NGX_HTTP_VERSION_30 => http::Version::HTTP_3,
        _ => unreachable!("unsupported HTTP version"),
    };

    let mut req = http::Request::builder().method(method).uri(uri).version(ver);
    for header in request.headers_in_iterator() {
        let hn = http::HeaderName::try_from(header.key.as_bytes())?;
        let hv = http::HeaderValue::try_from(header.value.as_bytes())?;
        req = req.header(hn, hv);
    }

    Ok(req)
}

extern "C" fn ngx_http_async_location_handler(r: *mut ngx_http_request_t) -> ngx_int_t {
    let r = unsafe { ngx::http::Request::from_ptr_mut(r) };
    let lcf = unsafe { r.get_module_loc_conf::<ModuleConfig>(&*addr_of!(ngx_http_async_ngx_module)) };
    let lcf = lcf.expect("module config is none");

    ngx_log_debug_http!(r, "async handler, enabled:{}", !lcf.endpoint.url.is_empty());

    if lcf.endpoint.url.is_empty() {
        return ngx::ffi::NGX_DECLINED as ngx_int_t;
    }

    unsafe { ngx::ffi::ngx_http_read_client_request_body(r.as_ptr(), Some(ngx_http_async_req_body_handler)) }
}

struct Timer {
    ev: ngx::core::Event,
    waker: Option<std::task::Waker>,
}

impl Timer {
    pub fn new(c: *mut ffi::ngx_connection_t) -> Self {
        let mut this = Self {
            ev: unsafe { std::mem::zeroed() },
            waker: None,
        };

        this.ev.data = c.cast();
        this.ev.log = unsafe { *c }.log;
        this.ev.set_cancelable(1);
        this.ev.handler = Some(Timer::timer_handler);

        this
    }

    unsafe extern "C" fn timer_handler(ev: *mut ffi::ngx_event_t) {
        let off = std::mem::offset_of!(Timer, ev) as isize;
        let timer = ev.offset(-off).cast::<Timer>();

        if let Some(waker) = (*timer).waker.take() {
            waker.wake();
        }
    }

    pub fn poll_sleep(
        self: &mut Pin<&mut Self>,
        duration: ffi::ngx_msec_t,
        context: &mut std::task::Context<'_>,
    ) -> Poll<Result<()>> {
        if self.ev.timedout() != 0 {
            Poll::Ready(Ok(()))
        } else if self.ev.timer_set() != 0 {
            Poll::Pending
        } else {
            self.ev.add_timer(duration);
            self.waker = Some(context.waker().clone());
            Poll::Pending
        }
    }
}

struct ModuleContext {
    #[allow(dead_code)]
    task: ngx_async::runtime::Task<()>,
}

unsafe extern "C" fn ngx_http_async_req_body_handler(r: *mut ngx_http_request_t) {
    let req = unsafe { ngx::http::Request::from_ptr_mut(r) };
    ngx_log_debug_http!(req, "async req body handler");

    let lcf = unsafe { req.get_module_loc_conf::<ModuleConfig>(&*addr_of!(ngx_http_async_ngx_module)) };
    let lcf = lcf.expect("module loc conf");

    if let Some(rb) = unsafe { (*req.as_ptr()).request_body.as_mut() } {
        unsafe {
            ffi::ngx_log_error_core(
                ffi::NGX_LOG_INFO as usize,
                req.log(),
                0,
                c"request body read: %p %p %L".as_ptr(),
                rb.buf,
                rb.bufs,
                rb.received,
            )
        };
    }

    let task = ngx_async::runtime::spawn(async move {
        let req = unsafe { ngx::http::Request::from_ptr_mut(r) };
        let pool = req.pool().as_ptr();
        let mut peer = Box::pin(NgxPeerConnection::default());

        if let Err(err) = async {
            std::future::poll_fn(|cx| peer.as_mut().poll_connect(pool, &lcf.endpoint, cx)).await?;
            let (mut sender, conn) = hyper::client::conn::http1::handshake(peer).await?;

            let _conn = ngx_async::runtime::spawn(async move {
                if let Err(err) = conn.await {
                    ngx_log_debug!((*pool).log, "connection failed: {:?}", err);
                }
            })?;

            /*
            ngx_log_debug_http!(req, "sleeping");

            let mut timer = std::pin::pin!(Timer::new(req.connection()));
            std::future::poll_fn(|cx| timer.poll_sleep(1000, cx)).await?;

            ngx_log_debug_http!(req, "sleep done");
            */

            let mut body = RequestBody::default();
            body.update(unsafe { &mut *req.as_ptr() });

            let http_req = to_request_builder(req)?.body(body)?;

            let response = sender.send_request(http_req).await?;

            let status = ngx::http::HTTPStatus::from_u16(response.status().as_u16()).expect("valid status code");
            req.set_status(status);

            for (name, value) in response.headers() {
                // always in lower case
                match name.as_str() {
                    "content-length" => {
                        let value = value.to_str().unwrap_or_default();
                        if let Ok(len) = value.parse::<usize>() {
                            req.set_content_length_n(len);
                        }
                    }
                    _ => {
                        req.add_header_out(name.as_str(), value);
                    }
                }
            }

            let rc = req.send_header();
            if rc == core::Status::NGX_ERROR || rc > core::Status::NGX_OK || req.header_only() {
                anyhow::bail!("header send failed");
            }

            use hyper::body::Body;

            let mut body = std::pin::pin!(response.into_body());

            while let Some(res) = std::future::poll_fn(|cx| body.as_mut().poll_frame(cx)).await {
                if let Ok(data) = res?.into_data() {
                    assert_eq!(write_buf(req, &data, body.is_end_stream()), 0);
                }
            }

            unsafe { ngx::ffi::ngx_http_finalize_request(req.as_ptr(), ngx::core::Status::NGX_OK.into()) };

            Ok::<_, anyhow::Error>(())
        }
        .await
        {
            ngx_log_debug_http!(req, "request failed: {:?}", err);
            unsafe {
                ngx::ffi::ngx_http_finalize_request(
                    req.as_ptr(),
                    ngx::http::HTTPStatus::INTERNAL_SERVER_ERROR.0 as ngx_int_t,
                )
            };
        }
    })
    .expect("task");

    if !task.is_finished() {
        let ctx = ModuleContext { task };
        let ctx = req.pool().allocate::<ModuleContext>(ctx);
        req.set_module_ctx(ctx.cast(), unsafe { &*addr_of!(ngx_http_async_ngx_module) });

        ngx_log_debug_http!(req, "save task to ctx");
    }
}

fn write_buf(r: &mut ngx::http::Request, data: &[u8], last: bool) -> isize {
    let buf = r.pool().alloc_type_zeroed::<ngx::ffi::ngx_buf_t>();
    let last = if last { 1 } else { 0 };
    unsafe {
        (*buf).set_memory(1);
        (*buf).set_last_buf(last);
        (*buf).set_last_in_chain(last);
        (*buf).start = data.as_ptr() as *mut u8;
        (*buf).end = (*buf).start.add(data.len());
        (*buf).pos = (*buf).start;
        (*buf).last = (*buf).end;
    }
    let mut chain = ngx_chain_t {
        buf,
        next: std::ptr::null_mut(),
    };
    unsafe {
        ngx::ffi::ngx_http_output_filter(r.as_ptr(), &mut chain);
    }

    unsafe { (*buf).last.offset_from((*buf).pos) }
}

struct NgxPeerConnection {
    pub pc: ffi::ngx_peer_connection_t,
    pub cev: Option<std::task::Waker>,
    pub rev: Option<std::task::Waker>,
    pub wev: Option<std::task::Waker>,
}

impl Default for NgxPeerConnection {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

unsafe extern "C" fn ngx_peer_conn_connect_handler(ev: *mut ffi::ngx_event_t) {
    let ev = ngx::core::EventRef::from_ptr_mut(ev);

    ngx_log_debug!(ev.log, "connect handler");

    let c: &mut ffi::ngx_connection_t = &mut *ev.data.cast();
    let this: &mut NgxPeerConnection = &mut *c.data.cast();

    if let Some(waker) = this.cev.take() {
        waker.wake();
    }
    /*
        if !ev.write {
            ffi::ngx_handle_read_event()
            ngx_handle_write_event
        }
    */
}

unsafe extern "C" fn ngx_peer_conn_read_handler(ev: *mut ffi::ngx_event_t) {
    ngx_log_debug!((*ev).log, "read handler");

    let c: *mut ffi::ngx_connection_t = (*ev).data.cast();
    let this: *mut NgxPeerConnection = (*c).data.cast();

    if let Some(waker) = (*this).rev.take() {
        waker.wake();
    }
}

unsafe extern "C" fn ngx_peer_conn_write_handler(ev: *mut ffi::ngx_event_t) {
    ngx_log_debug!((*ev).log, "write handler");

    let c: *mut ffi::ngx_connection_t = (*ev).data.cast();
    let this: *mut NgxPeerConnection = (*c).data.cast();

    if let Some(waker) = (*this).wev.take() {
        waker.wake();
    }
}

impl hyper::rt::Read for NgxPeerConnection {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        mut buf: hyper::rt::ReadBufCursor<'_>,
    ) -> Poll<std::result::Result<(), std::io::Error>> {
        let c = self.connection().unwrap();

        if c.read().timedout() != 0 {
            return Poll::Ready(Err(std::io::ErrorKind::TimedOut.into()));
        }

        let n = c.recv(unsafe { buf.as_mut() });

        if n == ffi::NGX_ERROR as isize {
            return Poll::Ready(Err(std::io::Error::last_os_error()));
        }

        let rev = c.read();

        if rev.handle_read(0).is_err() {
            return Poll::Ready(Err(std::io::ErrorKind::UnexpectedEof.into()));
        }

        if rev.active() != 0 {
            rev.add_timer(5000);
        } else if rev.timer_set() != 0 {
            rev.del_timer();
        }

        if n == ffi::NGX_AGAIN as isize {
            self.rev = Some(cx.waker().clone());
            return Poll::Pending;
        }

        if n > 0 {
            unsafe { buf.advance(n as _) };
        }

        Poll::Ready(Ok(()))
    }
}

impl hyper::rt::Write for NgxPeerConnection {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<std::result::Result<usize, std::io::Error>> {
        let c = self.connection().unwrap();
        let n = c.send(buf);

        ngx_log_debug!(c.log, "sent: {n}");

        if n == ffi::NGX_AGAIN as ngx_int_t {
            self.wev = Some(cx.waker().clone());
            Poll::Pending
        } else if n > 0 {
            Poll::Ready(Ok(n as usize))
        } else {
            Poll::Ready(Err(std::io::ErrorKind::UnexpectedEof.into()))
        }
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<std::result::Result<(), std::io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<std::result::Result<(), std::io::Error>> {
        if let Some(c) = self.connection() {
            c.shutdown(libc::SHUT_WR);
        }
        Poll::Ready(Ok(()))
    }
}

impl NgxPeerConnection {
    pub fn connect(&mut self, pool: *mut ffi::ngx_pool_t, url: &ngx_url_t) -> core::Status {
        let addrs = unsafe { std::slice::from_raw_parts(url.addrs, url.naddrs) };
        assert!(!addrs.is_empty());

        let pc = &mut self.pc;

        pc.sockaddr = addrs[0].sockaddr;
        pc.socklen = addrs[0].socklen;
        pc.name = std::ptr::addr_of!(addrs[0].name).cast_mut();
        pc.get = Some(ffi::ngx_event_get_peer);
        pc.log = unsafe { *pool }.log;
        pc.set_log_error(1); // FIXME

        let rc = unsafe { ffi::ngx_event_connect_peer(pc) };
        let rc = core::Status(rc);

        if rc == core::Status::NGX_ERROR || rc == core::Status::NGX_BUSY || rc == core::Status::NGX_DECLINED {
            ngx_log_debug!((*pool).log, "connect failed");
            return rc;
        }

        let c = unsafe { &mut *pc.connection };
        c.pool = pool;
        c.data = std::ptr::from_mut(self).cast();

        unsafe { *c.read }.handler = Some(ngx_peer_conn_read_handler);
        unsafe { *c.write }.handler = Some(ngx_peer_conn_write_handler);

        rc
    }

    pub fn poll_connect(
        self: &mut Pin<&mut Self>,
        pool: *mut ffi::ngx_pool_t,
        url: &ngx_url_t,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        if let Some(c) = self.connection() {
            ngx_log_debug!(c.log, "connect callback");

            if c.read().timedout() != 0 || c.write().timedout() != 0 {
                c.close();
                return Poll::Ready(Err(std::io::ErrorKind::TimedOut.into()));
            }

            if let Err(err) = c.test_connect() {
                return Poll::Ready(Err(std::io::Error::from_raw_os_error(err)));
            }

            c.read().handler = Some(ngx_peer_conn_read_handler);
            c.write().handler = Some(ngx_peer_conn_write_handler);

            return Poll::Ready(Ok(()));
        }

        match self.connect(pool, url) {
            core::Status::NGX_OK => Poll::Ready(Ok(())),
            core::Status::NGX_ERROR | core::Status::NGX_BUSY | core::Status::NGX_DECLINED => {
                Poll::Ready(Err(std::io::ErrorKind::ConnectionRefused.into()))
            }
            core::Status::NGX_AGAIN => {
                let c = self.connection().unwrap();
                ngx_log_debug!(c.log, "connect returned NGX_AGAIN");
                c.read().handler = Some(ngx_peer_conn_connect_handler);
                c.read().set_timer_set(5000);
                c.write().handler = Some(ngx_peer_conn_connect_handler);
                self.cev = Some(cx.waker().clone());

                Poll::Pending
            }
            _ => unreachable!("should not be here"),
        }
    }

    pub fn connection(&mut self) -> Option<&mut ngx::core::Connection> {
        if self.pc.connection.is_null() {
            None
        } else {
            Some(unsafe { ngx::core::Connection::from_ptr_mut(self.pc.connection) })
        }
    }
}

impl Drop for NgxPeerConnection {
    fn drop(&mut self) {
        ngx_log_debug!((*ffi::ngx_cycle).log, "closing peer connection");
        if let Some(c) = self.connection() {
            c.close()
        }
    }
}

extern "C" fn ngx_http_async_commands_async_pass(
    cf: *mut ngx_conf_t,
    _cmd: *mut ngx_command_t,
    conf: *mut c_void,
) -> *mut c_char {
    let clcf = unsafe { &mut *ngx_http_conf_get_module_loc_conf(cf, &*addr_of!(ngx_http_core_module)) };
    let lcf = unsafe { &mut *(conf as *mut ModuleConfig) };
    let cf = unsafe { NgxConfRef::from_ptr(cf) };

    if !lcf.endpoint.url.is_empty() {
        return "is duplicate\0".as_ptr() as *const c_char as *mut _;
    }

    clcf.handler = Some(ngx_http_async_location_handler);

    lcf.endpoint.url = *cf.args()[1].as_ref();
    lcf.endpoint.default_port = 80;

    let rc = unsafe { ffi::ngx_parse_url(cf.pool, &mut lcf.endpoint) };
    if rc != ffi::NGX_OK as isize && !lcf.endpoint.err.is_null() {
        unsafe {
            ffi::ngx_conf_log_error(
                ffi::NGX_LOG_EMERG as usize,
                cf.as_ptr(),
                0,
                c"%s in resolver \"%V\"".as_ptr(),
                lcf.endpoint.err,
                &lcf.endpoint.url,
            )
        };

        return core::NGX_CONF_ERROR as _;
    }

    std::ptr::null_mut()
}