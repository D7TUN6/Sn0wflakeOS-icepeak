#ifndef SNF_FCNTL_H
#define SNF_FCNTL_H

#include <sys/types.h>

#define O_RDONLY 00000
#define O_WRONLY 00001
#define O_RDWR 00002
#define O_ACCMODE 00003
#define O_CREAT 00100
#define O_EXCL 00200
#define O_TRUNC 01000
#define O_APPEND 02000

int open(const char *path, int flags, ...);

#endif
