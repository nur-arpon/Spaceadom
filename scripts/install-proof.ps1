# ---------------------------------------------------------------------------
# install-proof.ps1 — the evidence half of install-real.cmd.
#
# It is a separate FILE rather than a `powershell -Command` one-liner inside the
# .cmd because that one-liner has to survive two levels of quoting and a line
# continuation per line, and it did not: a path came out mangled and the check
# silently measured nothing. A script file has one level of quoting.
#
# Run it only through install-real.cmd, which explorer.exe launches from outside
# the agent's MSIX container (PROBLEM 143). Run from the agent shell it will
# read the container's copy and cheerfully agree with itself.
# ---------------------------------------------------------------------------
param(
  [Parameter(Mandatory = $true)][string]$Exe,
  [Parameter(Mandatory = $true)][string]$Root,
  [Parameter(Mandatory = $true)][string]$Out
)

$item = Get-Item $Exe
Add-Content $Out ("version: " + $item.VersionInfo.FileVersion)
Add-Content $Out ("written: " + $item.LastWriteTime)

$run = (Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' `
          -Name Spaceadom -ErrorAction SilentlyContinue).Spaceadom
Add-Content $Out ("Run key: " + $run)

# A RUST marker still works — those strings are not compressed.
$bytes = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($Exe))
Add-Content $Out ("rust marker 'rival install': " + ($bytes -match 'rival install'))

# The frontend chain: the marker is in the bundle, and the exe postdates it.
$assetDir = Join-Path $Root 'dist2\assets'
$bundle = (Get-ChildItem $assetDir -File | ForEach-Object { Get-Content $_.FullName -Raw }) -join "`n"
foreach ($m in 'spec-card', 'sld-tail', 'thrustOn', 'sky-genieIn') {
  Add-Content $Out ("bundle has {0}: {1}" -f $m, ($bundle -match [regex]::Escape($m)))
}

$newest = Get-ChildItem (Join-Path $Root 'dist2') -Recurse -File |
          Sort-Object LastWriteTime -Descending | Select-Object -First 1
Add-Content $Out ("newest dist2 file: " + $newest.LastWriteTime)
Add-Content $Out ("exe is newer than the bundle it embedded: " +
                  ($item.LastWriteTime -gt $newest.LastWriteTime))
