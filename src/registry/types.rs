#![allow(dead_code)]

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub mod media_types {
    pub const OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
    pub const OCI_INDEX: &str = "application/vnd.oci.image.index.v1+json";
    pub const DOCKER_MANIFEST_V2: &str = "application/vnd.docker.distribution.manifest.v2+json";
    pub const DOCKER_MANIFEST_LIST: &str =
        "application/vnd.docker.distribution.manifest.list.v2+json";
    /// Config-blob media type of a Helm chart stored as an OCI artifact.
    pub const HELM_CONFIG: &str = "application/vnd.cncf.helm.config.v1+json";
}

/// Ordered list of Accept values to send when fetching manifests.
pub const MANIFEST_ACCEPT: &str = concat!(
    "application/vnd.oci.image.manifest.v1+json,",
    "application/vnd.oci.image.index.v1+json,",
    "application/vnd.docker.distribution.manifest.v2+json,",
    "application/vnd.docker.distribution.manifest.list.v2+json",
);

/// True when `repo` is a valid OCI repository name: one or more `/`-separated
/// path components, each non-empty, made of lowercase ASCII letters, digits,
/// `.`, `_` and `-`, and never `.` or `..`.
///
/// The single rule for every repository name that comes from outside —
/// config seeds (`repositories = [...]`) and anything offered for the clipboard
/// — so the two cannot drift apart. Kept to an allow-list: nothing that could
/// mean something to a shell or a URL path gets through.
pub fn valid_repository_name(repo: &str) -> bool {
    !repo.is_empty()
        && repo.split('/').all(|part| {
            !part.is_empty()
                && part != ".."
                && part != "."
                && part.chars().all(|c| {
                    c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-')
                })
        })
}

#[derive(Debug, Clone, Deserialize)]
pub struct Catalog {
    #[serde(default)]
    pub repositories: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TagList {
    pub name: String,
    /// Some registries return `null` for repos with no tags.
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Platform {
    pub os: String,
    pub architecture: String,
    #[serde(default)]
    pub variant: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestDescriptor {
    pub media_type: String,
    pub size: i64,
    pub digest: String,
    #[serde(default)]
    pub platform: Option<Platform>,
}

/// Parsed image config blob (Docker v2 / OCI).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ImageConfigBlob {
    #[serde(default)]
    pub architecture: Option<String>,
    #[serde(default)]
    pub os: Option<String>,
    #[serde(default)]
    pub created: Option<String>,
    #[serde(default)]
    pub config: ImageConfigSection,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ImageConfigSection {
    #[serde(rename = "Labels", default)]
    pub labels: Option<HashMap<String, String>>,
}

/// Helm chart metadata (`Chart.yaml` as JSON) — the config blob of a Helm
/// chart pushed as an OCI artifact. Every field is optional so a sparse or
/// future chart config still renders whatever it does carry.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HelmChartConfig {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub app_version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// Single-arch image manifest (OCI v1 or Docker v2 schema 2).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageManifest {
    pub schema_version: u8,
    #[serde(default)]
    pub media_type: String,
    pub config: ManifestDescriptor,
    pub layers: Vec<ManifestDescriptor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<HashMap<String, String>>,
}

impl ImageManifest {
    /// True when the config blob is a Helm chart's, i.e. this "image" is a
    /// chart. Detected per manifest, never per registry: one host serves both.
    pub fn is_helm_chart(&self) -> bool {
        self.config.media_type == media_types::HELM_CONFIG
    }
}

/// Multi-arch manifest index (OCI) / manifest list (Docker).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestIndex {
    pub schema_version: u8,
    #[serde(default)]
    pub media_type: String,
    pub manifests: Vec<ManifestDescriptor>,
}

/// Discriminated manifest payload returned by the registry.
#[derive(Debug, Clone)]
pub enum Manifest {
    Image(ImageManifest),
    Index(ManifestIndex),
}

/// Full response from a manifest fetch, including raw bytes for re-push.
#[derive(Debug, Clone)]
pub struct ManifestResponse {
    pub manifest: Manifest,
    pub digest: String,
    pub content_type: String,
    pub raw: Vec<u8>,
}

/// Metadata returned by a blob HEAD request.
#[derive(Debug, Clone)]
pub struct BlobInfo {
    pub digest: String,
    pub size: u64,
}

/// Location returned after initiating a blob upload (POST).
#[derive(Debug, Clone)]
pub struct UploadLocation {
    pub location: url::Url,
}

/// One entry from a JFrog Artifactory `/api/repositories` response.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactoryRepo {
    pub key: String,
    #[serde(rename = "type")]
    pub repo_type: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub package_type: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifactory_repo_deserializes_sample_response() {
        let body = r#"[
            {
                "key": "docker-local",
                "type": "LOCAL",
                "url": "https://artifactory.example.com/artifactory/docker-local",
                "packageType": "Docker"
            },
            {
                "key": "docker-remote",
                "type": "REMOTE",
                "url": "https://artifactory.example.com/artifactory/docker-remote",
                "packageType": "Docker"
            }
        ]"#;
        let repos: Vec<ArtifactoryRepo> = serde_json::from_str(body).unwrap();
        assert_eq!(repos.len(), 2);
        assert_eq!(repos[0].key, "docker-local");
        assert_eq!(repos[0].repo_type, "LOCAL");
        assert_eq!(repos[0].package_type, "Docker");
        assert_eq!(repos[1].key, "docker-remote");
    }

    #[test]
    fn valid_repository_name_follows_the_oci_grammar() {
        for ok in [
            "n8n",
            "library/n8n",
            "actions/arc-charts/gha-runner-scale-set",
            "a.b_c-d/e",
        ] {
            assert!(valid_repository_name(ok), "{ok}");
        }
        for bad in [
            "", "A/b", "a//b", "/a", "a/", "./a", "a/..", "a b", "a;b", "a:b", "$(x)",
        ] {
            assert!(!valid_repository_name(bad), "{bad}");
        }
    }
}
