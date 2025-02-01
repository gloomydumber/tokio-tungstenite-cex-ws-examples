use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::connect_async;
use tokio::sync::mpsc;
use tokio::time::{sleep, interval, Duration};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tracing::{info, warn, error};
use tracing_subscriber::{fmt, Layer, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use chrono::DateTime;
use chrono_tz::Asia::Seoul;
use uuid::Uuid;
use bytes::Bytes;
use colored::Colorize;

#[tokio::main]
async fn main() {
    // Create a file writer for logging
    let log_file = std::fs::File::create("app.log").expect("Failed to create log file");

    // Terminal logging layer (general logs)
    let terminal_layer = fmt::layer()
        .with_writer(std::io::stdout) // Output logs to terminal
        .with_ansi(true)
        .with_filter(EnvFilter::new("info"));

    // File logging layer
    let file_layer = fmt::layer()
        .with_writer(move || log_file.try_clone().expect("Failed to clone log file"))
        .with_ansi(true)
        .with_filter(
            EnvFilter::new("info")
            .add_directive("terminal_only=off".parse().unwrap())
        );

    // Set up tracing-subscriber for both terminal and file logging
    tracing_subscriber::registry()
        .with(terminal_layer)
        .with(file_layer)
        .init();

    // Main logic
    let app_task = tokio::spawn(run_websocket());

    // Wait for Ctrl+C to gracefully shut down
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for Ctrl+C");

    // Graceful shutdown
    info!("{}", "Shutting down...".yellow());
    app_task.abort(); // Cancel the main task
    info!("{}", "Logs flushed, exiting.".yellow());
}

async fn run_websocket() {
    loop {
        let url = "wss://api.upbit.com/websocket/v1";

        let ws_stream = match connect_async(url).await {
            Ok((stream, _)) => {
                info!("{}", "WebSocket connection established".blue().bold());
                stream
            }
            Err(e) => {
                error!("WebSocket connection failed: {:?}. Retrying in 5 seconds.", e);
                sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        let (mut write, mut read) = ws_stream.split();
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();

        let writer_task = tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                if let Err(e) = write.send(message).await {
                    error!("Error sending message: {:?}", e);
                    break;
                }
            }
        });

        let subscription_message: serde_json::Value = json!([
            { "ticket": Uuid::new_v4().to_string() },
            { "type": "ticker", "codes": ["KRW-BTC"] }
        ]);

        let message: Message = Message::Text(subscription_message.to_string().into());
        if tx.send(message).is_err() {
            error!("Failed to send subscription message");
            break;
        }
        info!("{}", "Subscription message sent".blue());

        // TODO: Handle the case where no response is received after sending a ping frame for N seconds. This may indicate a disconnection from the server.
        // 2025-01-23T03:05:56.036323Z ERROR tokio_tungstenite_cex_ws_examples: Unknown WebSocket error: Io(Os { code: 10054, kind: ConnectionReset, message: "현재 연결은 원격 호스트에 의해 강제로 끊겼습니다." })
        let ping_tx = tx.clone();
        let ping_task = tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(30));
            loop {
                interval.tick().await;
                if ping_tx.send(Message::Ping(Bytes::from(vec![]))).is_err() {
                    error!("Failed to send ping frame");
                    break;
                }
                info!("{}", "Ping frame sent".yellow().bold());
            }
        });

        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Pong(bin)) => {
                    if bin.is_empty() {
                        warn!("{}", "Received empty PONG".yellow().bold());
                    } else if let Ok(text) = String::from_utf8(bin.to_vec()) {
                        info!("Received PONG (as text): {}", text);
                    } else {
                        warn!("Received PONG (binary data): {:?}", bin);
                    }
                }
                Ok(Message::Text(text)) => {
                    info!("Received: {}", text);
                }
                Ok(Message::Binary(bin)) => {
                    if let Ok(text) = String::from_utf8(bin.to_vec()) {
                        if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
                            let trade_price = parsed.get("trade_price");
                            let trade_timestamp = parsed
                                .get("trade_timestamp")
                                .unwrap()
                                .as_i64()
                                .unwrap();
                            let readable_timestamp =
                                DateTime::from_timestamp_millis(trade_timestamp).unwrap();
                            let kst_timestamp = readable_timestamp.with_timezone(&Seoul);

                            if let Some(price) = trade_price {
                                // tracing::event!(
                                //     target: "terminal_only",
                                //     tracing::Level::INFO,
                                //     "{}: {}, {}: {}",
                                //     "Trade Price".green().bold(),
                                //     price,
                                //     "Trade Timestamp".cyan().bold(),
                                //     kst_timestamp
                                // );
                                info!(
                                    target: "terminal_only",
                                    "{}: {}, {}: {}",
                                    "Trade Price".green().bold(),
                                    price,
                                    "Trade Timestamp".cyan().bold(),
                                    kst_timestamp
                                );
                            } else {
                                error!("Missing keys 'trade_price' or 'trade_timestamp'");
                            }
                        } else {
                            error!("Failed to parse JSON, Original Text: {}", text);
                        }
                    } else {
                        error!("Received non-UTF8 Binary Data: {:?}", bin);
                    }
                }
                Ok(Message::Close(reason)) => {
                    warn!("Connection closed, reason: {:?}", reason);
                    break;
                }
                Ok(_) => {
                    warn!("Received unexpected WebSocket message");
                }
                Err(e) => {
                    error!("Unknown WebSocket error: {:?}", e);
                    break;
                }
            }
        }

        writer_task.abort();
        ping_task.abort();
        warn!("Reconnecting to WebSocket in 5 seconds...");
        sleep(Duration::from_secs(5)).await;
    }
}
