//! The HTTPS client behind the downloads (sample kits, pitch and stem models).
//!
//! It trusts the operating system's certificate store, so a company proxy that re-signs HTTPS
//! with its own root works once IT has installed that root, as in a browser. When the proxy's
//! root is not in the store, "Ignore certificate errors" (Settings → General → Network) skips
//! the check. Every download is verified by its SHA-256 from the manifest built into the app,
//! so the files stay trustworthy either way.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use ureq::tls::{RootCerts, TlsConfig};

static INSECURE: AtomicBool = AtomicBool::new(false);
static AGENTS: OnceLock<[ureq::Agent; 2]> = OnceLock::new();

/// Skip (`true`) or keep (`false`) the certificate check of every download from now on.
pub fn set_insecure(on: bool) {
    INSECURE.store(on, Ordering::Relaxed);
}

pub fn insecure() -> bool {
    INSECURE.load(Ordering::Relaxed)
}

/// Loads the saved choice; the CLI calls this too, the GUI does so at startup.
pub fn init(insecure: bool) {
    set_insecure(insecure);
}

fn build(insecure: bool) -> ureq::Agent {
    let tls = TlsConfig::builder().root_certs(RootCerts::PlatformVerifier).disable_verification(insecure).build();
    ureq::Agent::config_builder().tls_config(tls).build().into()
}

/// The agent for the current choice (built once per choice, connections are pooled).
pub fn agent() -> &'static ureq::Agent {
    &AGENTS.get_or_init(|| [build(false), build(true)])[usize::from(insecure())]
}

/// Adds the way out to a certificate failure: the raw "UnknownIssuer" tells nobody what to do.
pub fn explain(e: &ureq::Error) -> String {
    let t = e.to_string();
    if t.to_lowercase().contains("certificate") {
        format!("{t} — a company proxy may be re-signing HTTPS; turn on \"Ignore certificate errors\" in Settings → General → Network")
    } else {
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choice_picks_the_agent_and_a_certificate_error_names_the_setting() {
        set_insecure(false);
        let strict = agent() as *const _;
        set_insecure(true);
        assert!(!std::ptr::eq(strict, agent()));
        set_insecure(false);
        assert!(std::ptr::eq(strict, agent()));
        let e = ureq::Error::Tls("invalid peer certificate: UnknownIssuer");
        assert!(explain(&e).contains("Ignore certificate errors"));
        assert!(!explain(&ureq::Error::Timeout(ureq::Timeout::Global)).contains("Ignore"));
    }

    /// Needs the internet: a host signed by a root nobody trusts, like a company proxy's.
    #[test]
    #[ignore = "needs the internet"]
    fn an_untrusted_root_fails_until_certificate_errors_are_ignored() {
        const URL: &str = "https://untrusted-root.badssl.com/";
        set_insecure(false);
        let err = agent().get(URL).call().expect_err("the root is not in the OS store");
        assert!(explain(&err).contains("Ignore certificate errors"), "{err}");
        set_insecure(true);
        assert!(agent().get(URL).call().is_ok());
        set_insecure(false);
    }
}
