#!/usr/bin/env bash

NOGRAPHIC=${NOGRAPHIC:-0}
DUMP_NETWORK=${DUMP_NETWORK:-0}
TAP_IF=${TAP_IF:-tap0}
GDB=${GDB:-0}
NUM_CORES=${NUM_CORES:-4}

if [ "$NOGRAPHIC" == "0" ]; then
  STDIO_CMD="-serial stdio"
else
  STDIO_CMD="-nographic"
fi

if [ "$DUMP_NETWORK" == "0" ]; then
  DUMP_NET_CMD=""
else
  DUMP_NET_CMD="-object filter-dump,id=n0,netdev=n0,file=network.dump"
fi

if [ "$GDB" == "0" ]; then
  GDB_CMD=""
else
  GDB_CMD="-s -S"
fi

KERNEL="$1"
rm -fr isodir
mkdir -p isodir/boot/grub
cp "$KERNEL" isodir/boot/myos.bin
cp grub.cfg isodir/boot/grub/grub.cfg
grub-mkrescue -o myos.iso isodir 2> /dev/null

qemu-system-i386 $GDB_CMD $STDIO_CMD $DUMP_NET_CMD -netdev tap,id=n0,ifname=$TAP_IF,script=no,downscript=no -device rtl8139,netdev=n0,bus=pci.0,addr=4,mac=12:34:56:78:9a:bc -device isa-debug-exit,iobase=0xf4,iosize=0x01 -cdrom myos.iso -smp $NUM_CORES -enable-kvm -cpu host -usb -device usb-mouse,bus=usb-bus.0,port=2

exit $(($? >> 1))
