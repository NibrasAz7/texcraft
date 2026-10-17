# fetch-fonts.ps1 — download the Amiri font family (SIL OFL-1.1) into
# fonts/ so the egui UI can render Arabic-script text (crates/ui-egui/
# src/fonts.rs registers every fonts/*.ttf as a glyph fallback).
#
# Amiri is the same family the `arabdoc` template typesets with. The zip
# ships its own OFL.txt license, copied alongside. Font files never get
# committed (fonts/ is gitignored — shared fonts belong in craft-fonts).
#
# Usage:  powershell -ExecutionPolicy Bypass -File scripts/fetch-fonts.ps1
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$fonts = Join-Path $root "fonts"
New-Item -ItemType Directory -Force -Path $fonts | Out-Null

$ver = "1.001"
$url = "https://github.com/aliftype/amiri/releases/download/$ver/Amiri-$ver.zip"
$zip = Join-Path $env:TEMP "amiri-$ver.zip"
$dir = Join-Path $env:TEMP "amiri-$ver"
Write-Host "fetching $url"
Invoke-WebRequest -Uri $url -OutFile $zip
Expand-Archive -Path $zip -DestinationPath $dir -Force

$wanted = @("Amiri-Regular.ttf", "Amiri-Bold.ttf", "Amiri-Italic.ttf", "Amiri-BoldItalic.ttf", "OFL.txt")
Get-ChildItem -Recurse -File -Path $dir | Where-Object { $wanted -contains $_.Name } | ForEach-Object {
    Copy-Item $_.FullName -Destination $fonts
}
Get-ChildItem $fonts | ForEach-Object { Write-Host "$($_.Name) -> $($_.Length) bytes" }
Write-Host "next: WarraqCraft picks fonts/*.ttf up automatically (fonts/ next to the exe and in ancestor dirs; `$WARRAQCRAFT_FONTS or `$CRAFT_FONTS_DIR override)."
