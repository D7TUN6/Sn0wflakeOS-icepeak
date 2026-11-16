/* Toolchain and libc smoke test. Compiled with `tools/snf-cc` and run in ring 3. */

#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static int check(const char *name, int ok) {
    printf("%-16s %s\n", name, ok ? "ok" : "FAIL");
    return ok ? 0 : 1;
}

int main(int argc, char **argv) {
    int failures = 0;
    printf("libc-test: argc=%d argv[0]=%s\n", argc, argc > 0 ? argv[0] : "?");

    char *buf = malloc(64);
    failures += check("malloc", buf != NULL);
    if (buf) {
        strcpy(buf, "hello from malloc");
        failures += check("strcpy", strcmp(buf, "hello from malloc") == 0);
        char *grown = realloc(buf, 256);
        failures += check("realloc", grown != NULL);
        if (grown) {
            strcat(grown, " + realloc");
            failures += check("strcat", strcmp(grown, "hello from malloc + realloc") == 0);
        }
        free(grown ? grown : buf);
    }

    int fd = open("/etc/os-release", O_RDONLY);
    failures += check("open/read", fd >= 0);
    if (fd >= 0) {
        char text[256];
        ssize_t n = read(fd, text, sizeof(text) - 1);
        if (n > 0) {
            text[n] = 0;
            printf("os-release: %s", text);
        }
        close(fd);
        failures += check("close", n > 0);
    }
    int out = open("/tmp/libc.txt", O_WRONLY | O_CREAT | O_TRUNC);
    failures += check("open/create", out >= 0);
    if (out >= 0) {
        const char *msg = "written by libc\n";
        ssize_t n = write(out, msg, strlen(msg));
        failures += check("write", n == (ssize_t)strlen(msg));
        close(out);
    }

    int in = open("/tmp/libc.txt", O_RDONLY);
    failures += check("reopen", in >= 0);
    if (in >= 0) {
        char text[64];
        ssize_t n = read(in, text, sizeof(text) - 1);
        if (n > 0) {
            text[n] = 0;
        }
        close(in);
        failures += check("roundtrip", n > 0 && strcmp(text, "written by libc\n") == 0);
    }

    /* Streaming descriptors: seek, append and a shared cursor. */
    int rw = open("/tmp/libc.txt", O_RDWR);
    failures += check("open/rw", rw >= 0);
    if (rw >= 0) {
        off_t end = lseek(rw, 0, SEEK_END);
        failures += check("lseek/end", end == 16);
        failures += check("write/at", write(rw, "tail\n", 5) == 5);
        failures += check("lseek/set", lseek(rw, 0, SEEK_SET) == 0);
        char text[64];
        ssize_t n = read(rw, text, sizeof(text) - 1);
        if (n > 0) {
            text[n] = 0;
        }
        failures += check("stream", n == 21 && strcmp(text, "written by libc\ntail\n") == 0);
        close(rw);
    }

    int app = open("/tmp/libc.txt", O_WRONLY | O_APPEND);
    failures += check("open/append", app >= 0);
    if (app >= 0) {
        failures += check("write/append", write(app, "!\n", 2) == 2);
        close(app);
    }

    int zero = open("/dev/zero", O_RDONLY);
    failures += check("open/zero", zero >= 0);
    if (zero >= 0) {
        unsigned char zeros[256];
        memset(zeros, 0xff, sizeof(zeros));
        ssize_t n = read(zero, zeros, sizeof(zeros));
        int clean = n == (ssize_t)sizeof(zeros);
        for (size_t i = 0; clean && i < sizeof(zeros); i++) {
            clean = zeros[i] == 0;
        }
        failures += check("read/zero", clean);
        close(zero);
    }

    int sink = open("/dev/null", O_WRONLY);
    failures += check("open/null", sink >= 0);
    if (sink >= 0) {
        failures += check("write/null", write(sink, "discard", 7) == 7);
        close(sink);
    }

    char small[32];
    snprintf(small, sizeof(small), "%d/%x/%s", 42, 255, "ok");
    failures += check("snprintf", strcmp(small, "42/ff/ok") == 0);

    printf("libc-test: %s\n", failures == 0 ? "ok" : "FAILED");
    return failures == 0 ? 0 : 1;
}
