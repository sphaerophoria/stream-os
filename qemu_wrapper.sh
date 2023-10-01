#!/usr/bin/env bash

NOGRAPHIC=${NOGRAPHIC:-0}
DUMP_NETWORK=${DUMP_NETWORK:-0}
TAP_IF=${TAP_IF:-tap0}

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


qemu-system-i386 $STDIO_CMD $DUMP_NET_CMD -netdev tap,id=n0,ifname=$TAP_IF,script=no,downscript=no -device rtl8139,netdev=n0,bus=pci.0,addr=4,mac=12:34:56:78:9a:bc -device isa-debug-exit,iobase=0xf4,iosize=0x01 -kernel "$1"

exit $(($? >> 1))
