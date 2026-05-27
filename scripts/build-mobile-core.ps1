$ErrorActionPreference = "Stop"

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Manifest = Join-Path $Root "native\gp-core\Cargo.toml"
$Target = $env:GP_CORE_TARGET

$Args = @("build", "--manifest-path", $Manifest, "--release")
if ($Target) {
    $Args += @("--target", $Target)
}

& cargo @Args
if ($LASTEXITCODE -ne 0) {
    throw "Mobile core build failed with exit code $LASTEXITCODE."
}
