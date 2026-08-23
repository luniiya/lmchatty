use crate::model::Mode;
use anyhow::{anyhow, bail, Result};

pub fn parse() -> Result<Mode> {
    parse_from(std::env::args().skip(1))
}

fn parse_from<I>(arguments: I) -> Result<Mode>
where
    I: IntoIterator<Item = String>,
{
    let mut args = arguments.into_iter();
    match args.next().as_deref() {
        None | Some("search") | Some("--search") | Some("-s") => Ok(Mode::Search),
        Some("new") | Some("--new") | Some("-n") => Ok(Mode::New),
        Some("setup") | Some("--setup") => Ok(Mode::Setup),
        Some("--chat") | Some("-c") => {
            Ok(Mode::Chat(args.next().ok_or_else(|| {
                anyhow!("--chat needs a conversation hash")
            })?))
        }
        Some("--help") | Some("-h") => {
            print_help();
            std::process::exit(0);
        }
        Some(argument) => bail!("Unknown argument: {argument}"),
    }
}

fn print_help() {
    println!(
        "lmchatty - conversational coding-agent launcher\n\nUsage:\n  lmchatty          Open search\n  lmchatty search   Open search\n  lmchatty new      Create a conversation\n  lmchatty setup    Review or change which agents are enabled, their\n                    rank, and the chats directory\n  lmchatty --chat HASH  Open a conversation\n\nSearch keys: /, j/k, gg, G, Enter, Space, n, c, q\n'c' opens setup from any page. Conversations run in Zellij. First run asks\nwhich coding agents to enable (Claude, Codex, OpenCode) and where to store\nconversations; see ~/.config/lmchatty/config.toml."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pages_and_hashes() {
        assert_eq!(parse_from(Vec::<String>::new()).unwrap(), Mode::Search);
        assert_eq!(parse_from(vec!["new".into()]).unwrap(), Mode::New);
        assert_eq!(
            parse_from(vec!["--chat".into(), "deadbeef".into()]).unwrap(),
            Mode::Chat("deadbeef".into())
        );
    }
}
