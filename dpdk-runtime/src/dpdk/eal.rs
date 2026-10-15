use crate::dpdk::ffi;
use std::ffi::CString;
use std::os::raw::c_int;

pub struct Eal;

impl Eal {
    pub fn init(args: &[&str]) -> c_int {
        let cstrings: Vec<CString> = args
            .iter()
            .map(|s| CString::new(*s).unwrap())
            .collect();
        let mut c_args: Vec<*mut i8> = cstrings
            .iter()
            .map(|s| s.as_ptr() as *mut i8)
            .collect();
        let argc = c_args.len() as c_int;
        let argv = c_args.as_mut_ptr();
        unsafe { ffi::rte_eal_init(argc, argv) }
    }

    pub fn cleanup() -> c_int {
        unsafe { ffi::rte_eal_cleanup() }
    }

    pub fn socket_id() -> i32 {
        unsafe { ffi::rte_socket_id() as i32 }
    }
}
