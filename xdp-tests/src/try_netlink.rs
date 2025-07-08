#![cfg(test)]

#[test]
fn test_list_routes() {
    let all_routes = xdp_util::get_ipv4_routes(None).unwrap();
    let gw = xdp_util::find_default_gateway(&all_routes).unwrap();
    println!("default GW: {:#?} ", gw);
    let routes = xdp_util::get_ipv4_routes(Some(gw.if_index)).unwrap();
    println!("{:#?}", routes);
}

#[test]
fn list_neighbors() {
    let all_routes = xdp_util::get_ipv4_routes(None).unwrap();
    let gw = xdp_util::find_default_gateway(&all_routes).unwrap();
    let neighbors = xdp_util::get_neighbors(Some(gw.if_index)).unwrap();
    for n in neighbors {
        println!("Neighbor: {:#?}", n);
    }
}

#[test]
fn test_list_addresses() {
    let addr = xdp_util::get_ipv4_address(None).unwrap();
    println!("Addresses: {:#?} ", addr);
}

#[test]
fn test_list_links() {
    let links = xdp_util::get_links().unwrap();
    for link in links {
        println!("Link: {:#?}", link);
    }
}
