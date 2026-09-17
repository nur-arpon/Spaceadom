# Renders Raycast-style Store screenshots: headline + whole screenshot (never
# cropped) on a theme background. Edit $slides, run: pwsh -File make-slides.ps1
# Output: store-slides\out\slide-NN.png (2560x1440). Nothing else is touched.
$ErrorActionPreference = 'Stop'
$root   = Split-Path -Parent $MyInvocation.MyCommand.Path
$shots  = Join-Path (Split-Path $root) 'assets\screenshots'
$font   = 'D:/Claude-Projects/SpaceToggle-V14/src/assets/fonts/outfit-latin-wght-normal.woff2'
$chrome = 'C:\Program Files\Google\Chrome\Application\chrome.exe'
$out    = Join-Path $root 'out'; New-Item -ItemType Directory -Force $out | Out-Null
$tmp    = Join-Path $root 'tmp'; New-Item -ItemType Directory -Force $tmp | Out-Null

$themes = @{
  earthy = @{ bg='radial-gradient(1600px 1000px at 15% 0%, #fbf3e3 0%, #f5ead8 55%, #eadac0 100%)'; text='#201e1d'; soft='#5c554d'; accent='#c67139'; shadow='rgba(90,60,30,.32)'; edge='rgba(90,60,30,.18)' }
  navy   = @{ bg='radial-gradient(1600px 1000px at 15% 0%, #1a2740 0%, #0d141f 60%, #070b12 100%)'; text='#e8e4dc'; soft='#b3bdcc'; accent='#6b8cd6'; shadow='rgba(0,0,0,.6)';    edge='rgba(255,255,255,.10)' }
  warcry = @{ bg='radial-gradient(1600px 1000px at 15% 0%, #2b1713 0%, #140b09 60%, #0a0504 100%)'; text='#f0ded0'; soft='#c8a892'; accent='#b83024'; shadow='rgba(0,0,0,.65)';   edge='rgba(255,255,255,.10)' }
}

# img = one file (shown whole, centred) or two files (side by side).
$slides = @(
  @{ n=1; img=@('store-02.png');                 theme='navy';   h='Hold Space, tap a letter. Boom &mdash; your app opens.';  s='Tap again, it minimises. Again, it is back.' }
  @{ n=2; img=@('Screenshot 2026-09-12 195730.png'); theme='warcry'; h='Hold the middle button. Move to an app. Let go.'; s='Launch anything without even touching the keyboard.' }
  @{ n=3; img=@('store-01.png','store-04.png');  theme='navy';   h='Any app. Any website. Any folder.';                    s='Click a key, choose what it opens.' }
  @{ n=4; img=@('store-05.png');                 theme='earthy'; h='Hold Space and see every shortcut.';                   s='No memorising, ever.' }
  @{ n=5; img=@('store-03.png');                 theme='navy';   h='Works everywhere in Windows.';                         s='Browser, game, full-screen video &mdash; Space is always yours.' }
  @{ n=6; img=@('Screenshot 2026-09-12 195806.png','store-01.png'); theme='navy'; h='Fun mode on: a living night sky. Off: plain and quiet.'; s='Same app. One switch in Settings.' }
  # 1.0.114 (2026-09-18) — the icon ring, captured over the app's own sky
  # (keyboard hidden) in two themes so the pair does not repeat itself.
  @{ n=7; img=@('ring-earthy-centre-crop.png');      theme='earthy'; h='Hold the middle button. Your apps bloom around the cursor.'; s='Release on one to open it. No keyboard needed.' }
  @{ n=8; img=@('ring-starry-corner-crop.png');      theme='navy';   h='At an edge or a corner, the ring folds into an arc.';        s='Every app stays on screen, on whichever monitor you are on.' }
)

foreach ($s in $slides) {
  $t = $themes[$s.theme]
  $imgs = $s.img | ForEach-Object { '<div class="shot"><img src="file:///' + ((Join-Path $shots $_) -replace '\\','/') + '"></div>' }
  $html = @"
<!doctype html><html><head><meta charset="utf-8"><style>
@font-face{font-family:Outfit;src:url('file:///$font') format('woff2');font-weight:100 900}
html,body{margin:0;width:2560px;height:1440px;overflow:hidden}
body{background:$($t.bg);font-family:Outfit,'Segoe UI Variable','Segoe UI',sans-serif;color:$($t.text);position:relative}
.h{margin:104px 150px 0;font-size:92px;font-weight:700;letter-spacing:-.02em;line-height:1.06}
.s{margin:26px 154px 0;font-size:42px;font-weight:400;color:$($t.soft)}
.dot{display:inline-block;width:24px;height:24px;border-radius:50%;background:$($t.accent);margin-right:20px;vertical-align:middle;position:relative;top:-5px}
.row{position:absolute;left:150px;right:150px;bottom:90px;height:900px;display:flex;gap:48px;align-items:center;justify-content:center}
.shot{max-height:900px;border-radius:26px;overflow:hidden;box-shadow:0 36px 110px $($t.shadow);border:2px solid $($t.edge);flex:0 1 auto;display:flex}
.shot img{max-height:896px;max-width:100%;display:block;object-fit:contain}
</style></head><body>
<div class="h">$($s.h)</div>
<div class="s"><span class="dot"></span>$($s.s)</div>
<div class="row">$($imgs -join '')</div>
</body></html>
"@
  $htmlPath = Join-Path $tmp ("slide-{0:D2}.html" -f $s.n)
  [IO.File]::WriteAllText($htmlPath, $html, [Text.UTF8Encoding]::new($false))
  $png = Join-Path $out ("slide-{0:D2}.png" -f $s.n)
  & $chrome --headless=new --disable-gpu --hide-scrollbars --allow-file-access-from-files --window-size=2560,1440 --screenshot="$png" ("file:///" + ($htmlPath -replace '\\','/')) 2>$null | Out-Null
  Write-Host ("slide {0}: {1} B" -f $s.n, (Get-Item $png).Length)
}
