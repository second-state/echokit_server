use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum JsonCommand {
    ASR { text: String },
    Action { action: String },
    StartAudio,
    EndAudio,
    StartVideo,
    EndVideo,
    InitSetting,
}

#[test]
fn test_json_command() {
    let json = r#"{"type":"Action","action":"say"}"#;
    let cmd: JsonCommand = serde_json::from_str(json).unwrap();
    match cmd {
        JsonCommand::Action { action } => {
            assert_eq!(action, "say");
        }
        _ => panic!("Unexpected command: {:?}", cmd),
    }
    let json = r#"{"type":"StartAudio"}"#;
    let cmd: JsonCommand = serde_json::from_str(json).unwrap();
    match cmd {
        JsonCommand::StartAudio => {}
        _ => panic!("Unexpected command: {:?}", cmd),
    }
    let json = r#"{"type":"EndAudio"}"#;
    let cmd: JsonCommand = serde_json::from_str(json).unwrap();
    match cmd {
        JsonCommand::EndAudio => {}
        _ => panic!("Unexpected command: {:?}", cmd),
    }
    let json = r#"{"type":"StartVideo"}"#;
    let cmd: JsonCommand = serde_json::from_str(json).unwrap();
    match cmd {
        JsonCommand::StartVideo => {}
        _ => panic!("Unexpected command: {:?}", cmd),
    }
    let json = r#"{"type":"EndVideo"}"#;
    let cmd: JsonCommand = serde_json::from_str(json).unwrap();
    match cmd {
        JsonCommand::EndVideo => {}
        _ => panic!("Unexpected command: {:?}", cmd),
    }
}
