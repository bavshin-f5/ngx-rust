use crate::ffi::*;

/// HTTP module configuration type.
/// See <http://nginx.org/en/docs/dev/development_guide.html#http_conf>
pub enum HttpModuleConfType {
    /// Main configuration — Applies to the entire `http` block. Functions as global settings for a module.
    Main,
    /// Server configuration — Applies to a single `server` block. Functions as server-specific settings for a module.
    Server,
    /// Location configuration — Applies to a single `location`, `if` or `limit_except` block. Functions as location-specific settings for a module.
    Location,
}

/// Utility trait for the module configuration objects
/// Implement this for your type if you want to use type-safe configuration access methods.
pub trait HttpModuleConf {
    /// The configuration type for this struct
    const CONTEXT: HttpModuleConfType;
    /// The module owning this configuration object
    fn module() -> &'static ngx_module_t;
}

/// Declare a configuration type for the HTTP module and context
#[macro_export]
macro_rules! ngx_http_module_conf {
    ( $context: ident, $module:expr, $type: ty) => {
        impl $crate::http::HttpModuleConf for $type {
            const CONTEXT: $crate::http::HttpModuleConfType = $crate::http::HttpModuleConfType::$context;

            fn module() -> &'static $crate::ffi::ngx_module_t {
                #[allow(clippy::macro_metavars_in_unsafe)]
                unsafe {
                    &*::core::ptr::addr_of!($module)
                }
            }
        }
    };
}

ngx_http_module_conf!(Main, ngx_http_core_module, ngx_http_core_main_conf_t);
ngx_http_module_conf!(Server, ngx_http_core_module, ngx_http_core_srv_conf_t);
ngx_http_module_conf!(Location, ngx_http_core_module, ngx_http_core_loc_conf_t);
#[cfg(ngx_feature = "http_ssl")]
ngx_http_module_conf!(Server, ngx_http_ssl_module, ngx_http_ssl_srv_conf_t);
ngx_http_module_conf!(Main, ngx_http_upstream_module, ngx_http_upstream_main_conf_t);
ngx_http_module_conf!(Server, ngx_http_upstream_module, ngx_http_upstream_srv_conf_t);
#[cfg(ngx_feature = "http_v2")]
ngx_http_module_conf!(Server, ngx_http_v2_module, ngx_http_v2_srv_conf_t);
#[cfg(ngx_feature = "http_v3")]
ngx_http_module_conf!(Server, ngx_http_v3_module, ngx_http_v3_srv_conf_t);

/// Utility trait for types containing HTTP module configuration
pub trait NgxHttpConfExt {
    /// Get a configuration structure for HTTP module
    fn get_http_module_conf<T: HttpModuleConf>(&self) -> Option<&'static T> {
        unsafe { self.get_http_module_conf_unchecked(T::CONTEXT, T::module()) }
    }
    /// Get a mutable reference to the configuration structure for HTTP module
    fn get_http_module_conf_mut<T: HttpModuleConf>(&self) -> Option<&'static mut T> {
        unsafe { self.get_http_module_conf_mut_unchecked(T::CONTEXT, T::module()) }
    }
    /// Get a configuration structure for HTTP module
    ///
    /// # Safety
    /// Caller must ensure that type `T` matches the configuration type for the specified module
    /// and context.
    unsafe fn get_http_module_conf_unchecked<T>(
        &self,
        context: HttpModuleConfType,
        module: &ngx_module_t,
    ) -> Option<&'static T>;
    /// Get a mutable reference to the configuration structure for HTTP module
    ///
    /// # Safety
    /// Caller must ensure that type `T` matches the configuration type for the specified module
    /// and context.
    unsafe fn get_http_module_conf_mut_unchecked<T>(
        &self,
        context: HttpModuleConfType,
        module: &ngx_module_t,
    ) -> Option<&'static mut T>;
}

impl NgxHttpConfExt for crate::ffi::ngx_conf_t {
    unsafe fn get_http_module_conf_unchecked<T>(
        &self,
        context: HttpModuleConfType,
        module: &ngx_module_t,
    ) -> Option<&'static T> {
        let conf_ctx = self.ctx.cast::<ngx_http_conf_ctx_t>();
        let conf_ctx = unsafe { conf_ctx.as_ref()? };

        let conf = match context {
            HttpModuleConfType::Main => conf_ctx.main_conf,
            HttpModuleConfType::Server => conf_ctx.srv_conf,
            HttpModuleConfType::Location => conf_ctx.loc_conf,
        };

        unsafe { (*conf.add(module.ctx_index)).cast::<T>().as_ref() }
    }

    unsafe fn get_http_module_conf_mut_unchecked<T>(
        &self,
        context: HttpModuleConfType,
        module: &ngx_module_t,
    ) -> Option<&'static mut T> {
        let conf_ctx = self.ctx.cast::<ngx_http_conf_ctx_t>();
        let conf_ctx = unsafe { conf_ctx.as_mut()? };

        let conf = match context {
            HttpModuleConfType::Main => conf_ctx.main_conf,
            HttpModuleConfType::Server => conf_ctx.srv_conf,
            HttpModuleConfType::Location => conf_ctx.loc_conf,
        };

        unsafe { (*conf.add(module.ctx_index)).cast::<T>().as_mut() }
    }
}

impl NgxHttpConfExt for ngx_http_upstream_srv_conf_t {
    unsafe fn get_http_module_conf_unchecked<T>(
        &self,
        context: HttpModuleConfType,
        module: &ngx_module_t,
    ) -> Option<&'static T> {
        let conf = match context {
            HttpModuleConfType::Server => self.srv_conf,
            _ => unreachable!(),
        };

        if conf.is_null() {
            return None;
        }

        unsafe { (*conf.add(module.ctx_index)).cast::<T>().as_ref() }
    }

    unsafe fn get_http_module_conf_mut_unchecked<T>(
        &self,
        context: HttpModuleConfType,
        module: &ngx_module_t,
    ) -> Option<&'static mut T> {
        let conf = match context {
            HttpModuleConfType::Server => self.srv_conf,
            _ => unreachable!(),
        };

        if conf.is_null() {
            return None;
        }

        unsafe { (*conf.add(module.ctx_index)).cast::<T>().as_mut() }
    }
}
