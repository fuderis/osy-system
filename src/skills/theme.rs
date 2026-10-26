use crate::prelude::*;

use anylm::Schema;
use system_utils::{SystemTheme, ThemeStyle};

#[derive(Deserialize, Schema)]
pub struct SetAction {
    /// Target theme style.
    #[schema(serde_json)]
    #[schema(variants = ["light", "dark"])]
    style: ThemeStyle,
}

#[log()]
pub async fn handle_get(tx: Sender<Bytes>) -> Result<()> {
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

#[log(style = %query.payload.style)]
pub async fn handle_set(tx: Sender<Bytes>, query: ToolQuery<SetAction>) -> Result<()> {
    let ToolQuery { payload, .. } = query;

    match SystemTheme::switch(payload.style.clone()).await {
        Ok(_) => {
            let msg = format!("System theme switched into `{}` style.", payload.style);
            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            Ok(())
        }
        Err(e) => Err(format!("Failed to switch system theme: {e}").into()),
    }
}
