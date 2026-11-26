# Install ok on Windows (PowerShell):
#
#   irm https://okrun.dev/install.ps1 | iex
#
# Downloads the release binary for this machine from GitHub, checks it
# against the release's SHA256SUMS, installs it as ok.exe in
# %LOCALAPPDATA%\ok\bin (or $env:OK_INSTALL_DIR) and adds that folder to the
# user PATH. Set $env:OK_VERSION = '0.1.0' to pin a version; the default is
# the latest release.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$Repo = 'https://github.com/xtr-dev/ok/releases'
$Base = if ($env:OK_VERSION) { "$Repo/download/v$($env:OK_VERSION.TrimStart('v'))" } else { "$Repo/latest/download" }
$Dir = if ($env:OK_INSTALL_DIR) { $env:OK_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'ok\bin' }

$Arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
switch ($Arch) {
    'X64'   { $Target = 'x86_64-pc-windows-msvc' }
    'Arm64' { $Target = 'aarch64-pc-windows-msvc' }
    default { throw "ok install: no prebuilt ok for Windows $Arch" }
}
$File = "ok-$Target.exe"

$Tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("ok-install-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $Tmp | Out-Null
try {
    Write-Host "downloading ok for $Target"
    Invoke-WebRequest -UseBasicParsing -Uri "$Base/$File" -OutFile (Join-Path $Tmp 'ok.exe')
    Invoke-WebRequest -UseBasicParsing -Uri "$Base/SHA256SUMS" -OutFile (Join-Path $Tmp 'SHA256SUMS')

    $Expected = $null
    foreach ($Line in Get-Content (Join-Path $Tmp 'SHA256SUMS')) {
        $Parts = $Line -split '\s+'
        if ($Parts.Count -ge 2 -and $Parts[1].TrimStart('*') -eq $File) { $Expected = $Parts[0].ToLower() }
    }
    if (-not $Expected) { throw "ok install: SHA256SUMS has no entry for $File" }
    $Actual = (Get-FileHash -Algorithm SHA256 (Join-Path $Tmp 'ok.exe')).Hash.ToLower()
    if ($Actual -ne $Expected) { throw "ok install: checksum mismatch (expected $Expected, got $Actual); not installing" }

    New-Item -ItemType Directory -Force -Path $Dir | Out-Null
    Move-Item -Force (Join-Path $Tmp 'ok.exe') (Join-Path $Dir 'ok.exe')
} finally {
    Remove-Item -Recurse -Force $Tmp -ErrorAction SilentlyContinue
}

$Version = & (Join-Path $Dir 'ok.exe') --version
Write-Host "installed $Version to $Dir\ok.exe"

$UserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (-not (($UserPath -split ';') -contains $Dir)) {
    $NewPath = if ($UserPath) { "$UserPath;$Dir" } else { $Dir }
    [Environment]::SetEnvironmentVariable('Path', $NewPath, 'User')
    $env:Path = "$env:Path;$Dir"
    Write-Host "added $Dir to your user PATH (open a new terminal to use it)"
}
