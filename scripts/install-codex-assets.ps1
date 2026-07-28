#!/usr/bin/env pwsh

param(
  [string]$RepoPath = "$HOME/src/plugins",
  [string]$RepoUrl = $(if ($env:REPO_URL) { $env:REPO_URL } else { "https://github.com/joshyorko/plugins.git" }),
  [string]$CodexHome = "$HOME/.codex",
  [string]$MarketplaceName = "plugins",
  [ValidateSet("auto", "link", "copy")] [string]$SkillMode = $(if ($env:SKILL_MODE) { $env:SKILL_MODE } else { "auto" }),
  [ValidateSet("git", "archive")] [string]$InstallMethod = $(if ($env:INSTALL_METHOD) { $env:INSTALL_METHOD } else { "git" }),
  [string]$Ref = $(if ($env:REF_SPEC) { $env:REF_SPEC } else { "" }),
  [string]$ResolvedRef = $(if ($env:RESOLVED_REF) { $env:RESOLVED_REF } else { "" }),
  [switch]$SkipRepoSync,
  [switch]$Force,
  [switch]$Help
)

$ErrorActionPreference = "Stop"

function Show-Usage {
  @"
Usage: pwsh -File scripts/install-codex-assets.ps1 [-RepoPath PATH] [-RepoUrl URL] [-CodexHome PATH] [-MarketplaceName NAME] [-SkillMode auto|link|copy] [-InstallMethod git|archive] [-Ref REF] [-ResolvedRef REF] [-SkipRepoSync] [-Force]

Bootstrap Codex plugins and skills from this repository into a user-level installation.

Examples:
  pwsh -File "$HOME/src/plugins/scripts/install-codex-assets.ps1" -RepoPath "$HOME/src/plugins"
  pwsh -File "$HOME/src/plugins/scripts/install-codex-assets.ps1" -RepoPath "$HOME/src/plugins" -SkillMode copy -Force
  pwsh -File "$HOME/src/plugins/scripts/install-codex-assets.ps1" -RepoPath "$HOME/src/plugins" -SkipRepoSync -InstallMethod archive -ResolvedRef v1.2.3
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

function Get-CurrentRef {
  if ($ResolvedRef) {
    return $ResolvedRef
  }

  if (-not (Test-Path -LiteralPath (Join-Path $RepoPath ".git"))) {
    if ($Ref) {
      return $Ref
    }
    return "unknown"
  }

  $tagsAtHead = @(& git -C $RepoPath tag --points-at HEAD 2>$null)
  $tag = $tagsAtHead | Select-Object -First 1
  if ($LASTEXITCODE -eq 0 -and $tag) {
    return $tag.Trim()
  }

  $branch = (& git -C $RepoPath symbolic-ref --short -q HEAD 2>$null)
  if ($LASTEXITCODE -eq 0 -and $branch) {
    return $branch.Trim()
  }

  return ((& git -C $RepoPath rev-parse --short HEAD).Trim())
}

function Sync-RepoRef {
  if (-not $Ref) {
    return
  }

  Log "Syncing repository to ref $Ref"
  & git -C $RepoPath fetch --tags --prune origin | Out-Null

  & git -C $RepoPath rev-parse --verify --quiet "$Ref^{commit}" | Out-Null
  if ($LASTEXITCODE -eq 0) {
    & git -C $RepoPath checkout --force $Ref | Out-Null
    return
  }

  & git -C $RepoPath fetch origin $Ref --depth=1 | Out-Null
  if ($LASTEXITCODE -eq 0) {
    & git -C $RepoPath checkout --force FETCH_HEAD | Out-Null
    return
  }

  throw "Unable to resolve ref $Ref in $RepoPath"
}

$RepoPath = Normalize-Path $RepoPath
$CodexHome = Normalize-Path $CodexHome
$MarketplaceStatus = "not attempted"
$script:MarketplaceAddArgs = $null

$SkillsRoot = Join-Path $CodexHome "skills"
$StateRoot = Join-Path $CodexHome "state"
$StatePath = Join-Path $StateRoot "plugins.json"
$CatalogPath = Join-Path $RepoPath "marketplaces/catalog.json"
$LegacyAgentsHome = Normalize-Path "$HOME/.agents"

$script:ManagedSkills = [System.Collections.Generic.List[string]]::new()
$script:LinkedCount = 0
$script:CopiedCount = 0
$script:SkippedCount = 0
$script:ActualSkillMode = ""

function Clone-Or-UpdateRepo {
  if (Test-Path -LiteralPath (Join-Path $RepoPath ".git")) {
    Log "Updating existing repository at $RepoPath"
    & git -C $RepoPath fetch --tags --prune | Out-Null
    if (-not $Ref) {
      & git -C $RepoPath pull --ff-only | Out-Null
    }
  }
  elseif (Test-Path -LiteralPath $RepoPath) {
    throw "Target path $RepoPath exists but is not a git repository."
  }
  else {
    Ensure-Directory (Split-Path -Parent $RepoPath)
    Log "Cloning $RepoUrl into $RepoPath"
    & git clone $RepoUrl $RepoPath | Out-Null
  }

  Sync-RepoRef
}

function Register-Marketplace {
  $codexCmd = Get-Command codex -ErrorAction SilentlyContinue
  $marketplaceAddArgs = Get-MarketplaceAddArgs
  $marketplaceAddCommand = "codex $($marketplaceAddArgs -join ' ')"
  if (-not $codexCmd) {
    $script:MarketplaceStatus = "not registered automatically (codex CLI not found)"
    Warn "codex CLI not found; skipping marketplace registration. Run manually: $(Get-MarketplaceAddCommandText)"
    return
  }

  $originalCodexHome = $env:CODEX_HOME
  $env:CODEX_HOME = $CodexHome
  try {
    & codex @marketplaceAddArgs $RepoPath | Out-Null
    if ($LASTEXITCODE -eq 0) {
      $script:MarketplaceStatus = "registered via ``$marketplaceAddCommand `"$RepoPath`"``"
      Log "Registered marketplace `"$MarketplaceName`" via $marketplaceAddCommand $RepoPath"
    }
    else {
      throw "$marketplaceAddCommand returned exit code $LASTEXITCODE"
    }
  }
  catch {
    $script:MarketplaceStatus = "not registered automatically ($marketplaceAddCommand failed)"
    Warn "failed to register marketplace via codex; run manually: $(Get-MarketplaceAddCommandText) ($($_.Exception.Message))"
  }
  finally {
    $env:CODEX_HOME = $originalCodexHome
  }
}

function Get-MarketplaceAddArgs {
  if ($script:MarketplaceAddArgs) {
    return $script:MarketplaceAddArgs
  }

  $script:MarketplaceAddArgs = @("plugin", "marketplace", "add")

  if (-not (Get-Command codex -ErrorAction SilentlyContinue)) {
    return $script:MarketplaceAddArgs
  }

  $originalCodexHome = $env:CODEX_HOME
  $env:CODEX_HOME = $CodexHome
  try {
    & codex plugin marketplace add --help *> $null
    if ($LASTEXITCODE -eq 0) {
      $script:MarketplaceAddArgs = @("plugin", "marketplace", "add")
      return $script:MarketplaceAddArgs
    }

    & codex marketplace add --help *> $null
    if ($LASTEXITCODE -eq 0) {
      $script:MarketplaceAddArgs = @("marketplace", "add")
      return $script:MarketplaceAddArgs
    }
  }
  finally {
    $env:CODEX_HOME = $originalCodexHome
  }

  return $script:MarketplaceAddArgs
}

function Get-MarketplaceAddCommandText {
  $marketplaceAddArgs = Get-MarketplaceAddArgs
  return "CODEX_HOME=`"$CodexHome`" codex $($marketplaceAddArgs -join ' ') `"$RepoPath`""
}

function Remove-LegacyMarketplace {
  $marketplaceFile = Join-Path $LegacyAgentsHome "plugins/marketplace.json"
  if (-not (Test-Path -LiteralPath $marketplaceFile)) {
    return
  }

  try {
    $data = Get-Content -Raw -LiteralPath $marketplaceFile | ConvertFrom-Json
    $entries = @()
    $style = "single"

    if ($data.PSObject.Properties.Name -contains "marketplaces" -and $data.marketplaces -is [System.Collections.IEnumerable]) {
      $entries = @($data.marketplaces)
      $style = "list"
    }
    elseif ($data.PSObject.Properties.Name -contains "plugins") {
      $entries = @($data)
    }
    else {
      throw "Unexpected marketplace format"
    }

    $filtered = $entries | Where-Object { $_.name -ne $MarketplaceName }
    if ($filtered.Count -eq $entries.Count) {
      return
    }

    if ($filtered.Count -eq 0) {
      Remove-Item -LiteralPath $marketplaceFile -Force
      Log "Removed legacy marketplace file $marketplaceFile"
      return
    }

    if ($style -eq "list" -or $filtered.Count -gt 1) {
      $output = @{ marketplaces = $filtered }
    }
    else {
      $output = $filtered[0]
    }

    $json = ($output | ConvertTo-Json -Depth 6)
    Set-Content -LiteralPath $marketplaceFile -Value ($json + "`n")
    Log "Removed legacy marketplace entry `"$MarketplaceName`" from $marketplaceFile"
  }
  catch {
    Warn "skipping legacy marketplace cleanup: $($_.Exception.Message)"
  }
}

function Test-IsSymlink {
  param([string]$Path)
  $item = Get-Item -LiteralPath $Path -ErrorAction SilentlyContinue
  return $null -ne $item -and ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)
}

function Resolve-Target {
  param([string]$Path)
  (Resolve-Path -LiteralPath $Path).Path
}

function Link-Skill {
  param([string]$Source, [string]$Target)
  $sourceFull = Normalize-Path $Source
  $targetFull = Normalize-Path $Target

  if (Test-Path -LiteralPath $targetFull) {
    if (Test-IsSymlink $targetFull) {
      $current = Resolve-Target $targetFull
      if ([string]::Equals($current, $sourceFull, [StringComparison]::OrdinalIgnoreCase)) {
        return $true
      }
      if (-not $Force) {
        Warn "skill $targetFull already points to $current; use -Force to replace"
        return $false
      }
      Remove-Item -LiteralPath $targetFull -Force
    }
    else {
      if (-not $Force) {
        Warn "skill $targetFull exists; use -Force to replace"
        return $false
      }
      Remove-Item -LiteralPath $targetFull -Recurse -Force
    }
  }

  try {
    New-Item -ItemType SymbolicLink -Path $targetFull -Target $sourceFull -Force | Out-Null
    return $true
  }
  catch {
    Warn "failed to create symlink $targetFull -> $sourceFull ($($_.Exception.Message))"
    return $false
  }
}

function Copy-Skill {
  param([string]$Source, [string]$Target)
  $targetFull = Normalize-Path $Target

  if (Test-Path -LiteralPath $targetFull) {
    if (-not $Force) {
      Warn "skill $targetFull exists; use -Force to replace"
      return $false
    }
    Remove-Item -LiteralPath $targetFull -Recurse -Force
  }

  Copy-Item -LiteralPath $Source -Destination $targetFull -Recurse
  return $true
}

function Install-OneSkill {
  param([System.IO.DirectoryInfo]$Skill)
  $target = Join-Path $SkillsRoot $Skill.Name

  switch ($SkillMode) {
    "link" {
      if (Link-Skill $Skill.FullName $target) {
        $script:ManagedSkills.Add($Skill.Name)
        $script:LinkedCount++
      }
      else {
        $script:SkippedCount++
      }
    }
    "copy" {
      if (Copy-Skill $Skill.FullName $target) {
        $script:ManagedSkills.Add($Skill.Name)
        $script:CopiedCount++
      }
      else {
        $script:SkippedCount++
      }
    }
    "auto" {
      if (Link-Skill $Skill.FullName $target) {
        $script:ManagedSkills.Add($Skill.Name)
        $script:LinkedCount++
      }
      else {
        Warn "falling back to copy mode for $($Skill.Name)"
        if (Copy-Skill $Skill.FullName $target) {
          $script:ManagedSkills.Add($Skill.Name)
          $script:CopiedCount++
        }
        else {
          $script:SkippedCount++
        }
      }
    }
  }
}

function Install-Skills {
  Ensure-Directory $SkillsRoot

  $skillDirs = Get-ChildItem -Path (Join-Path $RepoPath "plugins") -Directory -ErrorAction SilentlyContinue | ForEach-Object {
    Get-ChildItem -Path (Join-Path $_.FullName "skills") -Directory -ErrorAction SilentlyContinue
  }

  foreach ($skill in $skillDirs) {
    Install-OneSkill $skill
  }

  if ($LinkedCount -gt 0 -and $CopiedCount -gt 0) {
    $script:ActualSkillMode = "mixed"
  }
  elseif ($CopiedCount -gt 0) {
    $script:ActualSkillMode = "copy"
  }
  elseif ($LinkedCount -gt 0) {
    $script:ActualSkillMode = "link"
  }
  else {
    $script:ActualSkillMode = $SkillMode
  }

  Log "Skills installed: linked=$LinkedCount copied=$CopiedCount skipped=$SkippedCount"
}

function Write-State {
  Ensure-Directory $StateRoot

  $payload = [ordered]@{
    schema_version = 1
    repo_path = $RepoPath
    codex_home = $CodexHome
    marketplace_name = $MarketplaceName
    install_method = $InstallMethod
    skill_mode = $ActualSkillMode
    resolved_ref = Get-CurrentRef
    managed_skills = @($ManagedSkills)
    installed_at = [DateTimeOffset]::UtcNow.ToString("o")
  }

  $json = $payload | ConvertTo-Json -Depth 6
  Set-Content -LiteralPath $StatePath -Value ($json + "`n")
}

function Main {
  if (-not $SkipRepoSync) {
    Clone-Or-UpdateRepo
  }
  else {
    Log "Skipping repo sync for existing checkout at $RepoPath"
  }

  if (-not (Test-Path -LiteralPath $CatalogPath)) {
    throw "Catalog not found at $CatalogPath"
  }

  Ensure-Directory $CodexHome
  Remove-LegacyMarketplace
  Register-Marketplace
  Install-Skills
  Write-State

  @"

Codex assets installed.
- Repository path: $RepoPath
- Managed ref: $(Get-CurrentRef)
- Install method: $InstallMethod
- Marketplace: $MarketplaceStatus
- Skills directory: $SkillsRoot
- State file: $StatePath

Next steps:
- Restart Codex if marketplace registration succeeded.
- Run "/plugins" or inspect available skills in your client.
- If marketplace registration failed or Codex is not installed, run manually: $(Get-MarketplaceAddCommandText)
"@ | Write-Host
}

Main
