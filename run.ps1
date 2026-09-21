param([ValidateRange(1024,65535)][int]$Port = 8787, [switch]$NoOpen)
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
if (-not (Get-Command docker -ErrorAction SilentlyContinue)) { throw 'Install Docker with Compose first.' }
docker info *> $null
if ($LASTEXITCODE -ne 0) { throw 'Start your Docker engine first.' }
$env:AGENTWAY_PORT = "$Port"
docker compose up --build --detach --wait --wait-timeout 120
if ($LASTEXITCODE -ne 0) { docker compose logs --tail 60; throw 'Startup failed. Check logs and port availability.' }
$url = "http://127.0.0.1:$Port"
Write-Host "AgentWay is ready: $url"
if (-not $NoOpen) { Start-Process $url }
