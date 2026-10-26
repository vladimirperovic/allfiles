param(
    [ValidatePattern('^[A-Za-z0-9._+\-]+$')]
    [string]$Version = '0.1.0'
)

$ErrorActionPreference = 'Stop'
$projectDirectory = Split-Path -Parent $PSScriptRoot
$previousGOOS = $env:GOOS
$previousGOARCH = $env:GOARCH
$previousCGOEnabled = $env:CGO_ENABLED

Push-Location -LiteralPath $projectDirectory
try {
    New-Item -ItemType Directory -Path 'dist' -Force | Out-Null
    $env:CGO_ENABLED = '0'
    foreach ($target in @('linux/amd64', 'linux/arm64', 'darwin/amd64', 'darwin/arm64', 'windows/amd64')) {
        $parts = $target.Split('/')
        $env:GOOS = $parts[0]
        $env:GOARCH = $parts[1]
        $extension = if ($env:GOOS -eq 'windows') { '.exe' } else { '' }
        $outputPath = "dist/allfiles-$($env:GOOS)-$($env:GOARCH)$extension"
        Write-Host "Building $target"
        & go build -trimpath "-ldflags=-s -w -X main.Version=$Version" -o $outputPath .
        if ($LASTEXITCODE -ne 0) { throw "Build failed for $target" }
    }
    Copy-Item -LiteralPath 'README.md', 'THIRD_PARTY_NOTICES.txt' -Destination 'dist'
    Write-Host 'Binaries are in dist/. No catalogs or document content are included.'
}
finally {
    $env:GOOS = $previousGOOS
    $env:GOARCH = $previousGOARCH
    $env:CGO_ENABLED = $previousCGOEnabled
    Pop-Location
}
