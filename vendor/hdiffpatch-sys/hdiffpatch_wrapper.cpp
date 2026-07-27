#include "hdiffpatch_wrapper.h"
#include <cstring>
#include <cstdlib>
#include <vector>

#include "libHDiffPatch/HDiff/diff.h"
#include "libHDiffPatch/HPatch/patch.h"
#include "file_for_patch.h"

#define _CompressPlugin_zlib 1
#define _IsNeedIncludeDefaultCompressHead 1
#include "compress_plugin_demo.h"
#include "decompress_plugin_demo.h"

struct Cache {
    unsigned char* data;
};

static hpatch_BOOL on_diff_info(sspatch_listener_t* listener,
                                const hpatch_singleCompressedDiffInfo* info,
                                hpatch_TDecompress** out_decompressPlugin,
                                unsigned char** out_temp_cache,
                                unsigned char** out_temp_cacheEnd)
{
    if (info->compressedSize > 0) {
        *out_decompressPlugin = &zlibDecompressPlugin;
    } else {
        *out_decompressPlugin = nullptr;
    }

    size_t cacheSize = (size_t)info->stepMemSize + ((size_t)1 << 20);
    auto* cache = (Cache*)listener->import;
    cache->data = (unsigned char*)std::malloc(cacheSize);
    if (!cache->data && cacheSize > 0)
        return hpatch_FALSE;
    *out_temp_cache = cache->data;
    *out_temp_cacheEnd = cache->data + cacheSize;
    return hpatch_TRUE;
}

static void on_patch_finish(sspatch_listener_t* listener,
                            unsigned char* temp_cache,
                            unsigned char* temp_cacheEnd)
{
    auto* cache = (Cache*)listener->import;
    if (cache->data) {
        std::free(cache->data);
        cache->data = nullptr;
    }
}

int hdiffpatch_create(
    const unsigned char* old_data, size_t old_size,
    const unsigned char* new_data, size_t new_size,
    unsigned char** out_patch, size_t* out_patch_size,
    int thread_num,
    int use_compression,
    int fast_format)
{
    try {
        std::vector<unsigned char> diff;
        const hdiff_TCompress* compress = nullptr;
        if (use_compression)
            compress = (const hdiff_TCompress*)&zlibCompressPlugin;

        if (fast_format) {
            create_compressed_diff(
                new_data, new_data + new_size,
                old_data, old_data + old_size,
                diff,
                compress,
                4, false,
                (size_t)thread_num
            );
        } else {
            create_single_compressed_diff(
                new_data, new_data + new_size,
                old_data, old_data + old_size,
                diff,
                compress,
                1024 * 256, 4, false,
                (size_t)thread_num
            );
        }
        *out_patch_size = diff.size();
        if (diff.size() == 0) {
            *out_patch = nullptr;
            return 0;
        }
        *out_patch = (unsigned char*)std::malloc(diff.size());
        if (!*out_patch) return -8;
        std::memcpy(*out_patch, diff.data(), diff.size());
        return 0;
    } catch (...) {
        return -1;
    }
}

int hdiffpatch_create_file(
    const char* old_file,
    const char* new_file,
    const char* patch_file,
    int thread_num,
    int use_compression,
    int fast_format)
{
    hpatch_TFileStreamInput  oldStream;
    hpatch_TFileStreamInput  newStream;
    hpatch_TFileStreamOutput patchStream;
    int ret = 0;

    hpatch_TFileStreamInput_init(&oldStream);
    hpatch_TFileStreamInput_init(&newStream);
    hpatch_TFileStreamOutput_init(&patchStream);

    if (!hpatch_TFileStreamInput_open(&oldStream, old_file))
        { ret = -5; goto cleanup; }
    if (!hpatch_TFileStreamInput_open(&newStream, new_file))
        { ret = -6; goto cleanup; }
    if (!hpatch_TFileStreamOutput_open(&patchStream, patch_file, ~(hpatch_StreamPos_t)0))
        { ret = -7; goto cleanup; }
    hpatch_TFileStreamOutput_setRandomOut(&patchStream, hpatch_TRUE);

    try {
        const hdiff_TCompress* compress = nullptr;
        if (use_compression)
            compress = (const hdiff_TCompress*)&zlibCompressPlugin;

        size_t capped_threads = (size_t)thread_num;
        if (capped_threads < 1) capped_threads = 1;

        const hdiff_TMTSets_s mtsets = {
            capped_threads,
            capped_threads,
            hpatch_TRUE,
            hpatch_TRUE
        };

        hpatch_StreamPos_t oldSize = oldStream.base.streamSize;
        size_t kMatchBlockSize = 64;
        if      (oldSize > 500ULL << 20) kMatchBlockSize = 256;
        else if (oldSize > 100ULL << 20) kMatchBlockSize = 128;

        if (fast_format) {
            create_compressed_diff_stream(
                &newStream.base,
                &oldStream.base,
                &patchStream.base,
                compress,
                kMatchBlockSize,
                &mtsets
            );
        } else {
            create_single_compressed_diff_stream(
                &newStream.base,
                &oldStream.base,
                &patchStream.base,
                compress,
                1024 * 256,
                kMatchBlockSize,
                &mtsets
            );
        }
    } catch (...) {
        ret = -1;
    }

cleanup:
    if (patchStream.m_file) hpatch_TFileStreamOutput_close(&patchStream);
    if (newStream.m_file)   hpatch_TFileStreamInput_close(&newStream);
    if (oldStream.m_file)   hpatch_TFileStreamInput_close(&oldStream);
    return ret;
}

int hdiffpatch_apply(
    const unsigned char* old_data, size_t old_size,
    const unsigned char* patch_data, size_t patch_size,
    unsigned char** out_new_data, size_t* out_new_size,
    int thread_num)
{
    try {
        {
            hpatch_singleCompressedDiffInfo diffInfo;
            if (getSingleCompressedDiffInfo_mem(&diffInfo, patch_data, patch_data + patch_size)) {
                size_t new_size = (size_t)diffInfo.newDataSize;
                *out_new_size = new_size;
                if (new_size == 0) {
                    *out_new_data = nullptr;
                    return 0;
                }
                *out_new_data = (unsigned char*)std::malloc(new_size);
                if (!*out_new_data) return -2;

                hpatch_TStreamOutput out_newStream;
                hpatch_TStreamInput  oldStream;
                hpatch_TStreamInput  diffStream;
                mem_as_hStreamOutput(&out_newStream, *out_new_data, *out_new_data + new_size);
                mem_as_hStreamInput(&oldStream, old_data, old_data + old_size);
                mem_as_hStreamInput(&diffStream, patch_data, patch_data + patch_size);

                Cache cache = { nullptr };
                sspatch_listener_t listener;
                std::memset(&listener, 0, sizeof(listener));
                listener.import = &cache;
                listener.onDiffInfo = on_diff_info;
                listener.onPatchFinish = on_patch_finish;

                size_t capped_threads = (size_t)thread_num;
                if (capped_threads < 1) capped_threads = 1;
                if (capped_threads > 5) capped_threads = 5;

                hpatch_BOOL result = patch_single_stream(
                    &listener,
                    &out_newStream,
                    &oldStream,
                    &diffStream,
                    0,
                    nullptr,
                    capped_threads
                );

                if (!result) {
                    std::free(*out_new_data);
                    *out_new_data = nullptr;
                    *out_new_size = 0;
                    return -3;
                }
                return 0;
            }
        }

        {
            hpatch_compressedDiffInfo diffInfo;
            std::memset(&diffInfo, 0, sizeof(diffInfo));
            if (!getCompressedDiffInfo_mem(&diffInfo, patch_data, patch_data + patch_size))
                return -1;

            size_t new_size = (size_t)diffInfo.newDataSize;
            *out_new_size = new_size;
            if (new_size == 0) {
                *out_new_data = nullptr;
                return 0;
            }
            *out_new_data = (unsigned char*)std::malloc(new_size);
            if (!*out_new_data) return -2;

            hpatch_TDecompress* decompressPlugin = nullptr;
            if (diffInfo.compressedCount > 0)
                decompressPlugin = &zlibDecompressPlugin;

            hpatch_BOOL result = patch_decompress_mem(
                *out_new_data, *out_new_data + new_size,
                old_data, old_data + old_size,
                patch_data, patch_data + patch_size,
                decompressPlugin
            );

            if (!result) {
                std::free(*out_new_data);
                *out_new_data = nullptr;
                *out_new_size = 0;
                return -3;
            }
            return 0;
        }
    } catch (...) {
        if (*out_new_data) {
            std::free(*out_new_data);
            *out_new_data = nullptr;
        }
        *out_new_size = 0;
        return -4;
    }
}

int hdiffpatch_apply_file(
    const char* old_file,
    const unsigned char* patch_data, size_t patch_size,
    const char* output_file,
    int thread_num)
{
    int ret = 0;
    bool old_opened = false;
    hpatch_TFileStreamInput  oldStream;
    hpatch_TFileStreamOutput outStream;

    hpatch_TFileStreamInput_init(&oldStream);

    try {
        hpatch_TStreamInput diffStream;
        mem_as_hStreamInput(&diffStream, patch_data, patch_data + patch_size);

        {
            hpatch_singleCompressedDiffInfo diffInfo;
            if (getSingleCompressedDiffInfo_mem(&diffInfo, patch_data, patch_data + patch_size)) {
                if (!hpatch_TFileStreamInput_open(&oldStream, old_file))
                    { ret = -5; goto cleanup; }
                old_opened = true;

                hpatch_TFileStreamOutput_init(&outStream);
                if (!hpatch_TFileStreamOutput_open(&outStream, output_file, ~(hpatch_StreamPos_t)0))
                    { ret = -7; goto cleanup; }
                hpatch_TFileStreamOutput_setRandomOut(&outStream, hpatch_TRUE);

                size_t capped_threads = (size_t)thread_num;
                if (capped_threads < 1) capped_threads = 1;
                if (capped_threads > 5) capped_threads = 5;

                Cache cache = { nullptr };
                sspatch_listener_t listener;
                std::memset(&listener, 0, sizeof(listener));
                listener.import = &cache;
                listener.onDiffInfo = on_diff_info;
                listener.onPatchFinish = on_patch_finish;

                if (!patch_single_stream(&listener, &outStream.base, &oldStream.base,
                                         &diffStream, 0, nullptr, capped_threads))
                    { ret = -3; goto cleanup; }
                goto cleanup;
            }
        }

        {
            std::vector<unsigned char> old_data;
            if (!hpatch_TFileStreamInput_open(&oldStream, old_file))
                { ret = -5; goto cleanup; }
            old_opened = true;
            old_data.resize((size_t)oldStream.base.streamSize);
            if (!old_data.empty())
                oldStream.base.read(&oldStream.base, 0, old_data.data(),
                                    old_data.data() + old_data.size());
            hpatch_TFileStreamInput_close(&oldStream);
            old_opened = false;

            hpatch_compressedDiffInfo diffInfo;
            std::memset(&diffInfo, 0, sizeof(diffInfo));
            if (!getCompressedDiffInfo_mem(&diffInfo, patch_data, patch_data + patch_size))
                { ret = -1; goto cleanup; }

            size_t new_size = (size_t)diffInfo.newDataSize;
            std::vector<unsigned char> new_data(new_size);

            hpatch_TDecompress* decompressPlugin = nullptr;
            if (diffInfo.compressedCount > 0)
                decompressPlugin = &zlibDecompressPlugin;

            if (!patch_decompress_mem(new_data.data(), new_data.data() + new_size,
                                      old_data.data(), old_data.data() + old_data.size(),
                                      patch_data, patch_data + patch_size,
                                      decompressPlugin))
                { ret = -3; goto cleanup; }

            hpatch_TFileStreamOutput_init(&outStream);
            if (!hpatch_TFileStreamOutput_open(&outStream, output_file,
                                               ~(hpatch_StreamPos_t)0))
                { ret = -7; goto cleanup; }
            hpatch_TFileStreamOutput_setRandomOut(&outStream, hpatch_TRUE);
            outStream.base.write(&outStream.base, 0, new_data.data(),
                                 new_data.data() + new_data.size());
        }
    } catch (...) {
        ret = -4;
    }

cleanup:
    if (outStream.m_file) hpatch_TFileStreamOutput_close(&outStream);
    if (old_opened && oldStream.m_file) hpatch_TFileStreamInput_close(&oldStream);
    return ret;
}

void hdiffpatch_free(void* ptr)
{
    std::free(ptr);
}
