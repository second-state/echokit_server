use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ServerEvent {
    // set Hello
    HelloStart,
    HelloChunk { data: Vec<u8> },
    HelloEnd,

    ASR { text: String },
    Action { action: String },
    StartAudio { text: String },
    AudioChunk { data: Vec<u8> },
    EndAudio,
    StartVideo,
    EndVideo,
    EndResponse,
}

#[test]
fn test_rmp_command() {
    let event = ServerEvent::Action {
        action: "say".to_string(),
    };
    let data = rmp_serde::to_vec(&event).unwrap();
    println!("Serialized data: {:?}", data);
    println!("Serialized data: {}", String::from_utf8_lossy(&data));
    let data = rmp_serde::to_vec_named(&event).unwrap();
    println!("Serialized data: {:?}", data);
    println!("Serialized data: {}", String::from_utf8_lossy(&data));
    let cmd: ServerEvent = rmp_serde::from_slice(&data).unwrap();
    match cmd {
        ServerEvent::Action { action } => {
            assert_eq!(action, "say");
        }
        _ => panic!("Unexpected command: {:?}", cmd),
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "event")]
pub enum ClientCommand {
    StartRecord,
    StartChat,
    Submit,
    Text { input: String },
}

#[test]
fn test_rmp_client_command() {
    let cmd = ClientCommand::Text {
        input: "Hello".to_string(),
    };
    let data = serde_json::to_string(&cmd).unwrap();
    println!("Serialized data: {}", data);
    let cmd2: ClientCommand = serde_json::from_str(&data).unwrap();
    match cmd2 {
        ClientCommand::Text { input } => {
            assert_eq!(input, "Hello");
        }
        _ => panic!("Unexpected command: {:?}", cmd2),
    }
}
