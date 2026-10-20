[CmdletBinding()]
param([Parameter(Mandatory)][string]$Binary)
$ErrorActionPreference = 'Stop'
$root = Join-Path ([IO.Path]::GetTempPath()) ('tdssh scripts $ % ' + [guid]::NewGuid())
try {
    New-Item -ItemType Directory -Path $root | Out-Null
    $state = Join-Path $root 'private state'
    $exe = (Resolve-Path -LiteralPath $Binary).ProviderPath
    $plan = & "$PSScriptRoot/install-service.ps1" -Binary $exe -StateDir $state -ForceDirect -Interface 'Wi-Fi test' -AtBoot -DryRun | ConvertFrom-Json
    if (-not $plan.AtBoot -or -not $plan.Arguments.Contains('--force-direct')) { throw 'Missing boot/direct options' }
    $startInfo = [Diagnostics.ProcessStartInfo]::new($plan.Execute, ($plan.Arguments + ' --help'))
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($startInfo)
    $stdout = $process.StandardOutput.ReadToEnd()
    $stderr = $process.StandardError.ReadToEnd()
    $process.WaitForExit()
    if ($process.ExitCode -ne 0 -or -not $stdout.Contains('Usage:')) { throw "Invalid native arguments: $stderr" }
    & $exe --state-dir $state init | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'State initialization failed' }
    $acl = Get-Acl -LiteralPath (Join-Path $state 'identity.json')
    if (-not $acl.AreAccessRulesProtected) { throw 'Identity DACL must be protected' }
    $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    $allowed = @($sid, 'S-1-5-18')
    foreach ($rule in $acl.Access) {
        if ($rule.IdentityReference.Translate([Security.Principal.SecurityIdentifier]).Value -notin $allowed) { throw 'Unexpected identity permission' }
    }
    # Native CLI must handle the same special-character path from pwsh.
    $first = & $exe --state-dir $state id
    $second = & $exe --state-dir $state init
    if ($LASTEXITCODE -ne 0 -or $first -ne $second) { throw 'Identity did not persist' }
    Write-Output 'PASS: pwsh task argv, boot/direct plan and private Windows identity ACL'
} finally {
    Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
}
