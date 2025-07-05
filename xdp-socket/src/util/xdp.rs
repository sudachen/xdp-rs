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
