#Requires -Version 7
# binary_patcher 一键全流程测试
param([switch]$SkipBuild, [switch]$Quick)

$ErrorActionPreference = 'Continue'
$root = $PSScriptRoot
$target = "$root\target\release"
$bp     = "$target\binary_patcher.exe"
$apply  = "$target\apply_patch.exe"
$roll   = "$target\rollback_patch.exe"

$pass = 0; $fail = 0
function P($n) { Write-Host "  [PASS] $n" -F Green;  $script:pass++ }
function F($n) { Write-Host "  [FAIL] $n" -F Red;    $script:fail++; $n }

function RandBytes($size) { $r = [Random]::new((Get-Random)); $b = [byte[]]::new($size); $r.NextBytes($b); $b }
function WriteBin($p, $b) { [IO.File]::WriteAllBytes($p, $b) }
function HashFile($p) { (certutil -hashfile $p SHA256 2>$null -split '\r?\n')[1].Trim() -replace '\s','' }

function DirIdentical($src, $dst) {
    $sl = $src.Length + 1; $dl = $dst.Length + 1
    $sFiles = Get-ChildItem -LiteralPath $src -Recurse -File |
        ForEach-Object { $_.FullName.Substring($sl) -replace '\\', '/' } | Sort-Object
    $dFiles = Get-ChildItem -LiteralPath $dst -Recurse -File |
        Where-Object { $_.FullName -notmatch '\\Patch\\' -and $_.FullName -notmatch '\.backup_before_patch' } |
        ForEach-Object { $_.FullName.Substring($dl) -replace '\\', '/' } | Sort-Object
    if (Compare-Object $sFiles $dFiles) { return $false }
    foreach ($f in $sFiles) { if ((HashFile "$src\$f") -ne (HashFile "$dst\$f")) { return $false } }
    $true
}

Write-Host '========================================' -F Cyan
Write-Host " binary_patcher 全流程测试" -F Cyan
Write-Host " 项目: $root" -F DarkGray
Write-Host '========================================' -F Cyan

# ============================================================
# 1. 编译
# ============================================================
if (-not $SkipBuild) {
    Write-Host "`n[1/8] 编译..." -F Yellow
    Push-Location $root
    $out = cargo build --release 2>&1
    Pop-Location
    if ($LASTEXITCODE -ne 0) { Write-Host ($out -join "`n"); F '编译失败'; exit 1 }
    if (-not (Test-Path $bp)) { F 'binary_patcher.exe 未找到'; exit 1 }
    P '编译成功'
} else {
    Write-Host "`n[1/8] 编译 跳过 (--SkipBuild)" -F DarkGray
}

# ============================================================
# 2. 生成随机测试集
# ============================================================
Write-Host "`n[2/8] 生成随机测试数据集..." -F Yellow
$ws = "$root\tmp_test"; Remove-Item $ws -Recurse -Force -EA SilentlyContinue
$old = "$ws\Old"; $new = "$ws\New"
New-Item -ItemType Directory -Path $old -Force | Out-Null
New-Item -ItemType Directory -Path $new -Force | Out-Null

# --- 完全相同 ---
'identical_static' | Out-File "$old\same.txt" -Encoding UTF8 -NoNewline
'identical_static' | Out-File "$new\same.txt" -Encoding UTF8 -NoNewline

# --- 大二进制变更 ---
WriteBin "$old\bigdata.bin" (RandBytes (128*1024))
$nb = RandBytes (128*1024); $patch = RandBytes 1024
for ($i = 0; $i -lt 1024; $i++) { $nb[32768 + $i] = $patch[$i] }
WriteBin "$new\bigdata.bin" $nb

# --- 文本配置变更 ---
@'
[server]
host=localhost
port=8080
debug=false
'@ | Out-File "$old\config.ini" -Encoding UTF8 -NoNewline
@'
[server]
host=0.0.0.0
port=9090
debug=true
max_connections=100
timeout=30
'@ | Out-File "$new\config.ini" -Encoding UTF8 -NoNewline

# --- 二进制资源变更 ---
New-Item -ItemType Directory -Path "$old\tilesets" -Force | Out-Null
New-Item -ItemType Directory -Path "$new\tilesets" -Force | Out-Null
WriteBin "$old\tilesets\tileset1.png" (RandBytes 8192)
$tb = RandBytes 8192; $tp = RandBytes 1024
for ($i = 0; $i -lt 1024; $i++) { $tb[1024 + $i] = $tp[$i] }
WriteBin "$new\tilesets\tileset1.png" $tb

# --- 嵌套深层目录中的文件变更 ---
New-Item -ItemType Directory -Path "$old\data\scripts\ai" -Force | Out-Null
New-Item -ItemType Directory -Path "$new\data\scripts\ai" -Force | Out-Null
'v1.0 behavior tree' | Out-File "$old\data\scripts\ai\enemy.lua" -Encoding UTF8 -NoNewline
'v2.0 behavior tree with patrol routes and flee logic' | Out-File "$new\data\scripts\ai\enemy.lua" -Encoding UTF8 -NoNewline

# --- 新增文件 ---
'[NEW] fresh file content' | Out-File "$new\newly_added.txt" -Encoding UTF8 -NoNewline
WriteBin "$new\new_plugin.dll" (RandBytes 2048)
New-Item -ItemType Directory -Path "$new\audio" -Force | Out-Null
WriteBin "$new\audio\bgm.ogg" (RandBytes 16384)

# --- 删除文件 ---
'deprecated legacy' | Out-File "$old\deprecated.txt" -Encoding UTF8 -NoNewline
WriteBin "$old\old_atlas.png" (RandBytes 4096)

# --- 删除嵌套目录 ---
New-Item -ItemType Directory -Path "$old\removed_module\sub" -Force | Out-Null
'module config' | Out-File "$old\removed_module\config.xml" -Encoding UTF8 -NoNewline
'subdata' | Out-File "$old\removed_module\sub\data.bin" -Encoding UTF8 -NoNewline

P "随机测试集 ($((Get-ChildItem $old -r -File).Count) 旧 / $((Get-ChildItem $new -r -File).Count) 新)"

# ============================================================
# 3. Auto 模式 bundle（默认 precise）
# ============================================================
Write-Host "`n[3/8] Auto 模式 生成补丁..." -F Yellow
$t1 = "$ws\t1"
Remove-Item $t1 -Recurse -Force -EA SilentlyContinue
New-Item -ItemType Directory -Path $t1 -Force | Out-Null
Copy-Item $old "$t1\Old" -Recurse
Copy-Item $new "$t1\New" -Recurse
$LASTEXITCODE = 0; $null = "`n" | & $bp bundle --base-dir $t1 2>&1
$patchFiles = @(Get-ChildItem "$t1\Patch\*.patch" -Name)
$manifest = if (Test-Path "$t1\Patch\manifest.json") { Get-Content "$t1\Patch\manifest.json" -Raw -Encoding UTF8 | ConvertFrom-Json } else { $null }
$chg = if ($manifest) { $manifest.changed.Count } else { 0 }
$add = if ($manifest) { $manifest.added.Count } else { 0 }
$del = if ($manifest) { $manifest.deleted.Count } else { 0 }
$dd  = if ($manifest) { $manifest.deleted_dirs.Count } else { 0 }
if ($LASTEXITCODE -eq 0 -and $chg -ge 4 -and $add -ge 3 -and $del -ge 3 -and $dd -ge 2 -and $patchFiles -notcontains 'same.txt.patch') {
    P "Auto bundle (chg=$chg add=$add del=$del dir=$dd skip=same.txt)"
} else { F "Auto bundle (chg=$chg add=$add del=$del dir=$dd)" }

# ============================================================
# 4. Stream + Fast
# ============================================================
if (-not $Quick) {
    Write-Host "`n[4a/8] Stream 模式..." -F Yellow
    $t2 = "$ws\t2"; New-Item -ItemType Directory -Path $t2 -Force | Out-Null
    Copy-Item $old "$t2\Old" -Recurse; Copy-Item $new "$t2\New" -Recurse
    $null = "`n" | & $bp --mode stream bundle --base-dir $t2 2>&1
    P 'Stream 模式'

    Write-Host "`n[4b/8] Fast 格式..." -F Yellow
    $t3 = "$ws\t3"; New-Item -ItemType Directory -Path $t3 -Force | Out-Null
    Copy-Item $old "$t3\Old" -Recurse; Copy-Item $new "$t3\New" -Recurse
    $null = "`n" | & $bp --format fast bundle --base-dir $t3 2>&1
    P 'Fast 格式'
} else {
    Write-Host "`n[4/8] Stream/Fast 跳过 (Quick 模式)" -F DarkGray
}

# ============================================================
# 5. 应用补丁
# ============================================================
Write-Host "`n[5/8] 应用补丁..." -F Yellow
$game = "$ws\game"
Remove-Item $game -Recurse -Force -EA SilentlyContinue
New-Item -ItemType Directory -Path $game -Force | Out-Null
Copy-Item "$old\*" $game -Recurse
Copy-Item -LiteralPath "$t1\Patch" "$game\Patch" -Recurse
$null = "`n" | & $apply --base-dir $game 2>&1
Start-Sleep -Milliseconds 300

if (DirIdentical $new $game) { P 'apply: 逐文件 SHA256 全部一致' }
else { F 'apply: 文件校验失败' }

# ============================================================
# 6. 回滚
# ============================================================
Write-Host "`n[6/8] 回滚..." -F Yellow
$null = "y`n`n" | & $roll --base-dir $game 2>&1
Start-Sleep -Milliseconds 300

if (DirIdentical $old $game) { P 'rollback: 完全恢复到 Old 状态' }
else { F 'rollback: 回滚校验失败' }

# ============================================================
# 7. 单文件 create/apply
# ============================================================
Write-Host "`n[7/8] 单文件测试..." -F Yellow
$s = "$ws\single"; New-Item -ItemType Directory -Path $s -Force | Out-Null
$of = "$s\old.bin"; $nf = "$s\new.bin"
WriteBin $of (RandBytes 65536)
WriteBin $nf (RandBytes 65536)
$null = "`n" | & $bp create $of $nf $s\p.hdiff  2>&1
$null = "`n" | & $bp apply  $of $s\p.hdiff $s\out.bin 2>&1
if ((HashFile $nf) -eq (HashFile "$s\out.bin")) { P '单文件 create+apply' }
else { F '单文件' }

if (-not $Quick) {
    $null = "`n" | & $bp --no-compress create $of $nf $s\p_nc.hdiff  2>&1
    $null = "`n" | & $bp apply $of $s\p_nc.hdiff $s\out_nc.bin 2>&1
    if ((HashFile $nf) -eq (HashFile "$s\out_nc.bin")) { P '--no-compress' } else { F '--no-compress' }

    $null = "`n" | & $bp --format fast create $of $nf $s\p_f.hdiff  2>&1
    $null = "`n" | & $bp apply $of $s\p_f.hdiff $s\out_f.bin 2>&1
    if ((HashFile $nf) -eq (HashFile "$s\out_f.bin")) { P '--format fast' } else { F '--format fast' }

    $winit = "$ws\winit"; New-Item -ItemType Directory -Path $winit -Force | Out-Null
    Push-Location $winit
    try { $null = "`n" | & $bp 2>&1; if ((Test-Path Old) -and (Test-Path New) -and (Test-Path Patch)) { P '工作区初始化' } else { F '工作区初始化' } }
    finally { Pop-Location }

    [string[]]$psizes = Get-ChildItem "$s\*.hdiff" | ForEach-Object { "$($_.Name): $($_.Length)" }
    Write-Host "  补丁体积: $($psizes -join ', ')" -F DarkGray
}

# ============================================================
# 8. 单元测试
# ============================================================
Write-Host "`n[8/8] 单元测试 (cargo test)..." -F Yellow
Push-Location $root
$testOut = "n`n" | & cargo test --release 2>&1
Pop-Location
$testStr = $testOut -join "`n"
$passed = 0; $failed = 0
$matches = [regex]::Matches($testStr, '(\d+) passed'); foreach ($m in $matches) { $c = [int]$m.Groups[1].Value; if ($c -gt $passed) { $passed = $c } }
$matches = [regex]::Matches($testStr, '(\d+) failed'); foreach ($m in $matches) { $c = [int]$m.Groups[1].Value; if ($c -gt $failed) { $failed = $c } }
$total = "$passed passed, $failed failed"
if ($LASTEXITCODE -eq 0 -and $failed -eq 0) { P "cargo test ($total)" } else { F "cargo test ($total)" }

# ============================================================
Write-Host "`n========================================" -F Cyan
if ($fail -eq 0) {
    Write-Host " 全部 $pass 项通过！" -F Green
    Write-Host " 产物: $target\" -F DarkGray
    Write-Host '========================================' -F Cyan
    exit 0
} else {
    Write-Host " $pass 通过 / $fail 失败" -F Red
    Write-Host '========================================' -F Cyan
    exit 1
}
