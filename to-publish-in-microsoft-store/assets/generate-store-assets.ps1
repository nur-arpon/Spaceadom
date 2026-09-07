<#
  generate-store-assets.ps1
  One-off generator for Spaceadom's Microsoft Store LISTING placeholder
  images. Not part of the app build — run manually if these need
  regenerating (e.g. after real screenshots exist, or the icon changes).

  Source icon: src-tauri/icons/_original-square-icons/app-icon.png (1024x1024,
  the highest-resolution source in the repo; the working icon.png is only
  512x512). Colours are pasted from src/styles/design-system.css's --st-*
  tokens (Earthy = light/default, Nocturne/Starry = dark). Font: the CSS
  calls for Outfit (bundled webfont) with Caprasimo for headings, neither of
  which is installed as a system font here, so text uses "Segoe UI
  Semibold" as the nearest installed geometric-sans fallback.
#>

param(
  [string]$Root = "D:\Claude-Projects\SpaceToggle-V14",
  [string]$OutDir = "D:\Claude-Projects\SpaceToggle-V14\to-publish-in-microsoft-store\assets"
)

try   { Add-Type -AssemblyName System.Drawing -ErrorAction Stop }
catch { Add-Type -AssemblyName System.Drawing.Common -ErrorAction Stop }

$ErrorActionPreference = 'Stop'

$SrcIcon = Join-Path $Root 'src-tauri\icons\_original-square-icons\app-icon.png'
if (-not (Test-Path $SrcIcon)) { throw "source icon not found: $SrcIcon" }

New-Item -ItemType Directory -Force $OutDir | Out-Null

# ---- palette, from src/styles/design-system.css --st-* tokens ----
function Hex($h) { [System.Drawing.ColorTranslator]::FromHtml($h) }

$Earthy = @{
  bg      = Hex '#f5ead8'   # --st-bg
  card    = Hex '#fdf6e9'   # --st-card
  text    = Hex '#201e1d'   # --st-text
  accent  = Hex '#c67139'   # --st-accent (terracotta)
}
$Starry = @{
  bg      = Hex '#0d141f'   # --st-bg (nocturne)
  card    = Hex '#1c2534'   # --st-card (nocturne)
  text    = Hex '#e8e4dc'   # --st-text (nocturne)
  accent  = Hex '#6b8cd6'   # --st-accent (nocturne)
}

$Tagline = "Hold Space, tap any app's initial letter - boom! it opens."
$FontFamily = 'Segoe UI Semibold'   # Outfit/Caprasimo not installed; nearest system geometric sans

function New-Bitmap([int]$w, [int]$h) {
  $bmp = New-Object System.Drawing.Bitmap($w, $h, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $bmp.SetResolution(96, 96)
  return $bmp
}

function Get-RoundedRectPath([single]$x, [single]$y, [single]$w, [single]$h, [single]$r) {
  $path = New-Object System.Drawing.Drawing2D.GraphicsPath
  $d = $r * 2
  $path.AddArc($x, $y, $d, $d, 180, 90)
  $path.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
  $path.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90)
  $path.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
  $path.CloseFigure()
  return $path
}

# Draws the source icon into a rounded, drop-shadowed square of side $size,
# with its top-left corner at ($x, $y) on $g.
function Draw-IconTile($g, [System.Drawing.Image]$icon, [single]$x, [single]$y, [single]$size) {
  $radius = $size * 0.20

  # soft shadow: a handful of decreasing-alpha rounded rects, offset down-right
  for ($i = 6; $i -ge 1; $i--) {
    $off = $i * ($size * 0.012)
    $alpha = [int](5 + ($i * 2))
    $shadowBrush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb($alpha, 0, 0, 0))
    $shadowPath = Get-RoundedRectPath ($x + $off) ($y + $off * 1.4) $size $size $radius
    $g.FillPath($shadowBrush, $shadowPath)
    $shadowBrush.Dispose(); $shadowPath.Dispose()
  }

  $tilePath = Get-RoundedRectPath $x $y $size $size $radius
  $oldClip = $g.Clip
  $g.SetClip($tilePath)
  $g.DrawImage($icon, $x, $y, $size, $size)
  $g.Clip = $oldClip
  $tilePath.Dispose()
}

function Fill-Background($g, [int]$w, [int]$h, [System.Drawing.Color]$color) {
  $brush = New-Object System.Drawing.SolidBrush $color
  $g.FillRectangle($brush, 0, 0, $w, $h)
  $brush.Dispose()
}

function Configure-Graphics($g) {
  $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
  $g.PixelOffsetMode   = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
  $g.SmoothingMode     = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
  $g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAliasGridFit
}

$icon = [System.Drawing.Image]::FromFile($SrcIcon)

# ---------------------------------------------------------------------------
# 1. StoreLogo-300x300.png - REQUIRED. Maps to Partner Center's Store
#    listings > "1:1 App tile icon (300 x 300 pixels)" field. Plain resize;
#    the source icon already fills its own square, no extra background needed.
# ---------------------------------------------------------------------------
$bmp = New-Bitmap 300 300
$g = [System.Drawing.Graphics]::FromImage($bmp)
Configure-Graphics $g
$g.Clear([System.Drawing.Color]::Transparent)
$g.DrawImage($icon, 0, 0, 300, 300)
$g.Dispose()
$bmp.Save((Join-Path $OutDir 'StoreLogo-300x300.png'), [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Output 'StoreLogo-300x300.png'

# ---------------------------------------------------------------------------
# 2. AppTile-1080x1080.png - OPTIONAL placeholder. Per Microsoft's current
#    docs this "1:1 box art" size is documented as a GAMES-only field ("does
#    not apply to apps") - see assets/README.md. Generated anyway per brief,
#    on the Earthy background, in case the wizard still offers the slot.
# ---------------------------------------------------------------------------
$size = 1080
$bmp = New-Bitmap $size $size
$g = [System.Drawing.Graphics]::FromImage($bmp)
Configure-Graphics $g
Fill-Background $g $size $size $Earthy.bg
$tile = [single]($size * 0.62)
$pos = [single](($size - $tile) / 2)
Draw-IconTile $g $icon $pos $pos $tile
$g.Dispose()
$bmp.Save((Join-Path $OutDir 'AppTile-1080x1080.png'), [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Output 'AppTile-1080x1080.png'

# ---------------------------------------------------------------------------
# Shared hero-drawing routine: bg fill, icon tile on the left, wrapped
# tagline on the right, vertically centred, kept out of the bottom third
# per Microsoft's screenshot/hero guidance.
# ---------------------------------------------------------------------------
function New-HeroWithTagline([int]$w, [int]$h, [hashtable]$palette, [string]$outFile) {
  $bmp = New-Bitmap $w $h
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  Configure-Graphics $g
  Fill-Background $g $w $h $palette.bg

  $iconSize = [single]($h * 0.56)
  $marginX  = [single]($w * 0.07)
  $iconY    = [single](($h - $iconSize) / 2 - $h * 0.03)
  Draw-IconTile $g $icon $marginX $iconY $iconSize

  $textX = $marginX + $iconSize + ($w * 0.05)
  $textW = $w - $textX - $marginX
  # keep text out of the bottom third (store screenshot text-overlay guidance)
  $textH = [single]($h * 0.62)
  $textY = [single](($h - $textH) / 2 - $h * 0.03)
  $rect  = New-Object System.Drawing.RectangleF ($textX, $textY, $textW, $textH)

  $fontSize = [single]($h * 0.062)
  $font = New-Object System.Drawing.Font($FontFamily, $fontSize, [System.Drawing.FontStyle]::Bold, [System.Drawing.GraphicsUnit]::Pixel)
  $format = New-Object System.Drawing.StringFormat
  $format.Alignment = [System.Drawing.StringAlignment]::Near
  $format.LineAlignment = [System.Drawing.StringAlignment]::Center
  $textBrush = New-Object System.Drawing.SolidBrush $palette.text
  $g.DrawString($Tagline, $font, $textBrush, $rect, $format)

  # small accent rule under the wordmark area, using the theme accent colour
  $ruleY = $textY + $textH + ($h * 0.02)
  $ruleBrush = New-Object System.Drawing.SolidBrush $palette.accent
  $g.FillRectangle($ruleBrush, $textX, $ruleY, [single]($w * 0.12), [single]($h * 0.012))

  $textBrush.Dispose(); $ruleBrush.Dispose(); $font.Dispose(); $format.Dispose()
  $g.Dispose()
  $bmp.Save((Join-Path $OutDir $outFile), [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
  Write-Output $outFile
}

# ---------------------------------------------------------------------------
# 3. Hero-1920x1080.png - marketing hero WITH tagline, Starry palette.
#    NOTE: Microsoft's "16:9 Super hero art" wizard slot explicitly forbids
#    text in the image - this file is for README / social / press use, not
#    that upload slot. See Hero-1920x1080-textfree.png for the wizard slot.
# ---------------------------------------------------------------------------
New-HeroWithTagline 1920 1080 $Starry 'Hero-1920x1080.png'

# ---------------------------------------------------------------------------
# 4. Hero-2400x1200.png - marketing hero WITH tagline, Earthy palette, wide
#    2:1 crop. NOTE: in Partner Center this exact size (2400x1200) maps to
#    the Holographic-device image, which does not apply to Spaceadom (no
#    HoloLens support declared). Generic wide banner only (README, social).
# ---------------------------------------------------------------------------
New-HeroWithTagline 2400 1200 $Earthy 'Hero-2400x1200.png'

# ---------------------------------------------------------------------------
# 5. Hero-1920x1080-textfree.png - the one actually meant for Partner
#    Center's optional "16:9 Super hero art" upload: no text, icon centred
#    with a soft accent glow behind it, Starry palette.
# ---------------------------------------------------------------------------
$w = 1920; $h = 1080
$bmp = New-Bitmap $w $h
$g = [System.Drawing.Graphics]::FromImage($bmp)
Configure-Graphics $g
Fill-Background $g $w $h $Starry.bg

# soft radial glow behind the icon, accent-tinted, so the frame isn't empty
$glowR = [single]($h * 0.62)
$cx = [single]($w / 2); $cy = [single]($h / 2)
$glowPath = New-Object System.Drawing.Drawing2D.GraphicsPath
$glowPath.AddEllipse($cx - $glowR, $cy - $glowR, $glowR * 2, $glowR * 2)
$glowBrush = New-Object System.Drawing.Drawing2D.PathGradientBrush($glowPath)
$glowBrush.CenterColor = [System.Drawing.Color]::FromArgb(90, $Starry.accent.R, $Starry.accent.G, $Starry.accent.B)
$glowBrush.SurroundColors = @([System.Drawing.Color]::FromArgb(0, $Starry.accent.R, $Starry.accent.G, $Starry.accent.B))
$g.FillPath($glowBrush, $glowPath)
$glowBrush.Dispose(); $glowPath.Dispose()

$iconSize = [single]($h * 0.60)
Draw-IconTile $g $icon ([single](($w - $iconSize) / 2)) ([single](($h - $iconSize) / 2 - $h * 0.02)) $iconSize
$g.Dispose()
$bmp.Save((Join-Path $OutDir 'Hero-1920x1080-textfree.png'), [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Output 'Hero-1920x1080-textfree.png'

$icon.Dispose()
Write-Output "Done. Files written to $OutDir"
