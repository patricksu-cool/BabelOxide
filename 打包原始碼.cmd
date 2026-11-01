@echo off
setlocal EnableExtensions DisableDelayedExpansion
"%SystemRoot%\System32\chcp.com" 65001 >nul
set "SOURCE_REVIEW_SELF=%~f0"
set "SOURCE_REVIEW_ROOT=%~dp0"
"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "try { $text = [IO.File]::ReadAllText($env:SOURCE_REVIEW_SELF, [Text.Encoding]::UTF8); $marker = [char]10 + '# SOURCE_REVIEW_POWERSHELL'; $offset = $text.IndexOf($marker, [StringComparison]::Ordinal); if ($offset -lt 0) { throw 'Missing embedded script' }; & ([ScriptBlock]::Create($text.Substring($offset + $marker.Length))) } catch { Write-Host ('[ERROR] Unable to start packager: ' + $_.Exception.GetBaseException().GetType().Name); exit 1 }"
set "SOURCE_REVIEW_EXIT=%ERRORLEVEL%"
echo.
echo Exit code: %SOURCE_REVIEW_EXIT%
if /I not "%~1"=="--no-pause" pause
exit /b %SOURCE_REVIEW_EXIT%
# SOURCE_REVIEW_POWERSHELL
# UTF-8 without BOM; keep the CMD launcher above in ASCII and use CRLF endings.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object Text.UTF8Encoding($false)
$script:Failure = ''
$script:ExitCode = 1
$script:Sensitive = $false
$script:PrivateFiles = 0
$script:Excluded = 0
$stage = '初始化'
$temporary = $null
$ownsTemporary = $false
$published = $false
$records = New-Object 'System.Collections.Generic.List[object]'

function Stop-Review([string]$Reason, [int]$Code = 2) {
    $script:Failure = $Reason
    $script:ExitCode = $Code
    throw [InvalidOperationException]::new('Source review packaging stopped.')
}

function Get-Sha256([byte[]]$Bytes) {
    $hasher = [Security.Cryptography.SHA256]::Create()
    try { return [BitConverter]::ToString($hasher.ComputeHash($Bytes)).Replace('-', '').ToLowerInvariant() }
    finally { $hasher.Dispose() }
}

# Only these roots and root-level files are eligible. Git is never invoked.
$sourceRoots = @('crates', 'python-worker', 'script', 'tests', 'docs', '.cargo', '.github', 'resources')
$rootFiles = @(
    'Cargo.toml', 'Cargo.lock', 'pyproject.toml', 'uv.lock', 'README.md', '.gitignore',
    'LICENSE', 'THIRD_PARTY_NOTICES.md', '打包原始碼.cmd', '打包原始碼.command', 'AGENTS.md', 'CHANGELOG.md', 'CONTRIBUTING.md', 'SECURITY.md',
    '.gitattributes', '.gitmodules', '.editorconfig', '.env.example', 'rust-toolchain.toml',
    'rust-toolchain', 'rustfmt.toml', '.rustfmt.toml', 'clippy.toml', '.clippy.toml',
    'deny.toml', 'Makefile', 'justfile'
)
$required = @('Cargo.toml', 'Cargo.lock', 'pyproject.toml', 'uv.lock', 'README.md', '.gitignore',
    'LICENSE', 'python-worker/worker.py', '.cargo/config.toml', 'resources/config.example.toml',
    'resources/engine-assets.lock.json', 'resources/build-inputs.lock.json', 'THIRD_PARTY_NOTICES.md',
    'resources/babeloxide.svg', 'resources/babeloxide.png', 'resources/babeloxide.ico', 'resources/babeloxide.icns')
$textExtensions = @('.rs', '.py', '.pyi', '.ps1', '.psm1', '.psd1', '.cmd', '.bat', '.sh',
    '.bash', '.command', '.toml', '.lock', '.json', '.md', '.rst', '.txt', '.yaml', '.yml', '.ini', '.cfg',
    '.xml', '.manifest', '.csv', '.tsv', '.html', '.css', '.js', '.ts', '.tsx', '.jsx',
    '.sql', '.proto', '.c', '.h', '.cc', '.cpp', '.hpp', '.cmake', '.in', '.plain', '.feature', '.svg')
$binaryExtensions = @('.exe', '.dll', '.pyd', '.so', '.dylib', '.a', '.lib', '.o', '.obj',
    '.rlib', '.rmeta', '.pdb', '.msi', '.msix', '.zip', '.whl', '.7z', '.tar', '.gz', '.bz2',
    '.xz', '.br', '.deflate', '.gzip', '.zstd', '.pdf', '.docx', '.xlsx', '.pptx', '.png',
    '.jpg', '.jpeg', '.gif', '.ico', '.icns', '.webp', '.mp4', '.mp3', '.onnx', '.pt', '.pth', '.bin',
    '.safetensors', '.h5', '.tflite', '.ttf', '.otf', '.woff', '.woff2', '.db', '.sqlite',
    '.sqlite3', '.pyc', '.pyo', '.class', '.jar', '.wasm', '.dmp')
$excludedDirectories = '^(?i:target|dist|build|\.git|\.venv.*|venv|__pycache__|\.pytest_cache|' +
    '\.ruff_cache|\.mypy_cache|\.tox|node_modules|\.test-output|\.uv-cache|cache|\.cache|' +
    '\.idea|\.vscode|\.serena|\.codebase-memory|\.baseline-home|\.build-tools|\.agents|\.codex|outputs?|logs)$'
$excludedRootDirectories = @('config', 'build-inputs', 'pdf2zh_files', '--google', 'test')
$resourceFiles = @('resources/config.example.toml', 'resources/engine-assets.lock.json',
    'resources/engine-assets.json', 'resources/manifest.json', 'resources/build-inputs.lock.json',
    'resources/windows-README.md', 'resources/macos-README.md', 'resources/README.md',
    'resources/babeloxide.svg', 'resources/babeloxide.png', 'resources/babeloxide.ico', 'resources/babeloxide.icns')

# Original product artwork; only these reviewed binary files may enter source ZIPs.
$artworkHashes = @{
    'resources/babeloxide.png' = '750b663b661a0335a24ef4a4b052fe3fde26545b9c02e6fa00ca6b076d7daa67'
    'resources/babeloxide.ico' = '7b73177a261e02bc85ee08442e474d396765952ffc54abe08e52bc582197a8a9'
    'resources/babeloxide.icns' = '0c9a164605c59d3ff67f7887e80a60326eaaa883814943bb3527eb51be399f06'
}

# Reviewed synthetic HTTP bodies. Binary fixtures must match these exact bytes.
# Their decoded HTML/JSON companions are also included and scanned as text.
$fixtureHashes = @{
    'html.br' = 'd2329f3b58be1ed44293d05c7ac2ac049a0aa9d7042dd2f65186c4c0151b907a'
    'html.deflate' = 'dec66362e5cb5da04cb99fd1b76889bfe5d3226b268001cfc45d2a2135c16f4b'
    'html.gzip' = 'c85604e19d21faffbfa0f720d36a4c7b4e97687ea67f42ac315474c4ad607e81'
    'html.zstd' = 'f066aae30e3651ecbbd1de2d6364386db8912498e2b66ae55c1e44acf0bb24aa'
    'json.br' = 'cfd3b99e1c63672d6e7c421f81a82c24b451b801de1d5f95935cebd239b13698'
    'json.deflate' = 'fb852a0829a62dcedd8c0f34faf153032384cc4b81c3a4cc74fcb248f278ca6b'
    'json.gzip' = '08104fafbbe135372e832e321e6b1b54df43dceb78da138e3e2be2b491b5e0e6'
    'json.zstd' = 'fc005e6df2b88e67bc7cfec9a1df878bc4b0e2077c65b1cca60695a0ea85a596'
    'oversize.gzip' = '53c917864db91e9bfe5482299c950ebf3ec6c6f17110aa216b614f441fb2edf0'
}

# Exact dummy literals in existing loopback/config tests, stored as hashes.
# This is NOT a file exemption: every other literal in these files is scanned.
$dummyHashes = @{
    # Exact userinfo prefix in the URL-rejection unit test; no general URL exemption.
    'crates/app/src/settings_ui.rs' = @('093866188144b0216fb125431572e8ac2df8980dfeeb6ebeb3d9f3987d1e4c1d')
    'crates/config/tests/settings.rs' = @('ec875e4d285b285b668305e00236ff763d715805dd18cc739b5a11b892ff38e3')
    'crates/config/tests/store.rs' = @('ab1d629e15eb32f67f8fc6823ee39a0f98003b907d8c28dbe90a83c62f7a3bc5',
        '62c8ae2b244d92b6e8093098e683d2e736a7f46808520fbf7dee0f2143dbcca7')
    'crates/core/tests/cache_storage.rs' = @('5be3b05a2339aecdb0c543afefc6a563085f5861e6c109dce236363b3319566e',
        '1413c4efe71a5beb9f564fbceffd94b8fefa6f96e1d89c5c879dd2a909d9b201',
        '11adac4deca4835e20ea77c556c8f659f7bae73f561342a12526b159350c5b9e')
    'crates/core/tests/provider_compatibility.rs' = @('f98103e9217f099208569d295c1b276f1821348636c268c854bb2a086e0037cd',
        'f72e714302c899c35b32189fb6f90b6b2636b69348429ab54f40921b78cacbc0')
}
$strongMarkers = '(?im)-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY-----|' +
    '(?<![a-z0-9_-])sk-(?:proj-|svcacct-)?[a-z0-9_-]{16,}|' +
    '\b(?:gh[pousr]_[a-z0-9]{20,}|github_pat_[a-z0-9_]{30,}|xox[baprs]-[a-z0-9-]{15,}|' +
    'AIza[a-z0-9_-]{30,}|(?:AKIA|ASIA)[A-Z0-9]{16})\b|' +
    '\beyJ[a-z0-9_-]{10,}\.[a-z0-9_-]{10,}\.[a-z0-9_-]{10,}\b|' +
    '(?:https?|postgres(?:ql)?|mysql|mongodb(?:\+srv)?|redis|amqps?)://[^\s/:@]+:[^\s/@]+@'
$credentialName = '[a-z0-9_-]*(?:api[_-]?key|access[_-]?key|secret|password|passwd|pwd|token|authorization)[a-z0-9_-]*'
$assignment = '(?i)["'']?(?<name>' + $credentialName + ')["'']?\s*[:=]\s*' +
    '(?:(?:Some|String::from)\(\s*)?["''](?<value>[^"''\r\n]+)["'']'
$bareAssignment = '(?im)^\s*(?:set\s+["'']?|export\s+)?["'']?(?<name>' + $credentialName +
    ')["'']?\s*[:=]\s*(?<value>\$\{\{[^\r\n]*?\}\}|\$\{[^}\r\n]+\}|[^\s"''#;,{}\[\]]+)'

function Test-Placeholder([string]$Value, [string]$Path) {
    if ($Value -match '^(?:\$\{[A-Z0-9_]+\}|<[A-Za-z0-9 _-]+>|\*+|\[REDACTED\])$') { return $true }
    if ($Value -match '^(?:\$[A-Z_][A-Z0-9_]*|%[A-Z_][A-Z0-9_]*%)$') { return $true }
    if ($Value -match '^\$\{\{\s*(?:secrets|env|github|inputs|vars)\.[A-Z0-9_.]+\s*\}\}$') { return $true }
    if ($Value -match '^(?i:your[-_](?:api[-_]?key|token|password)|replace[-_]me)$') { return $true }
    # Synthetic credential labels are only accepted in tests and existing self-test hosts.
    $testFile = $Path -match '/tests?/|/[^/]*tests?\.(?:rs|py|ps1)$' -or
        $Path -in @('crates/app/src/app.rs', 'crates/core/src/lib.rs', 'crates/config/src/storage.rs',
            'script/build_hybrid_windows.py')
    if ($testFile -and $Value -match '^(?i:(?:Bearer )?(?:synthetic|fake|dummy|test|example|fixture|not)(?:[-_ ][a-z0-9]+)*)$') { return $true }
    return $dummyHashes.ContainsKey($Path) -and
        (Get-Sha256 ([Text.Encoding]::UTF8.GetBytes($Value))) -in $dummyHashes[$Path]
}

function Assert-SafeContent([string]$Path, [byte[]]$Bytes) {
    if ($artworkHashes.ContainsKey($Path)) {
        if ($Bytes.Length -gt 256KB -or (Get-Sha256 $Bytes) -ne $artworkHashes[$Path]) {
            Stop-Review "品牌圖示已變動，須先人工審查再更新允許雜湊：$Path"
        }
        return
    }
    if ($Path -match '^crates/core/tests/fixtures/compression/(?<fixture>[^/]+)$' -and
        $fixtureHashes.ContainsKey($Matches.fixture)) {
        if ($Bytes.Length -gt 65536 -or (Get-Sha256 $Bytes) -ne $fixtureHashes[$Matches.fixture]) {
            Stop-Review "二進位 Fixture 已變動，須先人工審查再更新允許雜湊：$Path"
        }
        return
    }
    try {
        if ($Bytes.Length -ge 2 -and $Bytes[0] -eq 255 -and $Bytes[1] -eq 254) {
            $text = [Text.UnicodeEncoding]::new($false, $true, $true).GetString($Bytes, 2, $Bytes.Length - 2)
        } elseif ($Bytes.Length -ge 2 -and $Bytes[0] -eq 254 -and $Bytes[1] -eq 255) {
            $text = [Text.UnicodeEncoding]::new($true, $true, $true).GetString($Bytes, 2, $Bytes.Length - 2)
        } else { $text = [Text.UTF8Encoding]::new($false, $true).GetString($Bytes) }
    } catch { Stop-Review "無法確認為 UTF-8／UTF-16 文字，停止以免收錄二進位內容：$Path" }
    if ($text -match '[\x00-\x08\x0B\x0E-\x1F\x7F]') { Stop-Review "文字檔含二進位控制字元：$Path" }
    # Normalize common source-string escapes so embedded config/JSON is scanned too.
    $scan = $text.Replace('\"', '"').Replace("\'", "'").Replace('\n', "`n").Replace('\r', "`r")
    $suspect = $false
    foreach ($match in [regex]::Matches($scan, $strongMarkers)) {
        if (-not (Test-Placeholder $match.Value $Path)) { $suspect = $true }
    }
    $matchesToCheck = @([regex]::Matches($scan, $assignment))
    if ([IO.Path]::GetExtension($Path) -in @('.yaml', '.yml', '.ini', '.cfg', '.toml', '.cmd', '.bat', '.sh', '.bash', '.command') -or
        [IO.Path]::GetFileName($Path) -match '^\.env\.(?:example|sample|template)$') {
        $matchesToCheck += @([regex]::Matches($scan, $bareAssignment))
    }
    foreach ($match in $matchesToCheck) {
        $value = $match.Groups['value'].Value.Trim()
        if ($value -in @('true', 'false', 'null', 'None')) { continue }
        if (-not (Test-Placeholder $value $Path)) { $suspect = $true }
    }
    foreach ($match in [regex]::Matches($scan, '(?i)(?:["''](?<auth>(?:Bearer|Basic) [a-z0-9+/_.=-]{8,})["'']|\bAuthorization\s*:\s*(?<auth>(?:Bearer|Basic) [a-z0-9+/_.=-]{8,}))')) {
        if (-not (Test-Placeholder $match.Groups['auth'].Value $Path)) { $suspect = $true }
    }
    if ($suspect) {
        $script:Sensitive = $true
        Stop-Review "發現疑似憑證，已停止；請人工檢查此檔（不顯示內容）：$Path" 3
    }
}

function Test-ExcludedFile([IO.FileInfo]$File) {
    if (($File.Name -match '^(?i:\.env(?:\..*)?|credentials(?:\..*)?|secrets?(?:\.(?:json|toml|ya?ml|txt))?|id_rsa|id_ed25519|\.netrc|\.npmrc|\.pypirc|.*\.(?:pem|key|p12|pfx)|.*\.(?:local|private)\..*|config\.v\d+\.toml)$') -and
        $File.Name -notmatch '^\.env\.(?:example|sample|template)$') {
        $script:PrivateFiles++
        return $true
    }
    return $File.Extension.ToLowerInvariant() -in $binaryExtensions -or
        $File.Name -match '(?i)\.(?:log|tmp|bak|orig|swp)$|^\.DS_Store$|^Thumbs\.db$'
}

try {
    $root = [IO.Path]::GetFullPath($env:SOURCE_REVIEW_ROOT).TrimEnd([char[]]'\/')
    $parent = [IO.Directory]::GetParent($root).FullName
    $destination = Join-Path $parent 'BabelOxide-source-review.zip'
    Write-Host 'BabelOxide 原始碼審查 ZIP 打包工具'
    Write-Host "專案：$root"
    if ((Get-Item -LiteralPath $root -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
        Stop-Review '專案根目錄是連結／接合點，請從實體目錄執行。'
    }
    foreach ($path in $required) {
        if (-not [IO.File]::Exists((Join-Path $root $path))) { Stop-Review "缺少必要檔案：$path" }
    }
    foreach ($name in @('crates', 'python-worker', 'script', 'tests', 'docs', '.cargo', 'resources')) {
        if (-not [IO.Directory]::Exists((Join-Path $root $name))) { Stop-Review "缺少必要目錄：$name" }
    }
    if (Test-Path -LiteralPath $destination) {
        $old = Get-Item -LiteralPath $destination -Force
        if ($old.PSIsContainer -or ($old.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            Stop-Review 'ZIP 目的地是目錄或連結，拒絕取代。'
        }
    }

    $stage = '允許清單與敏感資訊檢查'
    Write-Host '[1/4] 盤點允許清單、讀取快照並檢查敏感資訊...'
    $pending = New-Object 'System.Collections.Generic.Stack[System.IO.FileSystemInfo]'
    foreach ($item in Get-ChildItem -LiteralPath $root -Force) {
        if ($item.PSIsContainer) {
            if ($item.Name -in $sourceRoots) { $pending.Push($item) }
            elseif ($item.Name -match $excludedDirectories -or $item.Name -in $excludedRootDirectories) { $script:Excluded++ }
            else { Stop-Review "發現未分類的根目錄，請先維護允許清單：$($item.Name)" }
        } elseif ($item.Name -in $rootFiles) { $pending.Push($item) }
        elseif (Test-ExcludedFile $item) { $script:Excluded++ }
        else { Stop-Review "發現未分類的根目錄檔案，請先維護允許清單：$($item.Name)" }
    }
    $totalBytes = 0L
    while ($pending.Count -gt 0) {
        $item = $pending.Pop()
        $path = $item.FullName.Substring($root.Length + 1).Replace('\', '/')
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { Stop-Review "允許範圍內含連結／接合點，拒絕跟隨：$path" }
        if ($item.PSIsContainer) {
            foreach ($child in Get-ChildItem -LiteralPath $item.FullName -Force) {
                if ($child.PSIsContainer -and $child.Name -match $excludedDirectories) { $script:Excluded++ }
                else { $pending.Push($child) }
            }
            continue
        }
        $pinnedFixture = $path -match '^crates/core/tests/fixtures/compression/([^/]+)$' -and $fixtureHashes.ContainsKey($item.Name)
        $pinnedArtwork = $artworkHashes.ContainsKey($path)
        if (-not $pinnedFixture -and -not $pinnedArtwork -and (Test-ExcludedFile $item)) { $script:Excluded++; continue }
        if ($path.StartsWith('resources/') -and $path -notin $resourceFiles -and
            $path -notmatch '^resources/(?:licenses|manifests)/') {
            Stop-Review "resources 內出現未審查的文字資源，請先維護允許清單：$path"
        }
        $knownName = $item.Name -match '^(?i:LICENSE(?:[-.].*)?|COPYING(?:\..*)?|NOTICE(?:\..*)?|Makefile|justfile|rust-toolchain|\.gitignore|\.gitattributes|\.gitmodules|\.editorconfig|\.gitkeep|\.env\.(?:example|sample|template))$'
        if (-not $pinnedFixture -and -not $pinnedArtwork -and -not $knownName -and $item.Extension.ToLowerInvariant() -notin $textExtensions) {
            Stop-Review "未允許的檔案類型，請先檢查是否為必要原始碼：$path"
        }
        if ($item.Length -gt 5MB) { Stop-Review "檔案超過 5 MiB，須先人工確認：$path" }
        # FileShare.Read prevents a writer from changing bytes during this read.
        $stream = [IO.File]::Open($item.FullName, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
        try {
            $memory = New-Object IO.MemoryStream
            try { $stream.CopyTo($memory); $bytes = $memory.ToArray() } finally { $memory.Dispose() }
        } finally { $stream.Dispose() }
        $totalBytes += $bytes.LongLength
        if ($bytes.LongLength -gt 5MB -or $totalBytes -gt 64MB) { Stop-Review '快照超過大小上限（單檔 5 MiB／總計 64 MiB）。' }
        Assert-SafeContent $path $bytes
        $records.Add([PSCustomObject]@{ Path = $path; Bytes = $bytes; Hash = (Get-Sha256 $bytes) })
    }
    foreach ($path in $required) {
        if ($path -notin $records.Path) { Stop-Review "必要檔案被排除，拒絕建立不完整 ZIP：$path" }
    }
    if (-not [IO.Directory]::Exists((Join-Path $root '.github'))) { Write-Host '提示：目前沒有 .github/；日後新增時會自動納入。' }

    $stage = '建立暫存 ZIP'
    Write-Host '[2/4] 所有候選檔案檢查通過，建立專案外的暫存 ZIP...'
    Add-Type -AssemblyName System.IO.Compression
    $temporary = Join-Path $parent ('.BabelOxide-source-review.' + [Guid]::NewGuid().ToString('N') + '.tmp')
    $output = [IO.File]::Open($temporary, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    $ownsTemporary = $true
    try {
        $archive = [IO.Compression.ZipArchive]::new($output, [IO.Compression.ZipArchiveMode]::Create, $true, [Text.Encoding]::UTF8)
        try {
            foreach ($record in ($records | Sort-Object Path)) {
                $entry = $archive.CreateEntry($record.Path, [IO.Compression.CompressionLevel]::Optimal)
                $entryStream = $entry.Open()
                try { $entryStream.Write($record.Bytes, 0, $record.Bytes.Length) } finally { $entryStream.Dispose() }
            }
        } finally { $archive.Dispose() }
        $output.Flush($true)
    } finally { $output.Dispose() }

    $stage = 'ZIP 完整性與敏感資訊複查'
    Write-Host '[3/4] 重新開啟 ZIP，逐檔解壓、比對 SHA256 並複查敏感資訊...'
    $expected = @{}
    foreach ($record in $records) { $expected.Add($record.Path, $record) }
    $inputStream = [IO.File]::OpenRead($temporary)
    try {
        $archive = [IO.Compression.ZipArchive]::new($inputStream, [IO.Compression.ZipArchiveMode]::Read)
        try {
            if ($archive.Entries.Count -ne $records.Count) { Stop-Review 'ZIP 檔案數量不符。' 4 }
            foreach ($entry in $archive.Entries) {
                if (-not $expected.ContainsKey($entry.FullName)) { Stop-Review 'ZIP 有非預期或重複項目。' 4 }
                $record = $expected[$entry.FullName]
                if ($entry.Length -ne $record.Bytes.LongLength) { Stop-Review 'ZIP 內檔案長度不符。' 4 }
                $entryStream = $entry.Open()
                $memory = New-Object IO.MemoryStream
                try { $entryStream.CopyTo($memory); $bytes = $memory.ToArray() }
                finally { $memory.Dispose(); $entryStream.Dispose() }
                if ((Get-Sha256 $bytes) -ne $record.Hash) { Stop-Review 'ZIP 內檔案 SHA256 不符。' 4 }
                Assert-SafeContent $entry.FullName $bytes
                $expected.Remove($entry.FullName)
            }
        } finally { $archive.Dispose() }
    } finally { $inputStream.Dispose() }

    $stage = '原子發布 ZIP'
    Write-Host '[4/4] 檢查全部通過，取代上一版 ZIP...'
    # No delete-then-move fallback: a failed replacement must retain the old ZIP.
    if ([IO.File]::Exists($destination)) { [IO.File]::Replace($temporary, $destination, [Management.Automation.Language.NullString]::Value) }
    else { [IO.File]::Move($temporary, $destination) }
    $published = $true
    $script:ExitCode = 0
    Write-Host ''
    Write-Host '打包成功。'
    Write-Host "ZIP 完整路徑：$destination"
    Write-Host ('ZIP 大小：{0:N0} bytes（{1:N2} MiB）' -f ([IO.FileInfo]$destination).Length, (([IO.FileInfo]$destination).Length / 1MB))
    Write-Host "包含的檔案數量：$($records.Count)"
} catch {
    Write-Host ''
    Write-Host '打包失敗；未發布新 ZIP，既有 ZIP 保持不變。'
    if ($script:Failure) { Write-Host "原因：$script:Failure" }
    else { Write-Host ("原因：$stage 失敗（" + $_.Exception.GetBaseException().GetType().Name + '）。請確認檔案權限、磁碟空間或檔案是否被占用。') }
    if (Get-Variable destination -ErrorAction SilentlyContinue) { Write-Host "ZIP 目標路徑：$destination" }
} finally {
    if ($ownsTemporary -and -not $published) {
        try { [IO.File]::Delete($temporary) } catch { Write-Host '警告：專案外的暫存 ZIP 無法清理，請關閉占用程式後刪除。' }
    }
}
Write-Host '主要排除：target/、.venv/、dist/、.git/、.test-output/、快取、使用者 config/、build-inputs/、個人 PDF、翻譯輸出、模型、執行檔及編譯產物。'
Write-Host "已略過的目錄／檔案項目：$script:Excluded（不遞迴計算已排除目錄）。"
if ($script:Sensitive) { Write-Host '敏感資訊疑慮：有；已停止打包，未輸出任何憑證內容。' }
elseif ($script:PrivateFiles -gt 0) { Write-Host "敏感資訊疑慮：已依私密檔名排除 $script:PrivateFiles 個檔案；其餘候選檔案未發現疑似憑證。" }
elseif ($published) { Write-Host '敏感資訊疑慮：候選檔案與 ZIP 複查均未發現疑似憑證。' }
else { Write-Host '敏感資訊檢查未全部完成；未發布新 ZIP。' }
exit $script:ExitCode
