use dioxus::prelude::*;
use serde_json::Value;
use std::time::Duration;

/// Keep a desktop eval channel alive until Rust has consumed its result. WebKit
/// can otherwise garbage-collect the channel between JS return and Rust polling.
pub async fn eval(script: &str, data: Option<Value>) -> Result<Value, String> {
    let script = format!(
        r#"
        try {{
            const value=await (async()=>{{ {script} }})();
            dioxus.send({{ok:true,value}});
        }} catch(error) {{
            dioxus.send({{ok:false,error:String(error)}});
        }}
        await dioxus.recv();
    "#
    );
    let mut channel = document::eval(&script);
    if let Some(data) = data {
        channel.send(data).map_err(|e| e.to_string())?;
    }
    let received = tokio::time::timeout(Duration::from_secs(30), channel.recv::<Value>()).await;
    // Releasing the JS channel is safe now; its result has already crossed IPC.
    let _ = channel.send(Value::Null);
    let response = received
        .map_err(|_| "The native view did not respond.".to_owned())?
        .map_err(|e| e.to_string())?;
    if response["ok"] == true {
        Ok(response["value"].clone())
    } else {
        Err(response["error"]
            .as_str()
            .unwrap_or("The native view action failed.")
            .to_owned())
    }
}
