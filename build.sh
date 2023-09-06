#!/usr/bin/env bash

set -ex

CROSS_COMPILE=~/opt/gcc-cross/bin/i686-elf-

"${CROSS_COMPILE}"as boot.s -o boot.o
"${CROSS_COMPILE}"gcc -c kernel.c -o kernel.o -std=gnu99 -ffreestanding -O2 -Wall -Wextra
"${CROSS_COMPILE}"gcc -T linker.ld -o myos.bin -ffreestanding -O2 -nostdlib boot.o kernel.o -lgcc

rm -r isodir
mkdir -p isodir/boot/grub
cp myos.bin isodir/boot/myos.bin
cp grub.cfg isodir/boot/grub/grub.cfg
grub-mkrescue -o myos.iso isodir
