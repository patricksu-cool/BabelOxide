#Requires -Version 5.1
param([string]$ProjectRoot = (Join-Path $PSScriptRoot '..\..'))

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$ProjectRoot = [IO.Path]::GetFullPath($ProjectRoot)
$sandbox = Join-Path ([IO.Path]::GetTempPath()) ('source-review-test-' + [Guid]::NewGuid().ToString('N'))
$root = Join-Path $sandbox "中文 空格 O'Brien ! & (專案)"
$zip = Join-Path $sandbox 'PDFMathTranslate-next-source-review.zip'
$utf8 = New-Object Text.UTF8Encoding($false)
$passed = 0

function Assert-True([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

function Write-Fixture([string]$Path, [string]$Text) {
    $full = Join-Path $root $Path
    [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($full))
    [IO.File]::WriteAllText($full, $Text, $utf8)
}

function Invoke-Packager {
    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = $env:ComSpec
    $info.Arguments = '/d /v:off /s /c ""' + (Join-Path $root '打包原始碼.cmd') + '" --no-pause"'
    $info.WorkingDirectory = $sandbox
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.StandardOutputEncoding = [Text.Encoding]::UTF8
    $info.StandardErrorEncoding = [Text.Encoding]::UTF8
    $process = [Diagnostics.Process]::Start($info)
    try {
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(60000)) { $process.Kill(); throw 'Packager did not finish within 60 seconds.' }
        return [PSCustomObject]@{ Code = $process.ExitCode; Output = $stdout.Result + $stderr.Result }
    } finally { $process.Dispose() }
}

function Read-Entries {
    $archive = [IO.Compression.ZipFile]::OpenRead($zip)
    try { return @($archive.Entries | ForEach-Object { $_.FullName }) }
    finally { $archive.Dispose() }
}

function Get-TreeSnapshot {
    $snapshot = @{}
    foreach ($file in Get-ChildItem -LiteralPath $root -Recurse -Force -File) {
        $snapshot[$file.FullName] = '{0}:{1}:{2}' -f $file.Length, $file.LastWriteTimeUtc.Ticks,
            (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash
    }
    return $snapshot
}

function Assert-NoTemporary {
    Assert-True (@(Get-ChildItem -LiteralPath $sandbox -Filter '.PDFMathTranslate-next-source-review.*.tmp' -Force).Count -eq 0) 'A temporary archive was left behind.'
}

function Confirm-Case([string]$Name) {
    $script:passed++
    Write-Host "PASS $Name"
}

function Test-Rejection([string]$Name, [string]$Path, [string]$Text, [int]$Code, [string]$MustNotLog = '') {
    $before = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash
    Write-Fixture $Path $Text
    try {
        $result = Invoke-Packager
        Assert-True ($result.Code -eq $Code) "$Name returned an unexpected exit code ($($result.Code))."
        Assert-True ((Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash -eq $before) "$Name replaced the old ZIP."
        if ($MustNotLog) { Assert-True (-not $result.Output.Contains($MustNotLog)) "$Name exposed a credential in output." }
        Assert-NoTemporary
        Confirm-Case $Name
    } finally { [IO.File]::Delete((Join-Path $root $Path)) }
}

try {
    [void][IO.Directory]::CreateDirectory($root)
    [IO.File]::Copy((Join-Path $ProjectRoot '打包原始碼.cmd'), (Join-Path $root '打包原始碼.cmd'))
    $included = @{
        'Cargo.toml' = '[workspace]'
        'Cargo.lock' = '# fixture lock'
        'pyproject.toml' = '[project]'
        'uv.lock' = 'version = 1'
        'README.md' = 'fixture source review'
        '.gitignore' = '/target/'
        'LICENSE' = '2. Basic Permissions.'
        '.cargo/config.toml' = '[build]'
        'crates/config/src/lib.rs' = 'pub fn config_source() {}'
        'crates/core/src/lib.rs' = 'pub fn core_source() {}'
        'python-worker/worker.py' = 'raise RuntimeError("worker must never be executed")'
        'script/never_run.py' = 'raise RuntimeError("build scripts must never be executed")'
        'tests/fixtures/small.txt' = 'small fixture'
        'docs/ARCHITECTURE.md' = '# architecture'
        'resources/config.example.toml' = '[translation]'
        'resources/engine-assets.lock.json' = '{}'
        'resources/licenses/LICENSE-MIT' = 'fixture license'
        '.github/workflows/check.yml' = 'token: ${{ secrets.GITHUB_TOKEN }}'
        '.env.example' = 'API_KEY=${API_KEY}'
    }
    foreach ($path in $included.Keys) { Write-Fixture $path $included[$path] }
    $opaque = [Guid]::NewGuid().ToString('N') + [Guid]::NewGuid().ToString('N')
    $excluded = @('target/leak.rs', '.venv/pyvenv.cfg', 'dist/app.exe', '.git/HEAD',
        '.test-output/private.pdf', 'config/private.toml', 'cache/private.db',
        '.pytest_cache/state.json', 'python-worker/__pycache__/worker.pyc',
        'resources/model.onnx', 'tests/private.pdf', 'script/.env', 'docs/credentials.json')
    foreach ($path in $excluded) { Write-Fixture $path $opaque }

    $before = Get-TreeSnapshot
    $result = Invoke-Packager
    Assert-True ($result.Code -eq 0) "Initial CMD run failed: $($result.Output)"
    $entries = Read-Entries
    Assert-True ($entries.Count -eq $included.Count + 1) 'Archive entry count does not match the allowlist.'
    foreach ($path in $included.Keys) { Assert-True ($path -in $entries) "Missing source: $path" }
    foreach ($path in $excluded) { Assert-True ($path -notin $entries) "Excluded file was archived: $path" }
    Assert-True (-not $result.Output.Contains($opaque)) 'Excluded content leaked into output.'
    $after = Get-TreeSnapshot
    Assert-True ($before.Count -eq $after.Count) 'Packager created/deleted a project file.'
    foreach ($path in $before.Keys) { Assert-True ($before[$path] -eq $after[$path]) 'Packager changed a project file or its timestamp.' }
    $extracted = Join-Path $sandbox '解壓縮驗證'
    [IO.Compression.ZipFile]::ExtractToDirectory($zip, $extracted)
    foreach ($path in $entries) {
        Assert-True ((Get-FileHash -LiteralPath (Join-Path $root $path)).Hash -eq
            (Get-FileHash -LiteralPath (Join-Path $extracted $path)).Hash) "Extracted bytes differ: $path"
    }
    Assert-NoTemporary
    Confirm-Case 'CMD: Chinese, spaces, apostrophe, exclamation, ampersand, parentheses; extraction; exclusions; no project writes'

    $oldHash = (Get-FileHash -LiteralPath $zip).Hash
    Write-Fixture 'README.md' 'updated source'
    Write-Fixture 'crates/core/src/new.rs' 'pub fn new_source() {}'
    [IO.File]::Delete((Join-Path $root 'tests/fixtures/small.txt'))
    $result = Invoke-Packager
    Assert-True ($result.Code -eq 0) "Repeat CMD run failed: $($result.Output)"
    $entries = Read-Entries
    Assert-True ('crates/core/src/new.rs' -in $entries -and 'tests/fixtures/small.txt' -notin $entries) 'ZIP did not refresh additions/deletions.'
    Assert-True ((Get-FileHash -LiteralPath $zip).Hash -ne $oldHash) 'ZIP did not update.'
    Confirm-Case 'repeat run replaces ZIP and removes stale entries'

    Test-Rejection 'opaque assigned credential' 'docs/reject.md' ('api' + '_key = "' + $opaque + '"') 3 $opaque
    $prefixValue = 'sk-' + $opaque
    Test-Rejection 'token prefix in prose' 'docs/reject.md' ('prose: ' + $prefixValue) 3 $prefixValue
    Test-Rejection 'unquoted YAML credential' 'docs/reject.yml' ('access' + '_token: ' + $opaque) 3 $opaque
    Test-Rejection 'dotenv template credential' '.env.example' ('API' + '_KEY=' + $opaque) 3 $opaque
    Write-Fixture '.env.example' 'API_KEY=${API_KEY}'
    Test-Rejection 'CMD set credential' 'script/reject.cmd' ('set "API' + '_KEY=' + $opaque + '"') 3 $opaque
    $encoded = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes('qa:' + $opaque))
    Test-Rejection 'HTTP authorization header' 'docs/reject.md' ('Authorization: Basic ' + $encoded) 3 $encoded
    $header = '-----BEGIN ' + 'PRIVATE KEY-----'
    Test-Rejection 'private key block' 'docs/reject.md' $header 3 $header
    Test-Rejection 'unknown root source' 'new-tool.py' '# new source requires review' 2
    Test-Rejection 'unknown text resource' 'resources/unreviewed.txt' 'review resource policy' 2
    Test-Rejection 'unknown source extension' 'tests/unreviewed.newtype' 'new source format' 2

    $before = (Get-FileHash -LiteralPath $zip).Hash
    $badText = Join-Path $root 'tests/binary.txt'
    [IO.File]::WriteAllBytes($badText, [byte[]]@(0, 255, 254, 1))
    try {
        Assert-True ((Invoke-Packager).Code -eq 2) 'Disguised binary text was accepted.'
        Assert-True ((Get-FileHash -LiteralPath $zip).Hash -eq $before) 'Binary rejection changed the ZIP.'
        Confirm-Case 'binary disguised as text'
    } finally { [IO.File]::Delete($badText) }

    $fixturePath = 'crates/core/tests/fixtures/compression/html.gzip'
    $fullFixture = Join-Path $root $fixturePath
    [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($fullFixture))
    [IO.File]::Copy((Join-Path $ProjectRoot $fixturePath), $fullFixture)
    Assert-True ((Invoke-Packager).Code -eq 0) 'Reviewed compressed fixture was excluded.'
    Assert-True ($fixturePath -in (Read-Entries)) 'Reviewed compressed fixture is missing.'
    Confirm-Case 'reviewed binary fixture is preserved'
    $before = (Get-FileHash -LiteralPath $zip).Hash
    [IO.File]::WriteAllBytes($fullFixture, [byte[]]@(1, 2, 3))
    try {
        Assert-True ((Invoke-Packager).Code -eq 2) 'Modified compressed fixture was accepted.'
        Assert-True ((Get-FileHash -LiteralPath $zip).Hash -eq $before) 'Fixture rejection changed the ZIP.'
        Confirm-Case 'modified binary fixture requires review'
    } finally { [IO.File]::Delete($fullFixture) }

    $before = (Get-FileHash -LiteralPath $zip).Hash
    $handle = [IO.File]::Open($zip, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    try {
        Assert-True ((Invoke-Packager).Code -ne 0) 'Locked destination was reported as successful.'
        Assert-True ((Get-FileHash -LiteralPath $zip).Hash -eq $before) 'Failed replacement damaged the old ZIP.'
        Assert-NoTemporary
        Confirm-Case 'failed atomic replacement retains old ZIP and cleans temporary archive'
    } finally { $handle.Dispose() }

    $junction = Join-Path $root 'docs/linked'
    [void](New-Item -ItemType Junction -Path $junction -Target $extracted)
    try {
        Assert-True ((Invoke-Packager).Code -eq 2) 'Junction inside the source tree was followed.'
        Assert-True ((Get-FileHash -LiteralPath $zip).Hash -eq $before) 'Junction rejection changed the ZIP.'
        Confirm-Case 'junction is rejected without traversing its target'
    } finally { [IO.Directory]::Delete($junction) }

    $large = Join-Path $root 'tests/large.txt'
    [IO.File]::WriteAllBytes($large, (New-Object byte[] (5MB + 1)))
    try {
        Assert-True ((Invoke-Packager).Code -eq 2) 'Oversized source was accepted.'
        Assert-True ((Get-FileHash -LiteralPath $zip).Hash -eq $before) 'Oversize rejection changed the ZIP.'
        Confirm-Case 'oversized file requires review'
    } finally { [IO.File]::Delete($large) }
    Assert-NoTemporary
    Write-Host "$passed integration checks passed. Original project files were never used as test fixtures."
} finally {
    # Delete only the unique temporary directory created above, never the project.
    $resolved = [IO.Path]::GetFullPath($sandbox)
    $tempPrefix = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
    if (-not $resolved.StartsWith($tempPrefix, [StringComparison]::OrdinalIgnoreCase) -or
        [IO.Path]::GetFileName($resolved) -notmatch '^source-review-test-[a-f0-9]{32}$') {
        throw 'Refusing cleanup outside the expected temporary test directory.'
    }
    if ([IO.Directory]::Exists($resolved)) { Remove-Item -LiteralPath $resolved -Recurse -Force }
}
