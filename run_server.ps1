$ErrorActionPreference = "Stop"

# Set persistence path to a local directory 'fts_persistence' in the script's location
$env:FTS_PERSISTENCE_PATH = "$PSScriptRoot\fts_persistence"

# Ensure the persistence directory exists
if (!(Test-Path -Path $env:FTS_PERSISTENCE_PATH)) {
    Write-Host "Creating persistence directory at $env:FTS_PERSISTENCE_PATH"
    New-Item -ItemType Directory -Path $env:FTS_PERSISTENCE_PATH | Out-Null
}

# Path to the virtual environment python
$pythonPath = "$PSScriptRoot\venv\Scripts\python.exe"

if (!(Test-Path -Path $pythonPath)) {
    Write-Error "Virtual environment not found at $PSScriptRoot\venv. Please run setup first (or wait for installation to complete)."
    exit 1
}

# Check for FTSConfig.yaml
$configPath = Join-Path $env:FTS_PERSISTENCE_PATH "FTSConfig.yaml"

if (!(Test-Path -Path $configPath)) {
    Write-Host "Configuration file not found. Generating default configuration..."
    try {
        & $pythonPath -c "from FreeTAKServer.core.configuration.configuration_wizard import autogenerate_config; autogenerate_config()"
        Write-Host "Configuration generated successfully."
    } catch {
        Write-Error "Failed to generate configuration: $_"
        exit 1
    }
} else {
    Write-Host "Configuration file found at $configPath"
}

# Ensure Logs directory exists (MainConfig requires it to exist)
$logsPath = Join-Path $env:FTS_PERSISTENCE_PATH "Logs"
if (!(Test-Path -Path $logsPath)) {
    Write-Host "Creating Logs directory at $logsPath"
    New-Item -ItemType Directory -Path $logsPath | Out-Null
}

# Ensure Database file exists (MainConfig requires it to exist)
$dbPath = Join-Path $env:FTS_PERSISTENCE_PATH "FTSDataBase.db"
if (!(Test-Path -Path $dbPath)) {
    Write-Host "Creating empty database file at $dbPath"
    New-Item -ItemType File -Path $dbPath | Out-Null
}

Write-Host "Starting FreeTakServer..."
Write-Host "Press Ctrl+C to stop."

# Run the server
& $pythonPath -m FreeTAKServer.controllers.services.FTS
