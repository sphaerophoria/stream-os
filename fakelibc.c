#include <stddef.h>
#include <stdio.h>
#include <sys/types.h>
#include <stdint.h>
#include <stdbool.h>

struct PrintfParser;
struct PrintfParser* printf_parser_new(const char* format_string);
struct PrintfParser* printf_parser_new_with_buf(const char* format_string, char* buf, uint32_t size);
void printf_parser_free(struct PrintfParser*);
void printf_parser_push_arg(struct PrintfParser*, const char* arg);
int32_t printf_parser_advance(struct PrintfParser*);

void panic_c(const char*);

int mkdir(const char *path, mode_t mode) {
	// Fuck you, I have no fs
}
char *strstr(const char *haystack, const char *needle) {
	panic_c("strstr unimplemented");
}
char *strchr(const char *s, int c) {
	panic_c("strchr unimplemented");
}
char *strrchr(const char *s, int c) {
	panic_c("strrchr unimplemented");
}
int atoi(const char *nptr) {
	panic_c("atoi unimplemented");
}
double atof(const char *nptr) {
	panic_c("atof unimplemented");
}
int abs(int j) {
	panic_c("abs unimplemented");
}
void exit(int status) {
	panic_c("exit unimplemented");
}
int system(const char *command) {
	// no command can run we have no binaries idiot
	return 1;
}

void print_address(void*);

int printf(const char *restrict format, ...) {

	va_list args;
	va_start(args, format);

	vfprintf(stdout, format, args);

	va_end(args);
}

int      fprintf(FILE *restrict, const char *restrict, ...) {
	panic_c("fprintf unimplemented");
}
int      snprintf(char *restrict buf, size_t size, const char *restrict format, ...) {

	va_list args;
	va_start(args, format);

	vsnprintf(buf, size, format, args);

	va_end(args);
}

int vfprintf(FILE *restrict stream, const char *restrict format, va_list args) {
	struct PrintfParser* parser = printf_parser_new(format);

	do_printf(parser, format, args);

	printf_parser_free(parser);
}

int vsnprintf(char *restrict str, size_t size, const char *restrict format, va_list args) {
	struct PrintfParser* parser = printf_parser_new_with_buf(format, str, size);
	do_printf(parser, format, args);
	printf_parser_free(parser);
}
int rename(const char *old, const char *new) {
	panic_c("rename unimplemented");
}
int remove(const char *pathname) {
	panic_c("remove unimplemented");
}
int fflush(FILE *stream) {
	// No files, no flushing baybee
}
int sscanf(const char *restrict str, const char *restrict format, ...) {
	panic_c("sscanf unimplemented");
}
double fabs(double x) {
	panic_c("fabs unimplemented");
}
int isspace(int c) {
	panic_c("isspace unimplemented");
}

int errno = 0;

void DG_Init() {}
void DG_SleepMs(uint32_t ms) {
}
void DG_SetWindowTitle(const char * title) {}


void do_printf(struct PrintfParser* parser, const char *restrict format, va_list args) {
	int arg_size = 0;

	while (true) {
		arg_size = printf_parser_advance(parser);
		if (arg_size == 0) {
			break;
		}
		char* arg = va_arg_ptr(args, arg_size);
		printf_parser_push_arg(parser, arg);
	}

}


void test_printf(void) {
	char name[32];
	memset(name, '.', 32);
	snprintf(name, sizeof(name), "key_multi_msgplayer%i", 3);
	printf("Test: %s\n", name);
}

void print_long_size(void) {
	printf("size of long, is %d\n", sizeof(long));
}
