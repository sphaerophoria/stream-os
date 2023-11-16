# Toy OS written for fun

This is an OS written as part of my twitch stream. See development live at https://twitch.tv/sphaerophoria or on youtube at https://youtube.com/@sphaerophoria.

There is no real goal, other than understanding computers better. We will implement what we feel like, when we feel like, with no expectations

## Current state
- Boots
- Memory allocation
- Async/Await
- Serial Logging
- Unit testing
- RTC (clock)
- PCI
- Ethernet
- ARP
- UDP
- TCP (kinda)
- HTTP
- Graphics
- Keyboard
- Multicore
- USB (1.1, no hub, mouse only)

## Usage

Dependencies are tracked by shell.nix (to an extent)

Set up a tap device for host\<-\>guest networking, e.g.
```
nmcli connection add type tun ifname tap0 con-name tap0 mode tap owner `id -u` ipv4.method manual ip4 192.168.2.1/24
```

Check environment variables in `qemu_wrapper.sh` for configuration

```
cargo run --release
```
