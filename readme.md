# tokio-tungstenite-cex-ws-examples

you should describe explicitly tls features on cargo.toml to use tls connection (wss for websocket).

For example,

```toml
tokio-tungstenite = { version = "0.26", features = ["native-tls"] }
```