use crate::dpdk::ethdev::EthDev;
use crate::dpdk::ffi;
use crate::dpdk::mbuf::Mbuf;
use crate::net::ethernet::{EtherHdr, ETHERTYPE_IPV4, ETHERTYPE_ARP, ETHER_HDR_LEN};
use crate::net::ipv4::{Ipv4Hdr, IPV4_HDR_LEN, IPPROTO_ICMP};
use crate::net::icmp::{IcmpHdr, ICMP_ECHO_REPLY};
use crate::net::arp::{ArpHdr, ARP_OP_REQUEST, ARP_OP_REPLY, ARP_HDR_LEN};
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::task::Waker;

pub struct Session {
    pub ident: u16,
    pub waker: Option<Waker>,
    pub reply: Option<Mbuf>,
    pub reply_t2: u64,
    pub reply_seq: u16,
}

pub struct Reactor {
    eth: EthDev,
    queue_id: u16,
    mempool: *mut ffi::rte_mempool,
    sessions: HashMap<u16, Session>,
    local_mac: [u8; 6],
    local_ip: Ipv4Addr,
    gateway_mac: [u8; 6],
    arp_cache: HashMap<Ipv4Addr, [u8; 6]>,
}

unsafe impl Send for Reactor {}

impl Reactor {
    pub fn new(
        eth: EthDev,
        queue_id: u16,
        mempool: *mut ffi::rte_mempool,
        local_mac: [u8; 6],
        local_ip: Ipv4Addr,
        gateway_mac: [u8; 6],
    ) -> Self {
        Reactor {
            eth,
            queue_id,
            mempool,
            sessions: HashMap::new(),
            local_mac,
            local_ip,
            gateway_mac,
            arp_cache: HashMap::new(),
        }
    }

    pub fn register_session(&mut self, ident: u16) {
        self.sessions.insert(ident, Session {
            ident,
            waker: None,
            reply: None,
            reply_t2: 0,
            reply_seq: 0,
        });
    }

    pub fn set_session_waker(&mut self, ident: u16, waker: Waker) {
        if let Some(session) = self.sessions.get_mut(&ident) {
            session.waker = Some(waker);
        }
    }

    pub fn take_reply(&mut self, ident: u16) -> Option<Mbuf> {
        if let Some(session) = self.sessions.get_mut(&ident) {
            session.reply.take()
        } else {
            None
        }
    }

    pub fn reply_t2(&self, ident: u16) -> u64 {
        self.sessions.get(&ident).map(|s| s.reply_t2).unwrap_or(0)
    }

    pub fn reply_seq(&self, ident: u16) -> u16 {
        self.sessions.get(&ident).map(|s| s.reply_seq).unwrap_or(0)
    }

    pub fn has_reply(&self, ident: u16) -> bool {
        self.sessions.get(&ident).map(|s| s.reply.is_some()).unwrap_or(false)
    }

    pub fn poll_once(&mut self) -> usize {
        let mut rx_pkts: [Option<Mbuf>; 64] = std::array::from_fn(|_| None);
        let n = self.eth.rx_burst(self.queue_id, &mut rx_pkts);
        let t2 = crate::stats::rdtsc();
        for i in 0..n as usize {
            if let Some(mbuf) = rx_pkts[i].take() {
                self.handle_packet(mbuf, t2);
            }
        }
        n as usize
    }

    pub fn drain(&mut self) {
        for _ in 0..1000 {
            self.poll_once();
            self.eth.tx_burst_empty(self.queue_id);
            self.eth.tx_done_cleanup(self.queue_id);
        }
        self.eth.stop();
        self.eth.close();
        std::thread::sleep(std::time::Duration::from_millis(100));
        for session in self.sessions.values_mut() {
            session.reply = None;
        }
    }

    fn handle_packet(&mut self, mut mbuf: Mbuf, t2: u64) {
        let data_len = mbuf.data_len() as usize;
        if data_len < ETHER_HDR_LEN {
            return;
        }

        let data_ptr = mbuf.data_ptr();
        let data = unsafe { std::slice::from_raw_parts(data_ptr, data_len) };
        let eth_hdr = EtherHdr::from_bytes(data);

        match eth_hdr.ether_type() {
            ETHERTYPE_IPV4 => {
                if data_len < ETHER_HDR_LEN + IPV4_HDR_LEN {
                    return;
                }
                let ip_hdr = Ipv4Hdr::from_bytes(&data[ETHER_HDR_LEN..]);
                if ip_hdr.protocol() == IPPROTO_ICMP {
                    let icmp_offset = ETHER_HDR_LEN + ip_hdr.header_len();
                    if data_len < icmp_offset + 8 {
                        return;
                    }
                    let icmp_hdr = IcmpHdr::from_bytes(&data[icmp_offset..]);
                    if icmp_hdr.icmp_type() == ICMP_ECHO_REPLY {
                        let ident = icmp_hdr.echo_ident();
                        if let Some(session) = self.sessions.get_mut(&ident) {
                            let raw = mbuf.into_raw();
                            session.reply = Some(unsafe { Mbuf::from_raw(raw) });
                            session.reply_t2 = t2;
                            session.reply_seq = icmp_hdr.echo_seq();
                            if let Some(waker) = session.waker.take() {
                                waker.wake();
                            }
                            return;
                        }
                    }
                }
            }
            ETHERTYPE_ARP => {
                if data_len < ETHER_HDR_LEN + ARP_HDR_LEN {
                    return;
                }
                let arp_hdr = ArpHdr::from_bytes(&data[ETHER_HDR_LEN..]);
                let op = arp_hdr.opcode();
                if op == ARP_OP_REPLY {
                    let ip = arp_hdr.sender_ip();
                    let mac = *arp_hdr.sender_mac();
                    self.arp_cache.insert(ip, mac);
                } else if op == ARP_OP_REQUEST {
                    if arp_hdr.target_ip() == self.local_ip {
                        let sender_mac = *arp_hdr.sender_mac();
                        let sender_ip = arp_hdr.sender_ip();
                        self.send_arp_reply(&sender_mac, sender_ip);
                    }
                }
            }
            _ => {}
        }
    }

    pub fn send_packet(&self, data: &[u8]) {
        let mut mbuf = match unsafe { Mbuf::alloc_from_raw(self.mempool) } {
            Some(m) => m,
            None => return,
        };
        let ptr = mbuf.append(data.len() as u16);
        if !ptr.is_null() {
            unsafe {
                std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
            }
            let mut tx_buf = [mbuf];
            self.eth.tx_burst(self.queue_id, &mut tx_buf);
        }
    }

    fn send_arp_reply(&self, target_mac: &[u8; 6], target_ip: Ipv4Addr) {
        let mut buf = [0u8; ETHER_HDR_LEN + ARP_HDR_LEN];
        let eth = EtherHdr::from_bytes_mut(&mut buf);
        eth.set(target_mac, &self.local_mac, ETHERTYPE_ARP);
        let arp = ArpHdr::from_bytes_mut(&mut buf[ETHER_HDR_LEN..]);
        arp.set_reply(&self.local_mac, self.local_ip, target_mac, target_ip);
        self.send_packet(&buf);
    }

    pub fn local_mac(&self) -> &[u8; 6] {
        &self.local_mac
    }

    pub fn local_ip(&self) -> Ipv4Addr {
        self.local_ip
    }

    pub fn gateway_mac(&self) -> &[u8; 6] {
        &self.gateway_mac
    }

    pub fn arp_lookup(&self, ip: Ipv4Addr) -> Option<[u8; 6]> {
        self.arp_cache.get(&ip).copied()
    }

    pub fn arp_insert(&mut self, ip: Ipv4Addr, mac: [u8; 6]) {
        self.arp_cache.insert(ip, mac);
    }
}
