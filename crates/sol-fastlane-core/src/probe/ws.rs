use crate::error::{FastlaneError, Result};
use crate::types::WsSlotSample;
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, error, info, warn};

/// WebSocket probe for tracking real-time slot propagation and jitter.
pub struct WsSlotProbe {
    endpoint: String,
    ws_url: String,
}

impl WsSlotProbe {
    pub fn new(endpoint: impl Into<String>, ws_url: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            ws_url: ws_url.into(),
        }
    }

    pub fn ws_url(&self) -> &str {
        &self.ws_url
    }

    /// Spawns a background task that streams slot notifications into the provided channel.
    /// Returns a JoinHandle that can be awaited or aborted.
    pub fn start_stream(
        &self,
        tx: mpsc::Sender<WsSlotSample>,
        mut shutdown_rx: tokio::sync::broadcast::Receiver<()>,
    ) -> tokio::task::JoinHandle<()> {
        let ws_url = self.ws_url.clone();
        let endpoint = self.endpoint.clone();

        tokio::spawn(async move {
            info!("Starting WebSocket slot subscription on {}", ws_url);

            // Security check: validate websocket protocol scheme
            if !ws_url.starts_with("ws://") && !ws_url.starts_with("wss://") {
                error!("Invalid WebSocket scheme for URL: {}. Must start with ws:// or wss://", ws_url);
                return;
            }

            let mut last_arrival: Option<Instant> = None;
            let expected_slot_interval_ms = 400.0; // Solana target block time is ~400ms

            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        debug!("Shutdown received for WS slot probe {}", ws_url);
                        break;
                    }
                    conn_result = connect_async(&ws_url) => {
                        match conn_result {
                            Ok((ws_stream, _)) => {
                                info!("Connected to WebSocket {}", ws_url);
                                let (mut write, mut read) = ws_stream.split();

                                let sub_msg = json!({
                                    "jsonrpc": "2.0",
                                    "id": 1,
                                    "method": "slotSubscribe"
                                });

                                if let Err(e) = write.send(Message::Text(sub_msg.to_string().into())).await {
                                    warn!("Failed to send slotSubscribe on {}: {}", ws_url, e);
                                    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                                    continue;
                                }

                                while let Some(msg_result) = read.next().await {
                                    match msg_result {
                                        Ok(Message::Text(text)) => {
                                            let now = Instant::now();
                                            let jitter_ms = if let Some(last) = last_arrival {
                                                let delta = (now.duration_since(last).as_secs_f64() * 1000.0) - expected_slot_interval_ms;
                                                delta.abs()
                                            } else {
                                                0.0
                                            };
                                            last_arrival = Some(now);

                                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                                                if let Some(params) = v.get("params") {
                                                    if let Some(res) = params.get("result") {
                                                        let slot = res.get("slot").and_then(|s| s.as_u64()).unwrap_or(0);
                                                        let parent = res.get("parent").and_then(|s| s.as_u64()).unwrap_or(0);
                                                        let root = res.get("root").and_then(|s| s.as_u64()).unwrap_or(0);

                                                        let sample = WsSlotSample {
                                                            endpoint: endpoint.clone(),
                                                            timestamp: Utc::now(),
                                                            slot,
                                                            parent_slot: parent,
                                                            root_slot: root,
                                                            lag_slots: 0, // Computed downstream relative to cluster tip
                                                            jitter_ms,
                                                        };

                                                        if tx.send(sample).await.is_err() {
                                                            debug!("Receiver dropped, stopping WS probe for {}", ws_url);
                                                            return;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        Ok(Message::Ping(p)) => {
                                            let _ = write.send(Message::Pong(p)).await;
                                        }
                                        Ok(Message::Close(_)) => {
                                            warn!("WS closed by server for {}", ws_url);
                                            break;
                                        }
                                        Err(e) => {
                                            warn!("WS read error on {}: {}", ws_url, e);
                                            break;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Failed to connect to WS {}: {}", ws_url, e);
                                tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;
                            }
                        }
                    }
                }

                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
        })
    }

    /// Helper to test if WS endpoint connects and completes handshake.
    pub async fn test_connection(&self) -> Result<()> {
        let (ws_stream, _) = connect_async(&self.ws_url)
            .await
            .map_err(|e| FastlaneError::WebSocket {
                endpoint: self.endpoint.clone(),
                message: e.to_string(),
            })?;
        let (mut write, _) = ws_stream.split();
        let _ = write.close().await;
        Ok(())
    }
}
