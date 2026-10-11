//! What the command line and the environment say about collaboration.
//!
//! The rules are those of `kadakas serve`: `--collab-url` (or `KADAKAS_COLLAB_URL`),
//! `--collab-token` (or `KADAKAS_COLLAB_TOKEN`) and `--allow-collab-url` (or the comma list
//! `KADAKAS_COLLAB_ALLOW_URL`). A flag wins over the variable of the same setting. A flag shows in
//! the list of processes, so the variables are the better way for a token.

/// The collaboration settings of the program start.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct CollabArgs {
    /// The default server (a base address, or a full `ws://host/ws/<doc>` address).
    pub url: Option<String>,
    /// The default token. It goes only to the default URL. Keep it in memory only.
    pub token: Option<String>,
    /// Servers that are not on this computer and that may be used (a host, `*.domain` or an
    /// address prefix).
    pub allow: Vec<String>,
}

impl std::fmt::Debug for CollabArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CollabArgs")
            .field("url", &self.url)
            .field("token", &self.token.as_ref().map(|_| "<hidden>"))
            .field("allow", &self.allow)
            .finish()
    }
}

/// The values of a flag, in order: `--name value` and `--name=value`.
fn flag_values(args: &[String], name: &str) -> Vec<String> {
    let long = format!("--{name}");
    let prefix = format!("--{name}=");
    let mut out = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if *a == long {
            if let Some(v) = it.next() {
                out.push(v.clone());
            }
        } else if let Some(v) = a.strip_prefix(&prefix) {
            out.push(v.to_string());
        }
    }
    out
}

fn non_empty(v: Option<String>) -> Option<String> {
    v.map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

impl CollabArgs {
    /// Reads the flags and the variables. `env` looks up a variable (tests pass a table).
    pub fn parse(args: &[String], env: &dyn Fn(&str) -> Option<String>) -> CollabArgs {
        let one = |flag: &str, var: &str| {
            non_empty(flag_values(args, flag).pop()).or_else(|| non_empty(env(var)))
        };
        let mut allow: Vec<String> = flag_values(args, "allow-collab-url")
            .iter()
            .flat_map(|v| v.split(','))
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
            .collect();
        if allow.is_empty() {
            allow = env("KADAKAS_COLLAB_ALLOW_URL")
                .unwrap_or_default()
                .split(',')
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
                .collect();
        }
        CollabArgs {
            url: one("collab-url", "KADAKAS_COLLAB_URL"),
            // The token is not trimmed inside: only the ends.
            token: one("collab-token", "KADAKAS_COLLAB_TOKEN"),
            allow,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn env<'a>(table: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| {
            table
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn nothing_given_gives_nothing() {
        let a = CollabArgs::parse(&[], &env(&[]));
        assert_eq!(a, CollabArgs::default());
    }

    #[test]
    fn variables_are_read() {
        let e = env(&[
            ("KADAKAS_COLLAB_URL", "ws://127.0.0.1:8787"),
            ("KADAKAS_COLLAB_TOKEN", " tok "),
            ("KADAKAS_COLLAB_ALLOW_URL", "a.example.org, *.b.org ,,"),
        ]);
        let a = CollabArgs::parse(&[], &e);
        assert_eq!(a.url.as_deref(), Some("ws://127.0.0.1:8787"));
        assert_eq!(a.token.as_deref(), Some("tok"));
        assert_eq!(a.allow, ["a.example.org", "*.b.org"]);
    }

    #[test]
    fn a_flag_wins_over_the_variable_in_both_spellings() {
        let e = env(&[
            ("KADAKAS_COLLAB_URL", "ws://env"),
            ("KADAKAS_COLLAB_ALLOW_URL", "env.org"),
        ]);
        let a = CollabArgs::parse(
            &args(&[
                "kadakas",
                "--collab-url",
                "wss://flag.example.org",
                "--collab-token=t2",
                "--allow-collab-url",
                "one.org,two.org",
                "--allow-collab-url=three.org",
            ]),
            &e,
        );
        assert_eq!(a.url.as_deref(), Some("wss://flag.example.org"));
        assert_eq!(a.token.as_deref(), Some("t2"));
        assert_eq!(a.allow, ["one.org", "two.org", "three.org"]);
    }

    #[test]
    fn empty_values_and_a_flag_without_a_value_count_as_not_given() {
        let e = env(&[("KADAKAS_COLLAB_URL", "  ")]);
        let a = CollabArgs::parse(&args(&["--collab-url", "", "--collab-token"]), &e);
        assert_eq!(a, CollabArgs::default());
        // The map-service flag is a different setting.
        let a = CollabArgs::parse(&args(&["--allow-url", "maps.example.org"]), &e);
        assert!(a.allow.is_empty());
    }

    #[test]
    fn the_debug_text_hides_the_token() {
        let a = CollabArgs {
            token: Some("secret-token".into()),
            ..Default::default()
        };
        let text = format!("{a:?}");
        assert!(!text.contains("secret-token"), "{text}");
        assert!(text.contains("hidden"));
    }
}
