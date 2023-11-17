#pragma once

#include <stddef.h>
#include <stdarg.h>

struct FILE;

typedef struct FILE FILE;

extern FILE* stdout;
extern FILE* stderr;
extern FILE* stdin;

int printf(const char *restrict format, ...);
int      fprintf(FILE *restrict, const char *restrict, ...);
int      snprintf(char *restrict, size_t, const char *restrict, ...);
int vfprintf(FILE *restrict stream,
	   const char *restrict format, va_list ap);
int vsnprintf(char *restrict str, size_t size,
	   const char *restrict format, va_list ap);
int rename(const char *old, const char *new);
int remove(const char *pathname);

FILE    *fopen(const char *restrict, const char *restrict);
int      fclose(FILE *);
size_t fread(void *restrict ptr, size_t size, size_t nmemb,
	    FILE *restrict stream);
long ftell(FILE *stream);
int fflush(FILE *stream);
int fseek(FILE *stream, long offset, int whence);
size_t fwrite(const void *restrict ptr, size_t size, size_t nmemb,
	    FILE *restrict stream);

int puts(const char *s);
int putchar(int c);

int sscanf(const char *restrict str,
	  const char *restrict format, ...);


#define SEEK_END 3
#define SEEK_SET 2
#define SEEK_CUR 1
