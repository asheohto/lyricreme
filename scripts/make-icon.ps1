# Regenerates assets/logo.ico from assets/logo.png as a multi-resolution icon.
#
# Why: a single 256x256 PNG-compressed ICO entry renders as a blank/blurry blob in
# the notification area. Windows picks the closest image for the tray's small icon,
# so we emit uncompressed 32bpp BMP entries at the sizes the shell actually uses
# (16/24/32/48/64) plus a PNG entry for 256. run: powershell -File scripts/make-icon.ps1
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$root = Split-Path -Parent $PSScriptRoot
$src  = Join-Path $root 'assets\logo.png'
$out  = Join-Path $root 'assets\logo.ico'

# BMP-encoded entry sizes (used by Explorer/tray), and one PNG entry for large views.
$bmpSizes = @(16, 24, 32, 48, 64)
$pngSize  = 256

$source = [System.Drawing.Image]::FromFile($src)

function Resize-Bitmap([System.Drawing.Image]$img, [int]$size) {
    $bmp = New-Object System.Drawing.Bitmap($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.SmoothingMode     = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
    $g.PixelOffsetMode   = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.Clear([System.Drawing.Color]::Transparent)
    # Preserve aspect ratio, centred in a square canvas.
    $scale = [Math]::Min($size / $img.Width, $size / $img.Height)
    $w = [int]($img.Width * $scale); $h = [int]($img.Height * $scale)
    $g.DrawImage($img, [int](($size - $w) / 2), [int](($size - $h) / 2), $w, $h)
    $g.Dispose()
    return $bmp
}

# Builds an uncompressed 32bpp BMP icon image: BITMAPINFOHEADER + bottom-up BGRA + zero AND mask.
function Build-BmpEntry([System.Drawing.Bitmap]$bmp, [int]$size) {
    $rect = New-Object System.Drawing.Rectangle(0, 0, $size, $size)
    $data = $bmp.LockBits($rect, [System.Drawing.Imaging.ImageLockMode]::ReadOnly,
                          [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $stride = $data.Stride
    $raw = New-Object byte[] ($stride * $size)
    [System.Runtime.InteropServices.Marshal]::Copy($data.Scan0, $raw, 0, $raw.Length)
    $bmp.UnlockBits($data)

    $ms = New-Object System.IO.MemoryStream
    $bw = New-Object System.IO.BinaryWriter($ms)
    # BITMAPINFOHEADER (height doubled: XOR + AND)
    $bw.Write([uint32]40); $bw.Write([int32]$size); $bw.Write([int32]($size * 2))
    $bw.Write([uint16]1);  $bw.Write([uint16]32); $bw.Write([uint32]0)
    $bw.Write([uint32]($size * $size * 4)); $bw.Write([int32]0); $bw.Write([int32]0)
    $bw.Write([uint32]0); $bw.Write([uint32]0)
    # XOR pixels, bottom-up BGRA
    for ($y = $size - 1; $y -ge 0; $y--) { $bw.Write($raw, $y * $stride, $size * 4) }
    # AND mask (all zero — the alpha channel is authoritative)
    $maskRow = [int]([Math]::Ceiling($size / 32.0) * 4)
    $bw.Write((New-Object byte[] ($maskRow * $size)), 0, $maskRow * $size)
    $bw.Flush()
    return $ms.ToArray()
}

function Build-PngEntry([System.Drawing.Bitmap]$bmp) {
    $ms = New-Object System.IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    return $ms.ToArray()
}

$entries = @()
foreach ($s in $bmpSizes) {
    $b = Resize-Bitmap $source $s
    $entries += [pscustomobject]@{ Size = $s; Data = (Build-BmpEntry $b $s) }
    $b.Dispose()
}
$big = Resize-Bitmap $source $pngSize
$entries += [pscustomobject]@{ Size = $pngSize; Data = (Build-PngEntry $big) }
$big.Dispose()
$source.Dispose()

# Assemble ICONDIR + ICONDIRENTRY[] + images
$outMs = New-Object System.IO.MemoryStream
$ow = New-Object System.IO.BinaryWriter($outMs)
$ow.Write([uint16]0); $ow.Write([uint16]1); $ow.Write([uint16]$entries.Count)
$offset = 6 + (16 * $entries.Count)
foreach ($e in $entries) {
    $dim = if ($e.Size -ge 256) { 0 } else { $e.Size }
    $ow.Write([byte]$dim); $ow.Write([byte]$dim); $ow.Write([byte]0); $ow.Write([byte]0)
    $ow.Write([uint16]1); $ow.Write([uint16]32)
    $ow.Write([uint32]$e.Data.Length); $ow.Write([uint32]$offset)
    $offset += $e.Data.Length
}
foreach ($e in $entries) { $ow.Write($e.Data, 0, $e.Data.Length) }
$ow.Flush()
[System.IO.File]::WriteAllBytes($out, $outMs.ToArray())

Write-Output "Wrote $out ($((Get-Item $out).Length) bytes) with $($entries.Count) entries: $($bmpSizes -join ', '), ${pngSize}(png)"
