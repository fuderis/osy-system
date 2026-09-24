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
            let msg = format!("System theme switched into `{}` mode.", action.style);
            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            Ok(())
        }
        Err(e) => Err(format!("Switching system theme failed: {e}").into()),
    }
}

#[log()]
pub async fn handle_get(_tx: Sender<Bytes>, _action: JsonValue) -> Result<()> {
    // TODO: write get_theme tool
    Err(Error::Custom(str!("Action `get theme` is not implemented yet.")).into())
}
