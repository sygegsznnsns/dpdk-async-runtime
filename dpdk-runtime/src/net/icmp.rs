pub const ICMP_ECHO_REQUEST: u8 = 8;
pub const ICMP_ECHO_REPLY: u8 = 0;
pub const ICMP_HDR_LEN: usize = 8;

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct IcmpHdr {
    pub icmp_type: u8,
    pub code: u8,
    pub checksum: u16,
    pub echo_ident: u16,
    pub echo_seq: u16,
}

impl IcmpHdr {
    pub fn from_bytes(data: &[u8]) -> &IcmpHdr {
        unsafe { &*(data.as_ptr() as *const IcmpHdr) }
    }

    pub fn from_bytes_mut(data: &mut [u8]) -> &mut IcmpHdr {
        unsafe { &mut *(data.as_mut_ptr() as *mut IcmpHdr) }
    }

    pub fn set_echo_request(&mut self, ident: u16, seq: u16) {
        self.icmp_type = ICMP_ECHO_REQUEST;
        self.code = 0;
        self.checksum = 0;
        self.echo_ident = ident.to_be();
        self.echo_seq = seq.to_be();
    }

    pub fn icmp_type(&self) -> u8 {
        self.icmp_type
    }

    pub fn echo_ident(&self) -> u16 {
        u16::from_be(self.echo_ident)
    }

    pub fn echo_seq(&self) -> u16 {
        u16::from_be(self.echo_seq)
    }

    pub fn to_bytes(&self) -> [u8; ICMP_HDR_LEN] {
        unsafe { std::mem::transmute(*self) }
    }
}

pub fn icmp_checksum(data: &[u8]) -> u16 {
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

/// RFC 1624 incremental checksum update.
/// Given the old checksum (host order, as returned by `icmp_checksum`)
/// and a 16-bit word change from `old_word` to `new_word` (both host order,
/// as the checksum routine sees them via `from_be_bytes`), returns the
/// updated checksum in host order.
pub fn rfc1624_update(old_cksum: u16, old_word: u16, new_word: u16) -> u16 {
    let hc = old_cksum as u32;
    let m = old_word as u32;
    let m_prime = new_word as u32;
    let mut sum = (!hc & 0xffff) + (!m & 0xffff) + m_prime;
    while (sum >> 16) != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}
