param(
  [ValidateRange(1024,65535)][int]$Port = 8787,
  [ValidateRange(1024,65535)][int]$BridgePort = 8788,
  [switch]$Share,
  [switch]$NoOpen
)
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
if ($Port -eq $BridgePort) { throw 'App and bridge ports must differ.' }
if (-not (Get-Command docker -ErrorAction SilentlyContinue)) { throw 'Install Docker with Compose first.' }
docker info *> $null
if ($LASTEXITCODE -ne 0) { throw 'Start your Docker engine first.' }
$env:AGENTWAY_PORT = "$Port"
$env:AGENTWAY_BRIDGE_PORT = "$BridgePort"
docker compose up --build --detach --wait --wait-timeout 120
if ($LASTEXITCODE -ne 0) { docker compose logs --tail 60; throw 'Startup failed. Check logs and port availability.' }
$url = "http://127.0.0.1:$Port"
if ($Share) {
  docker compose --profile share up --detach --force-recreate tunnel
  if ($LASTEXITCODE -ne 0) { throw 'Could not start tunnel.' }
  $tunnelUrl = $null
  for ($attempt = 0; $attempt -lt 30; $attempt++) {
    $logs = docker compose logs --no-color tunnel 2>&1 | Out-String
    $matchesFound = [regex]::Matches($logs, 'https://[a-z0-9-]+\.trycloudflare\.com')
    if ($matchesFound.Count -gt 0) { $tunnelUrl = $matchesFound[-1].Value; break }
    Start-Sleep -Seconds 1
  }
  if (-not $tunnelUrl) { throw 'Tunnel did not start. See docker compose logs tunnel.' }
  Invoke-RestMethod -Method Post -Uri "$url/api/publishing/bridge" -ContentType 'application/json' -Body (@{ url = $tunnelUrl } | ConvertTo-Json) | Out-Null
  Write-Host "Agent endpoint: $tunnelUrl (authenticated; temporary address)"
}
Write-Host "AgentWay is ready: $url"
if (-not $NoOpen) { Start-Process $url }
