with import <nixpkgs> {};

mkShell {
	buildInputs = [ grub2 xorriso qemu rustup ];
}
