//! Links Synkopa opens in the system browser. Every URL is built here from a fixed host and a
//! checked argument, so the UI can never make the app open an arbitrary address.

/// Where "Ask AI" sends a question.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AskService {
    /// ChatGPT takes the prompt in the link (`?q=`) and runs it.
    Chatgpt,
    /// DeepWiki answers from an index of this repository; its links take no question, so
    /// the UI copies the question and this opens the repository's page.
    Deepwiki,
}

/// The longest link Synkopa hands to the browser. ChatGPT drops prompts past a short limit
/// (undocumented), so a longer question goes through the clipboard instead.
pub const MAX_ASK_URL: usize = 2000;

const CHATGPT_URL: &str = "https://chatgpt.com/?q=";
const CHATGPT_HOME: &str = "https://chatgpt.com/";
const DEEPWIKI_URL: &str = "https://deepwiki.com/miskibin/synkopa";

/// Percent-encodes everything but RFC 3986 unreserved characters (UTF-8 bytes, upper-case hex).
/// `ui/src/lib/askLink.ts` encodes the same way, so both sides agree on a link's length.
pub fn encode_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The link for `service`. A ChatGPT prompt must fit in [`MAX_ASK_URL`]; without one (the UI
/// copied a long question instead) it opens a new chat. DeepWiki ignores the prompt.
pub fn ask_url(service: AskService, prompt: Option<&str>) -> Result<String, String> {
    match service {
        AskService::Deepwiki => Ok(DEEPWIKI_URL.to_string()),
        AskService::Chatgpt => {
            let Some(p) = prompt.map(str::trim).filter(|p| !p.is_empty()) else {
                return Ok(CHATGPT_HOME.to_string());
            };
            let url = format!("{CHATGPT_URL}{}", encode_component(p));
            if url.len() > MAX_ASK_URL {
                return Err(format!("the question is too long for a link ({} > {MAX_ASK_URL} characters)", url.len()));
            }
            Ok(url)
        }
    }
}

/// Opens an https `url` in the system browser. Callers build `url` from fixed hosts only.
pub fn open_browser(url: &str) -> Result<(), String> {
    debug_assert!(url.starts_with("https://"));
    #[cfg(windows)]
    let mut cmd = {
        let mut c = std::process::Command::new("rundll32");
        c.args(["url.dll,FileProtocolHandler", url]);
        c
    };
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    };
    #[cfg(not(any(windows, target_os = "macos")))]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    cmd.spawn().map(|_| ()).map_err(|e| format!("could not open {url}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_like_the_ui() {
        assert_eq!(encode_component("a-z_0.9~"), "a-z_0.9~");
        assert_eq!(encode_component("jak dodać efekt?"), "jak%20doda%C4%87%20efekt%3F");
        assert_eq!(encode_component("a&b=c/d#e\n"), "a%26b%3Dc%2Fd%23e%0A");
        assert_eq!(encode_component("(x)!*'"), "%28x%29%21%2A%27");
    }

    #[test]
    fn chatgpt_link_carries_the_prompt() {
        let u = ask_url(AskService::Chatgpt, Some("  how?  ")).unwrap();
        assert_eq!(u, "https://chatgpt.com/?q=how%3F");
    }

    #[test]
    fn chatgpt_without_a_question_opens_a_new_chat() {
        assert_eq!(ask_url(AskService::Chatgpt, None).unwrap(), CHATGPT_HOME);
        assert_eq!(ask_url(AskService::Chatgpt, Some("   ")).unwrap(), CHATGPT_HOME);
    }

    #[test]
    fn chatgpt_rejects_a_link_past_the_limit() {
        let fits = "a".repeat(MAX_ASK_URL - CHATGPT_URL.len());
        assert_eq!(ask_url(AskService::Chatgpt, Some(&fits)).unwrap().len(), MAX_ASK_URL);
        let over = "a".repeat(MAX_ASK_URL - CHATGPT_URL.len() + 1);
        assert!(ask_url(AskService::Chatgpt, Some(&over)).is_err());
        // multi-byte text is measured encoded: 400 × "ą" is 2400 characters in the link
        assert!(ask_url(AskService::Chatgpt, Some(&"ą".repeat(400))).is_err());
    }

    #[test]
    fn deepwiki_opens_the_repository_whatever_the_prompt() {
        assert_eq!(ask_url(AskService::Deepwiki, Some("javascript:alert(1)")).unwrap(), DEEPWIKI_URL);
        assert_eq!(ask_url(AskService::Deepwiki, None).unwrap(), DEEPWIKI_URL);
    }

    #[test]
    fn every_link_stays_on_its_host() {
        for (s, host) in [(AskService::Chatgpt, "https://chatgpt.com/"), (AskService::Deepwiki, "https://deepwiki.com/")] {
            let u = ask_url(s, Some("x https://evil.example/")).unwrap();
            assert!(u.starts_with(host), "{u}");
        }
    }
}
