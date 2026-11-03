use std::io::Write;
use std::path::Path;

use tar::{Builder, Header};

use crate::registry::{Manifest, ManifestResponse, RegistryClient};

/// Export a single-arch image as an OCI image layout tarball.
///
/// The resulting file is compatible with `skopeo copy oci-archive:<path>`.
/// Multi-arch index manifests are not supported.
///
/// The archive is written to `<dest>.partial` and renamed into place only when
/// complete, so a file at `dest` is always a whole export — never the stub a
/// failed or interrupted run would otherwise leave looking like a valid tar.
pub async fn export_image(
    client: &RegistryClient,
    repo: &str,
    tag: &str,
    dest: &Path,
    on_progress: impl Fn(usize, usize),
) -> anyhow::Result<()> {
    let partial = partial_path(dest);
    let result = write_export(client, repo, tag, &partial, on_progress).await;
    finalize_export(&partial, dest, result)
}

/// `<dest>.partial`, beside `dest` so the final rename stays on one filesystem.
fn partial_path(dest: &Path) -> std::path::PathBuf {
    let mut name = dest.as_os_str().to_owned();
    name.push(".partial");
    std::path::PathBuf::from(name)
}

/// Promote a finished export, or clean up after a failed one.
fn finalize_export(partial: &Path, dest: &Path, result: anyhow::Result<()>) -> anyhow::Result<()> {
    match result {
        Ok(()) => {
            std::fs::rename(partial, dest)?;
            Ok(())
        }
        Err(e) => {
            // Best effort: the original error is the one worth reporting.
            let _ = std::fs::remove_file(partial);
            Err(e)
        }
    }
}

async fn write_export(
    client: &RegistryClient,
    repo: &str,
    tag: &str,
    dest: &Path,
    on_progress: impl Fn(usize, usize),
) -> anyhow::Result<()> {
    let manifest_resp = client.get_manifest(repo, tag).await?;

    let blob_digests = match &manifest_resp.manifest {
        Manifest::Image(img) => {
            let mut digests = vec![img.config.digest.clone()];
            digests.extend(img.layers.iter().map(|l| l.digest.clone()));
            digests
        }
        Manifest::Index(_) => {
            return Err(anyhow::anyhow!(
                "Multi-arch index — select a specific platform tag to export"
            ));
        }
    };

    let total_steps = blob_digests.len() + 1; // blobs + manifest

    let file = std::fs::File::create(dest)?;
    let mut tar = Builder::new(file);

    // oci-layout marker.
    write_entry(&mut tar, "oci-layout", br#"{"imageLayoutVersion":"1.0.0"}"#)?;

    // Blob entries.
    for (i, digest) in blob_digests.iter().enumerate() {
        let bytes = client.get_blob(repo, digest).await?;
        let hex = digest_hex(digest);
        write_entry(&mut tar, &format!("blobs/sha256/{hex}"), &bytes)?;
        on_progress(i + 1, total_steps);
    }

    // Manifest blob.
    let manifest_hex = digest_hex(&manifest_resp.digest);
    write_entry(
        &mut tar,
        &format!("blobs/sha256/{manifest_hex}"),
        &manifest_resp.raw,
    )?;
    on_progress(total_steps, total_steps);

    // index.json.
    let index = build_index_json(&manifest_resp, tag)?;
    write_entry(&mut tar, "index.json", index.as_bytes())?;

    tar.finish()?;
    Ok(())
}

fn write_entry<W: Write>(tar: &mut Builder<W>, path: &str, data: &[u8]) -> anyhow::Result<()> {
    let mut header = Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_mtime(0);
    header.set_cksum();
    tar.append_data(&mut header, path, data)?;
    Ok(())
}

fn digest_hex(digest: &str) -> &str {
    digest.strip_prefix("sha256:").unwrap_or(digest)
}

fn build_index_json(resp: &ManifestResponse, tag: &str) -> anyhow::Result<String> {
    let index = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.index.v1+json",
        "manifests": [{
            "mediaType": resp.content_type,
            "digest": resp.digest,
            "size": resp.raw.len(),
            "annotations": {
                "org.opencontainers.image.ref.name": tag,
            }
        }]
    });
    Ok(serde_json::to_string_pretty(&index)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_path_sits_beside_dest() {
        assert_eq!(
            partial_path(Path::new("/x/y/img.tar")),
            Path::new("/x/y/img.tar.partial")
        );
    }

    #[test]
    fn successful_export_leaves_only_the_final_file() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("img.tar");
        let partial = partial_path(&dest);
        std::fs::write(&partial, b"whole archive").unwrap();

        finalize_export(&partial, &dest, Ok(())).unwrap();

        assert!(dest.exists());
        assert!(!partial.exists());
        assert_eq!(std::fs::read(&dest).unwrap(), b"whole archive");
    }

    /// A failed export must leave neither a truncated `.tar` that looks valid
    /// nor a stray `.partial`, and must surface the original error.
    #[test]
    fn failed_export_leaves_no_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("img.tar");
        let partial = partial_path(&dest);
        std::fs::write(&partial, b"half").unwrap();

        let err = finalize_export(&partial, &dest, Err(anyhow::anyhow!("blob fetch failed")))
            .unwrap_err();

        assert_eq!(err.to_string(), "blob fetch failed");
        assert!(!dest.exists());
        assert!(!partial.exists());
    }

    /// A failure must not clobber an export that already exists at `dest`.
    #[test]
    fn failed_export_keeps_a_previous_complete_export() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("img.tar");
        std::fs::write(&dest, b"previous good export").unwrap();
        let partial = partial_path(&dest);
        std::fs::write(&partial, b"half").unwrap();

        let _ = finalize_export(&partial, &dest, Err(anyhow::anyhow!("boom")));

        assert_eq!(std::fs::read(&dest).unwrap(), b"previous good export");
    }
}
