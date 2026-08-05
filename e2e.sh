#!/usr/bin/env bash
# binary_patcher 一键全流程测试（Linux，对应 e2e.ps1）
# 用法: ./e2e.sh [--skip-build] [--quick]
set -u

root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
target="$root/target/release"
bp="$target/binary_patcher"
apply="$target/apply_patch"
roll="$target/rollback_patch"

skip_build=0
quick=0
for a in "$@"; do
  case "$a" in
    --skip-build) skip_build=1 ;;
    --quick) quick=1 ;;
    *)
      echo "未知参数: $a" >&2
      exit 2
      ;;
  esac
done

pass=0
fail=0
P() { printf "  \033[32m[PASS]\033[0m %s\n" "$1"; pass=$((pass + 1)); }
F() { printf "  \033[31m[FAIL]\033[0m %s\n" "$1"; fail=$((fail + 1)); }

hash_file() { sha256sum "$1" | awk '{print $1}'; }

dir_identical() {
  local src="$1" dst="$2"
  local sl=$(( ${#src} + 1 ))
  local i rel s d
  mapfile -t sList < <(find "$src" -type f | sort)
  mapfile -t dList < <(find "$dst" -type f | grep -Ev '/Patch/|backup_before_patch' | sort)
  if [ "${#sList[@]}" -ne "${#dList[@]}" ]; then return 1; fi
  for ((i = 0; i < ${#sList[@]}; i++)); do
    rel="${sList[$i]:$sl}"
    d="$dst/$rel"
    [ -f "$d" ] || return 1
    [ "$(hash_file "${sList[$i]}")" = "$(hash_file "$d")" ] || return 1
  done
  return 0
}

echo '========================================'
echo " binary_patcher 全流程测试"
echo " 项目: $root"
echo '========================================'

# ============================================================
# 1. 编译
# ============================================================
if [ "$skip_build" -eq 0 ]; then
  echo
  echo "[1/8] 编译..."
  (cd "$root" && cargo build --release 2>&1)
  if [ $? -ne 0 ]; then F '编译失败'; exit 1; fi
  if [ ! -x "$bp" ]; then F 'binary_patcher 未找到'; exit 1; fi
  P '编译成功'
else
  echo
  echo "[1/8] 编译 跳过 (--skip-build)"
fi

# ============================================================
# 2. 生成随机测试集
# ============================================================
echo
echo "[2/8] 生成随机测试数据集..."
ws="$root/tmp_test"
rm -rf "$ws"
old="$ws/Old"
new="$ws/New"
mkdir -p "$old" "$new"

# --- 完全相同 ---
printf 'identical_static' > "$old/same.txt"
printf 'identical_static' > "$new/same.txt"

# --- 大二进制变更 ---
head -c 131072 /dev/urandom > "$old/bigdata.bin"
head -c 131072 /dev/urandom > "$new/bigdata.bin"
head -c 1024 /dev/urandom | dd of="$new/bigdata.bin" bs=1 seek=32768 conv=notrunc 2>/dev/null

# --- 文本配置变更 ---
cat > "$old/config.ini" <<'EOF'
[server]
host=localhost
port=8080
debug=false
EOF
cat > "$new/config.ini" <<'EOF'
[server]
host=0.0.0.0
port=9090
debug=true
max_connections=100
timeout=30
EOF

# --- 二进制资源变更 ---
mkdir -p "$old/tilesets" "$new/tilesets"
head -c 8192 /dev/urandom > "$old/tilesets/tileset1.png"
head -c 8192 /dev/urandom > "$new/tilesets/tileset1.png"
head -c 1024 /dev/urandom | dd of="$new/tilesets/tileset1.png" bs=1 seek=1024 conv=notrunc 2>/dev/null

# --- 嵌套深层目录中的文件变更 ---
mkdir -p "$old/data/scripts/ai" "$new/data/scripts/ai"
printf 'v1.0 behavior tree' > "$old/data/scripts/ai/enemy.lua"
printf 'v2.0 behavior tree with patrol routes and flee logic' > "$new/data/scripts/ai/enemy.lua"

# --- 新增文件 ---
printf '[NEW] fresh file content' > "$new/newly_added.txt"
head -c 2048 /dev/urandom > "$new/new_plugin.dll"
mkdir -p "$new/audio"
head -c 16384 /dev/urandom > "$new/audio/bgm.ogg"

# --- 删除文件 ---
printf 'deprecated legacy' > "$old/deprecated.txt"
head -c 4096 /dev/urandom > "$old/old_atlas.png"

# --- 删除嵌套目录 ---
mkdir -p "$old/removed_module/sub"
printf 'module config' > "$old/removed_module/config.xml"
printf 'subdata' > "$old/removed_module/sub/data.bin"

P "随机测试集 ($(find "$old" -type f | wc -l) 旧 / $(find "$new" -type f | wc -l) 新)"

# ============================================================
# 3. Auto 模式 bundle（默认 precise）
# ============================================================
echo
echo "[3/8] Auto 模式 生成补丁..."
t1="$ws/t1"
mkdir -p "$t1"
cp -r "$old" "$t1/Old"
cp -r "$new" "$t1/New"
rc=0
printf '\n' | "$bp" bundle --base-dir "$t1" >/dev/null 2>&1 || rc=$?
manifest="$t1/Patch/manifest.json"
if [ -f "$manifest" ]; then
  chg=$(grep -o '"patch_file"' "$manifest" | wc -l)
  add=$(grep -o '"file"' "$manifest" | wc -l)
  tot=$(grep -o '"path"' "$manifest" | wc -l)
  del=$((tot - chg - add))
  dd=$(awk '/"deleted_dirs": \[/{in_arr=1; next} in_arr && /\]/{exit} in_arr && /"/{c++} END{print c+0}' "$manifest")
else
  chg=0; add=0; del=0; dd=0
fi
if [ "$rc" -eq 0 ] && [ "$chg" -ge 4 ] && [ "$add" -ge 3 ] && [ "$del" -ge 3 ] && [ "$dd" -ge 2 ] && [ ! -f "$t1/Patch/same.txt.patch" ]; then
  P "Auto bundle (chg=$chg add=$add del=$del dir=$dd skip=same.txt)"
else
  F "Auto bundle (chg=$chg add=$add del=$del dir=$dd)"
fi

# ============================================================
# 4. Stream + Fast
# ============================================================
if [ "$quick" -eq 0 ]; then
  echo
  echo "[4a/8] Stream 模式..."
  t2="$ws/t2"
  mkdir -p "$t2"
  cp -r "$old" "$t2/Old"
  cp -r "$new" "$t2/New"
  printf '\n' | "$bp" --mode stream bundle --base-dir "$t2" >/dev/null 2>&1
  P 'Stream 模式'

  echo
  echo "[4b/8] Fast 格式..."
  t3="$ws/t3"
  mkdir -p "$t3"
  cp -r "$old" "$t3/Old"
  cp -r "$new" "$t3/New"
  printf '\n' | "$bp" --format fast bundle --base-dir "$t3" >/dev/null 2>&1
  P 'Fast 格式'
else
  echo
  echo "[4/8] Stream/Fast 跳过 (--quick)"
fi

# ============================================================
# 5. 应用补丁
# ============================================================
echo
echo "[5/8] 应用补丁..."
game="$ws/game"
mkdir -p "$game"
cp -r "$old/." "$game/"
cp -r "$t1/Patch" "$game/Patch"
printf '\n' | "$apply" --base-dir "$game" >/dev/null 2>&1

if dir_identical "$new" "$game"; then
  P 'apply: 逐文件 SHA256 全部一致'
else
  F 'apply: 文件校验失败'
fi

# ============================================================
# 6. 回滚
# ============================================================
echo
echo "[6/8] 回滚..."
printf 'y\n\n' | "$roll" --base-dir "$game" >/dev/null 2>&1

if dir_identical "$old" "$game"; then
  P 'rollback: 完全恢复到 Old 状态'
else
  F 'rollback: 回滚校验失败'
fi

# ============================================================
# 7. 单文件 create/apply
# ============================================================
echo
echo "[7/8] 单文件测试..."
s="$ws/single"
mkdir -p "$s"
of="$s/old.bin"
nf="$s/new.bin"
head -c 65536 /dev/urandom > "$of"
head -c 65536 /dev/urandom > "$nf"
printf '\n' | "$bp" create "$of" "$nf" "$s/p.hdiff" >/dev/null 2>&1
printf '\n' | "$bp" apply "$of" "$s/p.hdiff" "$s/out.bin" >/dev/null 2>&1
if [ "$(hash_file "$nf")" = "$(hash_file "$s/out.bin")" ]; then
  P '单文件 create+apply'
else
  F '单文件'
fi

if [ "$quick" -eq 0 ]; then
  printf '\n' | "$bp" --no-compress create "$of" "$nf" "$s/p_nc.hdiff" >/dev/null 2>&1
  printf '\n' | "$bp" apply "$of" "$s/p_nc.hdiff" "$s/out_nc.bin" >/dev/null 2>&1
  if [ "$(hash_file "$nf")" = "$(hash_file "$s/out_nc.bin")" ]; then
    P '--no-compress'
  else
    F '--no-compress'
  fi

  printf '\n' | "$bp" --format fast create "$of" "$nf" "$s/p_f.hdiff" >/dev/null 2>&1
  printf '\n' | "$bp" apply "$of" "$s/p_f.hdiff" "$s/out_f.bin" >/dev/null 2>&1
  if [ "$(hash_file "$nf")" = "$(hash_file "$s/out_f.bin")" ]; then
    P '--format fast'
  else
    F '--format fast'
  fi

  winit="$ws/winit"
  mkdir -p "$winit"
  (cd "$winit" && printf '\n' | "$bp" >/dev/null 2>&1)
  if [ -d "$winit/Old" ] && [ -d "$winit/New" ] && [ -d "$winit/Patch" ]; then
    P '工作区初始化'
  else
    F '工作区初始化'
  fi

  sizes=""
  for f in "$s"/*.hdiff; do
    sizes="$sizes$(basename "$f"): $(wc -c < "$f")B, "
  done
  echo "  补丁体积: $sizes"
fi

# ============================================================
# 8. 单元测试
# ============================================================
echo
echo "[8/8] 单元测试 (cargo test)..."
test_out=$(cd "$root" && cargo test --release 2>&1)
test_rc=$?
passed=$(printf '%s' "$test_out" | grep -oE '[0-9]+ passed' | awk '{s+=$1} END {print s+0}')
failed=$(printf '%s' "$test_out" | grep -oE '[0-9]+ failed' | awk '{s+=$1} END {print s+0}')
if [ "$test_rc" -eq 0 ] && [ "$failed" -eq 0 ]; then
  P "cargo test ($passed passed, 0 failed)"
else
  F "cargo test ($passed passed, $failed failed)"
fi

# ============================================================
echo
echo '========================================'
if [ "$fail" -eq 0 ]; then
  echo " 全部 $pass 项通过！"
  echo " 产物: $target/"
  echo '========================================'
  exit 0
else
  echo " $pass 通过 / $fail 失败"
  echo '========================================'
  exit 1
fi
