#!/usr/bin/env bash
# Script para executar o processamento timelapse de todas as fotos do SFTP sem SQS (Modo Standalone CLI)
set -euo pipefail

echo "🎬 Executando Timelapse em Modo Standalone CLI..."
cargo run --release -- --cli
