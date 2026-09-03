$ErrorActionPreference = "Stop"

$Repo = "francis-du/wcode"
$Version = if ($env:WCODE_VERSION) { $env:WCODE_VERSION } else { "latest" }
if ($Version -eq "latest") {
    $BaseUrl = "https://github.com/$Repo/releases/latest/download"
}
elseif ($Version -match '^[A-Za-z0-9._-]+$') {
    $BaseUrl = "https://github.com/$Repo/releases/download/$Version"
}
else {
    throw "wcode install: invalid WCODE_VERSION: $Version"
}
$InstallDir = if ($env:WCODE_INSTALL_DIR) { $env:WCODE_INSTALL_DIR } else { Join-Path $env:USERPROFILE ".local\bin" }
$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("wcode-install-" + [System.Guid]::NewGuid().ToString("N"))
$Archive = "wcode-windows-x86_64.zip"
$InstallTemp = $null

$Architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
if ($Architecture -ne [System.Runtime.InteropServices.Architecture]::X64) {
    throw "wcode install: unsupported Windows architecture: $Architecture (x86_64 is currently required)"
}

New-Item -ItemType Directory -Force -Path $TempDir, $InstallDir | Out-Null

try {
    $ArchivePath = Join-Path $TempDir $Archive
    $ChecksumsPath = Join-Path $TempDir "SHA256SUMS"

    Write-Host "Downloading $Archive..."
    Invoke-WebRequest -UseBasicParsing -Uri "$BaseUrl/$Archive" -OutFile $ArchivePath

    Write-Host "Downloading checksums..."
    Invoke-WebRequest -UseBasicParsing -Uri "$BaseUrl/SHA256SUMS" -OutFile $ChecksumsPath

    $ChecksumLine = Get-Content $ChecksumsPath | Where-Object {
        $parts = $_ -split '\s+', 2
        $parts.Count -eq 2 -and $parts[1].TrimStart('*') -eq $Archive
    } | Select-Object -First 1

    if (-not $ChecksumLine) {
        throw "wcode install: checksum for $Archive was not found"
    }

    $Expected = ($ChecksumLine -split '\s+')[0].ToLowerInvariant()
    $Actual = (Get-FileHash -Algorithm SHA256 -Path $ArchivePath).Hash.ToLowerInvariant()

    if ($Actual -ne $Expected) {
        throw "wcode install: SHA-256 checksum mismatch"
    }

    $PackageDir = Join-Path $TempDir "package"
    Expand-Archive -Path $ArchivePath -DestinationPath $PackageDir -Force

    $Binary = Get-ChildItem -Path $PackageDir -Filter "wcode.exe" -File -Recurse | Select-Object -First 1
    if (-not $Binary) {
        throw "wcode install: wcode.exe was not found in the release archive"
    }

    $InstallPath = Join-Path $InstallDir "wcode.exe"
    $InstallTemp = Join-Path $InstallDir (".wcode-install-" + [System.Guid]::NewGuid().ToString("N") + ".exe")
    Copy-Item -Force $Binary.FullName $InstallTemp

    & $InstallTemp --version | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "wcode install: downloaded binary failed the version smoke test"
    }
    & $InstallTemp --help | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "wcode install: downloaded binary failed the help smoke test"
    }

    Move-Item -Force $InstallTemp $InstallPath
    $InstallTemp = $null

    Write-Host ""
    Write-Host "Installed wcode to $InstallPath"

    $PathEntries = $env:PATH -split ';'
    Write-Host ""
    if ($PathEntries -contains $InstallDir) {
        Write-Host "Next, from a repository:"
        Write-Host "  wcode setup"
        Write-Host "  wcode"
    }
    else {
        Write-Host "Add this directory to PATH if needed:"
        Write-Host "  $InstallDir"
        Write-Host ""
        Write-Host "Or use the installed binary directly from a repository:"
        Write-Host "  & `"$InstallPath`" setup"
        Write-Host "  & `"$InstallPath`""
    }

    & $InstallPath --version
}
finally {
    if ($InstallTemp) {
        Remove-Item -Force -ErrorAction SilentlyContinue $InstallTemp
    }
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $TempDir
}
