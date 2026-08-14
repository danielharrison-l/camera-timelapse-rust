use anyhow::{anyhow, Result};
use aws_config::BehaviorVersion;
use aws_sdk_sqs::Client as SqsClient;
use std::sync::Arc;
use tracing::{info, warn};

use crate::config::Config;
use crate::models::{InboundTimelapseEvent, OutboundTimelapseEvent};

#[derive(Clone)]
pub struct SqsService {
    client: SqsClient,
    config: Arc<Config>,
}

pub struct SqsMessage {
    pub receipt_handle: String,
    pub event: InboundTimelapseEvent,
}

impl SqsService {
    pub async fn new(config: Arc<Config>) -> Result<Self> {
        let mut aws_config_builder = aws_config::defaults(BehaviorVersion::latest())
            .region(aws_config::Region::new(config.aws_region.clone()));

        if let Some(ref endpoint) = config.aws_endpoint_url {
            aws_config_builder = aws_config_builder.endpoint_url(endpoint);
        }

        let aws_config = aws_config_builder.load().await;
        let client = SqsClient::new(&aws_config);

        Ok(Self { client, config })
    }

    pub async fn poll_inbound_messages(&self) -> Result<Vec<SqsMessage>> {
        let resp = self
            .client
            .receive_message()
            .queue_url(&self.config.sqs_inbound_queue_url)
            .max_number_of_messages(1)
            .visibility_timeout(self.config.sqs_visibility_timeout)
            .wait_time_seconds(self.config.sqs_wait_time_seconds)
            .send()
            .await?;

        let mut parsed_messages = Vec::new();

        if let Some(messages) = resp.messages {
            for msg in messages {
                let receipt_handle = msg.receipt_handle.unwrap_or_default();
                let body = msg.body.unwrap_or_default();

                match serde_json::from_str::<InboundTimelapseEvent>(&body) {
                    Ok(event) => {
                        parsed_messages.push(SqsMessage {
                            receipt_handle,
                            event,
                        });
                    }
                    Err(e) => {
                        warn!("SQS: Payload JSON de entrada é inválido: {}. Descarta mensagem da fila.", e);
                        let _ = self.delete_inbound_message(&receipt_handle).await;
                    }
                }
            }
        }

        Ok(parsed_messages)
    }

    pub async fn delete_inbound_message(&self, receipt_handle: &str) -> Result<()> {
        self.client
            .delete_message()
            .queue_url(&self.config.sqs_inbound_queue_url)
            .receipt_handle(receipt_handle)
            .send()
            .await
            .map_err(|e| anyhow!("Erro ao apagar mensagem da fila Inbound SQS: {}", e))?;
        Ok(())
    }

    pub async fn publish_outbound_event(&self, event: &OutboundTimelapseEvent) -> Result<()> {
        let payload_json = serde_json::to_string(event)?;

        info!(
            "SQS Outbound: Publicando evento de conclusão do job '{}' (Status: {}) em {}",
            event.job_id, event.status, self.config.sqs_outbound_queue_url
        );

        self.client
            .send_message()
            .queue_url(&self.config.sqs_outbound_queue_url)
            .message_body(payload_json)
            .send()
            .await
            .map_err(|e| anyhow!("Erro ao enviar mensagem para a fila Outbound SQS: {}", e))?;

        Ok(())
    }
}
