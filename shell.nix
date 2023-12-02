with import <nixpkgs> {};

mkShell {
	nativeBuildInputs = [ rustPlatform.bindgenHook ];
	buildInputs = [ grub2 xorriso qemu rustup python3 gdb ];
}
