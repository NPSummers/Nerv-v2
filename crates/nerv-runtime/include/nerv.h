#ifndef NERV_RUNTIME_H
#define NERV_RUNTIME_H
#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define NERV_ABI_VERSION 1
typedef struct NervString NervString;
typedef struct NervArray NervArray;

/* Enter one execution region, call the generated entry, print its result, release
   the region, and return process status 0. Fatal errors print to stderr and exit 1. */
int32_t nerv_start_i64(int64_t (*entry)(void));
int32_t nerv_start_void(void (*entry)(void));
void nerv_fail(int64_t code);
int64_t nerv_f64_to_i64(double value);
double nerv_sqrt(double value);
double nerv_pow(double left, double right);
int64_t nerv_abs_i64(int64_t value);
double nerv_abs_f64(double value);
int64_t nerv_random_i64(int64_t maximum);

void nerv_print_i64(int64_t value);
void nerv_println_i64(int64_t value);
void nerv_print_f64(double value);
void nerv_println_f64(double value);
void nerv_print_bool(bool value);
void nerv_println_bool(bool value);
void nerv_print_string(const NervString *value);
void nerv_println_string(const NervString *value);

NervString *nerv_string_new(const uint8_t *bytes, int64_t length);
int64_t nerv_string_len(const NervString *value);
NervString *nerv_string_concat(const NervString *left, const NervString *right);
NervString *nerv_str_to_upper(const NervString *value);
NervString *nerv_str_to_lower(const NervString *value);
NervString *nerv_str_trim(const NervString *value);
NervString *nerv_str_repeat(const NervString *value, int64_t count);
NervString *nerv_str_replace(const NervString *value, const NervString *from, const NervString *to);
NervString *nerv_str_slice(const NervString *value, int64_t start, int64_t end);
NervString *nerv_str_split(const NervString *value, const NervString *separator);
NervString *nerv_str_split_once(const NervString *value, const NervString *separator);
int64_t nerv_str_is_empty(const NervString *value);
int64_t nerv_str_contains(const NervString *value, const NervString *needle);
int64_t nerv_str_starts_with(const NervString *value, const NervString *prefix);
int64_t nerv_str_ends_with(const NervString *value, const NervString *suffix);
int64_t nerv_str_find(const NervString *value, const NervString *needle);
int64_t nerv_str_char_count(const NervString *value);
int64_t nerv_str_char_at(const NervString *value, int64_t index);

NervArray *nerv_array_i64_new(int64_t length, int64_t initial);
int64_t nerv_array_i64_len(const NervArray *array);
int64_t *nerv_array_i64_at(NervArray *array, int64_t index);
int64_t nerv_array_i64_get(const NervArray *array, int64_t index);
void nerv_array_i64_set(NervArray *array, int64_t index, int64_t value);
NervArray *nerv_array_i64_map(const NervArray *array, int64_t length, int64_t (*callback)(int64_t));
int64_t nerv_array_i64_reduce(const NervArray *array, int64_t length, int64_t initial, int64_t (*callback)(int64_t, int64_t));

/* Native-only escape hatches. Do not free a shared core handle; do not retain raw
   element pointers across mutation/free. Foreign callers must pass live handles
   of the correct type and callbacks must not free the iterated array. */
int64_t *nerv_array_i64_data(NervArray *array);
void nerv_array_i64_free(NervArray *array);
void nerv_string_free(NervString *value);
#ifdef __cplusplus
}
#endif
#endif
