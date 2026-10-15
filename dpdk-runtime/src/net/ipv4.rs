use std::net::Ipv4Addr;

pub const IPV4_HDR_LEN: usize = 20;
pub const IPPROTO_ICMP: u8 = 1;

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct Ipv4Hdr {
    pub version_ihl: u8,
    pub tos: u8,
    pub total_length: u16,
    pub packet_id: u16,
    pub fragment_offset: u16,
    pub ttl: u8,
    pub protocol: u8,
    pub hdr_checksum: u16,
    pub src_addr: [u8; 4],
    pub dst_addr: [u8; 4],
}

impl Ipv4Hdr {
    pub fn from_bytes(data: &[u8]) -> &Ipv4Hdr {
        unsafe { &*(data.as_ptr() as *const Ipv4Hdr) }
    }

    pub fn from_bytes_mut(data: &mut [u8]) -> &mut Ipv4Hdr {
        unsafe { &mut *(data.as_mut_ptr() as *mut Ipv4Hdr) }
    }

    pub fn set(&mut self, total_length: u16, packet_id: u16, protocol: u8, src: Ipv4Addr, dst: Ipv4Addr) {
        self.version_ihl = (4 << 4) | 5;
        self.tos = 0;
        self.total_length = total_length.to_be();
        self.packet_id = packet_id.to_be();
        self.fragment_offset = 0;
        self.ttl = 64;
        self.protocol = protocol;
        self.hdr_checksum = 0;
        self.src_addr = src.octets();
        self.dst_addr = dst.octets();
        self.hdr_checksum = ipv4_checksum(&self.to_bytes()).to_be();
    }

    pub fn total_length(&self) -> u16 {
        u16::from_be(self.total_length)
    }

    pub fn protocol(&self) -> u8 {
        self.protocol
    }

    pub fn src_addr(&self) -> Ipv4Addr {
        Ipv4Addr::from(self.src_addr)
    }

    pub fn dst_addr(&self) -> Ipv4Addr {
        Ipv4Addr::from(self.dst_addr)
    }

    pub fn to_bytes(&self) -> [u8; IPV4_HDR_LEN] {
        unsafe { std::mem::transmute(*self) }
    }

    pub fn header_len(&self) -> usize {
        ((self.version_ihl & 0x0f) * 4) as usize
    }
}

pub fn ipv4_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < data.len() {
        sum += u16::from_be_bytes([data[i], data[i + 1]]) as u32;
        i += 2;
    }
    if i < data.len() {
        sum += (data[i] as u32) << 8;
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}
