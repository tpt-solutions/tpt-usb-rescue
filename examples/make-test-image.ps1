# Builds a small synthetic disk image so you can try tpt-usb-rescue without any hardware.
#
# The image is random noise with two tiny PNGs and one GIF hidden inside it, so `carve` has
# something real to find. Works in Windows PowerShell 5.1 and PowerShell 7.
#
#   .\make-test-image.ps1 [-Out test.img]
param([string]$Out = "test.img")
$ErrorActionPreference = "Stop"

# A 1x1 PNG and a 1x1 GIF.
$png = [Convert]::FromBase64String("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==")
$gif = [Convert]::FromBase64String("R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7")
$rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()

$path = Join-Path (Get-Location) $Out
$stream = [System.IO.File]::Create($path)
try {
    function Add-Random([int]$count) {
        $bytes = New-Object byte[] $count
        $rng.GetBytes($bytes)
        $stream.Write($bytes, 0, $count)
    }
    Add-Random 262144
    $stream.Write($png, 0, $png.Length)
    Add-Random 100000
    $stream.Write($gif, 0, $gif.Length)
    Add-Random 65536
    $stream.Write($png, 0, $png.Length)
    Add-Random 131072
}
finally {
    $stream.Dispose()
    $rng.Dispose()
}

Write-Host "wrote $Out ($((Get-Item $path).Length) bytes) with 2 PNGs and 1 GIF hidden in it"
Write-Host "try:  tpt-usb-rescue image $Out recovered.img"
