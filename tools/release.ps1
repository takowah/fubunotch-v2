# Publishes the installed build (target\fast) as a GitHub release of Decka-tan/kanmuri (private),
# so the other laptop picks it up (tools\update.ps1 runs there at logon and every 3 hours).
#   powershell -NoProfile -ExecutionPolicy Bypass -File tools\release.ps1
# The tag is the time plus the commit, e.g. v2026.11.18-1530-90d1fd3; HEAD must be pushed first.
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$sha = (git -C $repo rev-parse --short HEAD).Trim()
$remote = (git -C $repo ls-remote github main) -split '\s+' | Select-Object -First 1
if (-not $remote.StartsWith($sha)) { throw "HEAD $sha is not on github/main ($remote): push first." }
$exe = Join-Path $repo 'target\fast\kanmuri.exe'
$hook = Join-Path $repo 'target\fast\kanmuri-hook.exe'
foreach ($f in $exe, $hook) { if (-not (Test-Path $f)) { throw "$f is missing: build first (cargo build --profile fast)." } }
$tag = 'v' + (Get-Date -Format 'yyyy.MM.dd-HHmm') + '-' + $sha
$tmp = Join-Path $env:TEMP "kanmuri-release-$sha"
Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $tmp | Out-Null
$zip = Join-Path $tmp 'kanmuri.zip'
Compress-Archive -Path $exe, $hook -DestinationPath $zip -CompressionLevel Optimal
$title = (git -C $repo log -1 --format=%s).Trim()
gh release create $tag $zip --repo Decka-tan/kanmuri --target main --title $tag --notes $title --latest
Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
"released $tag ($([math]::Round((Get-Item $exe).Length / 1MB)) MB exe)"
