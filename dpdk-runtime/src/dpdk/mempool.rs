use crate::dpdk::ffi;

pub struct Mempool {
    raw: *mut ffi::rte_mempool,
}

unsafe impl Send for Mempool {}
unsafe impl Sync for Mempool {}

impl Mempool {
    pub fn new(name: &str, n: u32, cache_size: u32, priv_size: u16, data_room_size: u16, socket_id: i32) -> Self {
        let c_name = std::ffi::CString::new(name).unwrap();
        let raw = unsafe {
            ffi::rte_pktmbuf_pool_create(
                c_name.as_ptr(),
                n,
                cache_size,
                priv_size,
                data_room_size,
                socket_id,
            )
        };
        if raw.is_null() {
            panic!("Failed to create mempool: {}", name);
        }
        Mempool { raw }
    }

    pub fn raw(&self) -> *mut ffi::rte_mempool {
        self.raw
    }

    pub fn avail_count(&self) -> u32 {
        unsafe { ffi::dpdk_rte_mempool_avail_count(self.raw) as u32 }
    }

    pub fn cache_flush(&self) {
        unsafe {
            let lcore_idx = ffi::rte_lcore_index(-1);
            ffi::dpdk_rte_mempool_cache_flush(self.raw, lcore_idx as u32);
        }
    }
}

impl Drop for Mempool {
    fn drop(&mut self) {
        unsafe {
            ffi::rte_mempool_free(self.raw);
        }
    }
}
