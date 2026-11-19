# Sends sample activities to a running FubuNotch v2 through its local webhook.
#   .\scripts\try.ps1            alert + progress + ask
#   .\scripts\try.ps1 -Clear     ends them
param([switch]$Clear)
$ErrorActionPreference = 'Stop'
$token = (Get-Content "$env:APPDATA\FubuNotch v2\token" -Raw).Trim()
$headers = @{ Authorization = "Bearer $token" }
$base = 'http://127.0.0.1:47821'

function Send($json) {
  $bytes = [Text.Encoding]::UTF8.GetBytes($json)
  Invoke-RestMethod "$base/activity" -Method Post -ContentType 'application/json; charset=utf-8' -Headers $headers -Body $bytes | Out-Null
}

if ($Clear) {
  foreach ($id in 'demo-build', 'demo-msg', 'demo-ask') {
    Invoke-RestMethod "$base/activity/$id" -Method Delete -Headers $headers | Out-Null
  }
  return
}

Send '{"id":"demo-build","source":"script","kind":"progress","title":"Building FubuNotch v2","progress":0.62}'
Send '{"id":"demo-msg","source":"discord","kind":"alert","title":"Mika","body":"See you at seven?","ttl_ms":null}'
Send '{"id":"demo-ask","source":"claude-code","kind":"ask","title":"Allow: npm run build?","body":"Claude Code waits for your answer"}'
