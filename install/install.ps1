# Install rt, the review-tools command line, on Windows from its GitHub releases, in
# PowerShell:
#
#   irm https://github.com/gschjetne/rt-cli/releases/latest/download/install.ps1 | iex
#
# The latest release, or RT_VERSION's (such as v0.1.0). Checks the download against the
# release's SHA256SUMS before running it. Puts rt.exe in %LOCALAPPDATA%\Programs\rt and
# adds that to the user's Path, or in RT_INSTALL_DIR, whose Path is left to you. x86-64
# (and arm64, emulated).
& {
  $ErrorActionPreference = "Stop"
  $repo = "https://github.com/gschjetne/rt-cli"
  $base = if ($env:RT_VERSION) { "$repo/releases/download/$($env:RT_VERSION)" } else { "$repo/releases/latest/download" }
  if ($env:PROCESSOR_ARCHITECTURE -notin @("AMD64", "ARM64")) {
    throw "no rt build for $env:PROCESSOR_ARCHITECTURE; build it from source ($repo)"
  }
  $dir = if ($env:RT_INSTALL_DIR) { $env:RT_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "Programs\rt" }
  New-Item -ItemType Directory -Force $dir | Out-Null
  $exe = Join-Path $dir "rt.exe"
  $tmp = Join-Path $dir ".rt.$PID.exe"
  $file = "rt-x86_64-windows.exe"
  Invoke-WebRequest "$base/$file" -OutFile $tmp -UseBasicParsing
  # What the release says it holds, checked before anything runs.
  $sums = (Invoke-WebRequest "$base/SHA256SUMS" -UseBasicParsing).Content
  if ($sums -is [byte[]]) { $sums = [Text.Encoding]::UTF8.GetString($sums) }
  $expected = ($sums -split "`n" | ForEach-Object { $f = $_.Trim() -split '\s+'; if ($f[1] -eq $file) { $f[0] } } | Select-Object -First 1)
  $actual = (Get-FileHash -Algorithm SHA256 $tmp).Hash.ToLower()
  if (-not $expected -or $expected -ne $actual) {
    Remove-Item -Force $tmp
    throw "the download of $file does not match the release's SHA256SUMS; not installed"
  }
  try {
    Move-Item -Force $tmp $exe
  } catch {
    Remove-Item -Force $tmp
    throw "$exe is in use: stop what runs it and try again"
  }
  Write-Host "installed $(& $exe --version) to $exe"
  $path = [Environment]::GetEnvironmentVariable("Path", "User")
  if ((";$path;" -notlike "*;$dir;*") -and (";$env:Path;" -notlike "*;$dir;*")) {
    if ($env:RT_INSTALL_DIR) {
      Write-Host "add $dir to Path"
    } else {
      [Environment]::SetEnvironmentVariable("Path", "$path;$dir", "User")
      Write-Host "added $dir to your Path; open a new terminal to use rt"
    }
  }
}
