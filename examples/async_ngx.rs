#![allow(unused)]

use core::ffi::{c_char, c_void};
use core::future;
use core::pin::{pin, Pin};
use core::ptr::{self, addr_of};
use core::task::Poll;
use core::{mem, slice, str};
use std::io;

use ngx::async_::{self as ngx_async, http::RequestBody};
use ngx::core::Status;
use ngx::ffi::{
    self, nginx_version, ngx_chain_t, ngx_command_t, ngx_conf_t, ngx_http_core_module,
    ngx_http_module_t, ngx_http_request_t, ngx_int_t, ngx_module_t, ngx_uint_t, ngx_url_t,
    NGX_CONF_TAKE1, NGX_HTTP_LOC_CONF, NGX_HTTP_LOC_CONF_OFFSET, NGX_HTTP_MODULE,
    NGX_RS_MODULE_SIGNATURE,
};
use ngx::http::{
    HttpModule, HttpModuleLocationConf, HttpModuleServerConf, MergeConfigError, NgxHttpCoreModule,
};
use ngx::{ngx_log_debug, ngx_log_debug_http, ngx_string};

type BoxError = Box<dyn core::error::Error>;

struct Module;

impl ngx::http::HttpModule for Module {
    fn module() -> &'static ngx_module_t {
        unsafe { &*ptr::addr_of!(ngx_http_async_ngx_module) }
    }
}

unsafe impl HttpModuleLocationConf for Module {
    type LocationConf = ModuleConfig;
}

struct ModuleConfig {
    endpoint: ngx_url_t,
}

impl Default for ModuleConfig {
    fn default() -> Self {
        ModuleConfig {
            endpoint: unsafe { mem::zeroed() },
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
        conf: NGX_HTTP_LOC_CONF_OFFSET,
        offset: 0,
        post: ptr::null_mut(),
    },
    ngx_command_t::empty(),
];

static NGX_HTTP_ASYNC_NGX_MODULE_CTX: ngx_http_module_t = ngx_http_module_t {
    preconfiguration: Some(Module::preconfiguration),
    postconfiguration: Some(Module::postconfiguration),
    create_main_conf: None,
    init_main_conf: None,
    create_srv_conf: None,
    merge_srv_conf: None,
    create_loc_conf: Some(Module::create_loc_conf),
    merge_loc_conf: Some(Module::merge_loc_conf),
};

// Generate the `ngx_modules` table with exported modules.
// This feature is required to build a 'cdylib' dynamic module outside of the NGINX buildsystem.
#[cfg(feature = "export-modules")]
ngx::ngx_modules!(ngx_http_async_ngx_module);

#[used]
#[allow(non_upper_case_globals)]
#[cfg_attr(not(feature = "export-modules"), no_mangle)]
pub static mut ngx_http_async_ngx_module: ngx_module_t = ngx_module_t {
    ctx: ptr::addr_of!(NGX_HTTP_ASYNC_NGX_MODULE_CTX)
        .cast_mut()
        .cast(),
    #[allow(static_mut_refs)]
    commands: unsafe { NGX_HTTP_ASYNC_NGX_COMMANDS.as_mut_ptr() },
    type_: NGX_HTTP_MODULE as ngx_uint_t,

    ..ngx_module_t::default()
};

fn to_request_builder(request: &ngx::http::Request) -> Result<http::request::Builder, http::Error> {
    let method = http::Method::try_from(request.method().as_str()).expect("supported method");
    let cscf = NgxHttpCoreModule::server_conf(request).expect("http core server conf");

    let authority = if request.as_ref().headers_in.server.len > 0 {
        request.as_ref().headers_in.server
    } else {
        cscf.server_name
    };

    let scheme = if request.as_ref().schema.len > 0 {
        request.as_ref().schema.to_str()
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

    let ver = match request.as_ref().http_version as u32 {
        ngx::ffi::NGX_HTTP_VERSION_9 => http::Version::HTTP_09,
        ngx::ffi::NGX_HTTP_VERSION_10 => http::Version::HTTP_10,
        ngx::ffi::NGX_HTTP_VERSION_11 => http::Version::HTTP_11,
        ngx::ffi::NGX_HTTP_VERSION_20 => http::Version::HTTP_2,
        ngx::ffi::NGX_HTTP_VERSION_30 => http::Version::HTTP_3,
        _ => unreachable!("unsupported HTTP version"),
    };

    let mut req = http::Request::builder()
        .method(method)
        .uri(uri)
        .version(ver);
    for (key, value) in request.headers_in_iterator() {
        let hn = http::HeaderName::try_from(key.as_bytes())?;
        let hv = http::HeaderValue::try_from(value.as_bytes())?;
        req = req.header(hn, hv);
    }

    Ok(req)
}

extern "C" fn ngx_http_async_location_handler(r: *mut ngx_http_request_t) -> ngx_int_t {
    let r = unsafe { ngx::http::Request::from_ngx_http_request(r) };
    let lcf = Module::location_conf(r).expect("module location conf");

    ngx_log_debug_http!(r, "async handler, enabled:{}", !lcf.endpoint.url.is_empty());

    if lcf.endpoint.url.is_empty() {
        return ngx::ffi::NGX_DECLINED as ngx_int_t;
    }

    unsafe {
        ngx::ffi::ngx_http_read_client_request_body(
            r.as_mut(),
            Some(ngx_http_async_req_body_handler),
        )
    }
}

struct ModuleContext {
    #[allow(dead_code)]
    task: ngx_async::Task<()>,
}

unsafe extern "C" fn ngx_http_async_req_body_handler(r: *mut ngx_http_request_t) {
    let req = unsafe { ngx::http::Request::from_ngx_http_request(r) };
    ngx_log_debug_http!(req, "async req body handler");
    let lcf = Module::location_conf(req).expect("module location conf");

    if let Some(rb) = unsafe { req.as_ref().request_body.as_mut() } {
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

    let task = ngx_async::spawn(async move {
        let req = unsafe { ngx::http::Request::from_ngx_http_request(r) };
        let mut pool = req.pool();
        let mut peer = Box::pin(NgxPeerConnection::default());

        if let Err(err) = async {
            future::poll_fn(|cx| peer.as_mut().poll_connect(pool.as_mut(), &lcf.endpoint, cx))
                .await?;
            let (mut sender, conn) = hyper::client::conn::http1::handshake(peer).await?;

            ngx_async::spawn(async move {
                if let Err(err) = conn.await {
                    ngx_log_debug!(pool.as_mut().log, "connection failed: {:?}", err);
                }
            })
            .detach();

            ngx_log_debug_http!(req, "sleeping");
            ngx::async_::sleep(core::time::Duration::from_secs(1)).await;
            ngx_log_debug_http!(req, "sleep done");

            let mut body = RequestBody::default();
            body.update(req.as_mut());

            let http_req = to_request_builder(req)?.body(body)?;

            let response = sender.send_request(http_req).await?;

            let status = ngx::http::HTTPStatus::from_u16(response.status().as_u16())
                .expect("valid status code");
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
                        req.add_header_out(
                            name.as_str(),
                            str::from_utf8_unchecked(value.as_bytes()),
                        );
                    }
                }
            }

            let rc = req.send_header();
            if rc == Status::NGX_ERROR || rc > Status::NGX_OK || req.header_only() {
                return Err("header send failed".into());
            }

            use hyper::body::Body;

            let mut body = pin!(response.into_body());

            while let Some(res) = future::poll_fn(|cx| body.as_mut().poll_frame(cx)).await {
                if let Ok(data) = res?.into_data() {
                    assert_eq!(write_buf(req, &data, body.is_end_stream()), 0);
                }
            }

            unsafe {
                ngx::ffi::ngx_http_finalize_request(req.as_mut(), ngx::core::Status::NGX_OK.into())
            };

            Ok::<_, BoxError>(())
        }
        .await
        {
            ngx_log_debug_http!(req, "request failed: {:?}", err);
            unsafe {
                ngx::ffi::ngx_http_finalize_request(
                    req.as_mut(),
                    ngx::http::HTTPStatus::INTERNAL_SERVER_ERROR.0 as ngx_int_t,
                )
            };
        }
    });

    if !task.is_finished() {
        let ctx = ModuleContext { task };
        let ctx = req.pool().allocate::<ModuleContext>(ctx);
        req.set_module_ctx(ctx.cast(), Module::module());

        ngx_log_debug_http!(req, "save task to ctx");
    }
}

fn write_buf(r: &mut ngx::http::Request, data: &[u8], last: bool) -> isize {
    let buf = r.pool().calloc_type::<ngx::ffi::ngx_buf_t>();
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
        next: ptr::null_mut(),
    };
    unsafe {
        ngx::ffi::ngx_http_output_filter(r.as_mut(), &mut chain);
    }

    unsafe { (*buf).last.offset_from((*buf).pos) }
}

struct NgxPeerConnection {
    pub pc: ffi::ngx_peer_connection_t,
    pub cev: Option<core::task::Waker>,
    pub rev: Option<core::task::Waker>,
    pub wev: Option<core::task::Waker>,
}

impl Default for NgxPeerConnection {
    fn default() -> Self {
        unsafe { mem::zeroed() }
    }
}

unsafe extern "C" fn ngx_peer_conn_connect_handler(ev: *mut ffi::ngx_event_t) {
    ngx_log_debug!((*ev).log, "connect handler");

    let c: &mut ffi::ngx_connection_t = &mut *(*ev).data.cast();
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
        cx: &mut core::task::Context<'_>,
        mut buf: hyper::rt::ReadBufCursor<'_>,
    ) -> Poll<core::result::Result<(), io::Error>> {
        let c = self.connection().unwrap();

        if c.read().timedout() != 0 {
            return Poll::Ready(Err(io::ErrorKind::TimedOut.into()));
        }

        let n = c.recv(unsafe { buf.as_mut() });

        if n == ffi::NGX_ERROR as isize {
            return Poll::Ready(Err(io::Error::last_os_error()));
        }

        let rev = c.read();

        if unsafe { ffi::ngx_handle_read_event(rev, 0) } != (ffi::NGX_OK as _) {
            return Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into()));
        }

        if rev.active() != 0 {
            unsafe { ffi::ngx_add_timer(rev, 5000) };
        } else if rev.timer_set() != 0 {
            unsafe { ffi::ngx_del_timer(rev) };
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
        cx: &mut core::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<core::result::Result<usize, io::Error>> {
        let c = self.connection().unwrap();
        let n = c.send(buf);

        ngx_log_debug!(c.log, "sent: {n}");

        if n == ffi::NGX_AGAIN as ngx_int_t {
            self.wev = Some(cx.waker().clone());
            Poll::Pending
        } else if n > 0 {
            Poll::Ready(Ok(n as usize))
        } else {
            Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into()))
        }
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        _cx: &mut core::task::Context<'_>,
    ) -> Poll<core::result::Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        _cx: &mut core::task::Context<'_>,
    ) -> Poll<core::result::Result<(), io::Error>> {
        if let Some(c) = self.connection() {
            c.shutdown(libc::SHUT_WR);
        }
        Poll::Ready(Ok(()))
    }
}

impl NgxPeerConnection {
    pub fn connect(&mut self, pool: *mut ffi::ngx_pool_t, url: &ngx_url_t) -> Status {
        assert!(url.naddrs > 0);
        let addrs = unsafe { slice::from_raw_parts(url.addrs, url.naddrs) };

        let pc = &mut self.pc;

        pc.sockaddr = addrs[0].sockaddr;
        pc.socklen = addrs[0].socklen;
        pc.name = ptr::addr_of!(addrs[0].name).cast_mut();
        pc.get = Some(ffi::ngx_event_get_peer);
        pc.log = unsafe { *pool }.log;
        pc.set_log_error(1); // FIXME

        let rc = unsafe { ffi::ngx_event_connect_peer(pc) };
        let rc = Status(rc);

        if rc == Status::NGX_ERROR || rc == Status::NGX_BUSY || rc == Status::NGX_DECLINED {
            ngx_log_debug!(unsafe { (*pool).log }, "connect failed");
            return rc;
        }

        let c = unsafe { &mut *pc.connection };
        c.pool = pool;
        c.data = ptr::from_mut(self).cast();

        unsafe { *c.read }.handler = Some(ngx_peer_conn_read_handler);
        unsafe { *c.write }.handler = Some(ngx_peer_conn_write_handler);

        rc
    }

    pub fn poll_connect(
        self: &mut Pin<&mut Self>,
        pool: *mut ffi::ngx_pool_t,
        url: &ngx_url_t,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<core::result::Result<(), io::Error>> {
        if let Some(c) = self.connection() {
            ngx_log_debug!(c.log, "connect callback");

            if c.read().timedout() != 0 || c.write().timedout() != 0 {
                c.close();
                return Poll::Ready(Err(io::ErrorKind::TimedOut.into()));
            }

            if let Err(err) = c.test_connect() {
                return Poll::Ready(Err(io::Error::from_raw_os_error(err)));
            }

            c.read().handler = Some(ngx_peer_conn_read_handler);
            c.write().handler = Some(ngx_peer_conn_write_handler);

            return Poll::Ready(Ok(()));
        }

        match self.connect(pool, url) {
            Status::NGX_OK => Poll::Ready(Ok(())),
            Status::NGX_ERROR | Status::NGX_BUSY | Status::NGX_DECLINED => {
                Poll::Ready(Err(io::ErrorKind::ConnectionRefused.into()))
            }
            Status::NGX_AGAIN => {
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
        ngx_log_debug!(unsafe { (*ffi::ngx_cycle).log }, "closing peer connection");
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
    let cf = unsafe { &mut *cf };
    let clcf = NgxHttpCoreModule::location_conf_mut(cf).expect("http core module location conf");
    let lcf = unsafe { &mut *(conf as *mut ModuleConfig) };

    if !lcf.endpoint.url.is_empty() {
        return c"is duplicate".as_ptr().cast_mut();
    }

    clcf.handler = Some(ngx_http_async_location_handler);

    let args =
        unsafe { slice::from_raw_parts((*cf.args).elts as *mut ffi::ngx_str_t, (*cf.args).nelts) };

    lcf.endpoint.url = args[1];
    lcf.endpoint.default_port = 80;

    let rc = unsafe { ffi::ngx_parse_url(cf.pool, &mut lcf.endpoint) };
    if rc != ffi::NGX_OK as isize && !lcf.endpoint.err.is_null() {
        unsafe {
            ffi::ngx_conf_log_error(
                ffi::NGX_LOG_EMERG as usize,
                cf,
                0,
                c"%s in resolver \"%V\"".as_ptr(),
                lcf.endpoint.err,
                &lcf.endpoint.url,
            )
        };

        return ngx::core::NGX_CONF_ERROR;
    }

    ngx::core::NGX_CONF_OK
}
