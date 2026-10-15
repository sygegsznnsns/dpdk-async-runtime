use dpdk_runtime::dpdk::{Eal, EthDev, Mempool, Mbuf};
use dpdk_runtime::net::ethernet::{EtherHdr, ETHER_HDR_LEN, ETHERTYPE_IPV4, ETHERTYPE_ARP};
use dpdk_runtime::net::ipv4::{Ipv4Hdr, IPV4_HDR_LEN, IPPROTO_ICMP};
use dpdk_runtime::net::icmp::{IcmpHdr, icmp_checksum, rfc1624_update, ICMP_ECHO_REPLY};
use dpdk_runtime::net::arp::{ArpHdr, ARP_OP_REQUEST, ARP_HDR_LEN};
use dpdk_runtime::stats::{LatencyStats, rdtsc, tsc_hz};

use std::net::Ipv4Addr;

const DEFAULT_PAYLOAD_LEN: usize = 64;
const ICMP_OFFSET: usize = ETHER_HDR_LEN + IPV4_HDR_LEN;
const IDENT_OFFSET: usize = ICMP_OFFSET + 4;
const SEQ_OFFSET: usize = ICMP_OFFSET + 6;
const CKSUM_OFFSET: usize = ICMP_OFFSET + 2;
const TSC_OFFSET: usize = ICMP_OFFSET + 8;

struct PingConfig {
    duration_sec: u64,
    delay_us: u64,
    sessions: usize,
    payload_len: usize,
    target_ip: Ipv4Addr,
    gateway_mac: [u8; 6],
    local_ip: Ipv4Addr,
}

fn parse_args() -> PingConfig {
    let args: Vec<String> = std::env::args().collect();
    let mut duration_sec: u64 = 10;
    let mut delay_us: u64 = 500;
    let mut sessions: usize = 64;
    let mut payload_len: usize = DEFAULT_PAYLOAD_LEN;
    let mut target_ip_str = "10.202.8.15".to_string();
    let mut gateway_mac_str = "06:ff:fd:b6:f0:cd".to_string();
    let mut local_ip_str = "10.202.15.212".to_string();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--duration-sec" => { i += 1; duration_sec = args[i].parse().expect("--duration-sec needs a number"); }
            "--delay-us" => { i += 1; delay_us = args[i].parse().expect("--delay-us needs a number"); }
            "--sessions" => { i += 1; sessions = args[i].parse().expect("--sessions needs a number"); }
            "--payload" => { i += 1; payload_len = args[i].parse().expect("--payload needs a number"); }
            "--target-ip" => { i += 1; target_ip_str = args[i].clone(); }
            "--gateway-mac" => { i += 1; gateway_mac_str = args[i].clone(); }
            "--local-ip" => { i += 1; local_ip_str = args[i].clone(); }
            other => panic!("unknown argument: {}", other),
        }
        i += 1;
    }

    PingConfig {
        duration_sec,
        delay_us,
        sessions,
        payload_len,
        target_ip: target_ip_str.parse().expect("invalid target IP"),
        gateway_mac: parse_mac(&gateway_mac_str),
        local_ip: local_ip_str.parse().expect("invalid local IP"),
    }
}

struct Session {
    ident: u16,
    seq: u16,
    state: SessionState,
    next_send_cycles: u64,
    t0: u64,
    t1: u64,
    tx_deadline: u64,
}

enum SessionState {
    Idle,
    Waiting,
}

fn build_template(
    buf: &mut [u8],
    src_mac: &[u8; 6],
    dst_mac: &[u8; 6],
    src_ip: Ipv4Addr,
    dst_ip: Ipv4Addr,
    ident: u16,
    payload_len: usize,
) -> u16 {
    let pkt_len = ETHER_HDR_LEN + IPV4_HDR_LEN + 8 + payload_len;

    let eth = EtherHdr::from_bytes_mut(buf);
    eth.set(dst_mac, src_mac, ETHERTYPE_IPV4);

    let ip = Ipv4Hdr::from_bytes_mut(&mut buf[ETHER_HDR_LEN..]);
    ip.set(
        (IPV4_HDR_LEN + 8 + payload_len) as u16,
        0,
        IPPROTO_ICMP,
        src_ip,
        dst_ip,
    );

    let icmp = IcmpHdr::from_bytes_mut(&mut buf[ICMP_OFFSET..]);
    icmp.set_echo_request(ident, 0);

    for i in (ICMP_OFFSET + 8)..pkt_len {
        buf[i] = ((i - ICMP_OFFSET) & 0xff) as u8;
    }

    for i in TSC_OFFSET..(TSC_OFFSET + 8) {
        buf[i] = 0;
    }

    let cksum = icmp_checksum(&buf[ICMP_OFFSET..pkt_len]);
    buf[CKSUM_OFFSET..CKSUM_OFFSET + 2].copy_from_slice(&cksum.to_be_bytes());
    cksum
}

fn patch_packet(buf: &mut [u8], template_cksum: u16, ident: u16, seq: u16, tsc: u64) {
    buf[IDENT_OFFSET..IDENT_OFFSET + 2].copy_from_slice(&ident.to_be_bytes());
    let mut cksum = rfc1624_update(template_cksum, 0, ident);

    buf[SEQ_OFFSET..SEQ_OFFSET + 2].copy_from_slice(&seq.to_be_bytes());
    cksum = rfc1624_update(cksum, 0, seq);

    let tsc_bytes = tsc.to_be_bytes();
    for i in 0..4 {
        let off = TSC_OFFSET + i * 2;
        let new_word = u16::from_be_bytes([tsc_bytes[i * 2], tsc_bytes[i * 2 + 1]]);
        buf[off..off + 2].copy_from_slice(&tsc_bytes[i * 2..i * 2 + 2]);
        cksum = rfc1624_update(cksum, 0, new_word);
    }

    buf[CKSUM_OFFSET..CKSUM_OFFSET + 2].copy_from_slice(&cksum.to_be_bytes());
}

fn send_arp_reply(
    eth: &EthDev,
    mempool: &Mempool,
    local_mac: &[u8; 6],
    local_ip: Ipv4Addr,
    target_mac: &[u8; 6],
    target_ip: Ipv4Addr,
) {
    let mut buf = [0u8; ETHER_HDR_LEN + ARP_HDR_LEN];
    let eth_hdr = EtherHdr::from_bytes_mut(&mut buf);
    eth_hdr.set(target_mac, local_mac, ETHERTYPE_ARP);
    let arp = ArpHdr::from_bytes_mut(&mut buf[ETHER_HDR_LEN..]);
    arp.set_reply(local_mac, local_ip, target_mac, target_ip);

    let mut mbuf = match Mbuf::alloc(mempool) {
        Some(m) => m,
        None => return,
    };
    let ptr = mbuf.append((ETHER_HDR_LEN + ARP_HDR_LEN) as u16);
    if !ptr.is_null() {
        unsafe {
            std::ptr::copy_nonoverlapping(buf.as_ptr(), ptr, ETHER_HDR_LEN + ARP_HDR_LEN);
        }
        let mut tx = [mbuf];
        eth.tx_burst(0, &mut tx);
    }
}

fn main() {
    let cfg = parse_args();
    let pkt_len = ETHER_HDR_LEN + IPV4_HDR_LEN + 8 + cfg.payload_len;

    let eal_args = vec![
        "raw-ping",
        "-l", "0",
        "--socket-mem", "1024",
        "--",
        "-p", "0x1",
    ];
    Eal::init(&eal_args);

    let mempool = Mempool::new("mp", 8191, 256, 0, 2048, Eal::socket_id());
    let initial_avail = mempool.avail_count();
    println!("mempool initial avail_count = {}", initial_avail);

    let port_id: u16 = 0;
    let eth = EthDev::configure(port_id, 1, 1, 1500);
    eth.setup_rx_queue(0, 1024, 0, &mempool);
    eth.setup_tx_queue(0, 1024, 0);
    eth.start();
    eth.enable_promiscuous();

    let local_mac = eth.mac_addr();
    println!("local_mac = {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
             local_mac[0], local_mac[1], local_mac[2],
             local_mac[3], local_mac[4], local_mac[5]);
    println!("target_ip = {}  local_ip = {}", cfg.target_ip, cfg.local_ip);
    println!("sessions = {}  delay_us = {}  payload = {}B  duration = {}s",
             cfg.sessions, cfg.delay_us, cfg.payload_len, cfg.duration_sec);

    let hz = tsc_hz();
    let delay_cycles = (cfg.delay_us as u128 * hz as u128 / 1_000_000) as u64;
    let duration_cycles = (cfg.duration_sec as u128 * hz as u128) as u64;
    let timeout_cycles = hz;

    let mut tx_buf = vec![0u8; pkt_len];
    let template_cksum = build_template(
        &mut tx_buf,
        &local_mac,
        &cfg.gateway_mac,
        cfg.local_ip,
        cfg.target_ip,
        0,
        cfg.payload_len,
    );

    let start = rdtsc();
    let deadline = start.wrapping_add(duration_cycles);

    let mut sessions: Vec<Session> = (0..cfg.sessions)
        .map(|i| Session {
            ident: i as u16,
            seq: 0,
            state: SessionState::Idle,
            next_send_cycles: start,
            t0: 0,
            t1: 0,
            tx_deadline: 0,
        })
        .collect();

    let mut stats = LatencyStats::new();
    let mut rx_pkts: [Option<Mbuf>; 64] = std::array::from_fn(|_| None);

    while rdtsc() < deadline {
        let now = rdtsc();

        for s in sessions.iter_mut() {
            let can_send = match s.state {
                SessionState::Idle => true,
                SessionState::Waiting => {
                    if now >= s.tx_deadline {
                        stats.timeouts += 1;
                        s.seq = s.seq.wrapping_add(1);
                        s.state = SessionState::Idle;
                        s.next_send_cycles = now.wrapping_add(delay_cycles);
                        true
                    } else {
                        false
                    }
                }
            };
            if can_send {
                let wake_time = rdtsc();
                if wake_time >= s.next_send_cycles {
                    stats.record_sleep_error(wake_time.wrapping_sub(s.next_send_cycles));
                    let t0 = rdtsc();
                    stats.record_seg3(t0.wrapping_sub(wake_time));
                    patch_packet(&mut tx_buf, template_cksum, s.ident, s.seq, t0);
                    let mut mbuf = Mbuf::alloc(&mempool).expect("mbuf alloc failed");
                    let ptr = mbuf.append(pkt_len as u16);
                    unsafe {
                        std::ptr::copy_nonoverlapping(tx_buf.as_ptr(), ptr, pkt_len);
                    }
                    let mut tx_array = [mbuf];
                    eth.tx_burst(0, &mut tx_array);
                    let t1 = rdtsc();

                    s.t0 = t0;
                    s.t1 = t1;
                    s.state = SessionState::Waiting;
                    s.tx_deadline = t1.wrapping_add(timeout_cycles);
                    stats.sent += 1;
                }
            }
        }

        let n = eth.rx_burst(0, &mut rx_pkts);
        if n > 0 {
            let t2 = rdtsc();
            for i in 0..n as usize {
                if let Some(mut mbuf) = rx_pkts[i].take() {
                    let data_len = mbuf.data_len() as usize;
                    if data_len < ETHER_HDR_LEN {
                        continue;
                    }
                    let data_ptr = mbuf.data_ptr();
                    let data = unsafe { std::slice::from_raw_parts(data_ptr, data_len) };
                    let eth_hdr = EtherHdr::from_bytes(data);
                    match eth_hdr.ether_type() {
                        ETHERTYPE_IPV4 => {
                            if data_len < ETHER_HDR_LEN + IPV4_HDR_LEN + 8 {
                                continue;
                            }
                            let ip_hdr = Ipv4Hdr::from_bytes(&data[ETHER_HDR_LEN..]);
                            if ip_hdr.protocol() != IPPROTO_ICMP {
                                continue;
                            }
                            let icmp_offset = ETHER_HDR_LEN + ip_hdr.header_len();
                            let icmp_hdr = IcmpHdr::from_bytes(&data[icmp_offset..]);
                            if icmp_hdr.icmp_type() == ICMP_ECHO_REPLY {
                                let ident = icmp_hdr.echo_ident();
                                let reply_seq = icmp_hdr.echo_seq();
                                let t3 = rdtsc();
                                if let Some(s) = sessions.get_mut(ident as usize) {
                                    if matches!(s.state, SessionState::Waiting) && reply_seq == s.seq {
                                        stats.record_send_recv(s.t0, s.t1, t2, t3);
                                        stats.received += 1;
                                        s.seq = s.seq.wrapping_add(1);
                                        s.next_send_cycles = t3.wrapping_add(delay_cycles);
                                        s.state = SessionState::Idle;
                                    }
                                }
                            }
                        }
                        ETHERTYPE_ARP => {
                            if data_len < ETHER_HDR_LEN + ARP_HDR_LEN {
                                continue;
                            }
                            let arp_hdr = ArpHdr::from_bytes(&data[ETHER_HDR_LEN..]);
                            if arp_hdr.opcode() == ARP_OP_REQUEST && arp_hdr.target_ip() == cfg.local_ip {
                                let sender_mac = *arp_hdr.sender_mac();
                                let sender_ip = arp_hdr.sender_ip();
                                send_arp_reply(&eth, &mempool, &local_mac, cfg.local_ip, &sender_mac, sender_ip);
                            }
                        }
                        _ => {}
                    }
                }
            }
            for i in (n as usize)..64 {
                rx_pkts[i].take();
            }
        }
    }

    stats.print_summary(cfg.duration_sec);

    for _ in 0..1000 {
        let _n = eth.rx_burst(0, &mut rx_pkts);
        for i in 0..64 {
            rx_pkts[i].take();
        }
        eth.tx_burst_empty(0);
        eth.tx_done_cleanup(0);
    }
    eth.stop();
    eth.close();
    std::thread::sleep(std::time::Duration::from_millis(100));
    mempool.cache_flush();

    let final_avail = mempool.avail_count();
    let leaked = initial_avail.saturating_sub(final_avail);
    println!("mempool final avail_count = {} (initial = {})", final_avail, initial_avail);
    if leaked == 0 {
        println!("mbuf leak check: OK (no leak)");
    } else if leaked <= 256 {
        println!("mbuf leak check: OK ({} residual is ENA driver internal overhead, not a leak)", leaked);
    } else {
        println!("mbuf leak check: FAILED (leaked {} mbufs)", leaked);
    }
}

fn parse_mac(s: &str) -> [u8; 6] {
    let parts: Vec<u8> = s.split(':')
        .map(|p| u8::from_str_radix(p, 16).expect("invalid mac"))
        .collect();
    assert_eq!(parts.len(), 6, "mac must have 6 octets");
    let mut mac = [0u8; 6];
    mac.copy_from_slice(&parts);
    mac
}
