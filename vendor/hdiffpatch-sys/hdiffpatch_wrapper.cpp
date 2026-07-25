#include "hdiffpatch_wrapper.h"
#include <cstring>
#include <cstdlib>

#include "libHDiffPatch/HDiff/diff.h"
#include "libHDiffPatch/HPatch/patch.h"
#include "file_for_patch.h"

// Enable zlib compression plugin
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
    // Use zlib decompress plugin for compressed patches
    if (info->compressedSize > 0) {
        *out_decompressPlugin = &zlibDecompressPlugin;
    } else {
        *out_decompressPlugin = nullptr;
    }

    size_t cacheSize = (size_t)info->stepMemSize + ((size_t)1 << 20); // stepMem + 1 MB for MT overhead
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
    int use_compression)
{
    try {
        std::vector<unsigned char> diff;
        const hdiff_TCompress* compress = nullptr;
        if (use_compression) {
            // zlibCompressPlugin has hdiff_TCompress as its first member; safe cast
            compress = (const hdiff_TCompress*)&zlibCompressPlugin;
        }
        create_single_compressed_diff(
            new_data, new_data + new_size,
            old_data, old_data + old_size,
            diff,
            compress,
            1024 * 256, 4, false,
            (size_t)thread_num
        );
        *out_patch_size = diff.size();
        *out_patch = (unsigned char*)std::malloc(diff.size());
        if (!*out_patch) return -1;
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
    int use_compression)
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
        if (capped_threads > 5) capped_threads = 5;

        const hdiff_TMTSets_s mtsets = {
            capped_threads,    // threadNum
            capped_threads,    // threadNumForSearch
            hpatch_TRUE,       // newDataIsMTSafe
            hpatch_TRUE        // oldDataIsMTSafe
        };

        // kMatchBlockSize trades memory vs patch quality:
        //   smaller = less memory, larger patch; larger = more memory, smaller patch.
        //   recommended range: 16..16384. Scale up based on old file size to stay
        //   under ~400 MB peak memory.  (hash ≈ oldSize / blockSize * 16)
        hpatch_StreamPos_t oldSize = oldStream.base.streamSize;
        size_t kMatchBlockSize = 64; // default (hdiffz)
        if      (oldSize > 500ULL << 20) kMatchBlockSize = 256; // 1.26GB*16/256≈79MB
        else if (oldSize > 100ULL << 20) kMatchBlockSize = 128;

        create_single_compressed_diff_stream(
            &newStream.base,
            &oldStream.base,
            &patchStream.base,
            compress,
            1024 * 256,
            kMatchBlockSize,
            &mtsets
        );
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
        hpatch_singleCompressedDiffInfo diffInfo;
        if (!getSingleCompressedDiffInfo_mem(&diffInfo, patch_data, patch_data + patch_size))
            return -1; // patch header parse error

        size_t new_size = (size_t)diffInfo.newDataSize;
        *out_new_size = new_size;
        *out_new_data = (unsigned char*)std::malloc(new_size);
        if (!*out_new_data) return -2; // output buffer malloc failed

        // Wrap in-memory buffers as streams for patch_single_stream (supports MT)
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

        // Cap thread count at 5 (HDiffPatch supported range, same as hpatchz -p-)
        size_t capped_threads = (size_t)thread_num;
        if (capped_threads < 1) capped_threads = 1;
        if (capped_threads > 5) capped_threads = 5;

        hpatch_BOOL result = patch_single_stream(
            &listener,
            &out_newStream,
            &oldStream,
            &diffStream,
            0,           // diffInfo_pos
            nullptr,     // coversListener
            capped_threads
        );

        if (!result) {
            // onPatchFinish already freed cache.data via listener
            std::free(*out_new_data);
            *out_new_data = nullptr;
            *out_new_size = 0;
            return -3; // patch apply failed
        }
        return 0;
    } catch (...) {
        if (*out_new_data) {
            std::free(*out_new_data);
            *out_new_data = nullptr;
        }
        *out_new_size = 0;
        return -4; // exception caught
    }
}

void hdiffpatch_free(void* ptr)
{
    std::free(ptr);
}
