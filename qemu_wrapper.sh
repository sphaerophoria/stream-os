#!/usr/bin/env bash

qemu-system-i386 -serial stdio -netdev user,id=n0 -device rtl8139,netdev=n0,bus=pci.0,addr=4,mac=12:34:56:78:9a:bc -device isa-debug-exit,iobase=0xf4,iosize=0x01 -kernel "$1"

exit $(($? >> 1))
