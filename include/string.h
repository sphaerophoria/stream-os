#pragma once

#include <strings.h>

void    *memset(void *, int, size_t);
void    *memcpy(void *restrict, const void *restrict, size_t);
void    *memmove(void *dest, const void *src, size_t n);

size_t strlen(const char *s);
char *strdup(const char *s);
int strcmp(const char *s1, const char *s2);
char *strstr(const char *haystack, const char *needle);
char *strchr(const char *s, int c);
char *strrchr(const char *s, int c);
int strncmp(const char *s1, const char *s2, size_t n);
char *strncpy(char *restrict dest, const char *restrict src, size_t n);
