# ---------------------------------------------------------------------------
# write-updater-manifests.ps1 - the two updater manifests (PROBLEM 245).
#
# Spaceadom ships TWO installers of different kinds, and an install must only
# ever be updated by the kind that made it (a per-user NSIS copy fed the .msi
# is PROBLEM 129 or PROBLEM 244). tauri-action writes ONE latest.json and picks
# one installer for it. This script writes BOTH, from the .sig files
# `tauri build` produced beside the installers, and the app chooses which one
# to read at runtime (src-tauri/src/updater.rs::plan):
#
#   latest.json      -> the NSIS setup.exe   (keys windows-x86_64, windows-x86_64-nsis)
#   latest-msi.json  -> the .msi             (keys windows-x86_64, windows-x86_64-msi)
#
# Each manifest carries the generic `windows-x86_64` key AND the
# installer-specific one. tauri-plugin-updater looks up
# `windows-x86_64-<its own bundle type>` first and falls back to the generic
# key, so whichever manifest the app was pointed at, it can only ever find the
# installer that manifest is about.
#
# Used by BOTH release.yml (BaseUrl = the GitHub release's download URL) and
# the local end-to-end proof (BaseUrl = a localhost HTTPS server). One script,
# so the file the proof exercised is the file CI publishes.
#
# ASCII ONLY. Windows PowerShell 5.1 reads a BOM-less file as ANSI, and an em
# dash inside a string broke the parse on 2026-09-04 (pwsh 7 had been fine):
# the proof server then held a new installer with an old signature, and the
# app correctly refused it. CI runs pwsh, but a script that parses under one
# shell only is a trap.
#
# FAILS LOUDLY on anything missing. A manifest that silently points at nothing
# is an update channel that silently stops.
# ---------------------------------------------------------------------------
param(
  # e.g. src-tauri/target/release/bundle
  [Parameter(Mandatory = $true)][string]$BundleDir,
  # e.g. https://github.com/nur-arpon/Spaceadom/releases/download/v1.0.100
  #  or  https://127.0.0.1:8765
  [Parameter(Mandatory = $true)][string]$BaseUrl,
  [Parameter(Mandatory = $true)][string]$Version,
  [Parameter(Mandatory = $true)][string]$OutDir,
  # Free text for the manifest's `notes` field.
  [string]$Notes = ""
)
$ErrorActionPreference = "Stop"

$nsis = Join-Path $BundleDir "nsis\Spaceadom_${Version}_x64-setup.exe"
$msi  = Join-Path $BundleDir "msi\Spaceadom_${Version}_x64_en-US.msi"

function Read-Sig([string]$file) {
  if (-not (Test-Path $file)) { throw "installer missing: $file" }
  $sig = "$file.sig"
  if (-not (Test-Path $sig)) {
    throw "signature missing: $sig - was the build run with TAURI_SIGNING_PRIVATE_KEY set and bundle.createUpdaterArtifacts = true?"
  }
  $text = (Get-Content $sig -Raw).Trim()
  if ($text.Length -lt 100) { throw "signature file looks empty: $sig" }
  return $text
}

function Write-Manifest([string]$outName, [string]$installer, [string]$bundleKey) {
  $sig  = Read-Sig $installer
  $name = Split-Path $installer -Leaf
  $url  = "$($BaseUrl.TrimEnd('/'))/$name"
  $entry = [ordered]@{ signature = $sig; url = $url }
  $manifest = [ordered]@{
    version   = $Version
    notes     = $Notes
    pub_date  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    platforms = [ordered]@{
      "windows-x86_64"            = $entry
      "windows-x86_64-$bundleKey" = $entry
    }
  }
  $path = Join-Path $OutDir $outName
  $json = $manifest | ConvertTo-Json -Depth 5
  [IO.File]::WriteAllText($path, $json, [Text.UTF8Encoding]::new($false))
  Write-Host "wrote $path -> $url ($((Get-Item $installer).Length) bytes, sig $($sig.Length) chars)"
}

New-Item -ItemType Directory -Force $OutDir | Out-Null
Write-Manifest "latest.json"     $nsis "nsis"
Write-Manifest "latest-msi.json" $msi  "msi"

# Cross-check: the two manifests must never point at the same file, and each
# must name the kind it claims. This is the whole point of the script.
$a = (Get-Content (Join-Path $OutDir "latest.json") -Raw | ConvertFrom-Json).platforms."windows-x86_64".url
$b = (Get-Content (Join-Path $OutDir "latest-msi.json") -Raw | ConvertFrom-Json).platforms."windows-x86_64".url
if ($a -notlike "*-setup.exe") { throw "latest.json must point at the setup.exe, got $a" }
if ($b -notlike "*.msi")       { throw "latest-msi.json must point at the .msi, got $b" }
if ($a -eq $b)                 { throw "both manifests point at $a" }
Write-Host "manifests OK: latest.json -> setup.exe, latest-msi.json -> .msi"
