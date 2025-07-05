use crate::util::xdp::{OwnedXdpProg, xdp_attach_program};
use include_bytes_aligned::include_bytes_aligned;
use std::io;

const XDP_PASS_CODE: &[u8] = include_bytes_aligned!(16, "../../xdp-pass.o");
const XDP_PASS_PROG: &str = "xdp_pass";

pub fn xdp_attach_pass_program(if_index: u32) -> io::Result<OwnedXdpProg> {
    xdp_attach_program(if_index, XDP_PASS_CODE, XDP_PASS_PROG)
}
