# fetch-dict.ps1 — download the Hunspell en_US dictionary into dicts/
# for the editor spell-checker (crates/ui-egui/src/spellcheck.rs).
# Source: the LibreOffice/dictionaries repo; en_US carries a permissive
# BSD-style notice of its own — see the repository for details.
#
# Usage:  powershell -ExecutionPolicy Bypass -File scripts/fetch-dict.ps1
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$dicts = Join-Path $root "dicts"
New-Item -ItemType Directory -Force -Path $dicts | Out-Null

$base = "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/en"
foreach ($name in @("en_US.aff", "en_US.dic")) {
    $url = "$base/$name"
    $out = Join-Path $dicts $name
    Write-Host "fetching $url"
    Invoke-WebRequest -Uri $url -OutFile $out
    $bytes = (Get-Item $out).Length
    Write-Host "$name -> $out ($bytes bytes)"
}
Write-Host "next: WarraqCraft picks them up automatically (spellcheck looks for dicts/ next to the exe and in ancestor dirs)."
