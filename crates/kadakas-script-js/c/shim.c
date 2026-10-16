/*
 * The Kadakas script module: QuickJS (quickjs-ng) with a small host interface.
 *
 * The module runs inside the WebAssembly sandbox of the plug-in host. It has two imports and
 * nothing else: no WASI, no files, no network, no clock. The clock functions are stubs (time is
 * always 0), so a script cannot read the time and a run gives the same result every time.
 *
 *   import kadakas.call(ptr, len) -> i32   send a request frame to the host; returns the length of
 *                                          the reply, or -1
 *   import kadakas.read(dst)               copy the reply of the last call to dst
 *
 *   export alloc(len) -> ptr, dealloc(ptr, len), run(ptr, len) -> (ptr << 32 | len)
 *
 * A frame is a u32 count and then, for each part, a u32 length and the bytes (little endian).
 *
 *   run request:      [code, args_json, options_json, buffer...]
 *   run reply:        [status ("ok" | "error"), payload, calls, log, buffer...]
 *   host call:        [method, params_json, buffer...]
 *   host call reply:  [status ("ok" | "error"), json, buffer...]
 */
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <sys/time.h>

#include "quickjs.h"

#define EXPORT(n) __attribute__((export_name(n)))
#define IMPORT(n) __attribute__((import_module("kadakas"), import_name(n)))

IMPORT("call") extern int32_t host_call(const uint8_t *ptr, uint32_t len);
IMPORT("read") extern void host_read(uint8_t *dst);

/* ---- no clock -------------------------------------------------------------------------- */

int clock_gettime(clockid_t id, struct timespec *ts)
{
    (void)id;
    ts->tv_sec = 0;
    ts->tv_nsec = 0;
    return 0;
}

int gettimeofday(struct timeval *tv, void *tz)
{
    (void)tz;
    tv->tv_sec = 0;
    tv->tv_usec = 0;
    return 0;
}

time_t time(time_t *t)
{
    if (t)
        *t = 0;
    return 0;
}

/* ---- no WASI ---------------------------------------------------------------------------
   The libc of wasi-sdk reaches the system through functions that the module would import from
   "wasi_snapshot_preview1". We define them here, so the module imports nothing but the two
   functions above. A write is swallowed (nobody reads stdout or stderr of a script); everything else
   says "not supported". The build checks the imports of the finished module. */

#define WASI_ENOSYS 52
#define WASI_EBADF 8

uint32_t __imported_wasi_snapshot_preview1_args_get(uint8_t **argv, uint8_t *buf)
{
    (void)argv;
    (void)buf;
    return 0;
}

uint32_t __imported_wasi_snapshot_preview1_args_sizes_get(uint32_t *argc, uint32_t *size)
{
    *argc = 0;
    *size = 0;
    return 0;
}

uint32_t __imported_wasi_snapshot_preview1_clock_time_get(uint32_t id, uint64_t precision, uint64_t *out)
{
    (void)id;
    (void)precision;
    *out = 0;
    return 0;
}

uint32_t __imported_wasi_snapshot_preview1_fd_close(uint32_t fd)
{
    (void)fd;
    return WASI_EBADF;
}

uint32_t __imported_wasi_snapshot_preview1_fd_fdstat_get(uint32_t fd, void *stat)
{
    (void)fd;
    (void)stat;
    return WASI_EBADF;
}

uint32_t __imported_wasi_snapshot_preview1_fd_seek(uint32_t fd, int64_t delta, uint32_t whence, uint64_t *out)
{
    (void)fd;
    (void)delta;
    (void)whence;
    (void)out;
    return WASI_ENOSYS;
}

typedef struct {
    const uint8_t *buf;
    uint32_t len;
} CIoVec;

uint32_t __imported_wasi_snapshot_preview1_fd_write(uint32_t fd, const CIoVec *iovs, uint32_t n, uint32_t *written)
{
    (void)fd;
    uint32_t total = 0;
    for (uint32_t i = 0; i < n; i++)
        total += iovs[i].len;
    *written = total;
    return 0;
}

__attribute__((noreturn)) void __imported_wasi_snapshot_preview1_proc_exit(uint32_t code)
{
    (void)code;
    __builtin_trap();
}

/* ---- memory for the host --------------------------------------------------------------- */

EXPORT("alloc") void *kad_alloc(uint32_t len)
{
    return malloc(len ? len : 1);
}

EXPORT("dealloc") void kad_dealloc(void *ptr, uint32_t len)
{
    (void)len;
    free(ptr);
}

/* ---- frames ---------------------------------------------------------------------------- */

typedef struct {
    uint8_t *d;
    size_t len, cap;
    int failed;
} Buf;

static void buf_reserve(Buf *b, size_t extra)
{
    if (b->failed || b->len + extra <= b->cap)
        return;
    size_t cap = b->cap ? b->cap : 256;
    while (cap < b->len + extra)
        cap *= 2;
    uint8_t *n = realloc(b->d, cap);
    if (!n) {
        b->failed = 1;
        return;
    }
    b->d = n;
    b->cap = cap;
}

static void buf_put(Buf *b, const void *p, size_t n)
{
    buf_reserve(b, n);
    if (b->failed)
        return;
    memcpy(b->d + b->len, p, n);
    b->len += n;
}

static void buf_u32(Buf *b, uint32_t v)
{
    uint8_t x[4] = { (uint8_t)v, (uint8_t)(v >> 8), (uint8_t)(v >> 16), (uint8_t)(v >> 24) };
    buf_put(b, x, 4);
}

static void buf_part(Buf *b, const void *p, size_t n)
{
    buf_u32(b, (uint32_t)n);
    buf_put(b, p, n);
}

static uint32_t rd_u32(const uint8_t *p)
{
    return (uint32_t)p[0] | ((uint32_t)p[1] << 8) | ((uint32_t)p[2] << 16) | ((uint32_t)p[3] << 24);
}

/* Reads the parts of a frame. Returns the number of parts, or -1 if the frame is broken. The parts
   point into `d`. */
static int frame_parts(const uint8_t *d, size_t len, const uint8_t **ptr, uint32_t *plen, int max)
{
    if (len < 4)
        return -1;
    uint32_t n = rd_u32(d);
    if (n > (uint32_t)max)
        return -1;
    size_t at = 4;
    for (uint32_t i = 0; i < n; i++) {
        if (len - at < 4)
            return -1;
        uint32_t l = rd_u32(d + at);
        at += 4;
        if (len - at < l)
            return -1;
        ptr[i] = d + at;
        plen[i] = l;
        at += l;
    }
    return (int)n;
}

/* ---- the log --------------------------------------------------------------------------- */

#define LOG_MAX (64 * 1024)
static Buf g_log;
static uint32_t g_log_dropped;

static JSValue js_log(JSContext *ctx, JSValueConst this_val, int argc, JSValueConst *argv)
{
    (void)this_val;
    if (argc < 1)
        return JS_UNDEFINED;
    size_t n;
    const char *s = JS_ToCStringLen(ctx, &n, argv[0]);
    if (!s)
        return JS_EXCEPTION;
    if (g_log.len + n + 1 <= LOG_MAX) {
        buf_put(&g_log, s, n);
        buf_put(&g_log, "\n", 1);
    } else {
        g_log_dropped++;
    }
    JS_FreeCString(ctx, s);
    return JS_UNDEFINED;
}

/* ---- the host call --------------------------------------------------------------------- */

/* __call(method, paramsJson, [ArrayBuffer...]) -> [status, json, ArrayBuffer...] */
static JSValue js_call(JSContext *ctx, JSValueConst this_val, int argc, JSValueConst *argv)
{
    (void)this_val;
    if (argc < 3)
        return JS_ThrowTypeError(ctx, "call needs a method, parameters and buffers");
    size_t mlen, plen;
    const char *method = JS_ToCStringLen(ctx, &mlen, argv[0]);
    if (!method)
        return JS_EXCEPTION;
    const char *params = JS_ToCStringLen(ctx, &plen, argv[1]);
    if (!params) {
        JS_FreeCString(ctx, method);
        return JS_EXCEPTION;
    }
    Buf req = { 0 };
    uint32_t nb = 0;
    JSValue len_v = JS_GetPropertyStr(ctx, argv[2], "length");
    JS_ToUint32(ctx, &nb, len_v);
    JS_FreeValue(ctx, len_v);
    buf_u32(&req, 2 + nb);
    buf_part(&req, method, mlen);
    buf_part(&req, params, plen);
    JS_FreeCString(ctx, method);
    JS_FreeCString(ctx, params);
    for (uint32_t i = 0; i < nb; i++) {
        JSValue b = JS_GetPropertyUint32(ctx, argv[2], i);
        size_t blen;
        uint8_t *bp = JS_GetArrayBuffer(ctx, &blen, b);
        if (!bp && blen == 0 && !JS_IsException(b)) {
            buf_part(&req, "", 0);
        } else if (!bp) {
            JS_FreeValue(ctx, b);
            free(req.d);
            return JS_EXCEPTION;
        } else {
            buf_part(&req, bp, blen);
        }
        JS_FreeValue(ctx, b);
    }
    if (req.failed) {
        free(req.d);
        return JS_ThrowInternalError(ctx, "out of memory");
    }
    int32_t n = host_call(req.d, (uint32_t)req.len);
    free(req.d);
    if (n < 0)
        return JS_ThrowInternalError(ctx, "the host call failed");
    uint8_t *reply = malloc(n ? (size_t)n : 1);
    if (!reply)
        return JS_ThrowInternalError(ctx, "out of memory");
    host_read(reply);
    const uint8_t *pp[1024];
    uint32_t pl[1024];
    int np = frame_parts(reply, (size_t)n, pp, pl, 1024);
    if (np < 2) {
        free(reply);
        return JS_ThrowInternalError(ctx, "the host gave a broken reply");
    }
    JSValue out = JS_NewArray(ctx);
    for (int i = 0; i < np; i++) {
        JSValue v;
        if (i < 2)
            v = JS_NewStringLen(ctx, (const char *)pp[i], pl[i]);
        else
            v = JS_NewArrayBufferCopy(ctx, pp[i], pl[i]);
        if (JS_IsException(v)) {
            free(reply);
            JS_FreeValue(ctx, out);
            return JS_EXCEPTION;
        }
        JS_SetPropertyUint32(ctx, out, (uint32_t)i, v);
    }
    free(reply);
    return out;
}

/* ---- the run --------------------------------------------------------------------------- */

#include "prelude.h" /* const char PRELUDE[]: the JavaScript of prelude.js */

static uint64_t pack_frame(Buf *b)
{
    if (b->failed || !b->d)
        return 0;
    return ((uint64_t)(uintptr_t)b->d << 32) | (uint64_t)b->len;
}

static uint64_t error_frame(const char *status, const char *msg)
{
    Buf out = { 0 };
    buf_u32(&out, 4);
    buf_part(&out, status, strlen(status));
    buf_part(&out, msg, strlen(msg));
    buf_part(&out, "0", 1);
    buf_part(&out, g_log.d ? (const char *)g_log.d : "", g_log.len);
    return pack_frame(&out);
}

static int64_t opt_int(JSContext *ctx, JSValueConst o, const char *key, int64_t dflt)
{
    JSValue v = JS_GetPropertyStr(ctx, o, key);
    int64_t r = dflt;
    if (!JS_IsUndefined(v) && !JS_IsException(v))
        JS_ToInt64(ctx, &r, v);
    JS_FreeValue(ctx, v);
    return r;
}

EXPORT("run") uint64_t kad_run(const uint8_t *req, uint32_t req_len)
{
    const uint8_t *pp[1024];
    uint32_t pl[1024];
    int nparts = frame_parts(req, req_len, pp, pl, 1024);
    if (nparts < 3)
        return error_frame("error", "{\"message\":\"the run request is broken\"}");

    JSRuntime *rt = JS_NewRuntime();
    if (!rt)
        return error_frame("error", "{\"message\":\"out of memory\"}");
    JSContext *ctx = JS_NewContext(rt);
    if (!ctx)
        return error_frame("error", "{\"message\":\"out of memory\"}");

    /* Options: {"memory": bytes, "stack": bytes, "seed": n, "max_calls": n} as JSON. */
    JSValue opts = JS_ParseJSON(ctx, (const char *)pp[2], pl[2], "<options>");
    int64_t mem = JS_IsException(opts) ? 0 : opt_int(ctx, opts, "memory", 0);
    int64_t stack = JS_IsException(opts) ? 0 : opt_int(ctx, opts, "stack", 0);
    if (mem > 0)
        JS_SetMemoryLimit(rt, (size_t)mem);
    JS_SetMaxStackSize(rt, stack > 0 ? (size_t)stack : 256 * 1024);

    JSValue global = JS_GetGlobalObject(ctx);
    JSValue host = JS_NewObject(ctx);
    JS_SetPropertyStr(ctx, host, "call", JS_NewCFunction(ctx, js_call, "call", 3));
    JS_SetPropertyStr(ctx, host, "log", JS_NewCFunction(ctx, js_log, "log", 1));
    JS_SetPropertyStr(ctx, global, "__host", host);

    uint64_t result = 0;
    JSValue pre = JS_Eval(ctx, PRELUDE, sizeof(PRELUDE) - 1, "<prelude>", JS_EVAL_TYPE_GLOBAL);
    if (JS_IsException(pre)) {
        JSValue ex = JS_GetException(ctx);
        const char *s = JS_ToCString(ctx, ex);
        result = error_frame("fatal", s ? s : "the prelude failed");
        if (s)
            JS_FreeCString(ctx, s);
        JS_FreeValue(ctx, ex);
        goto done;
    }
    JS_FreeValue(ctx, pre);

    JSValue run_fn = JS_GetPropertyStr(ctx, global, "__run");
    JSValue args[4];
    args[0] = JS_NewStringLen(ctx, (const char *)pp[0], pl[0]);
    args[1] = JS_NewStringLen(ctx, (const char *)pp[1], pl[1]);
    args[2] = JS_IsException(opts) ? JS_UNDEFINED : opts;
    args[3] = JS_NewArray(ctx);
    for (int i = 3; i < nparts; i++)
        JS_SetPropertyUint32(ctx, args[3], (uint32_t)(i - 3),
                             JS_NewArrayBufferCopy(ctx, pp[i], pl[i]));
    JSValue r = JS_Call(ctx, run_fn, JS_UNDEFINED, 4, args);
    JS_FreeValue(ctx, args[0]);
    JS_FreeValue(ctx, args[1]);
    JS_FreeValue(ctx, args[3]);
    JS_FreeValue(ctx, run_fn);
    if (JS_IsException(r)) {
        JSValue ex = JS_GetException(ctx);
        const char *s = JS_ToCString(ctx, ex);
        result = error_frame("fatal", s ? s : "the run failed");
        if (s)
            JS_FreeCString(ctx, s);
        JS_FreeValue(ctx, ex);
        goto done;
    }
    {
        /* r = [status, payload, calls, ArrayBuffer...] */
        JSValue len_v = JS_GetPropertyStr(ctx, r, "length");
        uint32_t n = 0;
        JS_ToUint32(ctx, &n, len_v);
        JS_FreeValue(ctx, len_v);
        Buf out = { 0 };
        buf_u32(&out, n + 1);
        for (uint32_t i = 0; i < n && !out.failed; i++) {
            JSValue v = JS_GetPropertyUint32(ctx, r, i);
            if (i >= 3) {
                size_t bl;
                uint8_t *bp = JS_GetArrayBuffer(ctx, &bl, v);
                buf_part(&out, bp ? (const char *)bp : "", bp ? bl : 0);
            } else {
                size_t sl;
                const char *s = JS_ToCStringLen(ctx, &sl, v);
                buf_part(&out, s ? s : "", s ? sl : 0);
                if (s)
                    JS_FreeCString(ctx, s);
            }
            JS_FreeValue(ctx, v);
            if (i == 2) {
                /* the log goes after the calls count */
                char extra[64];
                size_t el = 0;
                if (g_log_dropped) {
                    /* the number of lines that did not fit */
                    uint32_t d = g_log_dropped;
                    char tmp[16];
                    int k = 0;
                    while (d) {
                        tmp[k++] = (char)('0' + d % 10);
                        d /= 10;
                    }
                    const char *a = "[";
                    const char *b = " more lines were not kept]\n";
                    memcpy(extra + el, a, 1);
                    el += 1;
                    while (k)
                        extra[el++] = tmp[--k];
                    buf_put(&g_log, extra, el);
                    buf_put(&g_log, b, strlen(b));
                }
                buf_part(&out, g_log.d ? (const char *)g_log.d : "", g_log.len);
            }
        }
        result = pack_frame(&out);
    }
    JS_FreeValue(ctx, r);
done:
    /* The instance ends with the call, so nothing is freed: the memory goes with it. */
    return result;
}
