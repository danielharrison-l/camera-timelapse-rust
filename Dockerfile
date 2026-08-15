# ==============================================================================
# STAGE 1: Build Stage (Compilação Multi-Core de Alta Performance em Rust)
# ==============================================================================
FROM rust:slim-bookworm AS builder

# Instala dependências de compilação C/C++ necessárias para libssh2, OpenSSL e Cmake
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    cmake \
    build-essential \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/app

# Copia arquivos de manifesto e código fonte
COPY Cargo.toml Cargo.lock ./
COPY src ./src

# Compila o binário de produção otimizado com release mode
RUN cargo build --release

# ==============================================================================
# STAGE 2: Runtime Stage (Imagem de Execução Leve com FFmpeg e OpenSSL)
# ==============================================================================
FROM debian:bookworm-slim AS runner

# Instala o FFmpeg (para encoding multi-core de vídeo), certificados CA e bibliotecas SSL
RUN apt-get update && apt-get install -y --no-install-recommends \
    ffmpeg \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copia o binário compilado do estágio anterior
COPY --from=builder /usr/src/app/target/release/camera-timelapse-rust /usr/local/bin/camera-timelapse-rust

# Cria o diretório para saída de arquivos temporários do job
RUN mkdir -p /app/temp_output

# Define variáveis de ambiente padrão caso não sejam injetadas
ENV FFMPEG_PATH=ffmpeg \
    WORKER_CONCURRENCY=1 \
    LOG_LEVEL=info

# Comando de entrada
ENTRYPOINT ["/usr/local/bin/camera-timelapse-rust"]
