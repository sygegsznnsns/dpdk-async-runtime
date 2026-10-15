use crate::net::ethernet::ETHERTYPE_IPV4;
use std::net::Ipv4Addr;

pub const ARP_HDR_LEN: usize = 28;
pub const ARP_OP_REQUEST: u16 = 1;
pub const ARP_OP_REPLY: u16 = 2;

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct ArpHdr {
    pub hw_type: u16,
    pub proto_type: u16,
    pub hw_len: u8,
    pub proto_len: u8,
    pub opcode: u16,
    pub sender_mac: [u8; 6],
    pub sender_ip: [u8; 4],
    pub target_mac: [u8; 6],
    pub target_ip: [u8; 4],
}

impl ArpHdr {
    pub fn from_bytes(data: &[u8]) -> &ArpHdr {
        unsafe { &*(data.as_ptr() as *const ArpHdr) }
    }

    pub fn from_bytes_mut(data: &mut [u8]) -> &mut ArpHdr {
        unsafe { &mut *(data.as_mut_ptr() as *mut ArpHdr) }
    }

    pub fn set_request(&mut self, sender_mac: &[u8; 6], sender_ip: Ipv4Addr, target_ip: Ipv4Addr) {
        self.hw_type = 1u16.to_be();
        self.proto_type = ETHERTYPE_IPV4.to_be();
        self.hw_len = 6;
        self.proto_len = 4;
        self.opcode = ARP_OP_REQUEST.to_be();
        self.sender_mac = *sender_mac;
        self.sender_ip = sender_ip.octets();
        self.target_mac = [0; 6];
        self.target_ip = target_ip.octets();
    }

    pub fn set_reply(&mut self, sender_mac: &[u8; 6], sender_ip: Ipv4Addr, target_mac: &[u8; 6], target_ip: Ipv4Addr) {
        self.hw_type = 1u16.to_be();
        self.proto_type = ETHERTYPE_IPV4.to_be();
        self.hw_len = 6;
        self.proto_len = 4;
        self.opcode = ARP_OP_REPLY.to_be();
        self.sender_mac = *sender_mac;
        self.sender_ip = sender_ip.octets();
        self.target_mac = *target_mac;
        self.target_ip = target_ip.octets();
    }

    pub fn opcode(&self) -> u16 {
        u16::from_be(self.opcode)
    }

    pub fn sender_ip(&self) -> Ipv4Addr {
        Ipv4Addr::from(self.sender_ip)
    }

    pub fn target_ip(&self) -> Ipv4Addr {
        Ipv4Addr::from(self.target_ip)
    }

    pub fn sender_mac(&self) -> &[u8; 6] {
        &self.sender_mac
    }

    pub fn target_mac(&self) -> &[u8; 6] {
        &self.target_mac
    }
}
