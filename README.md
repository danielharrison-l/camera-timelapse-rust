# 🦀 Camera Timelapse Service (Rust Event-Driven Microservice)

Microserviço **100% isolado, sem banco de dados (database-less)** desenvolvido em **Rust (Tokio Async)** para processamento contínuo de vídeos Timelapse de alta performance.

O microserviço consome requisições da fila **AWS SQS Inbound (`camera-timelapse-inbound`)** geradas pela **Fiscaliza API**, realiza o download ultra-rápido das fotos via **SFTP Pipelining (93.7 MB/s)**, codifica o vídeo H.264 via **Multi-Core Parallel Chunk Encoding (450+ FPS)**, envia o arquivo MP4 para o **AWS S3** e emite um evento de conclusão na fila **AWS SQS Outbound (`camera-timelapse-outbound`)** para vincular à Fiscaliza API.

```
Fiscaliza API  ──(SQS Inbound)──>  camera-timelapse-rust  ──(SFTP Download)──>  FFmpeg Multi-Core
                                         │                                           │
Fiscaliza API  <──(SQS Outbound)─────────┴───────────────(S3 Upload)─────────────────┘
```

---

## ⚡ Performance & Destaques de Arquitetura

- **Sem Banco de Dados (Database-Less)**: Zero dependências SQL/NoSQL. Toda a persistência de metadados e vínculos de arquivos é delegada para a Fiscaliza API através da fila de saída.
- **Async Socket Windowed Read Pipelining**: Dispara requisições `READ` em rajada contínua no socket TCP sem aguardar acks intermediários, atingindo **93,73 MB/s de vazão no SFTP**.
- **Multi-Core Chunk Parallelization**: Divide os frames em 4 blocos temporais encodados em paralelo por 4 subprocessos `ffmpeg` em formato `.ts`, fundidos instantaneamente em MP4 final (`-c copy`).
- **Consumo Mínimo de Recursos**: Pico de memória RAM de **~14,2 MB** e **0ms de pausas de Garbage Collector (Zero GC)**.

---

## 📥 1. Evento de Entrada (`camera-timelapse-inbound`)

Publicado pela **Fiscaliza API**:

```json
{
  "jobId": "job-8f921a-2026",
  "session": "2026_07_28-2026_07_28",
  "cameraId": "cam-garagem-01",
  "fps": 12,
  "scale": "1280:720",
  "limit": null,
  "s3Bucket": "cameras-videos-bucket",
  "s3Key": "videos/2026_07_28-2026_07_28.mp4"
}
```

---

## 📤 2. Evento de Saída (`camera-timelapse-outbound`)

Emitido pelo **Rust Microservice** ao finalizar o processamento (consumido pela Fiscaliza API):

```json
{
  "jobId": "job-8f921a-2026",
  "status": "COMPLETED",
  "session": "2026_07_28-2026_07_28",
  "cameraId": "cam-garagem-01",
  "s3Bucket": "cameras-videos-bucket",
  "s3Key": "videos/2026_07_28-2026_07_28.mp4",
  "s3Url": "http://localhost:4566/cameras-videos-bucket/videos/2026_07_28-2026_07_28.mp4",
  "fileSizeBytes": 10758421,
  "durationSeconds": 17.99,
  "totalFrames": 2454,
  "fps": 12,
  "processedAt": "2026-08-14T14:15:00Z",
  "error": null
}
```

---

## 🚀 Como Executar

### 1. Inicializar Filas SQS e Bucket S3 no LocalStack
```bash
./scripts/localstack.sh up
```

### 2. Rodar o Microserviço Rust
```bash
cargo run --release
```

### 3. Simular Envio de Requisição (Fiscaliza API -> Inbound SQS)
```bash
./scripts/localstack.sh send-inbound '{"jobId":"job-001","session":"2026_07_28-2026_07_28","fps":12}'
```

### 4. Ler o Evento de Conclusão na Fila Outbound SQS
```bash
./scripts/localstack.sh poll-outbound
```

---

## 📄 Licença
MIT - Desenvolvido para a plataforma Fiscaliza.
