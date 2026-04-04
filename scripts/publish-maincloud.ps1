param(
    [string]$Database = "the-entity-ty5fs",
    [string]$Server = "maincloud",
    [switch]$SkipChecks,
    [switch]$DeleteData
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$spacetimeExe = "C:\Users\HP\AppData\Local\SpacetimeDB\spacetime.exe"
$cargoExe = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
$rustupExe = Join-Path $env:USERPROFILE ".cargo\bin\rustup.exe"

if (-not (Test-Path $spacetimeExe)) {
    throw "SpacetimeDB CLI was not found at $spacetimeExe. Install it first with: iwr https://windows.spacetimedb.com -UseBasicParsing | iex"
}

if (-not (Test-Path $cargoExe)) {
    throw "Cargo was not found at $cargoExe. Install Rust first."
}

$env:Path = "$($env:USERPROFILE)\.cargo\bin;C:\Users\HP\AppData\Local\SpacetimeDB;$env:Path"

Write-Host "Checking Maincloud login..." -ForegroundColor Cyan
$loginOutput = & $spacetimeExe login show 2>&1
if (($LASTEXITCODE -ne 0) -or ($loginOutput -match "not logged in")) {
    throw "You are not logged in to SpacetimeDB Maincloud. Run 'spacetime logout' and then 'spacetime login' first."
}

Push-Location $repoRoot
try {
    if (-not $SkipChecks) {
        Write-Host "Ensuring wasm32 target is installed..." -ForegroundColor Cyan
        & $rustupExe target add wasm32-unknown-unknown
        if ($LASTEXITCODE -ne 0) {
            throw "Failed to install wasm32-unknown-unknown target."
        }

        Write-Host "Running cargo checks..." -ForegroundColor Cyan
        & $cargoExe check
        if ($LASTEXITCODE -ne 0) {
            throw "cargo check failed."
        }

        & $cargoExe check --target wasm32-unknown-unknown
        if ($LASTEXITCODE -ne 0) {
            throw "cargo check --target wasm32-unknown-unknown failed."
        }
    }

    Write-Host "Publishing $Database to $Server..." -ForegroundColor Cyan
    if ($DeleteData) {
        & $spacetimeExe publish --server $Server -y --module-path . $Database --delete-data
    } else {
        & $spacetimeExe publish --server $Server -y --module-path . $Database
    }
    if ($LASTEXITCODE -ne 0) {
        throw "Publish failed."
    }

    Write-Host ""
    Write-Host "Deployment complete." -ForegroundColor Green
    Write-Host "Dashboard URL: https://spacetimedb.com/$Database"
    Write-Host "Host URI: https://maincloud.spacetimedb.com"
    Write-Host "Database Name: $Database"
    Write-Host "HTTP Base: https://maincloud.spacetimedb.com/v1/database/$Database"
    Write-Host "WebSocket Subscribe URL: wss://maincloud.spacetimedb.com/v1/database/$Database/subscribe"
    Write-Host ""
    Write-Host "Verifying schema..." -ForegroundColor Cyan
    & $spacetimeExe describe --json --server $Server $Database
}
finally {
    Pop-Location
}
