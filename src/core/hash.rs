use foreign_types::ForeignTypeRef;

use crate::ffi::{ngx_hash_strlow, ngx_pool_t, ngx_str_t, ngx_table_elt_t};

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

impl NgxTableElementRef {
    /// Assigns the key and the value to the table element
    pub fn set(&mut self, pool: &mut ngx_pool_t, key: &[u8], value: &[u8]) -> Option<()> {
        let this = self.as_mut();

        this.key = unsafe { ngx_str_t::from_bytes(pool, key)? };
        this.hash = unsafe { ngx_hash_strlow(this.lowcase_key, this.key.data, this.key.len) };
        this.value = unsafe { ngx_str_t::from_bytes(pool, value)? };

        Some(())
    }
}
