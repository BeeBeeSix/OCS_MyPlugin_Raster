<#
.SYNOPSIS
    Pubblica una nuova versione di Raster Tools.

.DESCRIPTION
    1. controlla il numero di versione e che il tag non esista già;
    2. aggiorna la versione in Cargo.toml e plugin.toml;
    3. in CHANGELOG.md sposta le voci di "[Non rilasciato]" in una nuova
       sezione "## [versione] - data";
    4. esegue i test e compila il plugin;
    5. dopo la tua conferma crea il commit e il tag e li carica su GitHub.
    Il tag avvia il workflow release.yml, che compila per Windows, Linux e
    macOS e pubblica la release con le note prese dal CHANGELOG.

    Se rispondi "no" alla conferma, i file tornano come prima.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\release.ps1 0.2.0

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\release.ps1 0.2.0 -SkipBuild
#>
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$Version,
    # Salta test e compilazione locale (li fa comunque GitHub).
    [switch]$SkipBuild,
    # Non chiede conferma prima di caricare su GitHub.
    [switch]$Yes
)

Set-StrictMode -Version 2
$ErrorActionPreference = 'Stop'

$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root
$Utf8 = New-Object System.Text.UTF8Encoding($false)

function Info($msg) { Write-Host $msg -ForegroundColor Cyan }
function Ok($msg) { Write-Host $msg -ForegroundColor Green }
function Fail($msg) { Write-Host "ERRORE: $msg" -ForegroundColor Red; exit 1 }

function Read-Text($path) { [System.IO.File]::ReadAllText((Join-Path $Root $path), $Utf8) }
function Write-Text($path, $text) { [System.IO.File]::WriteAllText((Join-Path $Root $path), $text, $Utf8) }

# Esegue un programma esterno e si ferma se fallisce.
function Run {
    param([string]$Exe, [string[]]$Arguments)
    & $Exe @Arguments
    if ($LASTEXITCODE -ne 0) { throw "'$Exe $($Arguments -join ' ')' non è riuscito (codice $LASTEXITCODE)." }
}

# ── Controlli ────────────────────────────────────────────────────────────────

$Version = $Version.Trim().TrimStart('v', 'V')
if ($Version -notmatch '^\d+\.\d+\.\d+$') {
    Fail "versione '$Version' non valida: usa tre numeri, per esempio 0.2.0"
}
$Tag = "v$Version"

foreach ($tool in 'git', 'cargo') {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        if ($tool -eq 'cargo' -and $SkipBuild) { continue }
        Fail "il comando '$tool' non è disponibile. Chiudi e riapri PowerShell, o installalo."
    }
}
foreach ($f in 'Cargo.toml', 'plugin.toml', 'CHANGELOG.md') {
    if (-not (Test-Path (Join-Path $Root $f))) { Fail "manca il file $f nella cartella del progetto." }
}

$branch = (& git rev-parse --abbrev-ref HEAD).Trim()
if ($branch -ne 'main') { Fail "sei sul ramo '$branch': passa a 'main' (git checkout main)." }

$prev = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
& git rev-parse -q --verify "refs/tags/$Tag" 2>&1 | Out-Null
$exists = ($LASTEXITCODE -eq 0)
$ErrorActionPreference = $prev
if ($exists) { Fail "il tag $Tag esiste già. Usa un numero di versione nuovo." }

Info "Controllo GitHub…"
$remoteTag = & git ls-remote --tags origin "refs/tags/$Tag"
if ($LASTEXITCODE -ne 0) { Fail "non riesco a contattare GitHub (git ls-remote). Controlla la connessione." }
if ($remoteTag) { Fail "il tag $Tag esiste già su GitHub. Usa un numero di versione nuovo." }

& git fetch -q origin main
if ($LASTEXITCODE -eq 0) {
    $behind = (& git rev-list --count HEAD..origin/main).Trim()
    if ([int]$behind -gt 0) {
        Fail "su GitHub ci sono $behind modifiche che non hai sul PC. Esegui prima: git pull"
    }
}

$cargo = Read-Text 'Cargo.toml'
$plugin = Read-Text 'plugin.toml'
$changelog = Read-Text 'CHANGELOG.md'
$verRe = '(?m)^(version\s*=\s*")([^"]*)(")'

$m = [regex]::Match($cargo, $verRe)
if (-not $m.Success) { Fail "non trovo 'version = ...' in Cargo.toml." }
$Current = $m.Groups[2].Value
if ([version]$Version -le [version]$Current) {
    Fail "la nuova versione ($Version) deve essere maggiore di quella attuale ($Current)."
}

# ── CHANGELOG ────────────────────────────────────────────────────────────────

$nl = "`n"
if ($changelog.Contains("`r`n")) { $nl = "`r`n" }
$date = Get-Date -Format 'yyyy-MM-dd'

if ($changelog -match ('(?m)^## \[' + [regex]::Escape($Version) + '\]')) {
    Info "CHANGELOG.md ha già la sezione ${Version}: la uso così com'è."
    $newChangelog = $changelog
}
else {
    $head = [regex]::Match($changelog, '(?m)^## \[Non rilasciato\][ \t]*(?=\r?$)')
    if (-not $head.Success) {
        Fail "in CHANGELOG.md manca il titolo '## [Non rilasciato]'."
    }
    $after = $changelog.Substring($head.Index + $head.Length)
    $next = [regex]::Match($after, '(?m)^## \[')
    $pending = $after
    if ($next.Success) { $pending = $after.Substring(0, $next.Index) }
    if ($pending.Trim().Length -eq 0) {
        Fail ("in CHANGELOG.md la sezione [Non rilasciato] è vuota.$nl" +
              "Scrivi lì le novità di questa versione (es. '### Aggiunto' e un elenco '- ...') e rilancia.")
    }
    $insertAt = $head.Index + $head.Length
    $newChangelog = $changelog.Substring(0, $insertAt) + $nl + $nl + "## [$Version] - $date" + $changelog.Substring($insertAt)
}

# ── Aggiornamento dei file ─────────────────────────────────────────────────

$originals = @{
    'Cargo.toml'   = $cargo
    'plugin.toml'  = $plugin
    'CHANGELOG.md' = $changelog
}
$lockPath = Join-Path $Root 'Cargo.lock'
$lockOriginal = $null
if (Test-Path $lockPath) { $lockOriginal = Read-Text 'Cargo.lock' }

function Restore-Files {
    foreach ($k in $originals.Keys) { Write-Text $k $originals[$k] }
    if ($null -ne $lockOriginal) { Write-Text 'Cargo.lock' $lockOriginal }
    Info "File riportati com'erano: nessuna modifica è stata caricata."
}

$rx = New-Object System.Text.RegularExpressions.Regex($verRe)
Write-Text 'Cargo.toml' ($rx.Replace($cargo, ('${1}' + $Version + '${3}'), 1))
Write-Text 'plugin.toml' ($rx.Replace($plugin, ('${1}' + $Version + '${3}'), 1))
Write-Text 'CHANGELOG.md' $newChangelog
Ok "Versione aggiornata: $Current -> $Version (Cargo.toml, plugin.toml, CHANGELOG.md)."

try {
    if (-not $SkipBuild) {
        Info "Test…"
        Run 'cargo' @('test', '--release')
        Info "Compilazione…"
        Run 'cargo' @('build', '--release')
        Ok "Test e compilazione riusciti."
    }
}
catch {
    Write-Host $_ -ForegroundColor Red
    Restore-Files
    exit 1
}

# ── Riepilogo e conferma ─────────────────────────────────────────────────────

Write-Host ""
Info "Note della versione $Version (da CHANGELOG.md):"
$sec = [regex]::Match($newChangelog, '(?ms)^## \[' + [regex]::Escape($Version) + '\][^\n]*\n(.*?)(?=^## \[|\z)')
Write-Host $sec.Groups[1].Value.Trim()
Write-Host ""
Info "File modificati:"
& git status --short
Write-Host ""

if (-not $Yes) {
    $answer = Read-Host "Pubblico la versione $Tag su GitHub? (s/N)"
    if ($answer -notmatch '^(s|si|sì|y|yes)$') {
        Restore-Files
        exit 0
    }
}

# ── Commit, tag, push ────────────────────────────────────────────────────────

try {
    Run 'git' @('add', '-A')
    Run 'git' @('commit', '-m', "Versione $Version")
    Run 'git' @('push', 'origin', 'main')
    Run 'git' @('tag', '-a', $Tag, '-m', "Raster Tools $Version")
    Run 'git' @('push', 'origin', $Tag)
}
catch {
    Write-Host $_ -ForegroundColor Red
    Fail "pubblicazione interrotta. Controlla il messaggio qui sopra; se il commit è stato creato ma non caricato, riprova con: git push origin main"
}

$url = ((& git remote get-url origin) -replace '\.git$', '').Trim()
Write-Host ""
Ok "Versione $Tag caricata."
Write-Host "La compilazione richiede circa 15-20 minuti:"
Write-Host "  $url/actions"
Write-Host "Poi la release sarà qui:"
Write-Host "  $url/releases/tag/$Tag"
