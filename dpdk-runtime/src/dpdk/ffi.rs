//! DPDK FFI bindings.

#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

extern "C" {
    pub fn dpdk_rte_pktmbuf_alloc(mp: *mut rte_mempool) -> *mut rte_mbuf;
    pub fn dpdk_rte_pktmbuf_free(m: *mut rte_mbuf);
    pub fn dpdk_rte_pktmbuf_data_len(m: *const rte_mbuf) -> u16;
    pub fn dpdk_rte_pktmbuf_pkt_len(m: *const rte_mbuf) -> u16;
    pub fn dpdk_rte_pktmbuf_append(m: *mut rte_mbuf, len: u16) -> *mut ::std::os::raw::c_char;
    pub fn dpdk_rte_pktmbuf_mtod(m: *mut rte_mbuf) -> *mut ::std::os::raw::c_void;
    pub fn dpdk_rte_eth_rx_burst(
        port_id: u16,
        queue_id: u16,
        rx_pkts: *mut *mut rte_mbuf,
        nb_pkts: u16,
    ) -> u16;
    pub fn dpdk_rte_eth_tx_burst(
        port_id: u16,
        queue_id: u16,
        tx_pkts: *mut *mut rte_mbuf,
        nb_pkts: u16,
    ) -> u16;
    pub fn dpdk_rte_eth_tx_done_cleanup(
        port_id: u16,
        queue_id: u16,
        free_cnt: u32,
    ) -> i32;
    pub fn dpdk_rte_pktmbuf_refcnt_update(m: *mut rte_mbuf, v: i16);
    pub fn dpdk_rte_pktmbuf_refcnt_count(m: *const rte_mbuf) -> u16;
    pub fn dpdk_rte_mempool_avail_count(mp: *const rte_mempool) -> ::std::os::raw::c_uint;
    pub fn dpdk_rte_mempool_cache_flush(mp: *mut rte_mempool, lcore_id: ::std::os::raw::c_uint);
    pub fn dpdk_rte_pktmbuf_adj(m: *mut rte_mbuf, len: u16);
    pub fn dpdk_rte_pktmbuf_trim(m: *mut rte_mbuf, len: u16);
    pub fn dpdk_rte_pktmbuf_headroom(m: *const rte_mbuf) -> u16;
    pub fn dpdk_rte_pktmbuf_tailroom(m: *const rte_mbuf) -> u16;
}
