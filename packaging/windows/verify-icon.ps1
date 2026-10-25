param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$app = (Resolve-Path -LiteralPath $Executable).Path
$ico = [IO.File]::ReadAllBytes((Join-Path $root 'assets/app-icon/farm.ico'))

if (-not ('FarmIconResources' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
public static class FarmIconResources {
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern IntPtr LoadLibraryEx(string path, IntPtr file, uint flags);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern IntPtr FindResource(IntPtr module, IntPtr name, IntPtr type);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern IntPtr LoadResource(IntPtr module, IntPtr resource);
    [DllImport("kernel32.dll")] static extern IntPtr LockResource(IntPtr resource);
    [DllImport("kernel32.dll")] static extern uint SizeofResource(IntPtr module, IntPtr resource);
    [DllImport("kernel32.dll")] static extern bool FreeLibrary(IntPtr module);
    public static byte[] Read(string path, int id, int type) {
        // Load resources as data only: never execute the application's code.
        var module = LoadLibraryEx(path, IntPtr.Zero, 0x02 | 0x20);
        if (module == IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error());
        try {
            var resource = FindResource(module, new IntPtr(id), new IntPtr(type));
            if (resource == IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error());
            var size = checked((int)SizeofResource(module, resource));
            var data = LockResource(LoadResource(module, resource));
            if (size == 0 || data == IntPtr.Zero) throw new Exception("Empty icon resource");
            var bytes = new byte[size];
            Marshal.Copy(data, bytes, 0, size);
            return bytes;
        } finally { FreeLibrary(module); }
    }
}
'@
}

function Hash-Bytes([byte[]]$bytes) {
    $hasher = [Security.Cryptography.SHA256]::Create()
    try { return [BitConverter]::ToString($hasher.ComputeHash($bytes)) }
    finally { $hasher.Dispose() }
}

$group = [FarmIconResources]::Read($app, 1, 14)
$count = [BitConverter]::ToUInt16($ico, 4)
if ($count -ne 9 -or [BitConverter]::ToUInt16($group, 4) -ne $count) { throw 'Missing multi-resolution icon group' }
$sizes = @()
for ($index = 0; $index -lt $count; $index++) {
    $entry = 6 + $index * 16
    $groupEntry = 6 + $index * 14
    for ($byte = 0; $byte -lt 12; $byte++) {
        if ($ico[$entry + $byte] -ne $group[$groupEntry + $byte]) { throw "Icon directory differs at entry $index" }
    }
    $length = [BitConverter]::ToUInt32($ico, $entry + 8)
    $offset = [BitConverter]::ToUInt32($ico, $entry + 12)
    $expected = [byte[]]::new($length)
    [Array]::Copy($ico, $offset, $expected, 0, $length)
    $resourceId = [BitConverter]::ToUInt16($group, $groupEntry + 12)
    $embedded = [FarmIconResources]::Read($app, $resourceId, 3)
    if ((Hash-Bytes $expected) -ne (Hash-Bytes $embedded)) { throw "Embedded icon differs at entry $index" }
    $sizes += $(if ($ico[$entry] -eq 0) { 256 } else { $ico[$entry] })
}
if (($sizes -join ',') -ne '16,20,24,32,40,48,64,128,256') { throw 'Incorrect icon resolutions' }
$version = [Diagnostics.FileVersionInfo]::GetVersionInfo($app)
if ($version.ProductName -ne 'EffectCraft Render Farm' -or $version.OriginalFilename -ne 'effectcraft-farm.exe') { throw 'Incorrect executable identity' }
Write-Output ('Approved icon matches every EXE resource: ' + ($sizes -join ', ') + 'px')
