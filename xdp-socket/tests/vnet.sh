#!/usr/bin/bash

if [ "$(id -u)" -ne 0 ]; then
    echo "This script must be run as root"
    exit 1
fi
# check if interfaces already exist
if [ "$1" == "remove" ]  || [ "$1" == "del" ]; then
    # remove them
    ip netns del host1
    ip netns del host2
    ip netns del router
    exit 0
fi


echo "Creating network namespaces: host1, host2, router"
ip netns add host1
ip netns add host2
ip netns add router
echo "... Done"

echo "Creating veth pairs..."
ip link add veth1 type veth peer name vebr1
ip link add veth2 type veth peer name vebr2
echo "... Done"

echo "Connecting interfaces to namespaces..."
# veth1 goes to host1
ip link set veth1 netns host1
# veth2 goes to host2
ip link set veth2 netns host2
# vebr1 and vebr2 go to the router
ip link set vebr1 netns router
ip link set vebr2 netns router
echo "... Done"

echo "Configuring host1..."
# Bring up the loopback interface
ip netns exec host1 ip link set lo up
# Bring up the veth1 interface
ip netns exec host1 ip link set veth1 up
# Assign an IP address to veth1 in the 10.0.1.0/24 network
ip netns exec host1 ip addr add 10.0.1.1/24 dev veth1
# Add a default route: all traffic from host1 should go to the router's IP (10.0.1.254)
ip netns exec host1 ip route add default via 10.0.1.254
echo "... Done"

echo "Configuring host2..."
# Bring up the loopback interface
ip netns exec host2 ip link set lo up
# Bring up the veth2 interface
ip netns exec host2 ip link set veth2 up
# Assign an IP address to veth2 in the 10.0.2.0/24 network
ip netns exec host2 ip addr add 10.0.2.1/24 dev veth2
# Add a default route: all traffic from host2 should go to the router's IP (10.0.2.254)
ip netns exec host2 ip route add default via 10.0.2.254
echo "... Done"

echo "Configuring the router..."
# Bring up the loopback interface
ip netns exec router ip link set lo up
# Bring up the vebr1 interface
ip netns exec router ip link set vebr1 up
# Assign the gateway IP for the 10.0.1.0/24 network
ip netns exec router ip addr add 10.0.1.254/24 dev vebr1
# Bring up the vebr2 interface
ip netns exec router ip link set vebr2 up
# Assign the gateway IP for the 10.0.2.0/24 network
ip netns exec router ip addr add 10.0.2.254/24 dev vebr2
echo "... Done"

echo "Enabling IP forwarding in the router namespace..."
ip netns exec router sysctl -w net.ipv4.ip_forward=1
echo "... Done"

echo "Setup is complete!"
echo "You can now test connectivity."