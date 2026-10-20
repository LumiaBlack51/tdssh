[CmdletBinding()]
param(
    [string]$Binary = (Join-Path $PSScriptRoot '..\target\release\tdssh.exe'),
    [string]$Destination = (Join-Path $env:LOCALAPPDATA 'tdssh\bin'),
    [switch]$AddToPath
)
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'On Ubuntu use install.sh; the tdssh executable itself works from pwsh too.' }
$source = (Resolve-Path -LiteralPath $Binary).ProviderPath
New-Item -ItemType Directory -Path $Destination -Force | Out-Null
Copy-Item -LiteralPath $source -Destination (Join-Path $Destination 'tdssh.exe') -Force
if ($AddToPath) {
    $userPath = [Environment]::GetEnvironmentVariable('PATH', 'User')
    if (($userPath -split ';') -notcontains $Destination) {
        [Environment]::SetEnvironmentVariable('PATH', "$userPath;$Destination", 'User')
    }
    $env:PATH += ";$Destination"
}
Write-Output "Installed $(Join-Path $Destination 'tdssh.exe')"
