# A complete recovery in one command: image the drive, look for filesystems, carve files, verify.
#
#   .\recover.ps1 <device-or-image> <work-folder> [extra options for `image`]
#
#   .\recover.ps1 \.\PhysicalDrive2 D:\rescue\usb1          (run as Administrator)
#   .\recover.ps1 \.\PhysicalDrive2 D:\rescue\usb1 --timeout 10 --chunk-kib 256
#
# It is safe to run again after an interruption: if a progress file exists, the copy resumes.
# Set $env:TPT to the path of the tool if it is not on your PATH.
param(
    [Parameter(Mandatory = $true, Position = 0)][string]$Source,
    [Parameter(Mandatory = $true, Position = 1)][string]$Work,
    [Parameter(ValueFromRemainingArguments = $true)][string[]]$Extra
)
$tool = if ($env:TPT) { $env:TPT } else { "tpt-usb-rescue" }

New-Item -ItemType Directory -Force -Path $Work | Out-Null
$img = Join-Path $Work "recovery.img"

$imageArgs = @("image", $Source, $img, "--hash")
if (Test-Path "$img.map") {
    Write-Host "== found an earlier run, resuming"
    $imageArgs += "--resume"
}
if ($Extra) { $imageArgs += $Extra }

Write-Host "== 1/4 imaging $Source -> $img"
& $tool @imageArgs
$code = $LASTEXITCODE
switch ($code) {
    0 { }
    5 { Write-Host "!! finished, but some sectors could not be read. They are zero-filled in the image." }
    3 { Write-Host "!! stopped early. Run this script again to resume."; exit 3 }
    default { Write-Host "!! imaging failed (exit $code)"; exit $code }
}

Write-Host "== 2/4 looking for filesystems"
& $tool analyze $img --extract (Join-Path $Work "volumes")

Write-Host "== 3/4 carving files by signature"
& $tool carve $img (Join-Path $Work "carved") --validate-media
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "== 4/4 verifying carved files against the image"
& $tool verify (Join-Path $Work "carved") $img
$vcode = $LASTEXITCODE

Write-Host ""
Write-Host "Done. Everything is in ${Work}:"
Write-Host "  recovery.img         the raw image (a copy; the drive was only read)"
Write-Host "  recovery.img.map     which sectors were recovered or bad"
Write-Host "  recovery.img.sha256  the image's hash"
Write-Host "  volumes\             files read from the filesystem, if one was found"
Write-Host "  carved\              files found by signature, with index.csv"
if ($code -eq 5) { exit 5 }
exit $vcode
