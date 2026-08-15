# Criptografia & Integração com Fiscaliza API

Este documento detalha o protocolo de criptografia e a resolução dinâmica de credenciais de acesso ao SFTP utilizadas no microserviço.

---

## 🔒 Modelo de Credenciais do SFTP no Fiscaliza API

No **Fiscaliza API**, as credenciais de acesso aos servidores SFTP das câmeras **não são variáveis de ambiente estáticas**. Elas são mantidas de forma segura na entidade de banco de dados (`CameraEntity`):
- `sftp_host`
- `sftp_port`
- `sftp_username` (Criptografado no banco)
- `sftp_password` (Criptografado no banco)

Quando a aplicação NestJS solicita a geração de um timelapse, os dados de acesso ao SFTP da câmera específica são enviados criptografados dentro da mensagem do **AWS SQS**.

---

## 🔑 Descriptografia AES-256-GCM (`RTSP_ENC_KEY`)

O microserviço Rust utiliza o módulo [`src/crypto.rs`](../src/crypto.rs) para descriptografar os campos `sftp_username` e `sftp_password` utilizando exatamente o mesmo padrão do NestJS (`RtspCipher`):

### Chave de Criptografia (`RTSP_ENC_KEY`)
- **Algoritmo**: `AES-256-GCM`
- **Tamanho da Chave**: 256 bits (64 caracteres hexadecimais).
- **Variável de Ambiente**: `RTSP_ENC_KEY`

### Formato do Payload Criptografado (`v1`)
As credenciais criptografadas vêm formatadas como uma string codificada em formato v1:
```text
v1:<IV_BASE64>:<AUTH_TAG_BASE64>:<CIPHERTEXT_BASE64>
```

### Exemplo de Payload no SQS
```json
{
  "camera_id": "cam_garagem_01",
  "start_date": "2026-08-01T00:00:00Z",
  "end_date": "2026-08-07T23:59:59Z",
  "sftp_host": "sftpgo.dev.vision360.app.br",
  "sftp_port": 22,
  "sftp_username": "v1:IV_HEX_OU_BASE64:TAG:CIPHERTEXT",
  "sftp_password": "v1:IV_HEX_OU_BASE64:TAG:CIPHERTEXT"
}
```

Se o campo não iniciar com `v1:`, o microserviço trata a credencial diretamente como texto plano, garantindo retrocompatibilidade para testes.
