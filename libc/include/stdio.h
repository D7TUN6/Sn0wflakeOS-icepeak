#ifndef SNF_STDIO_H
#define SNF_STDIO_H

#include <sys/types.h>

int printf(const char *fmt, ...);
int fprintf(void *stream, const char *fmt, ...);
int sprintf(char *buf, const char *fmt, ...);
int snprintf(char *buf, size_t size, const char *fmt, ...);
int puts(const char *s);
int putchar(int c);
int getchar(void);
ssize_t getline(char **line, size_t *cap, void *stream);

#define EOF (-1)

#endif
