param(
    [switch] $SkipDownload
)

$ErrorActionPreference = "Stop"

$workspaceRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$modelDirectory = Join-Path $workspaceRoot "models\bge-small-zh-v1.5-int8"
$modelFiles = @(
    @{
        Name = "config.json"
        Url = "https://huggingface.co/Xenova/bge-small-zh-v1.5/resolve/main/config.json"
        Sha256 = "D4193EAD3A810FD694FA8A31D7FC72FBAEBC0668B603E398734BF2F6538FF42F"
    },
    @{
        Name = "model_quantized.onnx"
        Url = "https://huggingface.co/Xenova/bge-small-zh-v1.5/resolve/main/onnx/model_quantized.onnx"
        Sha256 = "B9837C19CE154FF0726D398EE77ABBC03A7FAF0476C6F93016C84E531BE7EBB5"
    },
    @{
        Name = "special_tokens_map.json"
        Url = "https://huggingface.co/Xenova/bge-small-zh-v1.5/resolve/main/special_tokens_map.json"
        Sha256 = "B6D346BE366A7D1D48332DBC9FDF3BF8960B5D879522B7799DDBA59E76237EE3"
    },
    @{
        Name = "tokenizer.json"
        Url = "https://huggingface.co/Xenova/bge-small-zh-v1.5/resolve/main/tokenizer.json"
        Sha256 = "48CEA5D44424912A6FD1EA647BF4FE50B55AB8B1E5879C3275F80E339E8FAE26"
    },
    @{
        Name = "tokenizer_config.json"
        Url = "https://huggingface.co/Xenova/bge-small-zh-v1.5/resolve/main/tokenizer_config.json"
        Sha256 = "E6F3B96DB926A37D4039995FBF5AD17DE158DFB8F6343D607E4DBAAD18D75F5A"
    }
)

New-Item -ItemType Directory -Path $modelDirectory -Force | Out-Null

foreach ($file in $modelFiles) {
    $destination = Join-Path $modelDirectory $file.Name
    $valid = $false
    if (Test-Path -LiteralPath $destination) {
        $valid = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash -eq $file.Sha256
    }
    if (-not $valid) {
        if ($SkipDownload) {
            throw "RAG model file is missing or invalid: $destination"
        }
        $temporary = "$destination.download"
        Invoke-WebRequest -UseBasicParsing -Uri $file.Url -OutFile $temporary
        $actual = (Get-FileHash -LiteralPath $temporary -Algorithm SHA256).Hash
        if ($actual -ne $file.Sha256) {
            Remove-Item -LiteralPath $temporary -Force
            throw "RAG model checksum mismatch for $($file.Name): expected $($file.Sha256), got $actual"
        }
        Move-Item -LiteralPath $temporary -Destination $destination -Force
    }
}

Write-Host "Verified BAAI/bge-small-zh-v1.5 INT8 resources in $modelDirectory"
