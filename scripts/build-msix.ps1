<#
  build-msix.ps1 - lay out and pack Spaceadom as an MSIX for the Microsoft Store.
  PROBLEM 250.

  ---------------------------------------------------------------------------
  THIS SCRIPT NEVER INSTALLS ANYTHING. Read this before adding a step.
  ---------------------------------------------------------------------------
  A packaged Spaceadom and an NSIS Spaceadom on ONE machine both install a
  WH_KEYBOARD_LL hook and fight over the spacebar. The dev machine runs the NSIS
  copy. So there is no Add-AppxPackage anywhere in here, and the package is
  UNSIGNED unless you pass -Sign, which means Windows will refuse to install it
  even by accident. That is a feature. Test the .msix on a SECOND machine; the
  recipe is in to-publish-in-microsoft-store/SUBMIT-CHECKLIST.md, "Route B".

  ---------------------------------------------------------------------------
  ASCII ONLY. DO NOT PASTE AN EM DASH, AN ARROW OR A CURLY QUOTE IN HERE.
  ---------------------------------------------------------------------------
  On 2026-09-04 scripts/write-updater-manifests.ps1 failed to parse under
  Windows PowerShell 5.1 because of one em dash in a string: 5.1 reads a .ps1
  as ANSI unless it has a BOM, pwsh 7 reads it as UTF-8, so the script worked
  when it was written and broke when it ran for real. It served the NEW
  installer with the OLD signature and the whole release proof had to be redone.
  Every character in this file is deliberately 7-bit.

  ---------------------------------------------------------------------------
  WHAT GOES IN THE PACKAGE, AND WHY IT IS THE PLAIN RELEASE BUILD
  ---------------------------------------------------------------------------
  Not `npm run store`. That target exists to make the NSIS installer embed the
  WebView2 OFFLINE bootstrapper, because the Store forbids an installer that
  downloads bits when it runs (Route A). An MSIX contains the app's FILES, not
  an installer, and you cannot run an installer from inside a package - so the
  210 MB Store installer has literally nothing to contribute here. The layout is
  the plain `npm run tauri build` output:

      spaceadom.exe        src-tauri/target/release/spaceadom.exe
      spaceadom.pdb        src-tauri/symbols/spaceadom.pdb   (bundle.resources
                           installs it beside the exe for the other two
                           installers too - PROBLEM 131, symbols SHIP)
      Assets\*.png         generated here from src-tauri/icons/icon.png
      AppxManifest.xml     src-tauri/msix/AppxManifest.xml with the four
                           placeholders filled in

  WebView2 comes from the system Evergreen runtime. The reasoning, the
  alternatives and the residual risk are in the AppxManifest.xml header.

  ---------------------------------------------------------------------------
  USAGE
  ---------------------------------------------------------------------------
      npm run build ; npm run tauri build      # produce the release exe first
      npm run msix                             # pack, unsigned, and validate
      npm run msix -- -Sign                    # also sign with a local test cert

  Output: src-tauri/target/release/bundle/msix/Spaceadom_<version>_x64.msix

  ---------------------------------------------------------------------------
  EXIT CODES (REVIEW FIXES 2026-09-05, C2)
  ---------------------------------------------------------------------------
      0   a .msix was packed and validated. ONLY this means a package exists.
      1   Die - something is wrong with the source, the identity, the manifest
          or the pack itself. Fix it.
      2   the Windows SDK (MakeAppx.exe) is not installed. The layout is
          complete and correct; nothing is wrong with this repository. Install
          the SDK and re-run.

  The "SDK missing" branch used to exit 0. release.yml wrote `built=true` after
  it either way, so a runner without the SDK claimed to have produced a package
  that did not exist, and the upload step then failed the whole RELEASE on a
  Store-only extra - after both installers had already been built. A caller
  that cannot tell "skipped" from "succeeded" cannot make that call correctly,
  so the script now says which happened.
#>

[CmdletBinding()]
param(
  # Sign the package with a SELF-SIGNED certificate created into src-tauri/msix/
  # (gitignored). For local structural testing only - the Microsoft Store
  # re-signs the package it distributes with its own certificate, so this
  # signature never reaches a user. Off by default so an unsigned package cannot
  # be installed by accident on the dev machine.
  [switch]$Sign,

  # Skip the makeappx unpack round-trip. CI passes this only if the validation
  # itself is what is being debugged; normally you want it.
  [switch]$SkipValidate,

  # ARM64 (2026-09-17): 'x64' (default) or 'arm64'. Picks the release binary
  # from the matching cargo target dir, writes ProcessorArchitecture into
  # the manifest, and names the package Spaceadom_<v>_<arch>.msix. The Store
  # takes both packages under one submission; MakeAppx itself is always the
  # host's x64 tool.
  [ValidateSet('x64','arm64')][string]$Arch = 'x64'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Say  ($m) { Write-Host "build-msix: $m" }
function Die  ($m) { Write-Host "build-msix: ERROR - $m" -ForegroundColor Red; exit 1 }
function Warn ($m) { Write-Host "build-msix: WARNING - $m" -ForegroundColor Yellow }

$Root     = Split-Path -Parent $PSScriptRoot
$MsixSrc  = Join-Path $Root 'src-tauri\msix'
$Layout   = Join-Path $MsixSrc 'layout'
$Assets   = Join-Path $Layout  'Assets'
$OutDir   = Join-Path $Root 'src-tauri\target\release\bundle\msix'
# The cargo output tree for the architecture: the host tree for x64, the
# per-target tree for arm64 (`npm run arm64` = tauri build --target
# aarch64-pc-windows-msvc).
$RelDir   = if ($Arch -eq 'arm64') { Join-Path $Root 'src-tauri\target\aarch64-pc-windows-msvc\release' } else { Join-Path $Root 'src-tauri\target\release' }
Say "architecture: $Arch (release tree $RelDir)"

Say "repo root: $Root"

# ---------------------------------------------------------------------------
# 1. Version. package.json is the source of truth for all three version files
#    (the release workflow already fails a tag that disagrees with it), and the
#    Store wants a.b.c.d with d reserved, so d is always 0 and never asked for.
# ---------------------------------------------------------------------------
$pkgJsonPath = Join-Path $Root 'package.json'
if (-not (Test-Path $pkgJsonPath)) { Die "no package.json at $pkgJsonPath" }
$version3 = (Get-Content $pkgJsonPath -Raw | ConvertFrom-Json).version
if ($version3 -notmatch '^\d+\.\d+\.\d+$') {
  Die "package.json version '$version3' is not a.b.c - the manifest needs a.b.c.0"
}
$version4 = "$version3.0"
Say "version: $version3 -> package version $version4"

# ---------------------------------------------------------------------------
# 2. Identity. Three values from Partner Center, in a gitignored file.
# ---------------------------------------------------------------------------
$identityPath = Join-Path $MsixSrc 'identity.json'
if (-not (Test-Path $identityPath)) {
  Write-Host ""
  Write-Host "build-msix: src-tauri/msix/identity.json is missing." -ForegroundColor Yellow
  Write-Host ""
  Write-Host "  This is the ONE step that cannot be automated: the three values come"
  Write-Host "  from YOUR Partner Center account, and nobody else's package can use"
  Write-Host "  them. Do this once:"
  Write-Host ""
  Write-Host "    1. Partner Center > Spaceadom > Product management > Product identity"
  Write-Host "    2. copy src-tauri/msix/identity.example.json to identity.json"
  Write-Host "    3. paste in Package/Identity/Name, Package/Identity/Publisher and"
  Write-Host "       Package/Properties/PublisherDisplayName"
  Write-Host ""
  Write-Host "  identity.json is gitignored. Nothing was built."
  Write-Host ""
  exit 1
}
# [IO.File]::ReadAllText, not Get-Content -Raw. Windows PowerShell 5.1 reads a
# BOM-less file in the ANSI code page; .NET's ReadAllText honours a BOM and
# falls back to UTF-8. Both this file and AppxManifest.xml are kept pure ASCII
# as well, so neither reader can mangle them - belt AND braces, because the one
# time this project trusted a single layer of it (an em dash in
# write-updater-manifests.ps1) it shipped a mismatched signature.
$identity = [IO.File]::ReadAllText($identityPath) | ConvertFrom-Json
foreach ($field in @('name','publisher','publisherDisplayName')) {
  if (-not $identity.PSObject.Properties.Name.Contains($field)) {
    Die "identity.json has no '$field' - copy identity.example.json again"
  }
  if ([string]::IsNullOrWhiteSpace($identity.$field) -or $identity.$field -like '*PUT-YOUR-*') {
    Die "identity.json '$field' is still the placeholder value. Fill it in from Partner Center."
  }
}
if ($identity.publisher -notmatch '^CN=') {
  Die "identity.json 'publisher' must be an X.500 subject beginning with CN= (it is '$($identity.publisher)')"
}
Say "identity: $($identity.name) / $($identity.publisher)"

# A package built with a stand-in identity is structurally perfect and
# commercially useless: the Store rejects it at ingestion because the values do
# not belong to the account uploading it. That failure happens minutes after an
# upload, far from here, so it is announced HERE, loudly, every single run.
# The marker is a substring rather than an exact match so it survives someone
# editing the other two fields first.
$isTestIdentity = ($identity.name -like '*LOCALTEST*') -or ($identity.publisher -like '*LOCAL-TEST*')
if ($isTestIdentity) {
  Write-Host ""
  Write-Host "  ####################################################################" -ForegroundColor Yellow
  Write-Host "  #  THIS IS A LOCAL TEST IDENTITY, NOT A PARTNER CENTER ONE.        #" -ForegroundColor Yellow
  Write-Host "  #  The package it builds proves the PIPELINE works and would be    #" -ForegroundColor Yellow
  Write-Host "  #  rejected by the Store at ingestion. Before submitting, replace  #" -ForegroundColor Yellow
  Write-Host "  #  all three values in src-tauri/msix/identity.json with the ones  #" -ForegroundColor Yellow
  Write-Host "  #  on Partner Center > Product management > Product identity.      #" -ForegroundColor Yellow
  Write-Host "  ####################################################################" -ForegroundColor Yellow
  Write-Host ""
}

# ---------------------------------------------------------------------------
# 3. The binary. Refuse loudly rather than packing a stale or wrong one.
# ---------------------------------------------------------------------------
$exe = Join-Path $RelDir 'spaceadom.exe'
if (-not (Test-Path $exe)) {
  Die "no release binary at $exe. Run 'npm run build' then 'npm run tauri build' (or 'npm run arm64') first."
}
# The binary must BE the architecture the manifest will claim: PE machine
# type 0x8664 = x64, 0xAA64 = ARM64. A mismatch is unshippable and the Store
# reports it only after a long upload.
$peBytes = [IO.File]::ReadAllBytes($exe)
$peOff   = [BitConverter]::ToInt32($peBytes, 0x3C)
$machine = [BitConverter]::ToUInt16($peBytes, $peOff + 4)
$want    = if ($Arch -eq 'arm64') { 0xAA64 } else { 0x8664 }
if ($machine -ne $want) {
  Die ("the binary's PE machine type is 0x{0:X4} but -Arch is {1} (wants 0x{2:X4})" -f $machine, $Arch, $want)
}
$exeItem = Get-Item $exe
$exeVer  = $exeItem.VersionInfo.FileVersion
Say ("binary: {0:N0} bytes, FileVersion {1}, built {2}" -f $exeItem.Length, $exeVer, $exeItem.LastWriteTime)
# CLAUDE.md's rule, applied here: a version stamp that disagrees with the
# source is the worst state to debug from, because the binary REPORTS the old
# version while containing the new code. Catch it before it is inside a package
# that gets uploaded.
if ($exeVer -and ($exeVer -notlike "$version3*")) {
  Die "the release binary reports FileVersion '$exeVer' but package.json says '$version3'. Rebuild before packing - a package whose exe disagrees with its manifest is unshippable and undiagnosable."
}

$pdb = Join-Path $Root 'src-tauri\symbols\spaceadom.pdb'
$havePdb = Test-Path $pdb
if (-not $havePdb) {
  Warn "no $pdb - packing without debug symbols. The other two installers ship them (PROBLEM 131), so crash reports from this package will resolve to addresses instead of lines."
}

# ---------------------------------------------------------------------------
# 4. Assets, generated from the one 512x512 source so every size is exact.
#
#    ONLY 100%-scale assets are produced. A file called Logo.scale-200.png is
#    inert without a resources.pri built by makepri.exe - Windows never looks at
#    it - so generating them would be decoration that reads as correctness.
#    Windows scales the 100% assets instead. See the AppxManifest.xml header.
# ---------------------------------------------------------------------------
try   { Add-Type -AssemblyName System.Drawing -ErrorAction Stop }
catch { Add-Type -AssemblyName System.Drawing.Common -ErrorAction Stop }

function New-Logo {
  param([string]$Src, [string]$Dest, [int]$W, [int]$H)
  $img = [System.Drawing.Image]::FromFile($Src)
  try {
    $bmp = New-Object System.Drawing.Bitmap($W, $H, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    try {
      $g = [System.Drawing.Graphics]::FromImage($bmp)
      try {
        $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $g.PixelOffsetMode   = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
        $g.SmoothingMode     = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
        $g.Clear([System.Drawing.Color]::Transparent)
        # Letterbox: the source is square and the wide tile is not, so fit
        # rather than stretch. A stretched logo is the single most obvious
        # "this was packaged in a hurry" tell on a Store listing.
        $scale = [Math]::Min($W / $img.Width, $H / $img.Height)
        $dw = [int]([Math]::Round($img.Width  * $scale))
        $dh = [int]([Math]::Round($img.Height * $scale))
        $g.DrawImage($img, [int](($W - $dw) / 2), [int](($H - $dh) / 2), $dw, $dh)
      } finally { $g.Dispose() }
      $bmp.Save($Dest, [System.Drawing.Imaging.ImageFormat]::Png)
    } finally { $bmp.Dispose() }
  } finally { $img.Dispose() }
}

# ---------------------------------------------------------------------------
# 5. Lay the package out. The layout directory is REBUILT from scratch every
#    run: MakeAppx packs whatever is in it, so a file left behind by an earlier
#    run ships silently. Only this script's own output directory is removed.
# ---------------------------------------------------------------------------
if (Test-Path $Layout) { Remove-Item $Layout -Recurse -Force }
New-Item -ItemType Directory -Force -Path $Assets | Out-Null

$iconSrc = Join-Path $Root 'src-tauri\icons\icon.png'
if (-not (Test-Path $iconSrc)) { Die "no icon source at $iconSrc" }

$logos = @(
  @{ Name = 'Square44x44Logo.png';   W =  44; H =  44 },
  @{ Name = 'Square71x71Logo.png';   W =  71; H =  71 },
  @{ Name = 'Square150x150Logo.png'; W = 150; H = 150 },
  @{ Name = 'Square310x310Logo.png'; W = 310; H = 310 },
  @{ Name = 'Wide310x150Logo.png';   W = 310; H = 150 },
  @{ Name = 'StoreLogo.png';         W =  50; H =  50 }
)
foreach ($l in $logos) {
  New-Logo -Src $iconSrc -Dest (Join-Path $Assets $l.Name) -W $l.W -H $l.H
}
Say "assets: $($logos.Count) logos generated from icons/icon.png"

Copy-Item $exe (Join-Path $Layout 'spaceadom.exe') -Force
if ($havePdb) { Copy-Item $pdb (Join-Path $Layout 'spaceadom.pdb') -Force }

# The manifest, with its four placeholders filled in.
$manifestTemplate = Join-Path $MsixSrc 'AppxManifest.xml'
if (-not (Test-Path $manifestTemplate)) { Die "no template at $manifestTemplate" }
$manifest = [IO.File]::ReadAllText($manifestTemplate)   # see the note on identity.json
$manifest = $manifest.Replace('{{IDENTITY_NAME}}',         $identity.name)
$manifest = $manifest.Replace('{{IDENTITY_PUBLISHER}}',    $identity.publisher)
$manifest = $manifest.Replace('{{PUBLISHER_DISPLAY_NAME}}',$identity.publisherDisplayName)
$manifest = $manifest.Replace('{{VERSION}}',               $version4)
$manifest = $manifest.Replace('{{ARCH}}',                  $Arch)
if ($manifest -match '\{\{[A-Z_]+\}\}') {
  Die "a placeholder survived substitution in AppxManifest.xml: $($Matches[0]). MakeAppx would pack it and the Store would reject the package."
}
$manifestOut = Join-Path $Layout 'AppxManifest.xml'
# UTF8 WITHOUT a BOM: MakeAppx accepts a BOM, but the round-trip check below
# does an [xml] load and a BOM is one of the two things that has silently broken
# a parse in this project (the other is an ANSI em dash).
[IO.File]::WriteAllText($manifestOut, $manifest, (New-Object Text.UTF8Encoding($false)))

# The cross-file contract, checked rather than trusted. A TaskId here that does
# not match packaged::STARTUP_TASK_ID becomes a Store-only autostart bug whose
# log line is indistinguishable from "the WinRT API is unavailable".
#
# REVIEW FIXES 2026-09-05 (scripts) - A CHECK THAT CANNOT PRODUCE A NEGATIVE
# RESULT IS NOT A CHECK (CLAUDE.md's own rule, learned from the ASCII-marker
# and %LOCALAPPDATA% traps).
#
# $rustTaskId started as $null and the comparison below was guarded by
# `if ($rustTaskId -and ...)`, so EVERY way of failing to read it - packaged.rs
# renamed or moved, the constant reformatted across two lines, `&str` changed
# to `&'static str`, the regex simply not matching any more - silently
# satisfied the check and printed "matches packaged.rs". The one thing this
# check exists to catch is a TaskId drift, and its failure mode was to report
# agreement with a value it never read.
#
# So the unreadable cases are failures now, each named separately, because
# "the file is gone" and "the constant no longer looks like that" want
# different fixes. What is being protected is expensive to find later:
# StartupTask::GetAsync answers E_INVALIDARG for an unknown id, which in a log
# is indistinguishable from "the WinRT API is unavailable" - a Store-only
# autostart bug whose symptom points at the wrong subsystem.
$rustTaskId = $null
$packagedRs = Join-Path $Root 'src-tauri\src\packaged.rs'
if (-not (Test-Path $packagedRs)) {
  Die "cannot verify the startupTask id: no $packagedRs. That file holds STARTUP_TASK_ID, the constant this manifest's TaskId must equal; without reading it this script cannot tell agreement from a failed read, and a mismatch becomes a Store-only autostart bug that logs as 'the WinRT API is unavailable'."
}
$m = [regex]::Match((Get-Content $packagedRs -Raw), 'STARTUP_TASK_ID:\s*&(?:''static\s+)?str\s*=\s*"([^"]+)"')
if (-not $m.Success) {
  Die "cannot verify the startupTask id: STARTUP_TASK_ID was not found in $packagedRs. It has been renamed, reformatted, or moved. Fix this regex or the constant - do NOT pack an unverified manifest, because the check silently passing is exactly how a TaskId drift ships."
}
$rustTaskId = $m.Groups[1].Value
$xmlTaskId = ([xml]$manifest).Package.Applications.Application.Extensions.Extension.StartupTask.TaskId
if ([string]::IsNullOrWhiteSpace($xmlTaskId)) {
  Die "cannot verify the startupTask id: AppxManifest.xml has no Package/Applications/Application/Extensions/Extension/StartupTask/TaskId. Either the startupTask extension was removed - in which case a Store install has NO autostart at all and packaged.rs still thinks it does - or the element moved and this path needs updating."
}
if ($rustTaskId -ne $xmlTaskId) {
  Die "TaskId mismatch: AppxManifest.xml says '$xmlTaskId', packaged.rs says '$rustTaskId'. Autostart would fail on every Store install with an error that looks like something else."
}
Say "startupTask id '$xmlTaskId' matches packaged.rs (read from $packagedRs, not assumed)"

$layoutBytes = (Get-ChildItem $Layout -Recurse -File | Measure-Object -Property Length -Sum).Sum
Say ("layout: {0} files, {1:N1} MB at {2}" -f (Get-ChildItem $Layout -Recurse -File).Count, ($layoutBytes / 1MB), $Layout)

# ---------------------------------------------------------------------------
# 6. MakeAppx. The Windows SDK is not a dependency this repo can install, so a
#    missing one reports the command and stops THIS step rather than failing
#    the whole script silently.
# ---------------------------------------------------------------------------
function Find-SdkTool {
  param([string]$Name)
  $bin = 'C:\Program Files (x86)\Windows Kits\10\bin'
  if (-not (Test-Path $bin)) { return $null }
  Get-ChildItem $bin -Directory -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -match '^10\.' } |
    Sort-Object { [version]($_.Name) } -Descending |
    ForEach-Object { Join-Path $_.FullName "x64\$Name" } |
    Where-Object { Test-Path $_ } |
    Select-Object -First 1
}

$makeappx = Find-SdkTool 'makeappx.exe'
if (-not $makeappx) {
  Write-Host ""
  Warn "MakeAppx.exe was not found under 'C:\Program Files (x86)\Windows Kits\10\bin\*\x64\'."
  Write-Host "  The layout is complete and correct at:"
  Write-Host "    $Layout"
  Write-Host "  Install the Windows SDK and re-run to pack it:"
  Write-Host "    winget install --id Microsoft.WindowsSDK --exact"
  Write-Host "  (or the installer at https://developer.microsoft.com/windows/downloads/windows-sdk/)"
  Write-Host ""
  Say "STOPPING HERE. Nothing else in this script can run without MakeAppx."
  # REVIEW FIXES 2026-09-05 (C2) - EXIT 2, NOT 0.
  #
  # This used to exit 0, which reads as "success" to every caller that has no
  # eyes. release.yml ran the script and wrote `built=true` on the next line
  # regardless, so a runner without the SDK produced a step that claimed a
  # package existed; the upload step then failed the whole job on
  # `if-no-files-found: error`, AFTER both installers had been built. A
  # release was lost to a Store-only extra that had merely been skipped.
  #
  # 2 rather than 1 so the reason is legible in a log: 1 is Die's code (a
  # genuine error in the packing) and 2 is only ever this - the toolchain is
  # not installed, the layout above IS complete and correct, and nothing is
  # wrong with the source. release.yml turns this into a ::warning:: and
  # carries on; a developer running `npm run msix` by hand sees the same
  # message they always did, and now their shell knows it did not pack.
  exit 2
}
Say "makeappx: $makeappx"

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$msix = Join-Path $OutDir "Spaceadom_${version3}_${Arch}.msix"
if (Test-Path $msix) { Remove-Item $msix -Force }

& $makeappx pack /d $Layout /p $msix /o
if ($LASTEXITCODE -ne 0) { Die "makeappx pack failed with exit code $LASTEXITCODE" }
$msixItem = Get-Item $msix
Say ("packed: {0} ({1:N0} bytes, {2:N1} MB)" -f $msix, $msixItem.Length, ($msixItem.Length / 1MB))

# ---------------------------------------------------------------------------
# 7. Signing - LOCAL STRUCTURAL TEST ONLY.
#
#    The cert's Subject must equal Identity/Publisher character for character,
#    which is why it is derived from identity.json rather than typed. The .pfx
#    lands in src-tauri/msix/ which is gitignored; it is worthless outside this
#    machine and must not be committed anyway.
# ---------------------------------------------------------------------------
if ($Sign) {
  $signtool = Find-SdkTool 'signtool.exe'
  if (-not $signtool) {
    Warn "SignTool.exe not found - the package is packed but UNSIGNED. Install the Windows SDK to sign it."
  } else {
    $pfx = Join-Path $MsixSrc 'test-signing.pfx'
    $pw  = 'spaceadom-local-test'
    if (-not (Test-Path $pfx)) {
      Say "creating a self-signed test certificate with Subject $($identity.publisher)"
      $cert = New-SelfSignedCertificate `
        -Type Custom -KeyUsage DigitalSignature `
        -CertStoreLocation 'Cert:\CurrentUser\My' `
        -Subject $identity.publisher `
        -FriendlyName 'Spaceadom LOCAL MSIX TEST - not for distribution' `
        -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3', '2.5.29.19={text}')
      $securePw = ConvertTo-SecureString -String $pw -Force -AsPlainText
      Export-PfxCertificate -Cert "Cert:\CurrentUser\My\$($cert.Thumbprint)" -FilePath $pfx -Password $securePw | Out-Null
      Say "test certificate exported to $pfx (gitignored, local only)"
    }
    # /fd SHA256 must match the block-map hash MakeAppx used, which is SHA256
    # by default. A mismatch signs successfully and produces a package Windows
    # then refuses, with an error that names neither tool.
    & $signtool sign /fd SHA256 /a /f $pfx /p $pw $msix
    if ($LASTEXITCODE -ne 0) { Warn "signtool failed with exit code $LASTEXITCODE - the package is packed but unsigned." }
    else { Say "signed with the LOCAL TEST certificate. The Store re-signs on ingestion; this signature never reaches a user." }
  }
} else {
  Say "NOT signed (pass -Sign to sign with a local test certificate)."
  Say "An unsigned package cannot be installed, which is deliberate: see the header."
}

# ---------------------------------------------------------------------------
# 8. Validate. A pack that succeeds is a claim about MakeAppx, not about the
#    package - the same lesson as PROBLEM 127's installer exit codes. So the
#    package is opened again and what comes out is checked.
# ---------------------------------------------------------------------------
if ($SkipValidate) { Say "validation skipped by -SkipValidate"; exit 0 }

$unpacked = Join-Path $MsixSrc 'unpacked'
if (Test-Path $unpacked) { Remove-Item $unpacked -Recurse -Force }
& $makeappx unpack /p $msix /d $unpacked /o
if ($LASTEXITCODE -ne 0) { Die "makeappx unpack failed with exit code $LASTEXITCODE - the package it just wrote cannot be read back." }

$problems = New-Object System.Collections.Generic.List[string]

# 8a. Every file that went in came back out, byte for byte.
$expected = Get-ChildItem $Layout   -Recurse -File | ForEach-Object { $_.FullName.Substring($Layout.Length + 1) }
$actual   = Get-ChildItem $unpacked -Recurse -File | ForEach-Object { $_.FullName.Substring($unpacked.Length + 1) }
foreach ($f in $expected) {
  if ($actual -notcontains $f) { $problems.Add("missing from the package: $f") ; continue }
  $a = (Get-FileHash (Join-Path $Layout $f)   -Algorithm SHA256).Hash
  $b = (Get-FileHash (Join-Path $unpacked $f) -Algorithm SHA256).Hash
  if ($a -ne $b) { $problems.Add("round-trip changed the bytes of $f") }
}
Say "round-trip: $($expected.Count) files in, $($actual.Count) files out (the extras are MakeAppx's AppxBlockMap.xml / AppxSignature.p7x / [Content_Types].xml)"

# 8b. Manifest structure. Deliberately NOT Get-AppxPackageManifest: that cmdlet
#     reads an INSTALLED package, and nothing here may install anything.
[xml]$x = Get-Content (Join-Path $unpacked 'AppxManifest.xml') -Raw
$ns = New-Object System.Xml.XmlNamespaceManager($x.NameTable)
$ns.AddNamespace('d',  'http://schemas.microsoft.com/appx/manifest/foundation/windows10')
$ns.AddNamespace('uap','http://schemas.microsoft.com/appx/manifest/uap/windows10')
$ns.AddNamespace('dt', 'http://schemas.microsoft.com/appx/manifest/desktop/windows10')
$ns.AddNamespace('rc', 'http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities')

function Need($xpath, $what) {
  if (-not $x.SelectSingleNode($xpath, $ns)) { $problems.Add("manifest is missing $what ($xpath)") }
}
Need '/d:Package/d:Identity'                                  'Identity'
Need '/d:Package/d:Properties/d:DisplayName'                  'Properties/DisplayName'
Need '/d:Package/d:Properties/d:PublisherDisplayName'         'Properties/PublisherDisplayName'
Need '/d:Package/d:Properties/d:Logo'                         'Properties/Logo'
Need '/d:Package/d:Dependencies/d:TargetDeviceFamily'         'Dependencies/TargetDeviceFamily'
Need '/d:Package/d:Resources/d:Resource'                      'Resources/Resource'
Need '/d:Package/d:Applications/d:Application'                'Applications/Application'
Need '/d:Package/d:Applications/d:Application/uap:VisualElements' 'uap:VisualElements'
Need "/d:Package/d:Capabilities/rc:Capability[@Name='runFullTrust']" 'the runFullTrust capability'
Need "//dt:Extension[@Category='windows.startupTask']/dt:StartupTask" 'the windows.startupTask extension'

$id = $x.SelectSingleNode('/d:Package/d:Identity', $ns)
if ($id.Version -ne $version4)   { $problems.Add("Identity/Version is '$($id.Version)', expected '$version4'") }
if ($id.Version -notmatch '\.0$'){ $problems.Add("Identity/Version '$($id.Version)' does not end in .0 - the Store reserves the fourth field") }
if ($id.Name      -ne $identity.name)      { $problems.Add("Identity/Name is '$($id.Name)', expected '$($identity.name)'") }
if ($id.Publisher -ne $identity.publisher) { $problems.Add("Identity/Publisher is '$($id.Publisher)', expected '$($identity.publisher)'") }

$app = $x.SelectSingleNode('/d:Package/d:Applications/d:Application', $ns)
if ($app.Executable -ne 'spaceadom.exe')                     { $problems.Add("Application/Executable is '$($app.Executable)'") }
if ($app.EntryPoint -ne 'Windows.FullTrustApplication')      { $problems.Add("Application/EntryPoint is '$($app.EntryPoint)'") }

# 8c. Every logo the manifest names is actually in the package. A missing asset
#     is a Store ingestion failure and a blank tile, and nothing before this
#     step would have caught it.
$ve = $x.SelectSingleNode('/d:Package/d:Applications/d:Application/uap:VisualElements', $ns)
$logoRefs = @($x.SelectSingleNode('/d:Package/d:Properties/d:Logo', $ns).InnerText,
              $ve.Square150x150Logo, $ve.Square44x44Logo)
$tile = $ve.SelectSingleNode('uap:DefaultTile', $ns)
if ($tile) { $logoRefs += @($tile.Wide310x150Logo, $tile.Square71x71Logo, $tile.Square310x310Logo) }
foreach ($ref in ($logoRefs | Where-Object { $_ })) {
  if (-not (Test-Path (Join-Path $unpacked $ref))) { $problems.Add("the manifest names $ref but the package does not contain it") }
}
Say "assets: $(($logoRefs | Where-Object { $_ }).Count) logo references, all resolved"

Write-Host ""
if ($problems.Count -gt 0) {
  Write-Host "build-msix: VALIDATION FAILED" -ForegroundColor Red
  $problems | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
  exit 1
}
Say "VALIDATION PASSED."
Write-Host ""
# Re-stat: $msixItem was taken before signing, and signing APPENDS
# AppxSignature.p7x. Reporting the pre-signature size would be a number that
# never matches the file on disk - small, but this project has been burned by
# exactly that class of stale measurement often enough to spend two lines on it.
$msixItem = Get-Item $msix
Write-Host ("  package : {0}" -f $msix)
Write-Host ("  size    : {0:N0} bytes ({1:N1} MB)" -f $msixItem.Length, ($msixItem.Length / 1MB))
Write-Host ("  identity: {0} {1}" -f $identity.name, $version4)
Write-Host ("  signed  : {0}" -f $(if ($Sign) { 'with a LOCAL TEST certificate (the Store re-signs)' } else { 'no' }))
Write-Host ""
Write-Host "  DO NOT INSTALL THIS ON THE DEV MACHINE. The NSIS copy is already here"
Write-Host "  and two Spaceadoms fight over the spacebar. Test it on a second"
Write-Host "  machine - the recipe is in to-publish-in-microsoft-store/SUBMIT-CHECKLIST.md."
Write-Host ""
