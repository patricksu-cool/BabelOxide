[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$RuntimeDirectory,
    [Parameter(Mandatory = $true)][string]$LauncherPath,
    [string]$OutputDirectory = 'build',
    [string]$EnvironmentDirectory = 'dep_build/venv',
    [string]$Python = '3.13',
    [switch]$SkipOfflineAssets,
    [switch]$DirectML
)

$ErrorActionPreference = 'Stop'
$repoRoot = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$repoPrefix = $repoRoot.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar

function Resolve-ProjectPath([string]$Candidate) {
    $resolved = [IO.Path]::GetFullPath((Join-Path $repoRoot $Candidate))
    if (-not $resolved.StartsWith($repoPrefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Build destinations must be inside the project: $resolved"
    }
    return $resolved
}

$outputPath = Resolve-ProjectPath $OutputDirectory
$environmentPath = Resolve-ProjectPath $EnvironmentDirectory
$runtimePath = (Resolve-Path -LiteralPath $RuntimeDirectory).Path
$launcherSource = (Resolve-Path -LiteralPath $LauncherPath).Path
if (-not (Test-Path -LiteralPath (Join-Path $runtimePath 'python.exe') -PathType Leaf)) {
    throw 'RuntimeDirectory must contain an extracted embedded Python runtime'
}
if (-not (Test-Path -LiteralPath $launcherSource -PathType Leaf)) {
    throw 'LauncherPath must be a PyStand CLI executable'
}
if (Test-Path -LiteralPath $outputPath) {
    throw "Output already exists; choose a new directory to preserve its files: $outputPath"
}
Get-Command uv -CommandType Application -ErrorAction Stop | Out-Null

$previousEnvironment = $env:UV_PROJECT_ENVIRONMENT
Push-Location -LiteralPath $repoRoot
try {
    $env:UV_PROJECT_ENVIRONMENT = $environmentPath
    $syncArguments = @('sync', '--locked', '--no-dev', '--no-editable', '--reinstall-package', 'pdf2zh-next', '--python', $Python, '--no-python-downloads')
    if ($DirectML) { $syncArguments += @('--extra', 'directml') }
    & uv @syncArguments
    if ($LASTEXITCODE -ne 0) { throw "uv sync failed: $LASTEXITCODE" }
    $buildPython = Join-Path $environmentPath 'Scripts/python.exe'
    if ($DirectML) {
        # BabelDOC requires the CPU distribution as well as its DirectML extra.
        # Their wheels share onnxruntime/; install the selected locked DirectML
        # wheel last, as the former release pipeline did, without upgrading deps.
        $constraintsPath = Join-Path $environmentPath 'locked-runtime-constraints.txt'
        & uv export --locked --no-dev --extra directml --no-emit-project --no-hashes --format requirements-txt --output-file $constraintsPath | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'Could not export locked DirectML constraints' }
        $directMLVersion = & $buildPython -c 'import importlib.metadata; print(importlib.metadata.version("onnxruntime-directml"))'
        if ($LASTEXITCODE -ne 0) { throw 'Locked DirectML distribution is missing' }
        & uv pip install --python $buildPython --constraint $constraintsPath --reinstall-package onnxruntime-directml "onnxruntime-directml==$directMLVersion"
        if ($LASTEXITCODE -ne 0) { throw 'Could not stage the locked DirectML wheel' }
        & $buildPython -c 'import onnxruntime; assert "DmlExecutionProvider" in onnxruntime.get_available_providers(), onnxruntime.get_available_providers()'
        if ($LASTEXITCODE -ne 0) { throw 'The DirectML release profile has no DirectML provider' }
    }
    & uv pip check --python $buildPython
    if ($LASTEXITCODE -ne 0) { throw "Dependency check failed: $LASTEXITCODE" }

    New-Item -ItemType Directory -Path $outputPath | Out-Null
    Copy-Item -LiteralPath $runtimePath -Destination (Join-Path $outputPath 'runtime') -Recurse
    Copy-Item -LiteralPath $launcherSource -Destination (Join-Path $outputPath 'pdf2zh.exe')
    & $buildPython (Join-Path $PSScriptRoot 'package_windows.py') --source (Join-Path $environmentPath 'Lib/site-packages') --bundle $outputPath
    if ($LASTEXITCODE -ne 0) { throw "Runtime package staging failed: $LASTEXITCODE" }
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot '_pystand_static.int') -Destination $outputPath
    Copy-Item -LiteralPath (Join-Path $repoRoot 'LICENSE') -Destination $outputPath
    Copy-Item -LiteralPath (Join-Path $repoRoot 'uv.lock') -Destination $outputPath
    Copy-Item -Path (Join-Path $PSScriptRoot 'windows-release/*') -Destination $outputPath

    $requiredResources = @(
        'ui/resources/gui.css', 'ui/resources/translation.yaml',
        'assets/powered_by_siliconflow_light.png', 'ui/preview.py',
        'ui/components.py', 'ui/inputs.py', 'ui/tasks.py', 'ui/results.py',
        'config/cli.py', 'translation/batch.py'
    )
    foreach ($relativePath in $requiredResources) {
        $resource = Join-Path $outputPath "site-packages/pdf2zh_next/$relativePath"
        if (-not (Test-Path -LiteralPath $resource -PathType Leaf)) {
            throw "The installed wheel is missing a required resource: $relativePath"
        }
    }

    if (-not $SkipOfflineAssets) {
        & (Join-Path $environmentPath 'Scripts/babeldoc.exe') --generate-offline-assets $outputPath
        if ($LASTEXITCODE -ne 0) { throw "Offline asset generation failed: $LASTEXITCODE" }
    }
    $runtimeVersion = & (Join-Path $runtimePath 'python.exe') -I -S -c 'import platform; print(platform.python_version())'
    if ($LASTEXITCODE -ne 0) { throw 'Could not inspect embedded Python runtime' }
    $sourceCommit = & git rev-parse HEAD
    if ($LASTEXITCODE -ne 0) { throw 'Could not record source commit' }
    @{
        lock_sha256 = (Get-FileHash -LiteralPath (Join-Path $repoRoot 'uv.lock') -Algorithm SHA256).Hash
        python = $Python
        runtime_python = $runtimeVersion
        source_commit = $sourceCommit
        launcher_sha256 = (Get-FileHash -LiteralPath $launcherSource -Algorithm SHA256).Hash
        editable = $false
        directml = [bool]$DirectML
        offline_assets_requested = -not [bool]$SkipOfflineAssets
    } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $outputPath 'BUILD_INFO.json') -Encoding UTF8
    & $buildPython (Join-Path $PSScriptRoot 'package_windows.py') --bundle $outputPath --finalize
    if ($LASTEXITCODE -ne 0) { throw "Release manifest generation failed: $LASTEXITCODE" }
    Write-Host "Built Windows bundle: $outputPath"
}
finally {
    $env:UV_PROJECT_ENVIRONMENT = $previousEnvironment
    Pop-Location
}
