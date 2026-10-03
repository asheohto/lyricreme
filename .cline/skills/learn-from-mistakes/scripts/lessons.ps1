<#
lessons.ps1 - durable lesson memory for the learn-from-mistakes reflection loop.

Store: <cwd>/.agent-memory/lessons.jsonl (one JSON object per line; gitignore it)
Run the script from the repo root (the skill's commands already do).
Override the path with -Store for a temp file or a different project.

Actions:
  path                     print the store path
  list   [-Tags a,b] [-Last N]
  add    -Lesson "text" [-Tags area] [-Scope "target/path or command"]
  reap   -Tags area       remove lessons for an area that is now verified fixed
  dump   [-Tags area]     include timestamps

add refuses unsafe lessons (memory-poisoning guard) and prints the matched pattern.
#>
param(
    [Parameter(Position = 0)][string]$Action = 'list',
    [string]$Lesson,
    [string[]]$Tags = @(),
    [string]$Scope = '',
    [int]$Last = 0,
    [string]$Store = ''
)

$ErrorActionPreference = 'Stop'
if (-not $Store) {
    $root = (Get-Location).Path
    $Store = Join-Path $root '.agent-memory\lessons.jsonl'
}

# Normalize tags: -File argument binding can deliver "a,b" or "a b" as one
# string; flatten every shape into a clean string array.
$Tags = @($Tags | ForEach-Object { "$_" -split '[,;\s]+' } | Where-Object { $_ })

# ponytail: naive substring banlist, not semantic analysis; upgrade path is a
# reviewer checkpoint (human or stronger model) for lessons that pass here.
$UnsafePatterns = @(
    'ignore (previous|prior|all) (instructions|rules)',
    'you are (now|actually)',
    'disregard',
    'do not (test|verify|validate)',
    'skip (testing|review|validation)',
    'always trust',
    'secret|api[_ -]?key|password|token'   # never persist credentials
)

function Read-Lessons {
    if (Test-Path $Store) {
        Get-Content $Store -Encoding UTF8 | Where-Object { $_.Trim() } | ForEach-Object {
            $_ | ConvertFrom-Json
        }
    }
}

function Write-Lessons([object[]]$Lessons) {
    $dir = Split-Path $Store -Parent
    if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
    if ($Lessons) {
        $Lines = $Lessons | ForEach-Object {
            [ordered]@{
                ts     = $_.ts
                lesson = $_.lesson
                tags   = @($_.tags)
                scope  = $_.scope
            } | ConvertTo-Json -Compress
        }
        Set-Content -Path $Store -Value $Lines -Encoding UTF8
    } elseif (Test-Path $Store) {
        Remove-Item $Store
    }
}

function Test-SafeLesson([string]$Text) {
    foreach ($p in $UnsafePatterns) {
        if ($Text -match $p) { return $p }
    }
    return $null
}

switch ($Action) {
    'path' {
        Write-Output $Store
    }
    'list' {
        $all = @(Read-Lessons)
        if ($Tags.Count) {
            $all = @($all | Where-Object {
                $l = $_
                @($l.tags) | Where-Object { $Tags -contains $_ } | Select-Object -First 1
            })
        }
        if ($Last -gt 0) { $all = @($all | Select-Object -Last $Last) }
        if (-not $all) { Write-Output '(no lessons recorded)' }
        else {
            foreach ($l in $all) {
                $t = if ($l.tags) { " [$(@($l.tags) -join ',')]" } else { '' }
                $s = if ($l.scope) { " ($($l.scope))" } else { '' }
                Write-Output "- $($l.lesson)$t$s"
            }
        }
    }
    'add' {
        if (-not $Lesson -or -not $Lesson.Trim()) {
            Write-Error 'add requires -Lesson "text"'; exit 1
        }
        $bad = Test-SafeLesson $Lesson
        if ($bad) {
            Write-Output "REFUSED (unsafe lesson, matched: $bad)"
            exit 1
        }
        $entry = [ordered]@{
            ts     = (Get-Date).ToUniversalTime().ToString('o')
            lesson = $Lesson.Trim()
            tags   = @($Tags)
            scope  = $Scope
        }
        $dir = Split-Path $Store -Parent
        if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
        # Append one JSONL line; concurrent readers never see partial writes
        # because Set-Content/Add-Content open with write sharing disabled.
        Add-Content -Path $Store -Value ($entry | ConvertTo-Json -Compress) -Encoding UTF8
        Write-Output "stored: $($entry.lesson) [tags: $(@($Tags) -join ',')]"
    }
    'reap' {
        if (-not $Tags.Count) { Write-Error 'reap requires -Tags area'; exit 1 }
        $all = @(Read-Lessons)
        $kept = @($all | Where-Object {
            $l = $_
            -not (@($l.tags) | Where-Object { $Tags -contains $_ } | Select-Object -First 1)
        })
        $removed = $all.Count - $kept.Count
        Write-Lessons $kept
        Write-Output "reaped $removed lesson(s) for tag(s): $($Tags -join ',')"
    }
    'dump' {
        $all = @(Read-Lessons)
        if ($Tags.Count) {
            $all = @($all | Where-Object {
                $l = $_
                @($l.tags) | Where-Object { $Tags -contains $_ } | Select-Object -First 1
            })
        }
        if (-not $all) { Write-Output '(no lessons recorded)' }
        else { $all | ForEach-Object { $_ | ConvertTo-Json -Compress } }
    }
    default {
        Write-Error "unknown action '$Action' (use: path|list|add|reap|dump)"; exit 1
    }
}
