param([string]$Target = '')
$ErrorActionPreference = 'Stop'
Push-Location -LiteralPath (Split-Path -Parent $PSScriptRoot)
try {
    if (-not $Target) { $Target = ((& rustc -vV | Select-String '^host: ').ToString() -replace '^host: ', '') }
    $name = switch -Regex ($Target) {
        '^x86_64-unknown-linux-' { 'allfiles-linux-amd64'; break }
        '^aarch64-unknown-linux-' { 'allfiles-linux-arm64'; break }
        '^x86_64-apple-darwin$' { 'allfiles-darwin-amd64'; break }
        '^aarch64-apple-darwin$' { 'allfiles-darwin-arm64'; break }
        '^x86_64-pc-windows-' { 'allfiles-windows-amd64.exe'; break }
        default { throw "Unsupported target: $Target" }
    }
    & cargo build --release --locked --target $Target
    if ($LASTEXITCODE -ne 0) { throw 'Rust build failed' }
    $metadata = & cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'Cargo metadata failed' }
    $extension = if ($name.EndsWith('.exe')) { '.exe' } else { '' }
    New-Item -ItemType Directory -Path dist -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $metadata.target_directory "$Target/release/allfiles$extension") -Destination "dist/$name"
    Copy-Item -LiteralPath README.md, README.en.md, THIRD_PARTY_NOTICES.txt -Destination dist
    Write-Host "Built dist/$name. Catalogs are not included."
}
finally { Pop-Location }
