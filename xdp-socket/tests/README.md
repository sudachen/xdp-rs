

You need to add this line in ./etc/sudoers:
`%netadmins   ALL=(ALL) NOPASSWD: /usr/sbin/ip`
 
and add the user to the netadmins group:
`$ sudo usermod -aG netadmins $USER`

to set count of huge pages available in system use:
`sudo sysctl -w vm.nr_hugepages=<your number>`

