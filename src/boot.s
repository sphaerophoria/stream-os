/* Declare constants for the multiboot header. */
.set MULTIBOOT2_MAGIC,    0xE85250D6       /* multiboot 2 magic */
.set MULTIBOOT2_ARCHITECTURE, 0 /*i386 */

/*
Declare a multiboot2 header that marks the program as a kernel. These are magic
values that are documented in the multiboot2 standard. The bootloader will
search for this signature in the first 8 KiB of the kernel file, aligned at a
32-bit boundary. The signature is in its own section so the header can be
forced to be within the first 8 KiB of the kernel file.
*/
.section .multiboot
.align 4
.long MULTIBOOT2_MAGIC
.long MULTIBOOT2_ARCHITECTURE
/* Set size to 0, because alignment makes size calculation tricky, and grub
 * doesn't seem to care that it's wrong anyways*/
.long 0
.long 1<<32 - MULTIBOOT2_MAGIC - MULTIBOOT2_ARCHITECTURE

/*Framebuffer tag*/
.align 8
.short 5 /* type 5 */
.short 0 /* Don't ignore me */
.long 20 /* size 20 */
.long 640 /* 640 width */
.long 480 /* 480 height */
.long 32 /* 8 bits per channel */

/*Terminator tag */
.align 8
.short 0
.short 0
.long 8

.set MAX_NUM_CPUS, 8
.set STACK_SIZE, 16384

.section .bss
.align 16
stack_bottom:
.skip STACK_SIZE * MAX_NUM_CPUS
stack_top:
.skip 4 # We define stack top as the last element in our stack, but this is after all allocated space. Add another 4 bytes for one more element

/* clobbers eax, ebx, ecx, edx, and esp */
.macro set_cpu_stack
    mov     $1, %eax
    cpuid
    shrl    $24, %ebx
    add     $1, %ebx
    mov     $STACK_SIZE, %eax
    mul     %ebx
    add     $stack_bottom, %eax
    mov     %eax, %esp
.endmacro

/*
The linker script specifies _start as the entry point to the kernel and the
bootloader will jump to this position once the kernel has been loaded. It
doesn't make sense to return from this function as the bootloader is gone.
*/
.section .text
.global _start
.type _start, @function
_start:
	/* Stash multiboot info before clobbering registers when setting up our
	 * stack. Note that while we do not know which section of our stack we
	 * want to use for this CPU, we are still writing to valid memory. Our
	 * other CPUs haven't booted yet, so if we're wrong we don't care */
	mov %eax,stack_top
	mov %ebx,stack_top - 4

	set_cpu_stack

	/* Pull multiboot info back from where we put it, and push it to the
	 * stack where we wanted it*/
	mov (stack_top), %eax
	mov (stack_top - 4), %ebx
	push %ebx
	push %eax


	/*
	Enter the high-level kernel. The ABI requires the stack is 16-byte
	aligned at the time of the call instruction (which afterwards pushes
	the return pointer of size 4 bytes). The stack was originally 16-byte
	aligned above and we've pushed a multiple of 16 bytes to the
	stack since (pushed 0 bytes so far), so the alignment has thus been
	preserved and the call is well defined.
	*/
	call kernel_main

	/*
	If the system has nothing more to do, put the computer into an
	infinite loop. To do that:
	1) Disable interrupts with cli (clear interrupt enable in eflags).
	   They are already disabled by the bootloader, so this is not needed.
	   Mind that you might later enable interrupts and return from
	   kernel_main (which is sort of nonsensical to do).
	2) Wait for the next interrupt to arrive with hlt (halt instruction).
	   Since they are disabled, this will lock up the computer.
	3) Jump to the hlt instruction if it ever wakes up due to a
	   non-maskable interrupt occurring or due to system management mode.
	*/
	cli
1:	hlt
	jmp 1b


/*
Set the size of the _start symbol to the current location '.' minus its start.
This is useful when debugging or when you implement call tracing.
*/
.size _start, . - _start


.global ap_trampoline
.type ap_trampoline, @function

/* Loaded to 0x8000 at runtime */
.set LGDT_ADDR, load_gdt - ap_trampoline + 0x8000
.set GDT_ADDR, GDT_value - ap_trampoline + 0x8000
.set AP_PROTECTED_CODE_ADDR, ap_trampoline_protected - ap_trampoline + 0x8000
/* Trampoline starts in real mode */
    .code16
ap_trampoline:
    cli
    cld
    ljmp    $0, $LGDT_ADDR
    .align 16
GDT_table:
    /* Values for GDT table were stolen from our calculated GDT in gdt::init()
     * in rust code. I assume _this_ table has to be in low memory, as we only
     * have 16 bits to work with. It might make more sense for us to initialize
     * the GDT in rust code and then copy it to 0x8000 - GDT_size or something,
     * but this is good enough for now */
    .long  0x0, 0x0
    .long  0xffff, 0xcf9900
    .long  0xffff, 0xcf9300
GDT_value:
    .word GDT_value - GDT_table - 1
    .long GDT_table - ap_trampoline + 0x8000
    .long 0, 0
    .align 64
load_gdt:
    /* Load gdt */
    xorw    %ax, %ax
    movw    %ax, %ds
    lgdtl   GDT_ADDR

    /* Move into protected mode */
    movl    %cr0, %eax
    orl     $1, %eax
    movl    %eax, %cr0

    ljmp    $8, $AP_PROTECTED_CODE_ADDR
    .align 32
    .code32
ap_trampoline_protected:
    movw    $16, %ax
    movw    %ax, %ds
    movw    %ax, %ss
    set_cpu_stack
    ljmp    $8, $ap_startup
ap_trampoline_end:

.data
.global ap_trampoline_size
.align 4
ap_trampoline_size:
    .long ap_trampoline_end - ap_trampoline
.global max_num_cpus
max_num_cpus:
    .long MAX_NUM_CPUS

