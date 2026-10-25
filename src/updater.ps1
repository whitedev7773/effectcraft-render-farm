param([int]$ParentId, [string]$Stage, [string]$Executable, [string]$ResultFile)
$ErrorActionPreference = 'Stop'
$install = Split-Path -Parent $Executable
$targets = @($Executable, (Join-Path $install 'effectcraft-cli.exe'))
$sources = @((Join-Path $Stage 'effectcraft-farm.exe'), (Join-Path $Stage 'effectcraft-cli.exe'))
$backedUp = @()
$installed = @()
try {
    'ready' | Set-Content -LiteralPath (Join-Path $Stage 'installer-ready') -Encoding ASCII
    # The old process must release both executable files before replacement.
    $parent = Get-Process -Id $ParentId -ErrorAction SilentlyContinue
    if ($parent) { $parent.WaitForExit() }
    for ($i = 0; $i -lt 2; $i++) {
        if (Test-Path -LiteralPath $targets[$i]) {
            Copy-Item -LiteralPath $targets[$i] -Destination (Join-Path $Stage "backup-$i.exe")
            $backedUp += $i
        }
    }
    for ($i = 0; $i -lt 2; $i++) {
        # Track before copying, so even a partially failed copy gets restored.
        $installed += $i
        Copy-Item -LiteralPath $sources[$i] -Destination $targets[$i] -Force
    }
    'Update installed successfully.' | Set-Content -LiteralPath $ResultFile -Encoding UTF8
    Start-Process -FilePath $Executable -WorkingDirectory $install -WindowStyle Hidden
} catch {
    $failure = $_.Exception.Message
    $rollbackErrors = @()
    foreach ($i in $installed) {
        try {
            if ($backedUp -contains $i) {
                Copy-Item -LiteralPath (Join-Path $Stage "backup-$i.exe") -Destination $targets[$i] -Force
            } elseif (Test-Path -LiteralPath $targets[$i]) {
                Remove-Item -LiteralPath $targets[$i]
            }
        } catch { $rollbackErrors += $_.Exception.Message }
    }
    "Update failed: $failure`nRollback: $($rollbackErrors -join '; ')`nBackup: $Stage" | Set-Content -LiteralPath $ResultFile -Encoding UTF8
    if ($rollbackErrors.Count -eq 0) {
        Start-Process -FilePath $Executable -WorkingDirectory $install -WindowStyle Hidden
    }
}
