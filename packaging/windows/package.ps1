param([switch]$SkipBuild, [string]$BinDir)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$target = Join-Path $root 'target'
$version = [regex]::Match([IO.File]::ReadAllText((Join-Path $root 'Cargo.toml')), '(?m)^version = "([^"]+)"').Groups[1].Value
if ($version -notmatch '^\d+\.\d+\.\d+$') { throw 'MSI requires a stable numeric package version' }
$tools = Join-Path $target 'installer-tools'
New-Item -ItemType Directory -Path $tools -Force | Out-Null

# Portable WiX runs with the existing .NET 8 runtime. No build tool is installed globally.
function Get-PortablePackage([string]$id, [string]$hash) {
    $archive = Join-Path $tools "$id.5.0.2.nupkg"
    if (-not (Test-Path -LiteralPath $archive)) {
        Invoke-WebRequest -Uri "https://api.nuget.org/v3-flatcontainer/$id/5.0.2/$id.5.0.2.nupkg" -OutFile $archive
    }
    if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ine $hash) { throw "Build tool checksum mismatch: $id" }
    $directory = Join-Path $tools "$id-5.0.2"
    if (-not (Test-Path -LiteralPath $directory)) { [IO.Compression.ZipFile]::ExtractToDirectory($archive, $directory) }
    return $directory
}
$wix = Join-Path (Get-PortablePackage 'wix' 'F30EF0C74E2A986126539C5780BE93AC24E8136EAF723B1937B26272703AE173') 'tools/net6.0/any/wix.dll'
$ui = Join-Path (Get-PortablePackage 'wixtoolset.ui.wixext' '5EF2C707614B9F70B6BBADD2D4ABCB4124EFEE215E9B16BFBC80113079A604C7') 'wixext5/WixToolset.UI.wixext.dll'

if (-not $SkipBuild) {
    $previousFlags = $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS
    try {
        $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = '-C target-feature=+crt-static'
        Push-Location $root
        try {
            cargo build --release --locked --target x86_64-pc-windows-msvc
            if ($LASTEXITCODE -ne 0) { throw 'Rust release build failed' }
        } finally { Pop-Location }
    } finally { $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = $previousFlags }
}
if (-not $BinDir) { $BinDir = Join-Path $target 'x86_64-pc-windows-msvc/release' }
$app = Join-Path $BinDir 'effectcraft-farm.exe'
if (-not (Test-Path -LiteralPath $app)) { throw "Farm binary missing: $app" }
$bytes = [IO.File]::ReadAllBytes($app)
$pe = [BitConverter]::ToInt32($bytes, 0x3c)
if ([BitConverter]::ToUInt16($bytes, $pe + 4) -ne 0x8664 -or [BitConverter]::ToUInt16($bytes, $pe + 0x5c) -ne 2) {
    throw 'The package requires a Windows x64 GUI release binary'
}
$dist = Join-Path $target "release-v$version"
New-Item -ItemType Directory -Path $dist -Force | Out-Null
$license = Join-Path $dist 'license.rtf'
$licenseText = [IO.File]::ReadAllText((Join-Path $root 'LICENSE-MIT')).Replace('\', '\\').Replace('{', '\{').Replace('}', '\}').Replace("`r", '').Replace("`n", '\par ')
[IO.File]::WriteAllText($license, ('{\rtf1\ansi\deff0 {\fonttbl{\f0 Segoe UI;}}\f0\fs18 ' + $licenseText + '}'), [Text.Encoding]::ASCII)
$msi = Join-Path $dist "effectcraft-render-farm-v$version-windows-x64.msi"
dotnet $wix build (Join-Path $PSScriptRoot 'farm.wxs') -ext $ui -arch x64 -d "Version=$version" -d "Root=$root" -d "BinDir=$BinDir" -d "LicensePath=$license" -o $msi
if ($LASTEXITCODE -ne 0) { throw 'MSI build or validation failed' }
$hash = (Get-FileHash -LiteralPath $msi -Algorithm SHA256).Hash.ToLowerInvariant()
[IO.File]::WriteAllText((Join-Path $dist 'SHA256SUMS.txt'), "$hash  $([IO.Path]::GetFileName($msi))`n", [Text.UTF8Encoding]::new($false))
Write-Output "MSI: $msi"
Write-Output "SHA256: $hash"
