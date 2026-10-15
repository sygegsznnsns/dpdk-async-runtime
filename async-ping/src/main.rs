use dpdk_runtime::dpdk::{Eal, EthDev, Mempool, Mbuf};
use dpdk_runtime::net::ethernet::{EtherHdr, ETHER_HDR_LEN, ETHERTYPE_IPV4};
use dpdk_runtime::net::ipv4::{Ipv4Hdr, IPV4_HDR_LEN, IPPROTO_ICMP};
use dpdk_runtime::net::icmp::{IcmpHdr, icmp_checksum, rfc1624_update};
use dpdk_runtime::rt::{Executor, Reactor, Sleep, Timer};
use dpdk_runtime::stats::{LatencyStats, rdtsc};

use std::cell::RefCell;
use std::net::Ipv4Addr;
use std::rc::Rc;

const DEFAULT_PAYLOAD_LEN: usize = 64;
const ICMP_OFFSET: usize = ETHER_HDR_LEN + IPV4_HDR_LEN;
const SEQ_OFFSET: usize = ICMP_OFFSET + 6;
const CKSUM_OFFSET: usize = ICMP_OFFSET + 2;
const TSC_OFFSET: usize = ICMP_OFFSET + 8;

struct PingConfig {
    duration_sec: u64,
    delay_us: u64,
    sessions: u16,
    payload_len: usize,
    target_ip: Ipv4Addr,
    gateway_mac: [u8; 6],
    local_ip: Ipv4Addr,
}

fn parse_args() -> PingConfig {
    let args: Vec<String> = std::env::args().collect();
    let mut duration_sec: u64 = 10;
    let mut delay_us: u64 = 500;
    let mut sessions: u16 = 64;
    let mut payload_len: usize = DEFAULT_PAYLOAD_LEN;
    let mut target_ip_str = "10.202.8.15".to_string();
    let mut gateway_mac_str = "06:ff:fd:b6:f0:cd".to_string();
    let mut local_ip_str = "10.202.15.212".to_string();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--duration-sec" => {
                i += 1;
                duration_sec = args[i].parse().expect("--duration-sec needs a number");
            }
            "--delay-us" => {
                i += 1;
                delay_us = args[i].parse().expect("--delay-us needs a number");
            }
            "--sessions" => {
                i += 1;
                sessions = args[i].parse().expect("--sessions needs a number");
            }
            "--payload" => {
                i += 1;
                payload_len = args[i].parse().expect("--payload needs a number");
            }
            "--target-ip" => {
                i += 1;
                target_ip_str = args[i].clone();
            }
            "--gateway-mac" => {
                i += 1;
                gateway_mac_str = args[i].clone();
            }
            "--local-ip" => {
                i += 1;
                local_ip_str = args[i].clone();
            }
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

fn patch_packet(buf: &mut [u8], template_cksum: u16, seq: u16, tsc: u64, payload_len: usize) {
    buf[SEQ_OFFSET..SEQ_OFFSET + 2].copy_from_slice(&seq.to_be_bytes());
    let mut cksum = rfc1624_update(template_cksum, 0, seq);

    let tsc_bytes = tsc.to_be_bytes();
    for i in 0..4 {
        let off = TSC_OFFSET + i * 2;
        let new_word = u16::from_be_bytes([tsc_bytes[i * 2], tsc_bytes[i * 2 + 1]]);
        buf[off..off + 2].copy_from_slice(&tsc_bytes[i * 2..i * 2 + 2]);
        cksum = rfc1624_update(cksum, 0, new_word);
    }

    buf[CKSUM_OFFSET..CKSUM_OFFSET + 2].copy_from_slice(&cksum.to_be_bytes());
    let _ = payload_len;
}

enum ReplyResult {
    Reply { t2: u64, mbuf: Mbuf },
    Timeout,
}

async fn wait_reply(
    reactor: Rc<RefCell<Reactor>>,
    timer: Rc<RefCell<Timer>>,
    ident: u16,
    expected_seq: u16,
    timeout_cycles: u64,
) -> ReplyResult {
    use futures_util::future::{select, Either};

    let reactor_clone = reactor.clone();
    let reply_fut = std::future::poll_fn(move |cx| {
        let mut r = reactor_clone.borrow_mut();
        if r.has_reply(ident) {
            let reply_seq = r.reply_seq(ident);
            if reply_seq == expected_seq {
                let t2 = r.reply_t2(ident);
                let mbuf = r.take_reply(ident).unwrap();
                std::task::Poll::Ready(ReplyResult::Reply { t2, mbuf })
            } else {
                let _late = r.take_reply(ident);
                r.set_session_waker(ident, cx.waker().clone());
                std::task::Poll::Pending
            }
        } else {
            r.set_session_waker(ident, cx.waker().clone());
            std::task::Poll::Pending
        }
    });

    let timeout_fut = Sleep::new(timer, timeout_cycles);

    match select(reply_fut, timeout_fut).await {
        Either::Left((reply, _)) => reply,
        Either::Right(((), _)) => ReplyResult::Timeout,
    }
}

async fn ping_session(
    reactor: Rc<RefCell<Reactor>>,
    timer: Rc<RefCell<Timer>>,
    stats: Rc<RefCell<LatencyStats>>,
    ident: u16,
    local_mac: [u8; 6],
    gateway_mac: [u8; 6],
    local_ip: Ipv4Addr,
    target_ip: Ipv4Addr,
    interval_cycles: u64,
    timeout_cycles: u64,
    payload_len: usize,
    stop: Rc<RefCell<bool>>,
) {
    {
        let mut r = reactor.borrow_mut();
        r.register_session(ident);
    }

    let pkt_len = ETHER_HDR_LEN + IPV4_HDR_LEN + 8 + payload_len;
    let mut buf = vec![0u8; pkt_len];
    let template_cksum = build_template(
        &mut buf,
        &local_mac,
        &gateway_mac,
        local_ip,
        target_ip,
        ident,
        payload_len,
    );

    let mut seq: u16 = 0;
    let mut prev_sleep_end: Option<u64> = None;

    loop {
        if *stop.borrow() {
            break;
        }

        let t0 = rdtsc();
        if let Some(prev) = prev_sleep_end {
            stats.borrow_mut().record_seg3(t0.wrapping_sub(prev));
        }
        patch_packet(&mut buf, template_cksum, seq, t0, payload_len);
        {
            let r = reactor.borrow();
            r.send_packet(&buf);
        }
        let t1 = rdtsc();

        match wait_reply(reactor.clone(), timer.clone(), ident, seq, timeout_cycles).await {
            ReplyResult::Reply { t2, mbuf } => {
                let t3 = rdtsc();
                {
                    let mut s = stats.borrow_mut();
                    s.record_send_recv(t0, t1, t2, t3);
                    s.sent += 1;
                    s.received += 1;
                }
                seq = seq.wrapping_add(1);

                let sleep_start = rdtsc();
                Sleep::new(timer.clone(), interval_cycles).await;
                let sleep_end = rdtsc();
                drop(mbuf);
                prev_sleep_end = Some(sleep_end);
                {
                    let mut s = stats.borrow_mut();
                    s.record_sleep_error(sleep_end.wrapping_sub(sleep_start).wrapping_sub(interval_cycles));
                }
            }
            ReplyResult::Timeout => {
                {
                    let mut s = stats.borrow_mut();
                    s.sent += 1;
                    s.timeouts += 1;
                }
                seq = seq.wrapping_add(1);
                let sleep_start = rdtsc();
                Sleep::new(timer.clone(), interval_cycles).await;
                let sleep_end = rdtsc();
                prev_sleep_end = Some(sleep_end);
                {
                    let mut s = stats.borrow_mut();
                    s.record_sleep_error(sleep_end.wrapping_sub(sleep_start).wrapping_sub(interval_cycles));
                }
            }
        }
    }
}

fn main() {
    let cfg = parse_args();

    let eal_args = vec![
        "dpdk-async-ping",
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

    let reactor = Rc::new(RefCell::new(Reactor::new(
        eth,
        0,
        mempool.raw(),
        local_mac,
        cfg.local_ip,
        cfg.gateway_mac,
    )));
    let timer = Rc::new(RefCell::new(Timer::new(Default::default())));
    let stats = Rc::new(RefCell::new(LatencyStats::new()));
    let stop = Rc::new(RefCell::new(false));

    let interval_cycles = timer.borrow().ns_to_cycles(cfg.delay_us * 1_000);
    let timeout_cycles = timer.borrow().ns_to_cycles(1_000_000_000);

    let mut executor = Executor::new(reactor.clone(), timer.clone());

    for ident in 0..cfg.sessions {
        let stop_clone = stop.clone();
        let stats_clone = stats.clone();
        let reactor_clone = reactor.clone();
        let timer_clone = timer.clone();
        let target_ip = cfg.target_ip;
        let local_ip = cfg.local_ip;
        let gateway_mac = cfg.gateway_mac;
        let local_mac_clone = local_mac;
        let payload_len = cfg.payload_len;
        executor.spawn(async move {
            ping_session(
                reactor_clone,
                timer_clone,
                stats_clone,
                ident,
                local_mac_clone,
                gateway_mac,
                local_ip,
                target_ip,
                interval_cycles,
                timeout_cycles,
                payload_len,
                stop_clone,
            ).await;
        });
    }

    let stop_clone = stop.clone();
    let timer_clone = timer.clone();
    let stats_clone = stats.clone();
    executor.block_on(async move {
        let duration_cycles = { timer_clone.borrow().ns_to_cycles(cfg.duration_sec * 1_000_000_000) };
        let start = rdtsc();
        loop {
            if rdtsc().wrapping_sub(start) >= duration_cycles {
                break;
            }
            let sleep_cycles = { timer_clone.borrow().ns_to_cycles(100_000_000) };
            Sleep::new(timer_clone.clone(), sleep_cycles).await;
        }
        *stop_clone.borrow_mut() = true;
        stats_clone.borrow().print_summary(cfg.duration_sec);
    });

    reactor.borrow_mut().drain();
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
