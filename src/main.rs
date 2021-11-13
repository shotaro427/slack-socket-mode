use std::env;

use async_std::stream::StreamExt;
use dotenv::dotenv;
use futures_util::{sink::SinkExt};

#[derive(serde::Deserialize, Debug)]
pub struct OpenConnectionResponse {
    pub ok: bool,
    pub url: Option<String>,
    pub error: Option<String>
}

pub async fn open_connection(token: &str) -> surf::Result<OpenConnectionResponse> {
    surf::post("https://slack.com/api/apps.connections.open")
    .header(surf::http::headers::AUTHORIZATION, format!("Bearer {}", token))
    .recv_json().await
}

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum SocketModeMessage<'s> {
    Hello {  },
    Disconnect { reason: &'s str },
    SlashCommands { 
        envelope_id: &'s str,
        payload: SocketModeMessagePayload<'s>
    },
    Interactive { envelope_id: &'s str, }
}

#[derive(serde::Deserialize, Debug)]
pub struct SocketModeMessagePayload<'s> {
    pub token: &'s str,
    pub team_id: &'s str,
    pub team_domain: &'s str,
    pub channel_id: &'s str,
    pub channel_name: &'s str,
    pub user_id: &'s str,
    pub user_name: &'s str,
    pub command: String,
    pub text: &'s str,
    pub api_app_id: &'s str,
    pub is_enterprise_install: &'s str,
    pub response_url: String,
    pub trigger_id: &'s str,
}

#[derive(serde::Serialize)]
pub struct SocketModeAcknowledgeMessage<'s> {
    pub envelope_id: &'s str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<&'s Payload<'s>>
}

#[derive(serde::Serialize)]
pub struct Payload<'s> {
    pub text: &'s str,
    pub response_type: &'s str,
    pub blocks: Vec<Block<'s>>
}

#[derive(serde::Serialize)]
pub struct Block<'s> {
    pub text: &'s Text<'s>,
    pub r#type: &'s str,
    pub accessory: Option<&'s Accessory<'s>>
}

#[derive(serde::Serialize)]
pub struct Text<'s> {
    pub r#type: &'s str,
    pub text: &'s str,
}

#[derive(serde::Serialize)]
pub struct Accessory<'s> {
    pub r#type: &'s str,
    pub text: &'s Text<'s>,
    pub value: &'s str,
    pub action_id: &'s str,
}

#[async_std::main]
async fn main() {
    dotenv().ok();
    let tok = env::var("SLACK_APP_TOKEN").expect("falied to load env file");

    let con_result = open_connection(&tok).await.expect("failed to request apps.connections.open");
    if !con_result.ok {
        panic!("app.connections.open failed: {}", con_result.error.as_deref().unwrap_or("Unknow error"));
    }
    let full_url = con_result.url.expect("no url passed from server");
    let url = url::Url::parse(&full_url).expect("failed to parse entrypoint url");
    let domain = url.domain().expect("no domain name?");

    print!("full_url {}", full_url);

    let tcp_stream = async_std::net::TcpStream::connect(&format!("{}:443", domain)).await.expect("failed to connect tcp stream");
    let ecn_stream = async_tls::TlsConnector::default().connect(domain, tcp_stream).await.expect("failed to connect entrypoint stream");
    let (mut stream, _) = async_tungstenite::client_async(full_url, ecn_stream).await.expect("failed to connect websocket");

    while let Some(m) = stream.next().await {
        match m.expect("failed to decode websocket frame") {
            tungstenite::Message::Text(t) => match serde_json::from_str(&t) {
                Ok(SocketModeMessage::Hello { .. }) => { println!("Hello: {} \n", t) }
                Ok(SocketModeMessage::Disconnect { reason, .. }) => { println!("Disconnect request: {}", reason); break; },
                Ok(SocketModeMessage::SlashCommands { envelope_id, payload }) => {
                    println!("Slash Command Message: {}\n", envelope_id);
                    println!("Slash Command Payload: {:?}\n", &payload);
                    let payload = Payload {
                        text: "",
                        response_type: "ephemeral",
                        blocks: vec![
                            Block {
                                text: &Text { 
                                    r#type: "mrkdwn", 
                                    text: "hogehoge" 
                                }, 
                                r#type: "section", 
                                accessory: Some(&Accessory {
                                    r#type: "button",
                                    text: &Text {
                                        r#type: "plain_text",
                                        text: "Click Me."
                                    },
                                    value: "click-me",
                                    action_id: "button-action"
                                })
                            }
                        ]
                    };

                    let res = serde_json::to_string(
                        &SocketModeAcknowledgeMessage { envelope_id, payload: Some(&payload) }
                    ).expect("failed to reply ack message");

                    println!("res {}\n", &res);
                    stream.send(
                        tungstenite::Message::Text(res)
                    ).await.expect("failed to reply ack message");
                },
                Ok(SocketModeMessage::Interactive { envelope_id, .. }) => {
                    println!("Block Actions {}", t);
                    println!("Block Actions {}\n", envelope_id);

                    let res = serde_json::to_string(
                        &SocketModeAcknowledgeMessage { envelope_id, payload: None }
                    ).expect("failed to reply ack message");
                    stream.send(
                        tungstenite::Message::Text(res)
                    ).await.expect("failed to reply ack message");
                }
                Err(e) => { println!("Unknow text frame: {}: {:?} \n", t, e) }
            },
            tungstenite::Message::Ping(_bytes) => {},
            _ => println!("Unknow frame")
        }
    }
}
