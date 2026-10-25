param([int]$ParentId, [string]$Stage, [string]$Executable, [string]$ResultFile)
$ErrorActionPreference = 'Stop'
$install = Split-Path -Parent $Executable
$msi = Join-Path $Stage 'setup.msi'
$log = Join-Path $Stage 'setup.log'
try {
    # Validate package identity before the running app is allowed to exit.
    $installer = New-Object -ComObject WindowsInstaller.Installer
    $database = $installer.OpenDatabase($msi, 0)
    function Get-MsiProperty([string]$name) {
        $view = $database.OpenView("SELECT ``Value`` FROM ``Property`` WHERE ``Property``='$name'")
        [void]$view.Execute()
        $record = $view.Fetch()
        if (-not $record) { throw "Missing MSI property: $name" }
        $value = $record.StringData(1)
        [void]$view.Close()
        return $value
    }
    if ((Get-MsiProperty 'ProductName') -ne 'EffectCraft Render Farm' -or
        (Get-MsiProperty 'UpgradeCode') -ne '{648C6D1E-A6B1-49DB-96ED-D34D4F441B27}') {
        throw 'The MSI does not belong to EffectCraft Render Farm.'
    }
    [System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($database) | Out-Null
    [System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($installer) | Out-Null
    'ready' | Set-Content -LiteralPath (Join-Path $Stage 'installer-ready') -Encoding ASCII
    $parent = Get-Process -Id $ParentId -ErrorAction SilentlyContinue
    if ($parent) { $parent.WaitForExit() }
    # Windows Installer owns upgrade/rollback. The external EffectCraft installation is never modified.
    $arguments = @('/i', ('"' + $msi + '"'), '/passive', '/norestart', '/l*v', ('"' + $log + '"'), ('INSTALLFOLDER="' + $install + '"'))
    $process = Start-Process -FilePath (Join-Path $env:SystemRoot 'System32/msiexec.exe') -ArgumentList $arguments -WindowStyle Hidden -Wait -PassThru
    if ($process.ExitCode -eq 3010) {
        "Update installed; Windows restart required. Log: $log" | Set-Content -LiteralPath $ResultFile -Encoding UTF8
        Add-Type -AssemblyName System.Windows.Forms
        [System.Windows.Forms.MessageBox]::Show('The update requires a Windows restart. Please restart Windows before opening Render Farm.', 'EffectCraft Render Farm') | Out-Null
    } elseif ($process.ExitCode -ne 0) {
        throw "Windows Installer exited $($process.ExitCode). See $log"
    } else {
        'Update installed successfully.' | Set-Content -LiteralPath $ResultFile -Encoding UTF8
        Start-Process -FilePath $Executable -WorkingDirectory $install -WindowStyle Hidden
    }
} catch {
    "Update failed: $($_.Exception.Message)`nInstaller log: $log" | Set-Content -LiteralPath $ResultFile -Encoding UTF8
    # If setup failed after shutdown, open the existing installation again.
    if ((Test-Path -LiteralPath (Join-Path $Stage 'installer-ready')) -and (Test-Path -LiteralPath $Executable)) {
        Start-Process -FilePath $Executable -WorkingDirectory $install -WindowStyle Hidden
    }
    exit 1
}
