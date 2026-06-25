param(
    [string]$RepoId = "Xenova/bge-small-zh-v1.5",
    [string]$Revision = "main",
    [string]$ModelDir = "models\bge-small-zh-v1.5-int8"
)

$ErrorActionPreference = "Stop"

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Target = Join-Path $Root $ModelDir

function Resolve-Python {
    $candidates = @(
        (Join-Path $Root ".venv-cpython\Scripts\python.exe"),
        (Join-Path $Root ".venv\Scripts\python.exe"),
        "python"
    )

    foreach ($candidate in $candidates) {
        $command = Get-Command $candidate -ErrorAction SilentlyContinue
        if ($command) {
            return $command.Source
        }
    }

    throw "Python was not found. Create .venv-cpython or .venv, or add python to PATH."
}

$Python = Resolve-Python
New-Item -ItemType Directory -Force -Path $Target | Out-Null

& $Python -m pip install -r (Join-Path $Root "requirements.legacy-python.txt")
if ($LASTEXITCODE -ne 0) {
    throw "Failed to install Python requirements."
}

$script = @'
import json
import os
import shutil
import sys
from pathlib import Path

from huggingface_hub import snapshot_download

repo_id = sys.argv[1]
revision = sys.argv[2]
target = Path(sys.argv[3])

cache_dir = snapshot_download(
    repo_id=repo_id,
    revision=revision,
    allow_patterns=[
        "tokenizer.json",
        "tokenizer_config.json",
        "special_tokens_map.json",
        "config.json",
        "onnx/*.onnx",
        "*.onnx",
    ],
)

target.mkdir(parents=True, exist_ok=True)
for relative in [
    "tokenizer.json",
    "tokenizer_config.json",
    "special_tokens_map.json",
    "config.json",
]:
    source = Path(cache_dir) / relative
    if source.exists():
        shutil.copy2(source, target / source.name)

onnx_files = sorted((Path(cache_dir) / "onnx").glob("*.onnx"))
if not onnx_files:
    onnx_files = sorted(Path(cache_dir).glob("*.onnx"))
if not onnx_files:
    raise SystemExit("No ONNX model file was downloaded.")

preferred = None
for item in onnx_files:
    name = item.name.lower()
    if "quant" in name or "int8" in name:
        preferred = item
        break
preferred = preferred or onnx_files[0]
shutil.copy2(preferred, target / "model_quantized.onnx")

dim = 512
config_path = target / "config.json"
if config_path.exists():
    try:
        config = json.loads(config_path.read_text(encoding="utf-8"))
        dim = int(config.get("hidden_size") or dim)
    except Exception:
        pass

(target / "rag-embedding.json").write_text(
    json.dumps(
        {
            "repo_id": repo_id,
            "revision": revision,
            "model_file": "model_quantized.onnx",
            "model_id": "BAAI/bge-small-zh-v1.5",
            "embedding_backend": "onnxruntime",
            "embedding_quantization": "int8",
            "embedding_dim": dim,
            "normalized": True,
        },
        ensure_ascii=False,
        indent=2,
    ),
    encoding="utf-8",
)

print(f"Downloaded {repo_id}@{revision} to {target}")
'@

$TempDir = Join-Path $Root "output"
New-Item -ItemType Directory -Force -Path $TempDir | Out-Null
$Downloader = Join-Path $TempDir "download-rag-embedding-model.py"
Set-Content -LiteralPath $Downloader -Value $script -Encoding UTF8

& $Python $Downloader $RepoId $Revision $Target
if ($LASTEXITCODE -ne 0) {
    throw "Failed to download RAG embedding model."
}

Write-Host "RAG embedding model is ready: $Target"
