/* A small freestanding C library for Sn0wflakeOS.
 *
 * It speaks the icepeak `int 0x80` ABI directly. File descriptors above 2 are
 * implemented on top of the whole-file read/write syscalls: `open` snapshots a
 * file into memory and `close` flushes buffered writes. That is enough for the
 * POSIX programs we ship today and keeps the kernel ABI small while real
 * descriptor tables are added.
 */

#include <errno.h>
#include <fcntl.h>
#include <stdarg.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>

#define SYS_EXIT 0
#define SYS_WRITE 1
#define SYS_READ 2
#define SYS_GETPID 8
#define SYS_SLEEP 9
#define SYS_YIELD 10
#define SYS_MKDIR 12
#define SYS_UNLINK 13
#define SYS_RMDIR 27
#define SYS_CHDIR 16
#define SYS_GETCWD 17
#define SYS_TIME 20
#define SYS_READ_FILE 23
#define SYS_WRITE_FILE 24
#define SYS_EXISTS 25
#define SYS_OPEN 3
#define SYS_CLOSE 4
#define SYS_LSEEK 5
#define SYS_DUP2 45
#define SYS_STAT 29
#define SYS_GETUID 32
#define SYS_GETGID 33
#define SYS_SETUID 34

int errno;

static long sys6(long n, long a, long b, long c, long d, long e, long f) {
    register long r10 __asm__("r10") = d;
    register long r8 __asm__("r8") = e;
    register long r9 __asm__("r9") = f;
    long ret;
    __asm__ volatile("int $0x80"
                     : "=a"(ret)
                     : "a"(n), "D"(a), "S"(b), "d"(c), "r"(r10), "r"(r8), "r"(r9)
                     : "rcx", "r11", "memory");
    return ret;
}

static long sys3(long n, long a, long b, long c) {
    return sys6(n, a, b, c, 0, 0, 0);
}

/* ---- raw console and process syscalls ---- */

void _exit(int status) {
    sys3(SYS_EXIT, status, 0, 0);
    for (;;) {
        __asm__ volatile("hlt");
    }
}

void exit(int status) {
    _exit(status);
}

unsigned int getuid(void) { return (unsigned int)sys3(SYS_GETUID, 0, 0, 0); }
unsigned int getgid(void) { return (unsigned int)sys3(SYS_GETGID, 0, 0, 0); }

int setuid(unsigned int uid, unsigned int gid) {
    long r = sys3(SYS_SETUID, uid, gid, 0);
    if (r < 0) {
        errno = (int)-r;
        return -1;
    }
    return 0;
}

int chdir(const char *path) {
    long r = sys3(SYS_CHDIR, (long)path, (long)strlen(path), 0);
    if (r < 0) {
        errno = (int)-r;
        return -1;
    }
    return 0;
}

ssize_t getcwd(char *buf, size_t size) {
    long r = sys3(SYS_GETCWD, (long)buf, (long)size, 0);
    if (r < 0) {
        errno = (int)-r;
        return -1;
    }
    return r;
}

int unlink(const char *path) {
    long r = sys3(SYS_UNLINK, (long)path, (long)strlen(path), 0);
    if (r < 0) {
        errno = (int)-r;
        return -1;
    }
    return 0;
}

int rmdir(const char *path) {
    long r = sys3(SYS_RMDIR, (long)path, (long)strlen(path), 0);
    if (r < 0) {
        errno = (int)-r;
        return -1;
    }
    return 0;
}

int mkdir(const char *path, mode_t mode) {
    (void)mode;
    long r = sys3(SYS_MKDIR, (long)path, (long)strlen(path), 0);
    if (r < 0) {
        errno = (int)-r;
        return -1;
    }
    return 0;
}

int sleep_ms(unsigned long ms) {
    long r = sys3(SYS_SLEEP, (long)ms, 0, 0);
    return r < 0 ? -1 : 0;
}

/* ---- memory allocator ---- */

typedef struct block {
    size_t size;
    int free;
    struct block *next;
} block;

static unsigned char heap[1u << 20] __attribute__((aligned(16)));
static block *head;

static void heap_init(void) {
    head = (block *)heap;
    head->size = sizeof(heap) - sizeof(block);
    head->free = 1;
    head->next = NULL;
}

static size_t align16(size_t n) {
    return (n + 15u) & ~(size_t)15u;
}

void *malloc(size_t size) {
    if (size == 0) {
        size = 1;
    }
    if (!head) {
        heap_init();
    }
    size = align16(size);
    for (block *b = head; b; b = b->next) {
        if (!b->free || b->size < size) {
            continue;
        }
        if (b->size >= size + sizeof(block) + 16) {
            block *rest = (block *)((unsigned char *)(b + 1) + size);
            rest->size = b->size - size - sizeof(block);
            rest->free = 1;
            rest->next = b->next;
            b->next = rest;
            b->size = size;
        }
        b->free = 0;
        return b + 1;
    }
    errno = ENOMEM;
    return NULL;
}

void free(void *ptr) {
    if (!ptr) {
        return;
    }
    block *b = (block *)ptr - 1;
    b->free = 1;
    for (block *cur = head; cur; cur = cur->next) {
        if (cur->free && cur->next && cur->next->free) {
            cur->size += sizeof(block) + cur->next->size;
            cur->next = cur->next->next;
        }
    }
}

void *calloc(size_t count, size_t size) {
    size_t total = count * size;
    void *p = malloc(total);
    if (p) {
        memset(p, 0, total);
    }
    return p;
}

void *realloc(void *ptr, size_t size) {
    if (!ptr) {
        return malloc(size);
    }
    if (size == 0) {
        free(ptr);
        return NULL;
    }
    block *b = (block *)ptr - 1;
    if (b->size >= size) {
        return ptr;
    }
    void *next = malloc(size);
    if (!next) {
        return NULL;
    }
    memcpy(next, ptr, b->size);
    free(ptr);
    return next;
}

void abort(void) {
    _exit(134);
}

/* ---- string helpers ---- */

void *memcpy(void *dst, const void *src, size_t n) {
    unsigned char *d = dst;
    const unsigned char *s = src;
    for (size_t i = 0; i < n; i++) {
        d[i] = s[i];
    }
    return dst;
}

void *memmove(void *dst, const void *src, size_t n) {
    unsigned char *d = dst;
    const unsigned char *s = src;
    if (d < s) {
        for (size_t i = 0; i < n; i++) {
            d[i] = s[i];
        }
    } else if (d > s) {
        for (size_t i = n; i > 0; i--) {
            d[i - 1] = s[i - 1];
        }
    }
    return dst;
}

void *memset(void *dst, int c, size_t n) {
    unsigned char *d = dst;
    for (size_t i = 0; i < n; i++) {
        d[i] = (unsigned char)c;
    }
    return dst;
}

int memcmp(const void *a, const void *b, size_t n) {
    const unsigned char *x = a;
    const unsigned char *y = b;
    for (size_t i = 0; i < n; i++) {
        if (x[i] != y[i]) {
            return (int)x[i] - (int)y[i];
        }
    }
    return 0;
}

size_t strlen(const char *s) {
    size_t n = 0;
    while (s[n]) {
        n++;
    }
    return n;
}

size_t strnlen(const char *s, size_t max) {
    size_t n = 0;
    while (n < max && s[n]) {
        n++;
    }
    return n;
}

char *strcpy(char *dst, const char *src) {
    char *d = dst;
    while ((*d++ = *src++)) {
    }
    return dst;
}

char *strncpy(char *dst, const char *src, size_t n) {
    size_t i = 0;
    for (; i < n && src[i]; i++) {
        dst[i] = src[i];
    }
    for (; i < n; i++) {
        dst[i] = 0;
    }
    return dst;
}

char *strcat(char *dst, const char *src) {
    strcpy(dst + strlen(dst), src);
    return dst;
}

int strcmp(const char *a, const char *b) {
    while (*a && *a == *b) {
        a++;
        b++;
    }
    return (unsigned char)*a - (unsigned char)*b;
}

int strncmp(const char *a, const char *b, size_t n) {
    for (size_t i = 0; i < n; i++) {
        if (a[i] != b[i]) {
            return (unsigned char)a[i] - (unsigned char)b[i];
        }
        if (!a[i]) {
            return 0;
        }
    }
    return 0;
}

char *strchr(const char *s, int c) {
    for (; *s; s++) {
        if (*s == (char)c) {
            return (char *)s;
        }
    }
    return c == 0 ? (char *)s : NULL;
}

char *strrchr(const char *s, int c) {
    const char *last = NULL;
    for (; *s; s++) {
        if (*s == (char)c) {
            last = s;
        }
    }
    if (c == 0) {
        return (char *)s;
    }
    return (char *)last;
}

int atoi(const char *s) {
    while (*s == ' ' || *s == '\t') {
        s++;
    }
    int sign = 1;
    if (*s == '-') {
        sign = -1;
        s++;
    } else if (*s == '+') {
        s++;
    }
    int value = 0;
    while (*s >= '0' && *s <= '9') {
        value = value * 10 + (*s - '0');
        s++;
    }
    return value * sign;
}

long atol(const char *s) {
    return atoi(s);
}

char *getenv(const char *name) {
    (void)name;
    return NULL;
}

/* ---- file descriptors ----
 *
 * Descriptors are owned by the kernel: each task has a table of open files
 * with a per-descriptor cursor. `open` returns the slot the kernel picked, and
 * `read`/`write` stream through it, so large files never have to fit in memory.
 */

int open(const char *path, int flags, ...) {
    long r = sys3(SYS_OPEN, (long)path, (long)strlen(path), flags);
    if (r < 0) {
        errno = (int)-r;
        return -1;
    }
    return (int)r;
}

int close(int fd) {
    long r = sys3(SYS_CLOSE, fd, 0, 0);
    if (r < 0) {
        errno = (int)-r;
        return -1;
    }
    return 0;
}

ssize_t read(int fd, void *buf, size_t count) {
    long r = sys3(SYS_READ, fd, (long)buf, (long)count);
    if (r < 0) {
        errno = (int)-r;
        return -1;
    }
    return (ssize_t)r;
}

ssize_t write(int fd, const void *buf, size_t count) {
    long r = sys3(SYS_WRITE, fd, (long)buf, (long)count);
    if (r < 0) {
        errno = (int)-r;
        return -1;
    }
    return (ssize_t)r;
}

off_t lseek(int fd, off_t offset, int whence) {
    long r = sys3(SYS_LSEEK, fd, (long)offset, whence);
    if (r < 0) {
        errno = (int)-r;
        return (off_t)-1;
    }
    return (off_t)r;
}

int dup2(int oldfd, int newfd) {
    long r = sys3(SYS_DUP2, oldfd, newfd, 0);
    if (r < 0) {
        errno = (int)-r;
        return -1;
    }
    return (int)r;
}

int stat(const char *path, struct stat *buf) {
    char text[128];
    long n = sys6(SYS_STAT, (long)path, (long)strlen(path), (long)text,
                  sizeof(text), 0, 0);
    if (n <= 0) {
        errno = ENOENT;
        return -1;
    }
    const char *p = text;
    if (*p == 'd') {
        buf->st_mode = S_IFDIR | 0755;
    } else if (*p == 'l') {
        buf->st_mode = S_IFLNK | 0777;
    } else {
        buf->st_mode = S_IFREG | 0644;
    }
    while (*p && *p != ' ') {
        p++;
    }
    while (*p == ' ') {
        p++;
    }
    buf->st_size = (unsigned long)atol(p);
    while (*p && *p != ' ') {
        p++;
    }
    while (*p == ' ') {
        p++;
    }
    unsigned short mode = 0;
    while (*p >= '0' && *p <= '7') {
        mode = (unsigned short)(mode * 8 + (*p - '0'));
        p++;
    }
    buf->st_mode = (buf->st_mode & S_IFMT) | mode;
    while (*p && *p != ' ') {
        p++;
    }
    while (*p == ' ') {
        p++;
    }
    buf->st_uid = (unsigned int)atol(p);
    while (*p && *p != ' ') {
        p++;
    }
    while (*p == ' ') {
        p++;
    }
    buf->st_gid = (unsigned int)atol(p);
    return 0;
}

/* ---- formatted output ---- */

struct out {
    char *buf;
    size_t cap;
    size_t len;
    int fd;
    int overflow;
};

static void out_char(struct out *o, char c) {
    if (o->buf) {
        if (o->len + 1 < o->cap) {
            o->buf[o->len] = c;
        } else {
            o->overflow = 1;
        }
    } else if (o->fd >= 0) {
        char tmp = c;
        sys3(SYS_WRITE, o->fd, (long)&tmp, 1);
    }
    o->len++;
}

static void out_pad(struct out *o, char c, int n) {
    while (n-- > 0) {
        out_char(o, c);
    }
}

static void out_str(struct out *o, const char *s) {
    while (*s) {
        out_char(o, *s++);
    }
}

static void out_uint(struct out *o, unsigned long value, int base, int upper,
                     int width, int zero, int left, int precision) {
    char digits[32];
    const char *alphabet = upper ? "0123456789ABCDEF" : "0123456789abcdef";
    int n = 0;
    if (value == 0 && precision != 0) {
        digits[n++] = '0';
    }
    while (value) {
        digits[n++] = alphabet[value % (unsigned)base];
        value /= (unsigned)base;
    }
    while (n < precision) {
        digits[n++] = '0';
    }
    int pad = width > n ? width - n : 0;
    if (!left) {
        out_pad(o, zero ? '0' : ' ', pad);
    }
    while (n > 0) {
        out_char(o, digits[--n]);
    }
    if (left) {
        out_pad(o, ' ', pad);
    }
}

static void vformat(struct out *o, const char *fmt, va_list ap) {
    while (*fmt) {
        if (*fmt != '%') {
            out_char(o, *fmt++);
            continue;
        }
        fmt++;
        int left = 0;
        int zero = 0;
        while (*fmt == '-' || *fmt == '0') {
            if (*fmt == '-') {
                left = 1;
            } else {
                zero = 1;
            }
            fmt++;
        }
        int width = 0;
        while (*fmt >= '0' && *fmt <= '9') {
            width = width * 10 + (*fmt++ - '0');
        }
        int precision = -1;
        if (*fmt == '.') {
            fmt++;
            precision = 0;
            while (*fmt >= '0' && *fmt <= '9') {
                precision = precision * 10 + (*fmt++ - '0');
            }
        }
        int longs = 0;
        while (*fmt == 'l' || *fmt == 'z') {
            longs++;
            fmt++;
        }
        if (*fmt == 'h') {
            fmt++;
            if (*fmt == 'h') {
                fmt++;
            }
        }
        char c = *fmt++;
        switch (c) {
        case 'd':
        case 'i': {
            long value = longs ? va_arg(ap, long) : (long)va_arg(ap, int);
            int negative = value < 0;
            unsigned long mag = negative ? (unsigned long)(-value) : (unsigned long)value;
            char sign[2] = {negative ? '-' : 0, 0};
            if (negative) {
                out_char(o, '-');
                width--;
            }
            (void)sign;
            out_uint(o, mag, 10, 0, width, zero, left, precision);
            break;
        }
        case 'u':
            out_uint(o, longs ? va_arg(ap, unsigned long)
                              : (unsigned long)va_arg(ap, unsigned int),
                     10, 0, width, zero, left, precision);
            break;
        case 'x':
        case 'X':
            out_uint(o, longs ? va_arg(ap, unsigned long)
                              : (unsigned long)va_arg(ap, unsigned int),
                     16, c == 'X', width, zero, left, precision);
            break;
        case 'o':
            out_uint(o, longs ? va_arg(ap, unsigned long)
                              : (unsigned long)va_arg(ap, unsigned int),
                     8, 0, width, zero, left, precision);
            break;
        case 'p':
            out_str(o, "0x");
            out_uint(o, (unsigned long)va_arg(ap, void *), 16, 0, width, 1, left,
                     precision);
            break;
        case 'c':
            out_char(o, (char)va_arg(ap, int));
            break;
        case 's': {
            const char *s = va_arg(ap, const char *);
            if (!s) {
                s = "(null)";
            }
            int len = (int)strlen(s);
            if (precision >= 0 && len > precision) {
                len = precision;
            }
            int pad = width > len ? width - len : 0;
            if (!left) {
                out_pad(o, ' ', pad);
            }
            for (int i = 0; i < len; i++) {
                out_char(o, s[i]);
            }
            if (left) {
                out_pad(o, ' ', pad);
            }
            break;
        }
        case '%':
            out_char(o, '%');
            break;
        case 0:
            return;
        default:
            out_char(o, '%');
            out_char(o, c);
            break;
        }
    }
}

int printf(const char *fmt, ...) {
    struct out o = {0, 0, 0, STDOUT_FILENO, 0};
    va_list ap;
    va_start(ap, fmt);
    vformat(&o, fmt, ap);
    va_end(ap);
    return (int)o.len;
}

int fprintf(void *stream, const char *fmt, ...) {
    (void)stream;
    struct out o = {0, 0, 0, STDERR_FILENO, 0};
    va_list ap;
    va_start(ap, fmt);
    vformat(&o, fmt, ap);
    va_end(ap);
    return (int)o.len;
}

int vsnprintf(char *buf, size_t size, const char *fmt, va_list ap) {
    struct out o = {buf, size, 0, -1, 0};
    vformat(&o, fmt, ap);
    if (size) {
        size_t end = o.len < size - 1 ? o.len : size - 1;
        buf[end] = 0;
    }
    return (int)o.len;
}

int snprintf(char *buf, size_t size, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    int n = vsnprintf(buf, size, fmt, ap);
    va_end(ap);
    return n;
}

int sprintf(char *buf, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    int n = vsnprintf(buf, (size_t)-1, fmt, ap);
    va_end(ap);
    return n;
}

int puts(const char *s) {
    printf("%s\n", s);
    return 0;
}

int putchar(int c) {
    char ch = (char)c;
    write(STDOUT_FILENO, &ch, 1);
    return c;
}

int getchar(void) {
    char c;
    if (read(STDIN_FILENO, &c, 1) == 1) {
        return (unsigned char)c;
    }
    return EOF;
}
