# Guia de Execução - Microserviço Timelapse Rust

Este documento descreve as diferentes formas de compilar, executar e testar o microserviço de geração de timelapse.

---

## 🛠️ Requisitos Prévios

Dependendo de como você deseja executar, você precisará de:
* **Para rodar via Docker**: Apenas o **Docker** e **Docker Compose** instalados.
* **Para rodar nativamente (sem Docker)**: **Rust 1.85+**, **Cargo**, **FFmpeg** instalado no PATH e **Node.js** (opcional, para atalhos `npm`).

---

## 🚀 1. Execução via Docker (Recomendado)

Não requer a instalação manual do Rust ou FFmpeg na máquina host.

### A) Modo Standalone CLI (Sem dependência de SQS)
Neste modo, o microserviço lê as credenciais do SFTP no `.env`, varre todas as pastas a partir da raiz `/` no SFTP, baixa as imagens e gera o vídeo MP4 no seu computador.

```bash
# 1. Construir a imagem Docker
docker build -t camera-timelapse-rust:latest .

# 2. Executar em modo CLI (mapeando o diretório atual para salvar o vídeo output_standalone_timelapse.mp4)
docker run --rm --env-file .env -v .:/app camera-timelapse-rust:latest --cli
```

### B) Modo Worker (Escutando a Fila SQS em Produção)
```bash
docker run --rm --env-file .env camera-timelapse-rust:latest
```

---

## 📦 2. Execução via NPM (Atalhos no `package.json`)

Se você possui Node.js instalado, pode usar os atalhos declarados em [`package.json`](../package.json):

| Comando | Descrição |
| :--- | :--- |
| `npm run docker:build` | Constrói a imagem Docker localmente |
| `npm run docker:standalone` | Executa o teste de todo o período no SFTP via Docker |
| `npm run docker:compose` | Inicia o container em background escutando a fila SQS |
| `npm run standalone` | Executa nativamente via `cargo run --release -- --cli` |
| `npm run start` | Executa nativamente escutando a fila SQS |

```bash
# Exemplo rápido usando Docker via NPM:
npm run docker:build
npm run docker:standalone
```

---

## 🐙 3. Execução via Docker Compose

O arquivo [`docker-compose.yml`](../docker-compose.yml) permite subir o container de forma declarativa.

```bash
# Subir o microserviço como Worker de segundo plano:
docker compose up -d

# Ver os logs do container:
docker compose logs -f

# Rodar o modo Standalone CLI via Compose:
docker compose run --rm camera-timelapse --cli

# Encerrar os serviços:
docker compose down
```

---

## 💻 4. Execução Nativa (Cargo CLI)

Caso você possua o ecossistema Rust e o `ffmpeg` instalados na sua máquina:

```bash
# Compilar e rodar em modo Standalone CLI (Desenvolvimento)
cargo run -- --cli

# Compilar e rodar em modo Worker SQS (Desenvolvimento)
cargo run

# Compilar release otimizada
cargo build --release
```

---

## 🔐 Variáveis de Ambiente Necessárias (`.env`)

Certifique-se de configurar o arquivo `.env` baseado no [`.env.example`](../.env.example):

* `RTSP_ENC_KEY`: Chave AES-256-GCM de 64 caracteres hex para descriptografia das credenciais enviadas pelo banco/SQS da Fiscaliza API.
* `CLI_MODE`: `true` para rodar standalone ou `false` para escutar SQS.
* `SFTP_HOST`, `SFTP_PORT`, `SFTP_USERNAME`, `SFTP_PASSWORD`: Credenciais base do SFTP.
* `S3_ENDPOINT`, `S3_ACCESS_KEY_ID`, `S3_SECRET_ACCESS_KEY`, `S3_DEFAULT_BUCKET`: Credenciais do armazenamento S3/MinIO.
