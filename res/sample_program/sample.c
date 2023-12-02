#include "vtable.h"

static void (*EXIT)(int) = 0;

void exit_2(int code) {
	EXIT(code);
}

static void (*PRINT)(char*) = 0;

void print(char* s) {
	PRINT(s);
}

static void (*PANIC)(char*) = 0;

void panic(char* s) {
	PANIC(s);
}

void print_and_exit(void) {
	print("Hello world\n");
	exit_2(30);
	panic(">:(");
}

void _start(struct vtable* vtable) {
	PRINT = vtable->print;
	EXIT = vtable->exit;
	PANIC = vtable->panic;
	print_and_exit();
}
