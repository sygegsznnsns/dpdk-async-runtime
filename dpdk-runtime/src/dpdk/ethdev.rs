use crate::dpdk::ffi;
use crate::dpdk::mempool::Mempool;
use crate::dpdk::mbuf::Mbuf;
use std::mem;

pub struct EthDev {
    port_id: u16,
}

impl EthDev {
    pub fn configure(port_id: u16, rx_queues: u16, tx_queues: u16, mtu: u32) -> Self {
        let mut conf: ffi::rte_eth_conf = unsafe { mem::zeroed() };
        conf.rxmode.offloads = 0;
        conf.txmode.offloads = 0;

        let ret = unsafe {
            ffi::rte_eth_dev_configure(port_id, rx_queues, tx_queues, &conf)
        };
        if ret != 0 {
            panic!("rte_eth_dev_configure failed: {}", ret);
        }

        if mtu > 0 {
            let ret = unsafe { ffi::rte_eth_dev_set_mtu(port_id, mtu as u16) };
            if ret != 0 {
                eprintln!("rte_eth_dev_set_mtu({}) failed: {}", mtu, ret);
            }
        }

        EthDev { port_id }
    }

    pub fn setup_rx_queue(&self, queue_id: u16, nb_desc: u16, socket_id: u32, pool: &Mempool) {
        let rx_conf: ffi::rte_eth_rxconf = unsafe { mem::zeroed() };
        let ret = unsafe {
            ffi::rte_eth_rx_queue_setup(
                self.port_id,
                queue_id,
                nb_desc,
                socket_id,
                &rx_conf,
                pool.raw(),
            )
        };
        if ret != 0 {
            panic!("rte_eth_rx_queue_setup failed: {}", ret);
        }
    }

    pub fn setup_tx_queue(&self, queue_id: u16, nb_desc: u16, socket_id: u32) {
        let tx_conf: ffi::rte_eth_txconf = unsafe { mem::zeroed() };
        let ret = unsafe {
            ffi::rte_eth_tx_queue_setup(
                self.port_id,
                queue_id,
                nb_desc,
                socket_id,
                &tx_conf,
            )
        };
        if ret != 0 {
            panic!("rte_eth_tx_queue_setup failed: {}", ret);
        }
    }

    pub fn start(&self) {
        let ret = unsafe { ffi::rte_eth_dev_start(self.port_id) };
        if ret != 0 {
            panic!("rte_eth_dev_start failed: {}", ret);
        }
    }

    pub fn stop(&self) {
        unsafe { ffi::rte_eth_dev_stop(self.port_id); }
    }

    pub fn close(&self) {
        unsafe { ffi::rte_eth_dev_close(self.port_id); }
    }

    pub fn mac_addr(&self) -> [u8; 6] {
        let mut mac = [0u8; 6];
        let ret = unsafe {
            ffi::rte_eth_macaddr_get(self.port_id, mac.as_mut_ptr() as *mut ffi::rte_ether_addr)
        };
        if ret != 0 {
            panic!("rte_eth_macaddr_get failed: {}", ret);
        }
        mac
    }

    pub fn enable_promiscuous(&self) {
        unsafe { ffi::rte_eth_promiscuous_enable(self.port_id); }
    }

    pub fn rx_burst(&self, queue_id: u16, pkts: &mut [Option<Mbuf>]) -> u16 {
        let mut raw_pkts: [*mut ffi::rte_mbuf; 64] = [std::ptr::null_mut(); 64];
        let max = pkts.len().min(64);
        let n = unsafe {
            ffi::dpdk_rte_eth_rx_burst(self.port_id, queue_id, raw_pkts.as_mut_ptr(), max as u16)
        };
        for i in 0..n as usize {
            pkts[i] = Some(unsafe { Mbuf::from_raw(raw_pkts[i]) });
        }
        n
    }

    pub fn tx_burst(&self, queue_id: u16, pkts: &mut [Mbuf]) -> u16 {
        let mut raw_pkts: [*mut ffi::rte_mbuf; 64] = [std::ptr::null_mut(); 64];
        let max = pkts.len().min(64);
        for i in 0..max {
            raw_pkts[i] = pkts[i].raw();
        }
        let n = unsafe {
            ffi::dpdk_rte_eth_tx_burst(self.port_id, queue_id, raw_pkts.as_mut_ptr(), max as u16)
        };
        for i in 0..n as usize {
            std::mem::forget(pkts[i].take().unwrap());
        }
        n
    }

    pub fn tx_done_cleanup(&self, queue_id: u16) -> i32 {
        unsafe { ffi::dpdk_rte_eth_tx_done_cleanup(self.port_id, queue_id, 0) }
    }

    pub fn tx_burst_empty(&self, queue_id: u16) -> u16 {
        unsafe { ffi::dpdk_rte_eth_tx_burst(self.port_id, queue_id, std::ptr::null_mut(), 0) }
    }

    pub fn port_id(&self) -> u16 {
        self.port_id
    }
}
