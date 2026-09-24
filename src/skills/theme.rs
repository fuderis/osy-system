use crate::prelude::*;

use anylm::api::{Schema, Tool};
use system_utils::{SystemTheme, ThemeStyle};

pub fn tools_list() -> Vec<Tool> {
    vec![
        Tool::new("set", "Changes the system appearance theme.").required_property(
            "style",
            Schema::string("Target theme style.").variants(set!["light".into(), "dark".into()]),
        ),
        Tool::new("get", "Gets current system theme."),
    ]
}

#[derive(Deserialize)]
pub struct ThemeAction {
    style: ThemeStyle,
}

#[log(style = %action.style)]
pub async fn handle_set(tx: Sender<Bytes>, action: ThemeAction) -> Result<()> {
    match SystemTheme::switch(action.style.clone()).await {
        Ok(_) => {
            let msg = format!("System theme switched into `{}` style.", action.style);
            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            Ok(())
        }
        Err(e) => Err(format!("Failed to switch system theme: {e}").into()),
    }
}

#[log()]
pub async fn handle_get(tx: Sender<Bytes>, _action: JsonValue) -> Result<()> {
    match SystemTheme::current().await {
        Ok(style) => {
            let msg = format!("Current system theme: `{style}` style.");
            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            Ok(())
        }
        Err(e) => Err(format!("Failed to get system theme: {e}").into()),
    }
}
