#ifndef HDIFFPATCH_WRAPPER_H
#define HDIFFPATCH_WRAPPER_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

int hdiffpatch_create(
    const unsigned char* old_data, size_t old_size,
    const unsigned char* new_data, size_t new_size,
    unsigned char** out_patch, size_t* out_patch_size,
    int thread_num,
    int use_compression
);

int hdiffpatch_create_file(
    const char* old_file,
    const char* new_file,
    const char* patch_file,
    int thread_num,
    int use_compression
);

int hdiffpatch_apply(
    const unsigned char* old_data, size_t old_size,
    const unsigned char* patch_data, size_t patch_size,
    unsigned char** out_new_data, size_t* out_new_size,
    int thread_num
);

void hdiffpatch_free(void* ptr);

#ifdef __cplusplus
}
#endif

#endif
