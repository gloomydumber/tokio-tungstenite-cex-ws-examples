use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::connect_async;
use tokio::sync::mpsc;
use tokio::time::{sleep, interval, Duration};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use chrono::DateTime;
use chrono_tz::Asia::Seoul;
use colored::Colorize;
use bytes::Bytes;

#[tokio::main]
async fn main() {
    // WebSocket URL for Upbit API
    let url = "wss://api.upbit.com/websocket/v1";

    // Establish a WebSocket connection
    // let (ws_stream, _) = connect_async(url).await?;

    // let (ws_stream, _) = connect_async(url)
    // .await
    // .unwrap_or_else(|e| panic!("Connection failed: {:?}", e));

    let ws_stream = loop {
        match connect_async(url).await {
            Ok((stream, _)) => {
                print_warning("WebSocket Connection Established");
                break stream;
            }
            Err(e) => {
                print_error("WebSocket Connection Failed, Retrying after 5 Seconds", Some(Box::new(e)));
                sleep(Duration::from_secs(5)).await;
            }
        }
    };

    let (mut write, mut read) = ws_stream.split();

    // Create an unbounded channel for sending messages to the WebSocket writer
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();

    // Spawn a task to handle writing messages
    tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            if let Err(e) = write.send(message).await {
                print_error("Error Sending Message", Some(Box::new(e)));
                break;
            }
        }
    });

    // Subscription message
    let subscription_message: serde_json::Value = json!([
        { "ticket": "unique-ticket" },
        { "type": "ticker", "codes": ["KRW-BTC"] }
    ]);

    // Send the subscription message using the channel
    let message: Message = Message::Text(subscription_message.to_string().into());
    tx.send(message).expect("Failed to send subscription message");
    print_info("Subscription message sent");

    // Spawn a task to send periodic ping frames
    let ping_tx = tx.clone();
    tokio::spawn(async move {
        let mut interval = interval(Duration::from_secs(30)); // Adjust interval as needed
        loop {
            interval.tick().await;
            if ping_tx.send(Message::Ping(Bytes::from(vec![]))).is_err() {
                print_error("Failed to send ping frame", None);
                break;
            }
            println!("{}", "Ping frame sent".yellow());
        }
    });

    // Read messages from the WebSocket
    while let Some(msg) = read.next().await {
        match msg {
            Ok(Message::Pong(bin)) => {
                if bin.is_empty() {
                    println!("{}", "Received empty PONG".bright_yellow().bold());
                } else if let Ok(text) = String::from_utf8(bin.to_vec()) {
                    println!(
                        "{}: {}",
                        "Received PONG (as text)".bright_yellow().bold(),
                        text
                    );
                } else {
                    println!(
                        "{}: {:?}",
                        "Received PONG (binary data)".bright_yellow().bold(),
                        bin
                    );
                }
            }
            Ok(Message::Text(text)) => {
                println!("{}: {}", "Received".green().bold(), text);
            }
            Ok(Message::Binary(bin)) => {
                if let Ok(text) = String::from_utf8(bin.to_vec()) {
                    if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
                        let trade_price = parsed.get("trade_price");
                        let trade_timestamp = parsed.get("trade_timestamp").unwrap().as_i64().unwrap();
                        let readable_timestamp = DateTime::from_timestamp_millis(trade_timestamp).unwrap();
                        let kst_timestamp = readable_timestamp.with_timezone(&Seoul);

                        if let Some(price) = trade_price {
                            println!(
                                "{}: {}, {}: {}",
                                "Trade Price".green().bold(),
                                price,
                                "Trade Timestamp".cyan().bold(),
                                kst_timestamp
                            );
                        } else {
                            print_error("Missing keys 'trade_price' or 'trade_timestamp'", None);
                        }
                    } else {
                        print_error("Failed to parse JSON, Original Text:", None);
                        println!("{}", text);
                    }
                } else {
                    print_error("Received non-UTF8 Binary Data, Binary Data:", None);
                    println!("{:?}", bin);
                }
            }
            Ok(Message::Close(reason)) => {
                print_warning("Connection Closed, Reason:");
                println!("{:?}", reason);
                break;
            }
            Ok(_) => {
                print_warning("Received unexpected WebSocket message");
            }
            Err(e) => {
                print_error("Unknown WebSocket Error", Some(Box::new(e)));
                break;
            }
        }
    }
}

fn print_info(msg: &str) {
    println!("{}", msg.blue());
}

fn print_warning(msg: &str) {
    println!("{}", msg.yellow());
}

fn print_error(msg: &str, error: Option<Box<dyn std::error::Error>>) {
    match error {
        Some(err) => {
            println!("{}: {}", msg, err);
        },
        None => {
            println!("{}", msg);
        }
    }
}