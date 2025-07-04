use std::io;

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

