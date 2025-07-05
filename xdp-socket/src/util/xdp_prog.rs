//! # XDP Feature Querying
//!
//! ## Purpose
//!
//! This file provides a utility function to query the XDP features supported by a
//! given network interface driver. This allows an application to know if capabilities
//! like zero-copy are available.
//!
//! ## How it works
//!
//! It acts as a thin wrapper around the `libbpf_sys::bpf_xdp_query` function. It takes
//! a network interface index, calls the underlying libbpf function to query the driver's
//! XDP capabilities, and returns the result as a bitmask of feature flags.
//!
//! ## Main components
//!
//! - `xdp_features()`: The sole function that calls into `libbpf-sys` to perform the
//!   XDP feature query.

use std::io;
use std::mem::size_of;

/// Queries the XDP feature flags supported by a network interface driver.
///
/// This function is a safe wrapper around the `libbpf_sys::bpf_xdp_query` C function.
/// It determines which XDP features (e.g., zero-copy) are supported by the driver
/// for the specified interface.
///
/// # Arguments
/// * `if_index` - The index of the network interface to query.
///
/// # Returns
/// A `Result` containing a bitmask of `XDP_FEATURE_` flags on success, or an
/// `io::Error` on failure.
pub fn xdp_features(if_index: u32) -> io::Result<u32> {
    Ok(unsafe {
        let mut opts: libbpf_sys::bpf_xdp_query_opts = std::mem::zeroed();
        opts.sz = size_of::<libbpf_sys::bpf_xdp_query_opts>() as u64;
        if libbpf_sys::bpf_xdp_query(
            if_index as libc::c_int,
            libbpf_sys::XDP_FLAGS_DRV_MODE as libc::c_int,
            &mut opts,
        ) < 0
        {
            return Err(io::Error::other(format!(
                "Failed to query XDP features: {}",
                io::Error::last_os_error()
            )));
        }
        opts.feature_flags as u32
    })
}

pub fn xdp_attach_program(if_index: u32, code: &'static [u8], name: &'static str) -> io::Result<OwnedXdpProg> {

    let mut owned_prog = OwnedXdpProg {
        if_index,
        code,
        name,
        bpf_obj: std::ptr::null_mut(),
        bpf_link: std::ptr::null_mut(),
    };

    let bpf_obj = &mut owned_prog.bpf_obj;
    let bpf_link= &mut owned_prog.bpf_link;

    unsafe {
        let mut opts: libbpf_sys::bpf_object_open_opts = std::mem::zeroed();
        opts.sz = size_of::<libbpf_sys::bpf_object_open_opts>() as u64;
        *bpf_obj = libbpf_sys::bpf_object__open_mem(
            code.as_ptr() as *const std::ffi::c_void,
            code.len() as libbpf_sys::size_t,
            &opts,
        );

        if bpf_obj.is_null() {
            return Err(io::Error::other("Failed to open BPF object from memory"));
        }

        if 0 != libbpf_sys::bpf_object__load(*bpf_obj) {
            return Err(io::Error::other("Failed to load BPF object"));
        }

        let prog_name_cstr = std::ffi::CString::new(name)?;
        let bpf_prog =
            libbpf_sys::bpf_object__find_program_by_name(*bpf_obj, prog_name_cstr.as_ptr());
        if bpf_prog.is_null() {
            return Err(io::Error::other(format!("Failed to find BPF program '{}'",name)));
        }

        *bpf_link = libbpf_sys::bpf_program__attach_xdp(bpf_prog, if_index as i32);
        if bpf_link.is_null() {
            return Err(io::Error::other("Failed to attach XDP program"));
        }
    };

    Ok(owned_prog)
}

pub struct OwnedXdpProg {
    pub if_index: u32,
    pub code: &'static [u8],
    pub name: &'static str,
    pub bpf_obj: *mut libbpf_sys::bpf_object,
    pub bpf_link: *mut libbpf_sys::bpf_link
}

impl Drop for OwnedXdpProg {
    fn drop(&mut self) {
        if !self.bpf_link.is_null() {
            unsafe { libbpf_sys::bpf_link__destroy(self.bpf_link) };
        }
        if !self.bpf_obj.is_null() {
            unsafe { libbpf_sys::bpf_object__close(self.bpf_obj) };
        }
    }
}
