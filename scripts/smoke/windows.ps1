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
  [string]$Artifacts = '',
  # on: msiexec runs with MSIRESTARTMANAGERCONTROL=Disable and REBOOT=ReallySuppress. off: without them.
  [ValidateSet('on', 'off')][string]$MsiFlags = 'on'
)
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path

function Step($m) { Write-Host ("smoke: [{0:HH:mm:ss}] {1}" -f (Get-Date), $m) }
function Fail($m) { [Console]::Error.WriteLine("smoke: FAILED: $m"); exit 1 }

# Starts a program and waits for it, at most $seconds. A program that hangs is killed with its
# children and the job fails with the end of the log (if there is one).
function Run-Limited($file, $argList, $seconds, $what, $logPath) {
  $p = Start-Process -FilePath $file -ArgumentList $argList -PassThru
  $null = $p.Handle # keeps the exit code readable
  if (-not $p.WaitForExit($seconds * 1000)) {
    & taskkill /PID $p.Id /T /F 2>&1 | Out-Null
    if ($logPath -and (Test-Path $logPath)) {
      Write-Host "smoke: end of ${logPath}:"
      Get-Content -Path $logPath -Tail 60 -ErrorAction SilentlyContinue | ForEach-Object { Write-Host $_ }
    }
    Fail "$what did not end in $seconds seconds"
  }
  return $p.ExitCode
}

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
        $loc = if ($e.InstallLocation) { $e.InstallLocation.Trim('"').TrimEnd('\') } else { '' }
        if ($loc -and (Test-Path (Join-Path $loc 'kadakas-launcher.exe'))) { return $loc }
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

# Logs that stay when the step is cut short: a transcript, and a watchdog that, after 9 minutes
# (before the 12 minute limit of the step), lists the processes, ends msiexec and keeps what it found.
$logDir = if ($Artifacts) { $Artifacts } else { $work }
try { Start-Transcript -Path (Join-Path $logDir "transcript-$Kind.txt") -Force | Out-Null } catch { Write-Host "smoke: no transcript: $_" }
$watchdog = Start-Job -ArgumentList $logDir, $Kind -ScriptBlock {
  param($dir, $kind)
  Start-Sleep -Seconds 540
  $out = Join-Path $dir "watchdog-$kind.txt"
  "The smoke test of $kind did not end in 9 minutes. $(Get-Date -Format o)" | Out-File $out
  Get-Process | Sort-Object CPU -Descending | Select-Object -First 40 Id, ProcessName, CPU, StartTime, Path | Format-Table -AutoSize | Out-String -Width 250 | Out-File $out -Append
  Get-CimInstance Win32_Process | Where-Object { $_.Name -match 'msiexec|kadakas|setup|uninstall' } | Select-Object ProcessId, ParentProcessId, Name, CommandLine | Format-List | Out-String -Width 250 | Out-File $out -Append
  Get-EventLog -LogName Application -Newest 30 -ErrorAction SilentlyContinue | Where-Object { $_.Source -match 'MsiInstaller|Restart' } | Format-List TimeGenerated, Source, EventID, Message | Out-String -Width 250 | Out-File $out -Append
  Get-Process msiexec -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
}

if ($null -ne (Find-InstallDir)) { Fail "Kadakas is installed already on this machine: $(Find-InstallDir)" }

Step "install $Package"
$msiProps = if ($MsiFlags -eq 'on') { @('REBOOT=ReallySuppress', 'MSIRESTARTMANAGERCONTROL=Disable') } else { @() }
Step "msiexec properties: $($msiProps -join ' ') (flags $MsiFlags)"
# The MSI log goes straight to the artifacts folder: it stays there if the step is cut short.
$log = Join-Path $logDir "install-$Kind.log"
if ($Kind -eq 'msi') {
  $code = Run-Limited msiexec.exe (@('/i', "`"$Package`"", '/qn', '/norestart', '/l*v', "`"$log`"") + $msiProps) 300 'msiexec /i' $log
} else {
  $code = Run-Limited $Package @('/S') 300 'the installer' $null
}
# 3010 (a restart is needed) is a failure too: a fresh install on a clean runner must not need one.
if ($code -ne 0) { Fail "the installer ended with $code" }

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
  # An MSI shortcut is advertised: to read its target can make Windows Installer act. So the
  # target is read for NSIS only, in a job with a time limit (an MSI run hung after the install).
  if ($Kind -eq 'nsis') {
    $lnk = $links[0].FullName
    $job = Start-Job -ScriptBlock { param($f) (New-Object -ComObject WScript.Shell).CreateShortcut($f).TargetPath } -ArgumentList $lnk
    if (Wait-Job $job -Timeout 30) {
      $target = Receive-Job $job
      if ($target -notlike '*kadakas-launcher.exe') { Write-Host "smoke: WARNING: the Start menu entry shows the target '$target', not the launcher" }
    } else {
      Write-Host 'smoke: WARNING: the Start menu entry target was not read in 30 seconds'
      Stop-Job $job
    }
    Remove-Job $job -Force
  }
  Step "Start menu entry: $($links[0].FullName)"
  Step 'kadakas --version'
  $ver = & (Join-Path $dir 'kadakas.exe') --version
  if (-not $ver) { Fail 'kadakas --version prints nothing' }

  Step 'the starts (smoke_run.py)'
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
  $ulog = Join-Path $logDir "uninstall-$Kind.log"
  $code = Run-Limited msiexec.exe (@('/x', "`"$Package`"", '/qn', '/norestart', '/l*v', "`"$ulog`"") + $msiProps) 300 'msiexec /x' $ulog
  if ($code -ne 0) { Fail "msiexec /x ended with $code" }
} else {
  $uninstaller = Get-ChildItem -Path $dir -Filter 'uninstall*.exe' -ErrorAction SilentlyContinue | Select-Object -First 1
  if (-not $uninstaller) { Fail "no uninstaller in $dir" }
  # _?= keeps the uninstaller from copying itself away, so that the wait is for the real work.
  $code = Run-Limited $uninstaller.FullName @('/S', "_?=$dir") 300 'the uninstaller' $null
  if ($code -ne 0) { Fail "the uninstaller ended with $code" }
}
foreach ($f in @('kadakas-launcher.exe', 'kadakas-app.exe', 'kadakas-modeler.exe', 'kadakas.exe')) {
  if (-not (Wait-Gone (Join-Path $dir $f) 60)) { Fail "still there after the uninstall: $dir\$f" }
}
if (@(Find-Shortcuts).Count -ne 0) { Fail "a Start menu entry stays after the uninstall: $((Find-Shortcuts)[0].FullName)" }
# The desktop shortcuts (the MSI makes one on the public desktop) go with the uninstall too.
foreach ($d in @("$env:PUBLIC\Desktop", [Environment]::GetFolderPath('Desktop'))) {
  if ($d -and (Test-Path $d)) {
    $left = Get-ChildItem -Path $d -Filter 'Kadakas*.lnk' -ErrorAction SilentlyContinue
    if ($left) { Fail "a desktop shortcut stays after the uninstall: $($left[0].FullName)" }
  }
}
Stop-Job $watchdog -ErrorAction SilentlyContinue
Remove-Job $watchdog -Force -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
Step "OK ($Kind)"
try { Stop-Transcript | Out-Null } catch { }
