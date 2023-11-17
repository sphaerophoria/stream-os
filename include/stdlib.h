#pragma once

#include <stddef.h>

void *malloc(size_t size);
void *calloc(size_t nmemb, size_t size);
void *realloc(void *ptr, size_t size);
void free(void *ptr);
int atoi(const char *nptr);
double atof(const char *nptr);
int abs(int j);

void exit(int status);
int system(const char *command);
