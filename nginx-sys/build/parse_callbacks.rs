use bindgen::callbacks::IntKind;

#[derive(Debug)]
pub struct NginxParseCallbacks;

impl bindgen::callbacks::ParseCallbacks for NginxParseCallbacks {
    fn int_macro(&self, name: &str, value: i64) -> Option<IntKind> {
        INT_MACRO_RULES.get(name, value)
    }
}

enum NamePattern {
    Any,
    Exact(&'static str),
    Prefix(&'static str),
    Suffix(&'static str),
}

impl NamePattern {
    pub fn matches<'a>(&self, name: &'a str) -> Option<&'a str> {
        match self {
            NamePattern::Any => Some(name),
            NamePattern::Exact(x) if name == *x => Some(""),
            NamePattern::Exact(_) => None,
            NamePattern::Prefix(x) => name.strip_prefix(x),
            NamePattern::Suffix(x) => name.strip_suffix(x),
        }
    }
}

enum IntMacroItem {
    Kind(IntKind),
    Items(&'static [(NamePattern, IntMacroItem)]),
    Func(fn(i64) -> Option<IntKind>),
}

impl IntMacroItem {
    pub fn get(&self, name: &str, value: i64) -> Option<IntKind> {
        match self {
            Self::Kind(x) => Some(*x),
            Self::Func(f) => f(value),
            Self::Items(items) => {
                for (pattern, item) in items.iter() {
                    if let Some(kind) = pattern.matches(name).and_then(|x| item.get(x, value)) {
                        return Some(kind);
                    }
                }
                None
            }
        }
    }
}

static TARGET_BITS: std::sync::LazyLock<u32> = std::sync::LazyLock::new(|| {
    std::env::var("CARGO_CFG_TARGET_POINTER_WIDTH")
        .expect("always set for buildscripts")
        .parse::<u32>()
        .expect("an integer value")
});

/*
 * Type mappings for NGX_ macros from the public headers.
 * The list doesn't have to be exhaustive, but it should be good enough to call
 * common functions without type casts.
 *
 * Note that some definitions can be referenced in different contexts;
 * for example, HTTP status codes can be used both as ngx_int_t and ngx_uint_t.
 */
const INT_MACRO_RULES: IntMacroItem = const {
    use IntMacroItem::*;
    use NamePattern::*;

    // ngx_err_t is unsigned on Windows, but it shouldn't matter as all the values are unsigned too.
    const NGX_ERR_T: IntKind = IntKind::Custom { name: "ngx_err_t", is_signed: true };
    const NGX_FD_T: IntKind = IntKind::Custom { name: "ngx_fd_t", is_signed: true };
    const NGX_INT_T: IntKind = IntKind::Custom { name: "ngx_int_t", is_signed: true };
    const NGX_MSEC_T: IntKind = IntKind::Custom { name: "ngx_msec_t", is_signed: false };
    const NGX_PID_T: IntKind = IntKind::Custom { name: "ngx_pid_t", is_signed: true };
    const NGX_UINT_T: IntKind = IntKind::Custom { name: "ngx_uint_t", is_signed: false };
    const OFF_T: IntKind = IntKind::Custom { name: "off_t", is_signed: true };
    const SIZE_T: IntKind = IntKind::Custom { name: "usize", is_signed: false };
    const SSIZE_T: IntKind = IntKind::Custom { name: "isize", is_signed: true };
    const TIME_T: IntKind = IntKind::Custom { name: "time_t", is_signed: true };

    /*
     * During the lookup, all the rules are tried in order until the first match.
     * When descending into a subtree via prefix or suffix rule, the already matched part is
     * removed from the string (e.g. NGX_MAX_INT_T_VALUE -> MAX_INT_T_VALUE -> INT_T_VALUE).
     */

    Items(&[
        (Exact("nginx_version"), Kind(NGX_UINT_T)),
        (
            Prefix("NGX_"),
            Items(&[
                (Exact("OK"), Kind(NGX_INT_T)),
                (Exact("ERROR"), Kind(NGX_INT_T)),
                (Exact("AT_FDCWD"), Kind(NGX_FD_T)),
                (Exact("INVALID_FD"), Kind(NGX_FD_T)),
                (Exact("INVALID_PID"), Kind(NGX_PID_T)),
                (Exact("FILE_ERROR"), Kind(IntKind::Int)),
                (Exact("MODULE_UNSET_INDEX"), Kind(NGX_UINT_T)),
                (Exact("OPEN_FILE_DIRECTIO_OFF"), Kind(OFF_T)),
                (Exact("TIMER_INFINITE"), Kind(NGX_MSEC_T)),
                (Prefix("DISABLE_SYMLINKS_"), Kind(IntKind::UInt)),
                // NGX_ESCAPE_* in ngx_string.h
                (Prefix("ESCAPE_"), Kind(NGX_UINT_T)),
                (
                    Prefix("MAX_"),
                    Items(&[
                        (Exact("INT_T_VALUE"), Kind(NGX_INT_T)),
                        (Exact("INT32_VALUE"), Kind(IntKind::I32)),
                        (Exact("OFF_T_VALUE"), Kind(OFF_T)),
                        (Exact("SIZE_T_VALUE"), Kind(SIZE_T)),
                        (Exact("TIME_T_VALUE"), Kind(TIME_T)),
                        (Exact("UINT32_VALUE"), Kind(IntKind::U32)),
                    ]),
                ),
                (Prefix("LOG_"), Kind(NGX_UINT_T)),
                // NGX_*_CONF, ngx_command_t.type
                (Suffix("_CONF"), Kind(NGX_UINT_T)),
                // NGX_*_EVENT
                (
                    Suffix("_EVENT"),
                    Items(&[
                        // ngx_int_t event
                        (Exact("READ"), Kind(NGX_INT_T)),
                        (Exact("VNODE"), Kind(NGX_INT_T)),
                        (Exact("WRITE"), Kind(NGX_INT_T)),
                        // HAVE_, USE_, flags
                        (Any, Kind(NGX_UINT_T)),
                    ]),
                ),
                (Suffix("_MODULE"), Kind(NGX_UINT_T)),
                // NGX_{READ,WRITE,RDWR}_SHUTDOWN
                (Suffix("_SHUTDOWN"), Kind(IntKind::Int)),
                // Likely error codes from ngx_errno.h.
                // Anything that is not should be added above.
                (Prefix("E"), Kind(NGX_ERR_T)),
                (
                    Prefix("CONF_"),
                    Items(&[
                        (Exact("BLOCK_START"), Kind(NGX_INT_T)),
                        (Exact("BLOCK_DONE"), Kind(NGX_INT_T)),
                        (Exact("FILE_DONE"), Kind(NGX_INT_T)),
                        (Exact("UNSET_UINT"), Kind(NGX_UINT_T)),
                        (Exact("UNSET_SIZE"), Kind(SIZE_T)),
                        (Exact("UNSET_MSEC"), Kind(NGX_MSEC_T)),
                    ]),
                ),
                (
                    Prefix("SSL_"),
                    Items(&[
                        // ssize_t builtin_session_cache
                        (Suffix("_SCACHE"), Kind(SSIZE_T)),
                    ]),
                ),
                (
                    Prefix("HTTP_"),
                    Items(&[
                        (Exact("MAX_BLOCKED"), Kind(NGX_UINT_T)),
                        (Exact("COPY"), Kind(NGX_UINT_T)),
                        (Exact("MOVE"), Kind(NGX_UINT_T)),
                        (Exact("OPTIONS"), Kind(NGX_UINT_T)),
                        (Exact("UPSTREAM_EARLY_HINTS"), Kind(NGX_INT_T)),
                        (Exact("UPSTREAM_INVALID_HEADER"), Kind(NGX_INT_T)),
                        (Exact("V2_ENCODE_HUFF"), Kind(IntKind::UChar)),
                        (Prefix("CACHE_"), Kind(IntKind::UInt)),
                        (Prefix("GZIP_PROXIED_"), Kind(NGX_UINT_T)),
                        (Prefix("UPSTREAM_"), Kind(NGX_UINT_T)),
                        // Used as ngx_uint_t when passing to ngx_quic_finalize_connection
                        // and as ngx_int_t for various h3 method return values.
                        (Prefix("V3_ERR_"), Kind(NGX_UINT_T)),
                        (Suffix("_BUFFERED"), Kind(IntKind::UInt)),
                        // Likely status codes (ngx_int_t).
                        // If something matching is not, add to the list of exceptions above.
                        (Any, Func(|x| (100..600).contains(&x).then_some(NGX_INT_T))),
                    ]),
                ),
                // NGX_HTTPS_NO_CERT, NGX_HTTPS_CERT_ERROR, ...
                (Prefix("HTTPS_"), Kind(NGX_INT_T)),
                (
                    Prefix("RESOLVE_"),
                    Items(&[
                        (Exact("FORMERR"), Kind(NGX_INT_T)),
                        (Exact("SERVFAIL"), Kind(NGX_INT_T)),
                        (Exact("NXDOMAIN"), Kind(NGX_INT_T)),
                        (Exact("NOTIMP"), Kind(NGX_INT_T)),
                        (Exact("REFUSED"), Kind(NGX_INT_T)),
                        (Exact("TIMEDOUT"), Kind(NGX_INT_T)),
                    ]),
                ),
                (
                    Prefix("STREAM_"),
                    Items(&[
                        (Prefix("UPSTREAM_"), Kind(NGX_UINT_T)),
                        (Suffix("_BUFFERED"), Kind(IntKind::UInt)),
                        // Likely status codes (ngx_int_t).
                        // If something matching is not, add to the list of exceptions above.
                        (Any, Func(|x| (400..600).contains(&x).then_some(NGX_INT_T))),
                    ]),
                ),
                // Fallback behavior for NGX_ constants: try to extend to ngx_int_t/ngx_uint_t,
                // depending on the sign and value
                (
                    Any,
                    Func(|x| {
                        // On 32-bit platforms, 64-bit constants (off_t, time_t, ...) can exceed
                        // ngx_(u)int_t limits. Let bindgen assign types for these.
                        if *TARGET_BITS == 32 && (x < i32::MIN as i64 || x > u32::MAX as i64) {
                            return None;
                        }
                        Some(if x < 0 { NGX_INT_T } else { NGX_UINT_T })
                    }),
                ),
            ]),
        ),
    ])
};
