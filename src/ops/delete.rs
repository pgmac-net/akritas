//! Tag deletion.
//!
//! The registry API deletes a *manifest*, by digest — there is no "delete this
//! tag". Every tag pointing at that manifest disappears with it, and the
//! registry offers no "which tags point at this digest" query. So before a
//! delete the TUI looks the siblings up itself ([`find_siblings`]) and shows
//! them, and the delete itself is pinned to the digest the user confirmed
//! ([`delete_pinned`]) so a tag re-pushed in the meantime is never deleted
//! unseen (#163).

use std::sync::Arc;

use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::registry::{RegistryClient, RegistryError, Result};

/// At most this many loaded tags are checked for siblings. Above it the
/// confirm dialog warns generically instead of making one request per tag.
pub const SIBLING_LOOKUP_LIMIT: usize = 200;

/// Concurrent `HEAD` requests during a sibling lookup.
const LOOKUP_CONCURRENCY: usize = 8;

/// What a sibling lookup found — and, as importantly, what it could not
/// check, so the dialog never claims "no other tags" when it does not know.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SiblingReport {
    /// Other tags whose manifest digest equals the one being deleted.
    pub siblings: Vec<String>,
    /// Tags whose digest could not be read (request failed).
    pub unchecked: usize,
    /// The tag list was not fully loaded, so some tags were never checked.
    pub more_unloaded: bool,
}

impl SiblingReport {
    /// True when every tag was checked, so `siblings` is the whole answer.
    pub fn is_complete(&self) -> bool {
        self.unchecked == 0 && !self.more_unloaded
    }
}

/// Outcome of [`delete_pinned`] that is not a transport error.
#[derive(Debug, PartialEq, Eq)]
pub enum DeleteOutcome {
    Deleted,
    /// The tag now resolves to a different digest than the one confirmed;
    /// nothing was deleted.
    Moved {
        now: String,
    },
}

/// Find which of `tags` resolve to `digest`.
///
/// `tags` should exclude the tag being deleted. Requests run with bounded
/// concurrency; a failed request counts as unchecked rather than aborting
/// the lookup.
pub async fn find_siblings(
    client: &RegistryClient,
    repo: &str,
    digest: &str,
    tags: Vec<String>,
    more_unloaded: bool,
) -> SiblingReport {
    let permits = Arc::new(Semaphore::new(LOOKUP_CONCURRENCY));
    let mut set = JoinSet::new();
    for tag in tags {
        let client = client.clone();
        let repo = repo.to_owned();
        let permits = Arc::clone(&permits);
        set.spawn(async move {
            let _permit = permits.acquire_owned().await;
            let result = client.head_manifest(&repo, &tag).await;
            (tag, result)
        });
    }

    let mut results = Vec::new();
    while let Some(joined) = set.join_next().await {
        match joined {
            Ok(pair) => results.push(pair),
            // A panicked task is as unknown as a failed request.
            Err(_) => results.push((
                String::new(),
                Err(RegistryError::InvalidResponse(
                    "sibling lookup task failed".to_owned(),
                )),
            )),
        }
    }
    classify(results, digest, more_unloaded)
}

/// Pure core of [`find_siblings`]: split lookup results into matches and
/// unknowns. Siblings are returned sorted for a stable display.
fn classify(
    results: Vec<(String, Result<String>)>,
    digest: &str,
    more_unloaded: bool,
) -> SiblingReport {
    let mut report = SiblingReport {
        more_unloaded,
        ..SiblingReport::default()
    };
    for (tag, result) in results {
        match result {
            Ok(d) if d == digest => report.siblings.push(tag),
            Ok(_) => {}
            Err(_) => report.unchecked += 1,
        }
    }
    report.siblings.sort();
    report
}

/// Delete `repo:tag`, but only if it still resolves to `expected_digest` —
/// the digest the user saw and confirmed.
pub async fn delete_pinned(
    client: &RegistryClient,
    repo: &str,
    tag: &str,
    expected_digest: &str,
) -> Result<DeleteOutcome> {
    let now = client.head_manifest(repo, tag).await?;
    if now != expected_digest {
        return Ok(DeleteOutcome::Moved { now });
    }
    client.delete_manifest(repo, expected_digest).await?;
    Ok(DeleteOutcome::Deleted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use url::Url;

    /// A tiny registry: `HEAD /v2/<repo>/manifests/<tag>` answers with the
    /// configured digest (or 500 for an unknown tag), `DELETE` answers 202.
    /// Every request line is recorded so tests can assert what was sent.
    async fn mock_registry(
        digests: HashMap<&'static str, &'static str>,
    ) -> (RegistryClient, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let digests = digests.clone();
                let log = Arc::clone(&log);
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let line = req.lines().next().unwrap_or("").to_owned();
                    log.lock().unwrap().push(line.clone());
                    let mut parts = line.split_whitespace();
                    let method = parts.next().unwrap_or("");
                    let path = parts.next().unwrap_or("");
                    let tag = path.rsplit('/').next().unwrap_or("");
                    let resp = match (method, digests.get(tag)) {
                        ("DELETE", _) => "HTTP/1.1 202 Accepted\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_owned(),
                        ("HEAD", Some(d)) => format!(
                            "HTTP/1.1 200 OK\r\ndocker-content-digest: {d}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                        ),
                        _ => "HTTP/1.1 500 Internal Server Error\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_owned(),
                    };
                    let _ = sock.write_all(resp.as_bytes()).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        let client = RegistryClient::new(Url::parse(&format!("http://{addr}/")).unwrap());
        (client, seen)
    }

    #[tokio::test]
    async fn find_siblings_reports_matches_and_unchecked() {
        let (client, _) = mock_registry(HashMap::from([
            ("0.2.0", "sha256:aaa"),
            ("0.1.0", "sha256:bbb"),
            ("stable", "sha256:aaa"),
        ]))
        .await;
        let report = find_siblings(
            &client,
            "charts/demo",
            "sha256:aaa",
            vec![
                "0.2.0".into(),
                "0.1.0".into(),
                "stable".into(),
                "broken".into(),
            ],
            false,
        )
        .await;
        assert_eq!(report.siblings, ["0.2.0", "stable"]);
        assert_eq!(report.unchecked, 1);
        assert!(!report.is_complete());
    }

    #[tokio::test]
    async fn find_siblings_with_no_matches_is_complete_only_when_all_checked() {
        let (client, _) = mock_registry(HashMap::from([("0.1.0", "sha256:bbb")])).await;
        let full = find_siblings(&client, "r", "sha256:aaa", vec!["0.1.0".into()], false).await;
        assert!(full.siblings.is_empty() && full.is_complete());
        let partial = find_siblings(&client, "r", "sha256:aaa", vec!["0.1.0".into()], true).await;
        assert!(partial.siblings.is_empty() && !partial.is_complete());
    }

    /// The tag was re-pushed after the user confirmed: nothing may be deleted.
    #[tokio::test]
    async fn delete_pinned_refuses_a_moved_tag_and_sends_no_delete() {
        let (client, seen) = mock_registry(HashMap::from([("0.2.0", "sha256:new")])).await;
        let outcome = delete_pinned(&client, "r", "0.2.0", "sha256:old")
            .await
            .unwrap();
        assert_eq!(
            outcome,
            DeleteOutcome::Moved {
                now: "sha256:new".into()
            }
        );
        assert!(
            seen.lock()
                .unwrap()
                .iter()
                .all(|l| !l.starts_with("DELETE")),
            "{:?}",
            seen.lock().unwrap()
        );
    }

    #[tokio::test]
    async fn delete_pinned_deletes_the_confirmed_digest() {
        let (client, seen) = mock_registry(HashMap::from([("0.2.0", "sha256:aaa")])).await;
        let outcome = delete_pinned(&client, "r", "0.2.0", "sha256:aaa")
            .await
            .unwrap();
        assert_eq!(outcome, DeleteOutcome::Deleted);
        assert!(
            seen.lock()
                .unwrap()
                .iter()
                .any(|l| l.starts_with("DELETE /v2/r/manifests/sha256:aaa")),
            "{:?}",
            seen.lock().unwrap()
        );
    }

    #[test]
    fn classify_sorts_siblings_and_counts_failures() {
        let report = classify(
            vec![
                ("z".into(), Ok("d".into())),
                ("a".into(), Ok("d".into())),
                ("x".into(), Ok("other".into())),
                ("e".into(), Err(RegistryError::Unauthorized)),
            ],
            "d",
            true,
        );
        assert_eq!(report.siblings, ["a", "z"]);
        assert_eq!(report.unchecked, 1);
        assert!(report.more_unloaded);
    }
}
