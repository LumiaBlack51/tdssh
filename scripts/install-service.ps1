[CmdletBinding()]
param(
    [string]$Binary = (Join-Path $env:LOCALAPPDATA 'tdssh\bin\tdssh.exe'),
    [string]$StateDir = (Join-Path $env:LOCALAPPDATA 'tdssh'),
    [ValidateSet('serve', 'forward')][string]$Role = 'serve',
    [string]$Peer,
    [string]$Listen = '127.0.0.1:2222',
    [string]$Target = '127.0.0.1:22',
    [string]$TicketFile,
    [string[]]$Relay = @(),
    [switch]$ForceDirect,
    [string]$Interface,
    # S4U runs as the current user at boot without storing a password.
    # Registering this boot task requires an elevated PowerShell.
    [switch]$AtBoot,
    [switch]$DryRun,
    [switch]$Uninstall
)
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'Use install-service.sh on Ubuntu, including when running pwsh.' }
$taskName = 'tdssh-' + [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
if ($Uninstall) {
    if ($DryRun) { Write-Output "Would remove task $taskName"; return }
    Stop-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
    Unregister-ScheduledTask -TaskName $taskName -Confirm:$false
    return
}
$exe = (Resolve-Path -LiteralPath $Binary).ProviderPath
$state = [IO.Path]::GetFullPath($StateDir)
if ($Role -eq 'forward' -and -not $Peer) { throw '-Peer is required for forward.' }
$arguments = @('--state-dir', $state)
if ($ForceDirect -or $Interface) { $arguments += '--force-direct' }
if ($Interface) { $arguments += @('--interface', $Interface) }
foreach ($item in $Relay) { $arguments += @('--relay', $item) }
$arguments += $Role
if ($Role -eq 'serve') { $arguments += @('--target', $Target) }
else {
    $arguments += @('--peer', $Peer, '--listen', $Listen)
    if ($TicketFile) { $arguments += @('--ticket-file', (Resolve-Path -LiteralPath $TicketFile).ProviderPath) }
}
# Quote according to CommandLineToArgvW, including trailing backslashes.
function ConvertTo-NativeArgument([string]$Value) {
    if ($Value.Contains("`n") -or $Value.Contains("`r") -or $Value.Contains([char]0)) { throw 'Invalid argument' }
    '"' + [regex]::Replace([regex]::Replace($Value, '(\\*)"', '$1$1\"'), '(\\+)$', '$1$1') + '"'
}
$argumentLine = ($arguments | ForEach-Object { ConvertTo-NativeArgument $_ }) -join ' '
if ($DryRun) {
    [ordered]@{ TaskName=$taskName; Execute=$exe; Arguments=$argumentLine; AtBoot=[bool]$AtBoot; User=[Security.Principal.WindowsIdentity]::GetCurrent().Name } | ConvertTo-Json
    return
}
$user = [Security.Principal.WindowsIdentity]::GetCurrent().Name
if ($AtBoot) {
    $principalIdentity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principalCheck = [Security.Principal.WindowsPrincipal]::new($principalIdentity)
    if (-not $principalCheck.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw '-AtBoot requires an elevated pwsh to register an S4U task for the current user.'
    }
    $principal = New-ScheduledTaskPrincipal -UserId $user -LogonType S4U -RunLevel Limited
    $trigger = New-ScheduledTaskTrigger -AtStartup
} else {
    $principal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Limited
    $trigger = New-ScheduledTaskTrigger -AtLogOn -User $user
}
& $exe --state-dir $state init
if ($LASTEXITCODE -ne 0) { throw 'Identity initialization failed' }
$action = New-ScheduledTaskAction -Execute $exe -Argument $argumentLine
$settings = New-ScheduledTaskSettingsSet -StartWhenAvailable -RestartCount 999 -RestartInterval (New-TimeSpan -Minutes 1) -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
Register-ScheduledTask -TaskName $taskName -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force | Out-Null
Start-ScheduledTask -TaskName $taskName
Get-ScheduledTaskInfo -TaskName $taskName
