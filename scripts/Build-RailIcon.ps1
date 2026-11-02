$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$assets = Join-Path (Split-Path $PSScriptRoot -Parent) 'apps/rail/assets'
[xml]$svg = Get-Content -LiteralPath (Join-Path $assets 'rail-icon.svg') -Raw
$source = [System.Drawing.Bitmap]::new(1024,1024)
$graphics = [System.Drawing.Graphics]::FromImage($source)
$graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$graphics.ScaleTransform(4,4)
foreach ($rect in $svg.svg.rect) {
    $x = [single]$rect.x
    $y = [single]$rect.y
    $width = [single]$rect.width
    $height = [single]$rect.height
    $diameter = [single]$rect.rx * 2
    $path = [System.Drawing.Drawing2D.GraphicsPath]::new()
    $path.AddArc($x,$y,$diameter,$diameter,180,90)
    $path.AddArc($x+$width-$diameter,$y,$diameter,$diameter,270,90)
    $path.AddArc($x+$width-$diameter,$y+$height-$diameter,$diameter,$diameter,0,90)
    $path.AddArc($x,$y+$height-$diameter,$diameter,$diameter,90,90)
    $path.CloseFigure()
    $brush = [System.Drawing.SolidBrush]::new([System.Drawing.ColorTranslator]::FromHtml($rect.fill))
    $pen = [System.Drawing.Pen]::new([System.Drawing.ColorTranslator]::FromHtml($rect.stroke),[single]$rect.'stroke-width')
    $graphics.FillPath($brush,$path)
    $graphics.DrawPath($pen,$path)
    $pen.Dispose(); $brush.Dispose(); $path.Dispose()
}
$graphics.Dispose()
$source.Save((Join-Path $assets 'rail-icon.png'),[System.Drawing.Imaging.ImageFormat]::Png)
$entries = @()
foreach ($size in @(16,20,24,32,40,48,64,128,256)) {
    $bitmap = [System.Drawing.Bitmap]::new($size,$size)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $graphics.DrawImage($source,0,0,$size,$size)
    $stream = [System.IO.MemoryStream]::new()
    $bitmap.Save($stream,[System.Drawing.Imaging.ImageFormat]::Png)
    $entries += @{ Size=$size; Bytes=$stream.ToArray() }
    $stream.Dispose(); $graphics.Dispose(); $bitmap.Dispose()
}
$source.Dispose()
$writer = [System.IO.BinaryWriter]::new([System.IO.File]::Create((Join-Path $assets 'rail.ico')))
try {
    $writer.Write([uint16]0); $writer.Write([uint16]1); $writer.Write([uint16]$entries.Count)
    $offset = 6 + 16 * $entries.Count
    foreach ($entry in $entries) {
        $dimension = if ($entry.Size -eq 256) { 0 } else { $entry.Size }
        $writer.Write([byte]$dimension); $writer.Write([byte]$dimension)
        $writer.Write([uint16]0); $writer.Write([uint16]1); $writer.Write([uint16]32)
        $writer.Write([uint32]$entry.Bytes.Length); $writer.Write([uint32]$offset)
        $offset += $entry.Bytes.Length
    }
    foreach ($entry in $entries) { $writer.Write([byte[]]$entry.Bytes) }
} finally { $writer.Dispose() }
