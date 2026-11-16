#ifndef SNF_UNISTD_H
#define SNF_UNISTD_H

#include <sys/types.h>

#define STDIN_FILENO 0
#define STDOUT_FILENO 1
#define STDERR_FILENO 2

#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2

ssize_t read(int fd, void *buf, size_t count);
ssize_t write(int fd, const void *buf, size_t count);
int close(int fd);
int dup2(int oldfd, int newfd);
off_t lseek(int fd, off_t offset, int whence);
int chdir(const char *path);
ssize_t getcwd(char *buf, size_t size);
void _exit(int status);
unsigned int getuid(void);
unsigned int getgid(void);
int setuid(unsigned int uid, unsigned int gid);
int unlink(const char *path);
int rmdir(const char *path);
int mkdir(const char *path, mode_t mode);
int sleep_ms(unsigned long ms);

#endif
