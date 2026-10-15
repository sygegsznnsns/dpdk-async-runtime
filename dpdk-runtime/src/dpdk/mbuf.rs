use crate::dpdk::ffi;
use crate::dpdk::mempool::Mempool;

pub struct Mbuf {
    raw: *mut ffi::rte_mbuf,
}

unsafe impl Send for Mbuf {}

impl Mbuf {
    pub fn alloc(pool: &Mempool) -> Option<Self> {
        let raw = unsafe { ffi::dpdk_rte_pktmbuf_alloc(pool.raw()) };
        if raw.is_null() {
            None
        } else {
            Some(Mbuf { raw })
        }
    }

    pub unsafe fn alloc_from_raw(pool: *mut ffi::rte_mempool) -> Option<Self> {
        let raw = ffi::dpdk_rte_pktmbuf_alloc(pool);
        if raw.is_null() {
            None
        } else {
            Some(Mbuf { raw })
        }
    }

    pub unsafe fn from_raw(raw: *mut ffi::rte_mbuf) -> Self {
        Mbuf { raw }
    }

    pub fn raw(&self) -> *mut ffi::rte_mbuf {
        self.raw
    }

    pub fn into_raw(self) -> *mut ffi::rte_mbuf {
        let raw = self.raw;
        std::mem::forget(self);
        raw
    }

    pub fn take(&mut self) -> Option<Mbuf> {
        if self.raw.is_null() {
            None
        } else {
            let raw = self.raw;
            self.raw = std::ptr::null_mut();
            Some(unsafe { Mbuf::from_raw(raw) })
        }
    }

    pub fn data_len(&self) -> u16 {
        unsafe { ffi::dpdk_rte_pktmbuf_data_len(self.raw) }
    }

    pub fn pkt_len(&self) -> u16 {
        unsafe { ffi::dpdk_rte_pktmbuf_pkt_len(self.raw) }
    }

    pub fn data_ptr(&mut self) -> *mut u8 {
        unsafe { ffi::dpdk_rte_pktmbuf_mtod(self.raw) as *mut u8 }
    }

    pub fn append(&mut self, len: u16) -> *mut u8 {
        unsafe { ffi::dpdk_rte_pktmbuf_append(self.raw, len) as *mut u8 }
    }

    pub fn adj(&mut self, len: u16) {
        unsafe { ffi::dpdk_rte_pktmbuf_adj(self.raw, len) }
    }

    pub fn trim(&mut self, len: u16) {
        unsafe { ffi::dpdk_rte_pktmbuf_trim(self.raw, len) }
    }

    pub fn headroom(&self) -> u16 {
        unsafe { ffi::dpdk_rte_pktmbuf_headroom(self.raw) }
    }

    pub fn tailroom(&self) -> u16 {
        unsafe { ffi::dpdk_rte_pktmbuf_tailroom(self.raw) }
    }

    pub fn refcnt(&self) -> u16 {
        unsafe { ffi::dpdk_rte_pktmbuf_refcnt_count(self.raw) }
    }
}

impl Drop for Mbuf {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe { ffi::dpdk_rte_pktmbuf_free(self.raw) }
        }
    }
}
