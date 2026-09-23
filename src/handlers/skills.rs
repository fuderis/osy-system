use crate::{prelude::*, skills::SkillKind};
use osy_share::SkillExt;

/// API: Handles the skills list receiving
#[log()]
pub async fn handle_skills_list() -> Response {
    let skills = SkillKind::skills_list();
    Response::ok().json(&skills)
}

/// API: Handles the tools list receiving
#[log()]
pub async fn handle_tools_list(skill: Paths<SkillKind>) -> Response {
    let tools = skill.tools_list();
    Response::ok().json(&tools)
}

/// API: Handles the agent tool call
#[log(skill = %paths.0, tool = %paths.1)]
pub async fn handle_tool_call(
    Paths(paths): Paths<(SkillKind, String)>,
    payload: Json<JsonValue>,
) -> Response {
    let (skill, tool) = paths;
    info!("Initialized the `{skill}.{tool}` tool handling");

    Response::ok().stream(async move |tx| {
        if let Err(e) = skill.tool_call(tx.clone(), tool, payload.0).await {
            error!("{e}");
            tx.send(Event::Error(e.to_string())).ok();
        }
    })
}
