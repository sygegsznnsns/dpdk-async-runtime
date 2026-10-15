pub const ETHERTYPE_IPV4: u16 = 0x0800;
pub const ETHERTYPE_ARP: u16 = 0x0806;
pub const ETHER_HDR_LEN: usize = 14;

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct EtherHdr {
    pub d_mac: [u8; 6],
    pub s_mac: [u8; 6],
    pub ether_type: u16,
}

impl EtherHdr {
    pub fn from_bytes(data: &[u8]) -> &EtherHdr {
        unsafe { &*(data.as_ptr() as *const EtherHdr) }
    }

    pub fn from_bytes_mut(data: &mut [u8]) -> &mut EtherHdr {
        unsafe { &mut *(data.as_mut_ptr() as *mut EtherHdr) }
    }

    pub fn set(&mut self, dst: &[u8; 6], src: &[u8; 6], ether_type: u16) {
        self.d_mac = *dst;
        self.s_mac = *src;
        self.ether_type = ether_type.to_be();
    }

    pub fn ether_type(&self) -> u16 {
        u16::from_be(self.ether_type)
    }
}
