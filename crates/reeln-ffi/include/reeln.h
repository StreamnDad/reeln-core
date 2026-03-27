#ifndef REELN_H
#define REELN_H

/*
 * reeln-core C ABI header.
 *
 * NOTE: cbindgen does not yet support #[unsafe(no_mangle)] (Rust edition 2024).
 * This header is maintained manually until cbindgen adds support.
 * Keep in sync with crates/reeln-ffi/src/lib.rs.
 */

#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ── String management ──────────────────────────────────────────── */

/** Free a string returned by any reeln_* function. Null is a no-op. */
void reeln_free_string(char *s);

/* ── Version ────────────────────────────────────────────────────── */

/** Return the library version. Caller must free with reeln_free_string. */
char *reeln_version(void);

/* ── Probe ──────────────────────────────────────────────────────── */

typedef struct {
    double duration_secs; /* -1.0 if unknown */
    double fps;           /* -1.0 if unknown */
    uint32_t width;       /* 0 if unknown */
    uint32_t height;      /* 0 if unknown */
    char *codec;          /* caller must free, or NULL */
    char *error;          /* caller must free, or NULL on success */
} ReelnProbeResult;

/** Probe a media file. path must be a valid UTF-8 C string. */
ReelnProbeResult reeln_probe(const char *path);

/** Free heap fields inside a ReelnProbeResult. */
void reeln_probe_result_free(ReelnProbeResult *result);

/* ── Concat ─────────────────────────────────────────────────────── */

/**
 * Concatenate media segments into output.
 * Returns NULL on success, or an error string (caller must free).
 */
char *reeln_concat(const char *const *segments, size_t segment_count,
                   const char *output, bool copy);

/* ── Game directory ─────────────────────────────────────────────── */

/** Generate a game directory name. Caller must free. */
char *reeln_game_dir_name(const char *date, const char *home,
                          const char *away, uint32_t game_number);

/* ── Segment names ──────────────────────────────────────────────── */

/** Generate segment directory name (e.g. "period-1"). NULL if sport unknown. */
char *reeln_segment_dir_name(const char *sport, uint32_t segment_number);

/** Generate segment display name (e.g. "Period 1"). NULL if sport unknown. */
char *reeln_segment_display_name(const char *sport, uint32_t segment_number);

#ifdef __cplusplus
}
#endif

#endif /* REELN_H */
