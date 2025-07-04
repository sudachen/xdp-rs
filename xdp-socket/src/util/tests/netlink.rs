#![cfg(test)]


#[test]
fn test_list_routes() {
    let all_routes = crate::util::get_ipv4_routes(None).unwrap();
    let gw = crate::util::find_default_gateway(&all_routes).unwrap();
    println!("default GW: {:#?} ", gw);
    let routes = crate::util::get_ipv4_routes(Some(gw.if_index)).unwrap();
    println!("{:#?}", routes);
}

#[test]
fn list_neighbors() {
    let all_routes = crate::util::get_ipv4_routes(None).unwrap();
    let gw = crate::util::find_default_gateway(&all_routes).unwrap();
    let neighbors = crate::util::get_neighbors(Some(gw.if_index)).unwrap();
    for n in neighbors {
        println!("Neighbor: {:#?}", n);
    }
}

#[test]
fn test_list_addresses() {
    let addr = crate::util::get_ipv4_address(None).unwrap();
    println!("Addresses: {:#?} ", addr);
}

#[test]
fn test_list_links() {
    let links = crate::util::get_links().unwrap();
    for link in links {
        println!("Link: {:#?}", link);
    }
}
