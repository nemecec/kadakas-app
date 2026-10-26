# Smoke test of a Windows installer (PLAT-28): install it silently, check the files and the menu
# entry, start it the first time, start it again, ping the app, uninstall it.
#
#   pwsh scripts/smoke/windows.ps1 -Kind msi|nsis -Package PATH -Version VERSION [-Artifacts DIR]
#
# VERSION is the version of the app (0.1.0-beta.1), not the version in the MSI (0.1.0-10001).
# The script installs on the machine: run it on a runner or in a virtual machine. It needs Python 3
# (the checks are in scripts/smoke/smoke_run.py).
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][ValidateSet('msi', 'nsis')][string]$Kind,
  [Parameter(Mandatory = $true)][string]$Package,
  [Parameter(Mandatory = $true)][string]$Version,
  [string]$Artifacts = ''
)
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path

function Step($m) { Write-Host "smoke: $m" }
function Fail($m) { [Console]::Error.WriteLine("smoke: FAILED: $m"); exit 1 }

# The installed copy: the folder from the uninstall entry of the registry, or a known place.
function Find-InstallDir {
  $keys = @(
    'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*',
    'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*',
    'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*'
  )
  foreach ($k in $keys) {
    foreach ($e in (Get-ItemProperty -Path $k -ErrorAction SilentlyContinue)) {
      if ($e.DisplayName -and $e.DisplayName -like 'Kadakas*') {
        if ($e.InstallLocation -and (Test-Path (Join-Path $e.InstallLocation 'kadakas-launcher.exe'))) { return $e.InstallLocation.TrimEnd('\') }
        if ($e.UninstallString) {
          $dir = Split-Path -Parent ($e.UninstallString -replace '"', '' -replace ' /.*$', '')
          if (Test-Path (Join-Path $dir 'kadakas-launcher.exe')) { return $dir }
        }
      }
    }
  }
  foreach ($d in @("$env:ProgramFiles\Kadakas", "$env:LOCALAPPDATA\Kadakas", "$env:LOCALAPPDATA\Programs\Kadakas")) {
    if (Test-Path (Join-Path $d 'kadakas-launcher.exe')) { return $d }
  }
  return $null
}

function Find-Shortcuts {
  $roots = @(
    "$env:ProgramData\Microsoft\Windows\Start Menu\Programs",
    "$env:APPDATA\Microsoft\Windows\Start Menu\Programs"
  )
  $found = @()
  foreach ($r in $roots) {
    if (Test-Path $r) { $found += Get-ChildItem -Path $r -Recurse -Filter '*.lnk' -ErrorAction SilentlyContinue | Where-Object { $_.Name -like 'Kadakas*' } }
  }
  return $found
}

function Wait-Gone($path, $seconds) {
  $deadline = (Get-Date).AddSeconds($seconds)
  while ((Test-Path $path) -and ((Get-Date) -lt $deadline)) { Start-Sleep -Seconds 1 }
  return -not (Test-Path $path)
}

if (-not (Test-Path $Package)) { Fail "no package $Package" }
$Package = (Resolve-Path $Package).Path
$work = Join-Path ([System.IO.Path]::GetTempPath()) ("kadakas-smoke-" + [guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Path $work | Out-Null
if ($Artifacts) { New-Item -ItemType Directory -Force -Path $Artifacts | Out-Null; $Artifacts = (Resolve-Path $Artifacts).Path }

if ($null -ne (Find-InstallDir)) { Fail "Kadakas is installed already on this machine: $(Find-InstallDir)" }

Step "install $Package"
$log = Join-Path $work 'install.log'
if ($Kind -eq 'msi') {
  $p = Start-Process -FilePath msiexec.exe -ArgumentList @('/i', "`"$Package`"", '/qn', '/norestart', '/l*v', "`"$log`"") -Wait -PassThru
} else {
  $p = Start-Process -FilePath $Package -ArgumentList @('/S') -Wait -PassThru
}
if ($Artifacts -and (Test-Path $log)) { Copy-Item $log (Join-Path $Artifacts "install-$Kind.log") -ErrorAction SilentlyContinue }
if ($p.ExitCode -ne 0 -and $p.ExitCode -ne 3010) { Fail "the installer ended with $($p.ExitCode)" }

# An NSIS installer can finish in a child process: wait for the files.
$dir = $null
for ($i = 0; $i -lt 60 -and -not $dir; $i++) { $dir = Find-InstallDir; if (-not $dir) { Start-Sleep -Seconds 1 } }
if (-not $dir) { Fail 'the installer left no kadakas-launcher.exe (no uninstall entry, no known folder)' }
Step "installed in $dir"

try {
  foreach ($f in @('kadakas-launcher.exe', 'kadakas-app.exe', 'kadakas-modeler.exe', 'kadakas.exe')) {
    if (-not (Test-Path (Join-Path $dir $f))) { Fail "program is missing: $dir\$f" }
  }
  foreach ($f in @('LICENSE-APACHE', 'LICENSE-MIT', 'THIRD_PARTY_NOTICES.txt')) {
    if (-not (Get-ChildItem -Path $dir -Recurse -Filter $f -ErrorAction SilentlyContinue)) { Fail "the installer does not ship $f" }
  }
  $links = @(Find-Shortcuts)
  if ($links.Count -eq 0) { Fail 'no Start menu entry (Kadakas*.lnk)' }
  $shell = New-Object -ComObject WScript.Shell
  $target = $shell.CreateShortcut($links[0].FullName).TargetPath
  # An MSI shortcut is advertised: Windows may show no plain target for it. So this is a warning.
  if ($target -notlike '*kadakas-launcher.exe') { Write-Host "smoke: WARNING: the Start menu entry shows the target '$target', not the launcher" }
  Step "Start menu entry: $($links[0].FullName)"
  $ver = & (Join-Path $dir 'kadakas.exe') --version
  if (-not $ver) { Fail 'kadakas --version prints nothing' }

  $py = if (Get-Command python -ErrorAction SilentlyContinue) { 'python' } else { 'python3' }
  $shot = "powershell -NoProfile -ExecutionPolicy Bypass -File `"$here\screenshot.ps1`" {out}"
  $runArgs = @('-I', (Join-Path $here 'smoke_run.py'), 'run', '--launcher', (Join-Path $dir 'kadakas-launcher.exe'),
    '--version', $Version, '--work', (Join-Path $work 'run'), '--screenshot-cmd', $shot)
  if ($Artifacts) { $runArgs += @('--artifacts', $Artifacts) }
  if ($env:SMOKE_RUN_ARGS) { $runArgs += $env:SMOKE_RUN_ARGS -split ' ' }
  & $py @runArgs
  if ($LASTEXITCODE -ne 0) { Fail "the starts failed ($LASTEXITCODE)" }
}
finally {
  # Free the files before the uninstaller runs.
  Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path -like "$dir\*" } | Stop-Process -Force -ErrorAction SilentlyContinue
}

Step 'uninstall'
if ($Kind -eq 'msi') {
  $p = Start-Process -FilePath msiexec.exe -ArgumentList @('/x', "`"$Package`"", '/qn', '/norestart') -Wait -PassThru
  if ($p.ExitCode -ne 0 -and $p.ExitCode -ne 3010) { Fail "msiexec /x ended with $($p.ExitCode)" }
} else {
  $uninstaller = Get-ChildItem -Path $dir -Filter 'uninstall*.exe' -ErrorAction SilentlyContinue | Select-Object -First 1
  if (-not $uninstaller) { Fail "no uninstaller in $dir" }
  # _?= keeps the uninstaller from copying itself away, so that -Wait waits for it.
  $p = Start-Process -FilePath $uninstaller.FullName -ArgumentList @('/S', "_?=$dir") -Wait -PassThru
  if ($p.ExitCode -ne 0) { Fail "the uninstaller ended with $($p.ExitCode)" }
}
foreach ($f in @('kadakas-launcher.exe', 'kadakas-app.exe', 'kadakas-modeler.exe', 'kadakas.exe')) {
  if (-not (Wait-Gone (Join-Path $dir $f) 60)) { Fail "still there after the uninstall: $dir\$f" }
}
if (@(Find-Shortcuts).Count -ne 0) { Fail "a Start menu entry stays after the uninstall: $((Find-Shortcuts)[0].FullName)" }
Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
Step "OK ($Kind)"
