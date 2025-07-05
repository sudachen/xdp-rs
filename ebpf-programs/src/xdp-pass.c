#include <linux/bpf.h>
#include <bpf/bpf_helpers.h>

SEC("xdp")
int xdp_pass(struct xdp_md *ctx) {
    // Log the message to the trace pipe
    return XDP_PASS;
}

char LICENSE[] SEC("license") = "MIT";
