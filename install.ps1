#!/usr/bin/env pwsh

param(
  [string]$RepoPath = $(if ($env:AGENT_SKILLS_REPO_PATH) { $env:AGENT_SKILLS_REPO_PATH } else { "$HOME/src/plugins" }),
  [string]$RepoUrl = $(if ($env:AGENT_SKILLS_REPO_URL) { $env:AGENT_SKILLS_REPO_URL } else { "https://github.com/joshyorko/plugins.git" }),
  [string]$CodexHome = $(if ($env:AGENT_SKILLS_CODEX_HOME) { $env:AGENT_SKILLS_CODEX_HOME } else { "$HOME/.codex" }),
  [string]$MarketplaceName = $(if ($env:AGENT_SKILLS_MARKETPLACE_NAME) { $env:AGENT_SKILLS_MARKETPLACE_NAME } else { "plugins" }),
  [ValidateSet("auto", "link", "copy")] [string]$SkillMode = $(if ($env:AGENT_SKILLS_SKILL_MODE) { $env:AGENT_SKILLS_SKILL_MODE } else { "auto" }),
  [ValidateSet("auto", "git", "archive")] [string]$InstallMethod = $(if ($env:AGENT_SKILLS_INSTALL_METHOD) { $env:AGENT_SKILLS_INSTALL_METHOD } else { "auto" }),
  [string]$Ref = $(if ($env:AGENT_SKILLS_REF) { $env:AGENT_SKILLS_REF } else { "" }),
  [switch]$Force,
  [switch]$Help
)

$ErrorActionPreference = "Stop"

$RepoOwner = "joshyorko"
$RepoName = "plugins"
$ApiBase = "https://api.github.com/repos/$RepoOwner/$RepoName"

function Show-Usage {
  @"
Usage: install.ps1 [-RepoPath PATH] [-RepoUrl URL] [-CodexHome PATH] [-MarketplaceName NAME] [-SkillMode auto|link|copy] [-InstallMethod auto|git|archive] [-Ref REF] [-Force]

Remote bootstrap for the Agent Skills marketplace.

Examples:
  irm https://raw.githubusercontent.com/joshyorko/plugins/main/install.ps1 | iex
  `$env:AGENT_SKILLS_REF='v1.2.3'; irm https://raw.githubusercontent.com/joshyorko/plugins/main/install.ps1 | iex
"@
}

if ($Help) {
  Show-Usage
  exit 0
}

function Log { param([string]$Message) Write-Host "[codex-bootstrap] $Message" }
function Warn { param([string]$Message) Write-Warning "[codex-bootstrap] $Message" }

function Normalize-Path {
  param([string]$Path)
  $resolved = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Path)
  return [IO.Path]::GetFullPath($resolved)
}

function Ensure-Directory {
  param([string]$Path)
  if (-not (Test-Path -LiteralPath $Path)) {
    New-Item -ItemType Directory -Path $Path -Force | Out-Null
  }
}

function Get-LatestReleaseTag {
  try {
    $response = Invoke-RestMethod -Uri "$ApiBase/releases/latest" -Headers @{ Accept = "application/vnd.github+json" }
    if (-not $response.tag_name) {
      return $null
    }
    return [string]$response.tag_name
  }
  catch {
    return $null
  }
}

function Choose-InstallMethod {
  if ($InstallMethod -ne "auto") {
    return $InstallMethod
  }

  if (Test-Path -LiteralPath (Join-Path $RepoPath ".git")) {
    return "git"
  }

  if (Test-Path -LiteralPath (Join-Path $RepoPath "marketplaces/catalog.json")) {
    return "archive"
  }

  if (Get-Command git -ErrorAction SilentlyContinue) {
    return "git"
  }

  return "archive"
}

function Invoke-PowerShellFile {
  param([string[]]$ArgumentList)

  $pwshCmd = Get-Command pwsh -ErrorAction SilentlyContinue
  if ($pwshCmd) {
    & $pwshCmd.Source -NoLogo -NoProfile @ArgumentList
    if ($LASTEXITCODE -ne 0) {
      throw "PowerShell host $($pwshCmd.Source) exited with code $LASTEXITCODE"
    }
    return
  }

  $powershellCmd = Get-Command powershell -ErrorAction SilentlyContinue
  if ($powershellCmd) {
    & $powershellCmd.Source -NoLogo -NoProfile -ExecutionPolicy Bypass @ArgumentList
    if ($LASTEXITCODE -ne 0) {
      throw "PowerShell host $($powershellCmd.Source) exited with code $LASTEXITCODE"
    }
    return
  }

  $targetDescription = "run the installer"
  $fileArgIndex = [Array]::IndexOf($ArgumentList, "-File")
  if ($fileArgIndex -ge 0 -and $ArgumentList.Count -gt ($fileArgIndex + 1)) {
    $scriptPath = [string]$ArgumentList[$fileArgIndex + 1]
    if (-not [string]::IsNullOrWhiteSpace($scriptPath)) {
      $targetDescription = "run $scriptPath"
    }
  }
  throw "Unable to find a PowerShell host to $targetDescription. Install PowerShell and try again."
}

function Invoke-NativeCommand {
  param(
    [string]$Description,
    [scriptblock]$Command
  )

  & $Command
  if ($LASTEXITCODE -ne 0) {
    throw "$Description failed with exit code $LASTEXITCODE"
  }
}

function Install-FromGit {
  $gitCmd = Get-Command git -ErrorAction SilentlyContinue
  if (-not $gitCmd) {
    throw "git is required for install method 'git'"
  }

  if (Test-Path -LiteralPath (Join-Path $RepoPath ".git")) {
    Log "Updating existing git checkout at $RepoPath"
    Invoke-NativeCommand "git fetch" { & git -C $RepoPath fetch --tags --prune | Out-Null }
  }
  elseif (Test-Path -LiteralPath $RepoPath) {
    throw "Target path $RepoPath exists but is not a git checkout. Use -InstallMethod archive or remove it first."
  }
  else {
    Ensure-Directory (Split-Path -Parent $RepoPath)
    Log "Cloning $RepoUrl into $RepoPath"
    Invoke-NativeCommand "git clone" { & git clone $RepoUrl $RepoPath | Out-Null }
  }

  if ($Ref) {
    & git -C $RepoPath rev-parse --verify --quiet "$Ref^{commit}" | Out-Null
    if ($LASTEXITCODE -eq 0) {
      Invoke-NativeCommand "git checkout $Ref" { & git -C $RepoPath checkout --force $Ref | Out-Null }
    }
    else {
      & git -C $RepoPath fetch origin $Ref --depth=1 | Out-Null
      if ($LASTEXITCODE -eq 0) {
        Invoke-NativeCommand "git checkout FETCH_HEAD" { & git -C $RepoPath checkout --force FETCH_HEAD | Out-Null }
      }
      else {
        throw "Unable to resolve ref $Ref"
      }
    }
  }
  else {
    $branch = (& git -C $RepoPath symbolic-ref --short -q HEAD 2>$null)
    if ($LASTEXITCODE -eq 0 -and $branch) {
      Invoke-NativeCommand "git pull --ff-only" { & git -C $RepoPath pull --ff-only | Out-Null }
    }
    else {
      Log "Existing checkout is detached; skipping git pull --ff-only"
    }
  }

  $resolvedRef = $Ref
  if (-not $resolvedRef) {
    $tagsAtHead = @(& git -C $RepoPath tag --points-at HEAD 2>$null)
    $tag = $tagsAtHead | Select-Object -First 1
    if ($LASTEXITCODE -eq 0 -and $tag) {
      $resolvedRef = $tag.Trim()
    }
    else {
      $branch = (& git -C $RepoPath symbolic-ref --short -q HEAD 2>$null)
      if ($LASTEXITCODE -eq 0 -and $branch) {
        $resolvedRef = $branch.Trim()
      }
      else {
        $resolvedRef = (& git -C $RepoPath rev-parse --short HEAD).Trim()
      }
    }
  }

  $installerArgs = @(
    "-File", (Join-Path $RepoPath "scripts/install-codex-assets.ps1"),
    "-RepoPath", $RepoPath,
    "-CodexHome", $CodexHome,
    "-MarketplaceName", $MarketplaceName,
    "-SkillMode", $SkillMode,
    "-InstallMethod", "git",
    "-ResolvedRef", $resolvedRef,
    "-SkipRepoSync"
  )
  if ($Force) { $installerArgs += "-Force" }
  Invoke-PowerShellFile -ArgumentList $installerArgs
}

function Install-FromArchive {
  $resolvedRef = $Ref
  if (-not $resolvedRef) {
    $resolvedRef = Get-LatestReleaseTag
    if (-not $resolvedRef) {
      $resolvedRef = "main"
      Warn "No GitHub release found; falling back to main branch archive without checksum verification."
    }
  }

  $tmpRoot = Join-Path ([IO.Path]::GetTempPath()) ("plugins-" + [guid]::NewGuid().ToString("N"))
  Ensure-Directory $tmpRoot
  try {
    if ($resolvedRef -eq "main") {
      $archivePath = Join-Path $tmpRoot "plugins-main.zip"
      Invoke-WebRequest -Uri "https://github.com/$RepoOwner/$RepoName/archive/refs/heads/main.zip" -OutFile $archivePath
    }
    else {
      $archivePath = Join-Path $tmpRoot "plugins-$resolvedRef.zip"
      $checksumsPath = Join-Path $tmpRoot "SHA256SUMS"
      Invoke-WebRequest -Uri "https://github.com/$RepoOwner/$RepoName/releases/download/$resolvedRef/plugins-$resolvedRef.zip" -OutFile $archivePath -ErrorAction Stop
      Invoke-WebRequest -Uri "https://github.com/$RepoOwner/$RepoName/releases/download/$resolvedRef/SHA256SUMS" -OutFile $checksumsPath

      $expected = Select-String -Path $checksumsPath -Pattern "plugins-$resolvedRef.zip" | Select-Object -First 1
      if (-not $expected) {
        throw "Missing checksum for plugins-$resolvedRef.zip"
      }
      $expectedHash = ($expected.Line -split '\s+')[0].ToLowerInvariant()
      $actualHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $archivePath).Hash.ToLowerInvariant()
      if ($expectedHash -ne $actualHash) {
        throw "Checksum mismatch for plugins-$resolvedRef.zip"
      }
    }

    $extractRoot = Join-Path $tmpRoot "expanded"
    Expand-Archive -LiteralPath $archivePath -DestinationPath $extractRoot -Force
    $sourceRoot = Get-ChildItem -LiteralPath $extractRoot | Select-Object -First 1
    if (-not $sourceRoot) {
      throw "Archive extraction did not produce repository contents."
    }

    if (Test-Path -LiteralPath $RepoPath) {
      Remove-Item -LiteralPath $RepoPath -Recurse -Force
    }
    Ensure-Directory (Split-Path -Parent $RepoPath)
    Move-Item -LiteralPath $sourceRoot.FullName -Destination $RepoPath

    $installerArgs = @(
      "-File", (Join-Path $RepoPath "scripts/install-codex-assets.ps1"),
      "-RepoPath", $RepoPath,
      "-CodexHome", $CodexHome,
      "-MarketplaceName", $MarketplaceName,
      "-SkillMode", $SkillMode,
      "-InstallMethod", "archive",
      "-ResolvedRef", $resolvedRef,
      "-SkipRepoSync"
    )
    if ($Force) { $installerArgs += "-Force" }
    Invoke-PowerShellFile -ArgumentList $installerArgs
  }
  finally {
    Remove-Item -LiteralPath $tmpRoot -Recurse -Force -ErrorAction SilentlyContinue
  }
}

$RepoPath = Normalize-Path $RepoPath
$CodexHome = Normalize-Path $CodexHome

$method = Choose-InstallMethod
Log "Using install method: $method"

switch ($method) {
  "git" { Install-FromGit }
  "archive" { Install-FromArchive }
  default { throw "Unsupported install method: $method" }
}
