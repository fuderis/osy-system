// Copyright (C) 2026 Bulat Sh. (fuderis) <synapdrake@ya.ru>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

pub mod config;
pub mod error;
pub mod prelude;

pub mod handlers;
pub mod skills;

use osy_share::AgentMeta;
use pearce::Server;
use prelude::*;
use rigging::{CommandContext, Commands, pkg_meta};
use skills::SkillKind;

#[atoman::main]
async fn main() -> Result<()> {
    osy_share::macos_protection!();

    // init agent metadata
    AgentMeta::init(pkg_meta!(), SkillKind::skills_list()).await;

    // init settings && logger
    Config::init(path!("$config$/config.toml")).await?;
    Logger::init(path!("$state$/logs"), 1000).await?;

    // handle arguments
    Commands::new()
        .meta(pkg_meta!())
        .hide_cmd("serve", "Runs the agent server", serve)
        .cmd(
            "metadata",
            "Prints agent metadata in JSON format and exits",
            |_| async move {
                println!("{}", AgentMeta::get().to_json_string());
                Ok(())
            },
        )
        .run()
        .await
}

async fn serve(_: CommandContext) -> Result<()> {
    use handlers as hands;

    let (sock_name, sock_path) = {
        let meta = AgentMeta::get();
        (meta.sock_name.clone(), meta.sock_path.clone())
    };

    // start server:
    info!("Launching on `{}`...", sock_path.display());
    Server::new()
        //    HEALTH
        .get("/ping", hands::health::handle_ping)
        //    SKILLS
        .post("/skills/list", hands::skills::handle_skills_list)
        .post("/skills/{skill}/tools", hands::skills::handle_tools_list)
        .post(
            "/skills/{skill}/call/{tool}",
            hands::skills::handle_tool_call,
        )
        .callback(true)
        .run(sock_name)
        .await
}
