use std::ops::{Deref, DerefMut};

use foreign_types::ForeignTypeRef;

use crate::ffi::{ngx_hash_strlow, ngx_pnalloc, ngx_pool_t, ngx_str_t, ngx_table_elt_t};

foreign_types::foreign_type! {
    /// Wrapper struct for an [`ngx_table_elt_t`]
    pub unsafe type NgxTableElement: Send {
        type CType = ngx_table_elt_t;
        // No cleanup required for pool-allocated structs
        fn drop = |_|();
    }
}

impl AsRef<ngx_table_elt_t> for NgxTableElementRef {
    fn as_ref(&self) -> &ngx_table_elt_t {
        unsafe { &*self.as_ptr() }
    }
}

impl AsMut<ngx_table_elt_t> for NgxTableElementRef {
    fn as_mut(&mut self) -> &mut ngx_table_elt_t {
        unsafe { &mut *self.as_ptr() }
    }
}

impl Deref for NgxTableElementRef {
    type Target = ngx_table_elt_t;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.as_ptr() }
    }
}

impl DerefMut for NgxTableElementRef {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.as_ptr() }
    }
}

impl NgxTableElementRef {
    /// Table element key in lower case
    pub fn lowcase_key(&self) -> &[u8] {
        if self.key.len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(self.lowcase_key, self.key.len) }
        }
    }

    /// Assigns the key and the value to the table element
    pub fn set(&mut self, pool: &mut ngx_pool_t, key: &[u8], value: &[u8]) -> Option<()> {
        self.key = unsafe { ngx_str_t::from_bytes(pool, key)? };
        self.value = unsafe { ngx_str_t::from_bytes(pool, value)? };

        self.lowcase_key = unsafe { ngx_pnalloc(pool, key.len()).cast() };
        if self.lowcase_key.is_null() {
            return None;
        }
        self.hash = unsafe { ngx_hash_strlow(self.lowcase_key, self.key.data, self.key.len) };

        Some(())
    }
}
