#ifndef SNF_SYS_STAT_H
#define SNF_SYS_STAT_H

#include <sys/types.h>

#define S_IFMT 0170000
#define S_IFDIR 0040000
#define S_IFREG 0100000
#define S_IFLNK 0120000
#define S_ISDIR(m) (((m) & S_IFMT) == S_IFDIR)
#define S_ISREG(m) (((m) & S_IFMT) == S_IFREG)

struct stat {
    unsigned long st_size;
    unsigned short st_mode;
    unsigned int st_uid;
    unsigned int st_gid;
};

int stat(const char *path, struct stat *buf);

#endif
