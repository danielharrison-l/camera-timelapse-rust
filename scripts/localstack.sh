#!/usr/bin/env bash
# Sobe um LocalStack com SQS (Inbound/Outbound) + S3 e utilitarios.
#
#   ./scripts/localstack.sh up              # sobe filas e bucket
#   ./scripts/localstack.sh send-inbound '{"jobId":"job-001","session":"2026_07_28-2026_07_28","fps":12}'
#   ./scripts/localstack.sh poll-outbound   # lê os eventos publicados na fila de saída
#   ./scripts/localstack.sh ls              # lista os objetos gravados no S3
#   ./scripts/localstack.sh down            # derruba o container
set -euo pipefail

CONTAINER=camera-timelapse-localstack
ENDPOINT=http://localhost:4566
INBOUND_QUEUE_NAME=camera-timelapse-inbound
OUTBOUND_QUEUE_NAME=camera-timelapse-outbound
BUCKET=cameras-videos-bucket

INBOUND_QUEUE_URL="http://sqs.us-east-1.localhost.localstack.cloud:4566/000000000000/${INBOUND_QUEUE_NAME}"
OUTBOUND_QUEUE_URL="http://sqs.us-east-1.localhost.localstack.cloud:4566/000000000000/${OUTBOUND_QUEUE_NAME}"

export AWS_ACCESS_KEY_ID=test
export AWS_SECRET_ACCESS_KEY=test
export AWS_DEFAULT_REGION=us-east-1

aws_local() { aws --endpoint-url "$ENDPOINT" "$@"; }

case "${1:-up}" in
  up)
    if ! docker ps --filter "name=$CONTAINER" --format '{{.Names}}' | grep -q "$CONTAINER"; then
      docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
      docker run -d --name "$CONTAINER" -p 4566:4566 -e SERVICES=sqs,s3 \
        localstack/localstack:3 >/dev/null
    fi

    printf 'aguardando localstack'
    for _ in $(seq 1 60); do
      if curl -s "$ENDPOINT/_localstack/health" 2>/dev/null | grep -q '"sqs"'; then
        echo " pronto"
        break
      fi
      printf '.'
      sleep 2
    done

    aws_local sqs create-queue --queue-name "$INBOUND_QUEUE_NAME" --attributes VisibilityTimeout=1800 >/dev/null 2>&1 || true
    aws_local sqs create-queue --queue-name "$OUTBOUND_QUEUE_NAME" --attributes VisibilityTimeout=1800 >/dev/null 2>&1 || true
    aws_local s3api create-bucket --bucket "$BUCKET" >/dev/null 2>&1 || true

    echo "Fila Entrada (Inbound)  : $INBOUND_QUEUE_URL"
    echo "Fila Saída (Outbound)   : $OUTBOUND_QUEUE_URL"
    echo "Storage S3 Target       : s3://$BUCKET"
    echo
    echo "Agora você pode rodar o microserviço:"
    echo "  cargo run --release"
    ;;

  send-inbound)
    body="${2:?informe o corpo JSON da mensagem}"
    aws_local sqs send-message --queue-url "$INBOUND_QUEUE_URL" \
      --message-body "$body" --query MessageId --output text
    ;;

  poll-outbound)
    aws_local sqs receive-message --queue-url "$OUTBOUND_QUEUE_URL" \
      --max-number-of-messages 10 --wait-time-seconds 2
    ;;

  ls)
    aws_local s3api list-objects-v2 --bucket "$BUCKET" \
      --query 'Contents[].{Key:Key,Size:Size}' --output table 2>/dev/null \
      || echo "bucket vazio"
    ;;

  down)
    docker rm -f "$CONTAINER" >/dev/null 2>&1 && echo "localstack removido"
    ;;

  *)
    echo "uso: $0 {up|send-inbound <json>|poll-outbound|ls|down}" >&2
    exit 1
    ;;
esac
