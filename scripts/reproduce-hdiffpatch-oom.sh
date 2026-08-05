#!/usr/bin/env bash
# Reproduce HDiffPatch issue:
#   stream diff (-s + -SD + -c-zlib) silently produces a truncated/corrupted patch
#   under memory pressure (ulimit -v), or dies with
#   "terminate called without an active exception".
#
# Tested on: Fedora 44 x86_64, HDiffPatch master (v5.1.3, commit of 2026-08), 16 cores
# Requirements: git, make, g++, fallocate, dd
set -u

WORK="$(mktemp -d /tmp/hdiffpatch-repro-XXXXXX)"
trap 'rm -rf "$WORK"' EXIT
cd "$WORK" || exit 1

echo "==> [1/4] Cloning HDiffPatch..."
git clone -q --depth 1 https://github.com/sisong/HDiffPatch.git || exit 1

echo "==> [2/4] Building hdiffz with the zlib compress plugin (ZLIB=1)..."
if [ ! -d zlib ]; then
    git clone -q --depth 1 https://github.com/sisong/zlib.git zlib || exit 1
fi
(cd HDiffPatch && make ZLIB=1 BZIP2=0 LDEF=0 LZMA=0 BSD=0 ZSTD=0 MD5=0 DIR_DIFF=0 \
    -j"$(nproc)" hdiffz) > build.log 2>&1 || {
    echo "build failed, see $WORK/build.log"; exit 1; }
echo "    hdiffz built: $(ls -lh HDiffPatch/hdiffz | awk '{print $5}')"
HDZ="$WORK/HDiffPatch/hdiffz"

echo "==> [3/4] Generating test data (600 MB sparse old/new, 1 byte difference)..."
fallocate -l 600M old.bin
cp old.bin new.bin
printf 'X' | dd of=new.bin bs=1 seek=104857600 conv=notrunc 2>/dev/null

echo "==> [4/4] Diffing under memory limits..."
echo "    baseline (no limit):"
$HDZ -p-15 -s-256 -SD-256k -c-zlib-9 old.bin new.bin p_ref.hdiff 2>&1 | grep -E "diffDataSize|patch check"
REF_SIZE="$(stat -c %s p_ref.hdiff)"
echo "    baseline patch size: $REF_SIZE bytes"
echo

echo "    scanning ulimit -v 1100000..850000 (KB), 3 rounds each:"
echo "    (rc=exit code, size=patch bytes, selfcheck=built-in patch verification passed)"
for LIM in 1100000 1050000 1000000 950000 900000 850000; do
    for round in 1 2 3; do
        bash -c "ulimit -v $LIM; exec $HDZ -f -p-15 -s-256 -SD-256k -c-zlib-9 old.bin new.bin p_t.hdiff" \
            > run.log 2>&1
        rc=$?
        size="$(stat -c %s p_t.hdiff 2>/dev/null || echo 0)"
        selfcheck="$(grep -c 'patch check diff data ok' run.log)"
        status="ok"
        if grep -q "terminate called" run.log; then status="TERMINATED"
        elif [ "$rc" -ne 0 ]; then status="failed(rc=$rc)"
        elif [ "$size" -ne "$REF_SIZE" ]; then status="SILENT CORRUPTION"
        elif [ "$selfcheck" -eq 0 ]; then status="selfcheck failed"
        fi
        if [ "$status" != "ok" ]; then
            echo "    >>> last lines of run.log:"; sed 's/^/        /' run.log | tail -5
        fi
        printf "    ulimit=%s round=%s rc=%s size=%s selfcheck=%s -> %s\n" \
            "$LIM" "$round" "$rc" "$size" "$selfcheck" "$status"
    done
done

echo
echo "Done. If you see 'SILENT CORRUPTION' or 'TERMINATED', the issue is reproduced."
echo "Keep the baseline size ($REF_SIZE) in mind: a truncated patch is much smaller."
