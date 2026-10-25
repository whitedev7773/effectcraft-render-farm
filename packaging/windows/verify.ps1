param([Parameter(Mandatory)][string]$MsiPath)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$msi = (Resolve-Path -LiteralPath $MsiPath).Path
$work = Join-Path (Join-Path $root 'target') ('msi-verify-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null
$installer = New-Object -ComObject WindowsInstaller.Installer
$db = $installer.OpenDatabase($msi, 0)
$view = $db.OpenView('SELECT `FileName` FROM `File`')
[void]$view.Execute()
$files = @()
while ($record = $view.Fetch()) { $files += ($record.StringData(1) -split '\|')[-1] }
[void]$view.Close()
if ($files.Count -ne 4 -or $files -contains 'effectcraft-cli.exe' -or $files -notcontains 'effectcraft-farm.exe') { throw 'Unexpected MSI payload' }
[Runtime.InteropServices.Marshal]::FinalReleaseComObject($db) | Out-Null
[Runtime.InteropServices.Marshal]::FinalReleaseComObject($installer) | Out-Null
Write-Output ('MSI payload: ' + ($files -join ', '))

# Administrative extraction unpacks the MSI without installing Farm or creating shortcuts/registry entries.
$extracted = Join-Path $work 'extracted'
New-Item -ItemType Directory -Path $extracted | Out-Null
$process = Start-Process -FilePath (Join-Path $env:SystemRoot 'System32/msiexec.exe') -ArgumentList @('/a', ('"' + $msi + '"'), '/qn', ('TARGETDIR="' + $extracted + '"'), '/l*v', ('"' + (Join-Path $work 'extract.log') + '"')) -WindowStyle Hidden -Wait -PassThru
if ($process.ExitCode -ne 0) { throw "MSI extraction failed: $($process.ExitCode)" }
$apps = @(Get-ChildItem -LiteralPath $extracted -Recurse -Filter 'effectcraft-farm.exe')
if ($apps.Count -ne 1) { throw 'Expected exactly one Farm executable' }
$built = Join-Path $root 'target/x86_64-pc-windows-msvc/release/effectcraft-farm.exe'
if ((Get-FileHash -LiteralPath $apps[0].FullName).Hash -ne (Get-FileHash -LiteralPath $built).Hash) { throw 'MSI payload differs from release build' }
if (@(Get-ChildItem -LiteralPath $extracted -Recurse -Filter 'effectcraft-cli.exe').Count -ne 0) { throw 'Unexpected bundled CLI' }
Write-Output 'MSI extraction and app checksum passed.'

foreach ($scenario in @('valid', 'wrong-product')) {
    $stage = Join-Path $work $scenario
    New-Item -ItemType Directory -Path $stage | Out-Null
    $copy = Join-Path $stage 'setup.msi'
    Copy-Item -LiteralPath $msi -Destination $copy
    if ($scenario -eq 'wrong-product') {
        $installer = New-Object -ComObject WindowsInstaller.Installer
        $db = $installer.OpenDatabase($copy, 1)
        $view = $db.OpenView('UPDATE `Property` SET `Value`=''Another Product'' WHERE `Property`=''ProductName''')
        [void]$view.Execute(); [void]$view.Close(); [void]$db.Commit()
        [Runtime.InteropServices.Marshal]::FinalReleaseComObject($db) | Out-Null
        [Runtime.InteropServices.Marshal]::FinalReleaseComObject($installer) | Out-Null
    }
    $helper = Join-Path $root 'src/updater.ps1'
    $result = Join-Path $stage 'result.txt'
    $app = Join-Path $stage 'effectcraft-farm.exe'
    # The live test parent prevents installation. Always kill the helper before this parent exits.
    $arguments = @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', ('"' + $helper + '"'), '-ParentId', $PID, '-Stage', ('"' + $stage + '"'), '-Executable', ('"' + $app + '"'), '-ResultFile', ('"' + $result + '"'))
    $child = Start-Process -FilePath (Join-Path $env:SystemRoot 'System32/WindowsPowerShell/v1.0/powershell.exe') -ArgumentList $arguments -WindowStyle Hidden -PassThru
    try {
        if ($scenario -eq 'valid') {
            $deadline = [DateTime]::UtcNow.AddSeconds(10)
            while (-not (Test-Path -LiteralPath (Join-Path $stage 'installer-ready')) -and [DateTime]::UtcNow -lt $deadline -and -not $child.HasExited) { Start-Sleep -Milliseconds 50 }
            if (-not (Test-Path -LiteralPath (Join-Path $stage 'installer-ready')) -or $child.HasExited) { throw 'Valid MSI helper failed readiness/parent-wait check' }
            Write-Output 'Valid MSI helper handshake passed without installing.'
        } else {
            if (-not $child.WaitForExit(10000)) { throw 'Wrong-product rejection timed out' }
            if ($child.ExitCode -eq 0 -or (Test-Path -LiteralPath (Join-Path $stage 'installer-ready'))) { throw 'Wrong-product MSI was accepted' }
            Write-Output 'Wrong-product MSI rejected before shutdown.'
        }
    } finally {
        if (-not $child.HasExited) { $child.Kill(); $child.WaitForExit() }
        $child.Dispose()
    }
}
Write-Output "Verification logs: $work"
