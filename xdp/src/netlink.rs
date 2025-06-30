use netlink_packet_core::{
    NLM_F_DUMP, NLM_F_REQUEST, NetlinkDeserializable, NetlinkMessage, NetlinkPayload,
    NetlinkSerializable,
};
use netlink_packet_route::{
    AddressFamily, RouteNetlinkMessage,
    neighbour::{NeighbourAddress, NeighbourAttribute, NeighbourMessage},
    route::{RouteAddress, RouteAttribute, RouteMessage},
    address::{AddressMessage, AddressAttribute},
    link::{LinkMessage,LinkAttribute},
};
use netlink_sys::{Socket, SocketAddr};
use std::io;
use std::net::{IpAddr, Ipv4Addr};

#[derive(Clone,Debug,Default)]
pub struct Link {
    pub if_index: u32,
    pub name: String,
    pub mtu: u32,
    pub mac: [u8; 6],
}

#[derive(Clone, Copy, Debug)]
pub struct Ipv4Route {
    pub dest_prefix: u8,
    pub destination: Ipv4Addr,
    pub gateway: Option<Ipv4Addr>,
    pub out_if_index: Option<u32>,
    pub priority: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct Neighbor {
    pub ip: Ipv4Addr,
    pub mac: [u8; 6],
    pub if_index: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct Gateway {
    pub ip: Ipv4Addr,
    pub priority: u32,
    pub if_index: u32,
}

pub fn netlink<T, F, R>(mut req: NetlinkMessage<T>, f: F) -> Result<Vec<R>, io::Error>
where
    T: NetlinkSerializable + NetlinkDeserializable,
    F: Fn(NetlinkMessage<T>) -> Result<Option<R>, io::Error>,
{
    let mut socket = Socket::new(netlink_sys::constants::NETLINK_ROUTE)?;
    let kernel_addr = SocketAddr::new(0, 0);
    socket.bind(&kernel_addr)?;
    req.header.flags = NLM_F_REQUEST | NLM_F_DUMP;
    let mut send_buf = vec![0u8; req.buffer_len()];
    req.finalize();
    req.serialize(&mut send_buf);
    if socket.send(send_buf.as_slice(), 0)? != send_buf.len() {
        return Err(io::Error::other("Failed to send request"));
    };

    let (recv_buf, _) = socket.recv_from_full()?;
    let mut buffer_view = &recv_buf[..];
    let mut result = Vec::new();
    while !buffer_view.is_empty() {
        let msg = NetlinkMessage::<T>::deserialize(buffer_view).map_err(io::Error::other)?;
        let len = msg.header.length as usize;
        if let Some(r) = f(msg)? {
            result.push(r);
        }
        if len == 0 || len > buffer_view.len() {
            return Err(io::Error::other(
                "Received a malformed netlink message (invalid length)".to_string(),
            ));
        }
        buffer_view = &buffer_view[len..];
    }
    Ok(result)
}

pub fn get_neighbors(if_index: Option<u32>) -> Result<Vec<Neighbor>, io::Error> {
    let mut req_msg = NeighbourMessage::default();
    req_msg.header.family = AddressFamily::Inet;
    let req = NetlinkMessage::from(RouteNetlinkMessage::GetNeighbour(req_msg));
    netlink(req, |msg| match msg.payload {
        NetlinkPayload::InnerMessage(RouteNetlinkMessage::NewNeighbour(neigh_msg)) => {
            if if_index.is_some_and(|x| x != neigh_msg.header.ifindex) {
                return Ok(None); // Skip neighbors not matching the interface index
            }
            let mut neighbor = Neighbor {
                ip: Ipv4Addr::UNSPECIFIED,
                mac: [0; 6],
                if_index: neigh_msg.header.ifindex,
            };
            for attr in neigh_msg.attributes.iter() {
                match attr {
                    NeighbourAttribute::Destination(NeighbourAddress::Inet(ip)) => {
                        neighbor.ip = *ip;
                    }
                    NeighbourAttribute::LinkLocalAddress(mac) => {
                        if mac.len() == 6 {
                            neighbor.mac = mac[0..6].try_into().unwrap();
                        } else {
                            return Ok(None);
                        }
                    }
                    //NeighbourAttribute::CacheInfo(_nfo) => {
                    //
                    //}
                    _ => {}
                }
            }
            Ok(Some(neighbor))
        }
        _ => Ok(None),
    })
}

pub fn get_ipv4_routes(if_index: Option<u32>) -> Result<Vec<Ipv4Route>, io::Error> {
    let mut req_msg = RouteMessage::default();
    req_msg.header.address_family = AddressFamily::Inet;
    //req_msg.header.destination_prefix_length = 32;
    let req = NetlinkMessage::from(RouteNetlinkMessage::GetRoute(req_msg));
    netlink(req, |msg| {
        match msg.payload {
            NetlinkPayload::InnerMessage(RouteNetlinkMessage::NewRoute(ref route_msg)) => {
                let mut route = Ipv4Route {
                    dest_prefix: route_msg.header.destination_prefix_length,
                    destination: Ipv4Addr::UNSPECIFIED,
                    gateway: None,
                    out_if_index: None,
                    priority: 0,
                };
                for a in route_msg.attributes.iter() {
                    match a {
                        RouteAttribute::Destination(RouteAddress::Inet(dest)) => {
                            route.destination = *dest;
                        }
                        RouteAttribute::Gateway(RouteAddress::Inet(gateway)) => {
                            route.gateway = Some(*gateway);
                        }
                        RouteAttribute::Oif(ifd) => {
                            if if_index.is_some_and(|x| x != *ifd) {
                                break; // Skip routes not matching the interface IP
                            }
                            route.out_if_index = Some(*ifd);
                        }
                        RouteAttribute::Priority(priority) => {
                            route.priority = *priority;
                        }
                        _ => {}
                    }
                }
                Ok(route.out_if_index.and(Some(route)))
            }
            _ => Ok(None),
        }
    })
}

pub fn get_ipv4_address(if_index: Option<u32>) -> Result<Vec<(Ipv4Addr,u32)>, io::Error> {
    let mut req_msg = AddressMessage::default();
    req_msg.header.family = AddressFamily::Inet;
    let req = NetlinkMessage::from(RouteNetlinkMessage::GetAddress(req_msg));
    netlink(req, |msg| {
        match msg.payload {
            NetlinkPayload::InnerMessage(RouteNetlinkMessage::NewAddress(ref addr_msg)) => {
                if if_index.is_some_and(|x| x != addr_msg.header.index) {
                    return Ok(None); // Skip addresses not matching the interface index
                }
                for attr in addr_msg.attributes.iter() {
                    if let AddressAttribute::Address(IpAddr::V4(ip)) = attr {
                        return Ok(Some((*ip,addr_msg.header.index)));
                    }
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    })
}

pub fn get_links() -> Result<Vec<Link>, io::Error> {
    let req_msg = LinkMessage::default();
    let req = NetlinkMessage::from(RouteNetlinkMessage::GetLink(req_msg));
    netlink(req, |msg| {
        match msg.payload {
            NetlinkPayload::InnerMessage(RouteNetlinkMessage::NewLink(ref link_msg)) => {
                let mut link = Link {
                    if_index: link_msg.header.index,
                    .. Default::default()
                };
                for attr in link_msg.attributes.iter() {
                    match attr {
                        LinkAttribute::IfName(name) => {
                            link.name = name.to_string();
                        }
                        LinkAttribute::Mtu(mtu) => {
                            link.mtu = *mtu;
                        }
                        LinkAttribute::Address(mac) => {
                            if mac.len() == 6 {
                                link.mac = mac[0..6].try_into().map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
                            } else {
                                return Ok(None);
                            }
                        }
                        _ => {}
                    }
                }
                Ok(Some(link))
            }
            _ => Ok(None),
        }
    })
}

pub fn find_default_gateway(routes: &[Ipv4Route]) -> Option<Gateway> {
    routes
        .iter()
        .fold(None, |acc, x| {
            if let Ipv4Route {
                gateway: Some(gw),
                priority,
                out_if_index: Some(oif),
                dest_prefix: 0,
                ..
            } = x
            {
                match acc {
                    None => Some((*gw, *priority, *oif)),
                    Some((_, acc_priority, _)) if acc_priority < *priority => {
                        Some((*gw, *priority, *oif))
                    }
                    _ => acc,
                }
            } else {
                acc
            }
        })
        .map(|(ip, priority, if_index)| Gateway {
            ip,
            if_index,
            priority,
        })
}

