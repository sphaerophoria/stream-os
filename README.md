
<div align = center >

# StreamOS

*Toy operating system written for fun,*  
*live developed on Twitch / YouTube.*

<br>

[![Button YouTube]][YouTube]  
[![Button Twitch]][Twitch]

</div>

<br>
<br>

## Goal

***There is no real goal***, other than understanding computers better.  
We implement what & when we feel like it, with no expectations.

<br>
<br>

## Status

<kbd> <br> Boots <br> </kbd>  
<kbd> <br> Memory Allocation <br> </kbd>  
<kbd> <br> Async / Await <br> </kbd>  
<kbd> <br> Serial Logging <br> </kbd>

<kbd> <br> Unit Testing <br> </kbd>  
<kbd> <br> RTC ( Clock ) <br> </kbd>  
<kbd> <br> PCI <br> </kbd>  
<kbd> <br> Ethernet <br> </kbd>  
<kbd> <br> ARP <br> </kbd>

<kbd> <br> UDP <br> </kbd>  
<kbd> <br> TCP ( Kinda ) <br> </kbd>  
<kbd> <br> HTTP <br> </kbd>  
<kbd> <br> Graphics <br> </kbd>  
<kbd> <br> Keyboard <br> </kbd>

<kbd> <br> Multicore <br> </kbd>  
<kbd> <br> USB ( 1.1 | No Hub | Mouse Only ) <br> </kbd>  
<kbd> <br> UDP <br> </kbd>  

<br>
<br>

## Usage

To an extend dependencies are tracked by [`shell.nix`]

Set up a tap device for host ⬌ guest networking, e.g.

```sh
nmcli connection add type tun ifname tap0   \
    con-name tap0 mode tap owner `id -u`    \
    ipv4.method manual ip4 192.168.2.1/24
```

Check environment variables in [`qemu_wrapper.sh`] for configuration

```
cargo run --release
```

<br>


<!----------------------------------------------------------------------------->

[`qemu_wrapper.sh`]: ./qemu_wrapper.sh
[`shell.nix`]: ./shell.nix

[YouTube]: https://youtube.com/@sphaerophoria
[Twitch]: https://twitch.tv/sphaerophoria

[Button YouTube]: https://img.shields.io/badge/YouTube-FF0000?style=for-the-badge&logoColor=white&logo=YouTube
[Button Twitch]: https://img.shields.io/badge/Twitch-9146FF?style=for-the-badge&logoColor=white&logo=Twitch
