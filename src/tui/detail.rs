#![allow(dead_code)]

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::registry::{HelmChartConfig, ImageConfigBlob, Manifest, ManifestResponse};

#[derive(Debug, Clone)]
pub struct LayerInfo {
    pub digest: String,
    pub size: u64,
}

/// Chart facts shown instead of image facts when the manifest's config blob is
/// a Helm chart's. Detected per tag — one registry can serve both.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HelmInfo {
    pub name: Option<String>,
    pub version: Option<String>,
    pub app_version: Option<String>,
    pub description: Option<String>,
    /// Whether the tag being viewed equals the chart's `version` (as Helm
    /// writes it into a tag). `None` when the chart's version is unknown.
    ///
    /// `helm push` always tags a chart with its version, so a mismatch comes
    /// from other tooling — a retag, `crane`, `oras` — or from a deliberate
    /// floating tag like `stable`. Helm looks a chart up by tag, so the
    /// Details pane says when the two differ (#165).
    pub tag_matches_version: Option<bool>,
    /// `helm pull oci://<host>/<repo> --version <tag>` — the `--version` form
    /// is the unambiguous one; tag-in-reference support varies by Helm version.
    ///
    /// `None` when host, repository or tag fails OCI name validation: all
    /// three are registry- or user-supplied and this string is meant to be
    /// pasted into a shell, so an unvalidated `; rm -rf ~` in a tag would
    /// become a command. No command is offered rather than a quoted guess.
    pub pull_command: Option<String>,
}

/// The tag Helm uses for a chart `version`: `+` is not a legal OCI tag
/// character, so `helm push` writes build metadata `1.2.3+build5` as
/// `1.2.3_build5`.
pub fn helm_tag_for_version(version: &str) -> String {
    version.replace('+', "_")
}

/// Build the `helm pull` command, or `None` if any part is not a valid OCI
/// name. Allow-lists, so nothing shell-significant can get through.
fn helm_pull_command(host: &str, repo: &str, tag: &str) -> Option<String> {
    valid_oci_ref(host, repo, tag).then(|| format!("helm pull oci://{host}/{repo} --version {tag}"))
}

/// True when host, repository and tag are all valid OCI names — the gate for
/// anything copied to the clipboard, since pasted text reaches a shell.
fn valid_oci_ref(host: &str, repo: &str, tag: &str) -> bool {
    let host_ok = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'));
    // OCI distribution spec: lowercase path components, `/`-separated.
    let repo_ok = crate::registry::valid_repository_name(repo);
    // OCI tag grammar: [A-Za-z0-9_][A-Za-z0-9._-]{0,127}
    let mut tag_chars = tag.chars();
    let tag_ok = tag.len() <= 128
        && tag_chars
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        && tag_chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    host_ok && repo_ok && tag_ok
}

/// Registry-supplied text is rendered into the terminal: strip control
/// characters so a chart description cannot carry escape sequences.
fn clean(text: String) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

#[derive(Debug, Clone)]
pub struct ImageDetail {
    /// `Some` when this tag is a Helm chart rather than a container image.
    pub helm: Option<HelmInfo>,
    /// What `c` copies, or `None` when the name or tag fails OCI validation.
    /// Fails closed: the text is pasted into a shell, so an invalid reference
    /// is never offered, for charts and images alike.
    pub copyable: Option<String>,
    pub digest: String,
    pub content_type: String,
    pub created: Option<String>,
    pub os_arch: Option<String>,
    pub layers: Vec<LayerInfo>,
    pub total_size: u64,
    pub pull_url: String,
    pub labels: Vec<(String, String)>,
    pub is_index: bool,
    pub platforms: Vec<String>,
}

impl ImageDetail {
    /// What `c` puts on the clipboard: the `helm pull` command for a chart, the
    /// image pull reference otherwise.
    pub fn copy_text(&self) -> Option<&str> {
        self.copyable.as_deref()
    }

    pub fn from_manifest_and_config(
        resp: &ManifestResponse,
        config: Option<&ImageConfigBlob>,
        chart: Option<&HelmChartConfig>,
        repo: &str,
        tag: &str,
        registry_url: &str,
    ) -> Self {
        let mut detail = Self::build(resp, config, chart, repo, tag, registry_url);
        detail.copyable = match &detail.helm {
            Some(h) => h.pull_command.clone(),
            None => valid_oci_ref(registry_host(registry_url), repo, tag)
                .then(|| detail.pull_url.clone()),
        };
        detail
    }

    fn build(
        resp: &ManifestResponse,
        config: Option<&ImageConfigBlob>,
        chart: Option<&HelmChartConfig>,
        repo: &str,
        tag: &str,
        registry_url: &str,
    ) -> Self {
        let host = registry_host(registry_url);
        let pull_url = format!("{host}/{repo}:{tag}");

        match &resp.manifest {
            Manifest::Image(img) if img.is_helm_chart() => {
                let layers: Vec<LayerInfo> = img
                    .layers
                    .iter()
                    .map(|l| LayerInfo {
                        digest: l.digest.clone(),
                        size: l.size.max(0) as u64,
                    })
                    .collect();
                let total_size: u64 = layers.iter().map(|l| l.size).sum();
                let created = img
                    .annotations
                    .as_ref()
                    .and_then(|a| a.get("org.opencontainers.image.created"))
                    .cloned();
                let chart = chart.cloned().unwrap_or_default();
                let version = chart.version.map(clean);
                let tag_matches_version =
                    version.as_deref().map(|v| helm_tag_for_version(v) == tag);
                Self {
                    copyable: None,
                    helm: Some(HelmInfo {
                        name: chart.name.map(clean),
                        version,
                        app_version: chart.app_version.map(clean),
                        description: chart.description.map(clean),
                        tag_matches_version,
                        pull_command: helm_pull_command(host, repo, tag),
                    }),
                    digest: resp.digest.clone(),
                    content_type: resp.content_type.clone(),
                    created,
                    os_arch: None,
                    layers,
                    total_size,
                    pull_url,
                    labels: Vec::new(),
                    is_index: false,
                    platforms: Vec::new(),
                }
            }
            Manifest::Image(img) => {
                let layers: Vec<LayerInfo> = img
                    .layers
                    .iter()
                    .map(|l| LayerInfo {
                        digest: l.digest.clone(),
                        size: l.size.max(0) as u64,
                    })
                    .collect();
                let total_size: u64 = layers.iter().map(|l| l.size).sum();

                let (created, os_arch, labels) = config
                    .map(|c| {
                        let os_arch = match (&c.os, &c.architecture) {
                            (Some(os), Some(arch)) => Some(format!("{os}/{arch}")),
                            (Some(os), None) => Some(os.clone()),
                            _ => None,
                        };
                        let labels = c
                            .config
                            .labels
                            .clone()
                            .unwrap_or_default()
                            .into_iter()
                            .collect::<Vec<_>>();
                        (c.created.clone(), os_arch, labels)
                    })
                    .unwrap_or_default();

                Self {
                    copyable: None,
                    helm: None,
                    digest: resp.digest.clone(),
                    content_type: resp.content_type.clone(),
                    created,
                    os_arch,
                    layers,
                    total_size,
                    pull_url,
                    labels,
                    is_index: false,
                    platforms: Vec::new(),
                }
            }
            Manifest::Index(idx) => {
                let platforms: Vec<String> = idx
                    .manifests
                    .iter()
                    .map(|m| {
                        if let Some(p) = &m.platform {
                            let base = format!("{}/{}", p.os, p.architecture);
                            if let Some(v) = &p.variant {
                                format!("{base}/{v}")
                            } else {
                                base
                            }
                        } else {
                            m.digest[..16.min(m.digest.len())].to_owned()
                        }
                    })
                    .collect();

                Self {
                    copyable: None,
                    helm: None,
                    digest: resp.digest.clone(),
                    content_type: resp.content_type.clone(),
                    created: None,
                    os_arch: None,
                    layers: Vec::new(),
                    total_size: 0,
                    pull_url,
                    labels: Vec::new(),
                    is_index: true,
                    platforms,
                }
            }
        }
    }
}

pub fn format_bytes(n: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if n >= GB {
        format!("{:.1} GB", n as f64 / GB as f64)
    } else if n >= MB {
        format!("{:.1} MB", n as f64 / MB as f64)
    } else if n >= KB {
        format!("{:.1} KB", n as f64 / KB as f64)
    } else {
        format!("{n} B")
    }
}

fn registry_host(registry_url: &str) -> &str {
    registry_url
        .trim_end_matches('/')
        .trim_start_matches("https://")
        .trim_start_matches("http://")
}

const KEY_STYLE: Style = Style::new().fg(Color::Cyan);
const VAL_STYLE: Style = Style::new().fg(Color::White);
const DIM_STYLE: Style = Style::new().fg(Color::DarkGray);
const URL_STYLE: Style = Style::new().fg(Color::Green).add_modifier(Modifier::BOLD);

pub fn render_lines(detail: &ImageDetail) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    kv(
        &mut lines,
        if detail.helm.is_some() {
            "Helm pull"
        } else {
            "Pull URL"
        },
        Span::styled(
            detail.copyable.clone().unwrap_or_else(|| {
                "unavailable (name or tag is not a valid OCI reference)".to_owned()
            }),
            URL_STYLE,
        ),
    );

    let digest_short = if detail.digest.len() > 19 {
        format!("{}…", &detail.digest[..19])
    } else {
        detail.digest.clone()
    };
    kv(
        &mut lines,
        "Digest",
        Span::styled(detail.digest.clone(), VAL_STYLE),
    );
    drop(digest_short);

    kv(
        &mut lines,
        "Type",
        Span::styled(detail.content_type.clone(), DIM_STYLE),
    );

    if let Some(helm) = &detail.helm {
        kv(&mut lines, "Kind", Span::styled("Helm chart", VAL_STYLE));
        for (key, val) in [
            ("Chart", &helm.name),
            ("Version", &helm.version),
            ("App version", &helm.app_version),
            ("Description", &helm.description),
        ] {
            if let Some(v) = val {
                if key == "Version" && helm.tag_matches_version == Some(false) {
                    // The tag Helm resolves is not the version the chart
                    // declares; say so, dimly — it is accurate, not an error.
                    lines.push(Line::from(vec![
                        Span::styled(format!("{key:<12}"), KEY_STYLE),
                        Span::styled(v.clone(), VAL_STYLE),
                        Span::styled("  (tag differs)", DIM_STYLE),
                    ]));
                } else {
                    kv(&mut lines, key, Span::styled(v.clone(), VAL_STYLE));
                }
            }
        }
    }

    if detail.is_index {
        kv(
            &mut lines,
            "Kind",
            Span::styled("multi-platform index", VAL_STYLE),
        );
        lines.push(key_line("Platforms"));
        for p in &detail.platforms {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(format!("• {p}"), VAL_STYLE),
            ]));
        }
    } else {
        if let Some(created) = &detail.created {
            kv(
                &mut lines,
                "Created",
                Span::styled(created.clone(), VAL_STYLE),
            );
        }
        if let Some(os_arch) = &detail.os_arch {
            kv(
                &mut lines,
                "OS/Arch",
                Span::styled(os_arch.clone(), VAL_STYLE),
            );
        }

        kv(
            &mut lines,
            "Total size",
            Span::styled(format_bytes(detail.total_size), VAL_STYLE),
        );

        lines.push(key_line(&format!("Layers ({})", detail.layers.len())));
        for (i, layer) in detail.layers.iter().enumerate() {
            let short = if layer.digest.len() > 19 {
                format!("{}…", &layer.digest[..19])
            } else {
                layer.digest.clone()
            };
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(format!("[{i}] "), DIM_STYLE),
                Span::styled(short, DIM_STYLE),
                Span::raw("  "),
                Span::styled(format_bytes(layer.size), VAL_STYLE),
            ]));
        }

        if !detail.labels.is_empty() {
            lines.push(Line::raw(""));
            lines.push(key_line("Labels"));
            let mut sorted = detail.labels.clone();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            for (k, v) in sorted {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(format!("{k}: "), KEY_STYLE),
                    Span::styled(v, VAL_STYLE),
                ]));
            }
        }
    }

    lines
}

fn kv(lines: &mut Vec<Line<'static>>, key: &'static str, val: Span<'static>) {
    lines.push(Line::from(vec![
        Span::styled(format!("{key:<12}"), KEY_STYLE),
        val,
    ]));
}

fn key_line(key: &str) -> Line<'static> {
    Line::from(Span::styled(format!("{key}:"), KEY_STYLE))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{ImageManifest, ManifestDescriptor, media_types};
    use std::collections::HashMap;

    fn descriptor(media_type: &str, size: i64) -> ManifestDescriptor {
        ManifestDescriptor {
            media_type: media_type.to_owned(),
            size,
            digest: "sha256:abc".to_owned(),
            platform: None,
        }
    }

    fn response(
        config_type: &str,
        annotations: Option<HashMap<String, String>>,
    ) -> ManifestResponse {
        ManifestResponse {
            manifest: Manifest::Image(ImageManifest {
                schema_version: 2,
                media_type: media_types::OCI_MANIFEST.to_owned(),
                config: descriptor(config_type, 100),
                layers: vec![descriptor("application/octet-stream", 2048)],
                annotations,
            }),
            digest: "sha256:deadbeef".to_owned(),
            content_type: media_types::OCI_MANIFEST.to_owned(),
            raw: Vec::new(),
        }
    }

    fn text(lines: &[Line<'static>]) -> String {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn chart() -> HelmChartConfig {
        serde_json::from_str(
            r#"{"apiVersion":"v2","name":"n8n","version":"2.2.0-rc.1","appVersion":"1.9.0",
                "description":"Helm Chart for deploying n8n","type":"application"}"#,
        )
        .unwrap()
    }

    #[test]
    fn helm_chart_detail_shows_chart_fields_and_pull_command() {
        let mut ann = HashMap::new();
        ann.insert(
            "org.opencontainers.image.created".to_owned(),
            "2026-09-03T19:51:50+02:00".to_owned(),
        );
        let resp = response(media_types::HELM_CONFIG, Some(ann));
        let d = ImageDetail::from_manifest_and_config(
            &resp,
            None,
            Some(&chart()),
            "library/n8n",
            "2.2.0-rc.1",
            "https://8gears.container-registry.com/",
        );
        let cmd = "helm pull oci://8gears.container-registry.com/library/n8n --version 2.2.0-rc.1";
        assert_eq!(d.copy_text(), Some(cmd));
        let out = text(&render_lines(&d));
        assert!(out.contains("Helm chart"), "{out}");
        assert!(out.contains(cmd), "{out}");
        assert!(out.contains("1.9.0"), "{out}");
        assert!(out.contains("Helm Chart for deploying n8n"), "{out}");
        assert!(out.contains("2026-09-03T19:51:50+02:00"), "{out}");
        assert!(!out.contains("OS/Arch"), "{out}");
        assert!(!out.contains("Labels"), "{out}");
    }

    /// A chart whose config blob could not be fetched or parsed still
    /// identifies itself and still gives a working pull command.
    #[test]
    fn helm_chart_detail_survives_missing_chart_config() {
        let resp = response(media_types::HELM_CONFIG, None);
        let d = ImageDetail::from_manifest_and_config(
            &resp,
            None,
            None,
            "a/b",
            "1.0.0",
            "https://ghcr.io",
        );
        assert_eq!(
            d.copy_text(),
            Some("helm pull oci://ghcr.io/a/b --version 1.0.0")
        );
        assert!(text(&render_lines(&d)).contains("Helm chart"));
    }

    #[test]
    fn container_image_detail_is_unchanged() {
        let resp = response("application/vnd.oci.image.config.v1+json", None);
        let d = ImageDetail::from_manifest_and_config(
            &resp,
            None,
            None,
            "a/b",
            "1.0.0",
            "https://ghcr.io",
        );
        assert!(d.helm.is_none());
        assert_eq!(d.copy_text(), Some("ghcr.io/a/b:1.0.0"));
        assert!(!text(&render_lines(&d)).contains("Helm"));
    }

    /// Real chart config blobs: the ARC sample from the ticket, and a sparse one.
    #[test]
    fn helm_chart_config_parses_real_world_blobs() {
        let arc: HelmChartConfig = serde_json::from_str(
            r#"{"apiVersion":"v2","appVersion":"0.10.1","description":"A Helm chart for install actions-runner-controller CRD",
                "home":"https://github.com/actions/actions-runner-controller",
                "maintainers":[{"name":"actions"}],"name":"gha-runner-scale-set-controller",
                "sources":["x"],"type":"application","version":"0.10.1"}"#,
        )
        .unwrap();
        assert_eq!(arc.name.as_deref(), Some("gha-runner-scale-set-controller"));
        assert_eq!(arc.app_version.as_deref(), Some("0.10.1"));
        let empty: HelmChartConfig = serde_json::from_str("{}").unwrap();
        assert!(empty.name.is_none() && empty.version.is_none());
    }
}

#[cfg(test)]
mod security_tests {
    use super::*;

    #[test]
    fn pull_command_accepts_real_references() {
        assert_eq!(
            helm_pull_command(
                "ghcr.io",
                "actions/arc-charts/gha-runner-scale-set",
                "0.10.1-rc.1"
            )
            .as_deref(),
            Some(
                "helm pull oci://ghcr.io/actions/arc-charts/gha-runner-scale-set --version 0.10.1-rc.1"
            )
        );
        assert!(helm_pull_command("localhost:5000", "a/b", "_x").is_some());
    }

    /// Tags and repos come from the registry (or the user's typing) and the
    /// command is pasted into a shell: anything shell-significant must yield
    /// no command at all.
    #[test]
    fn pull_command_refuses_shell_metacharacters() {
        for tag in [
            "1.0;rm -rf ~",
            "$(id)",
            "a b",
            "`x`",
            "1.0\n",
            "-x",
            "",
            "a|b",
            "a&b",
        ] {
            assert!(
                helm_pull_command("ghcr.io", "a/b", tag).is_none(),
                "{tag:?}"
            );
        }
        for repo in ["a/b;x", "A/B", "a/../b", "a//b", "", "$(x)/y", "a b"] {
            assert!(
                helm_pull_command("ghcr.io", repo, "1.0").is_none(),
                "{repo:?}"
            );
        }
        assert!(helm_pull_command("h;x", "a/b", "1.0").is_none());
        assert!(helm_pull_command("ghcr.io", "a/b", &"v".repeat(129)).is_none());
    }

    #[test]
    fn clean_strips_control_characters() {
        assert_eq!(clean("a\u{1b}[31mb\n".to_owned()), "a[31mb");
    }
}

#[cfg(test)]
mod fail_closed_tests {
    use super::*;
    use crate::registry::{ImageManifest, ManifestDescriptor, media_types};

    fn resp(config_type: &str) -> ManifestResponse {
        let d = |mt: &str| ManifestDescriptor {
            media_type: mt.to_owned(),
            size: 1,
            digest: "sha256:abc".to_owned(),
            platform: None,
        };
        ManifestResponse {
            manifest: Manifest::Image(ImageManifest {
                schema_version: 2,
                media_type: media_types::OCI_MANIFEST.to_owned(),
                config: d(config_type),
                layers: vec![d("application/octet-stream")],
                annotations: None,
            }),
            digest: "sha256:x".to_owned(),
            content_type: media_types::OCI_MANIFEST.to_owned(),
            raw: Vec::new(),
        }
    }

    /// An invalid tag must copy *nothing* — not fall back to the raw
    /// reference — for charts and for plain images.
    #[test]
    fn invalid_reference_copies_nothing_for_charts_and_images() {
        for cfg in [
            media_types::HELM_CONFIG,
            "application/vnd.oci.image.config.v1+json",
        ] {
            let d = ImageDetail::from_manifest_and_config(
                &resp(cfg),
                None,
                None,
                "a/b",
                "1.0;rm -rf ~",
                "https://ghcr.io",
            );
            assert_eq!(d.copy_text(), None, "{cfg}");
        }
    }
}

#[cfg(test)]
mod tag_mismatch_tests {
    use super::*;
    use crate::registry::{ImageManifest, ManifestDescriptor, media_types};

    fn descriptor(media_type: &str) -> ManifestDescriptor {
        ManifestDescriptor {
            media_type: media_type.to_owned(),
            size: 1,
            digest: "sha256:abc".to_owned(),
            platform: None,
        }
    }

    fn chart_detail(version: Option<&str>, tag: &str) -> ImageDetail {
        let resp = ManifestResponse {
            manifest: Manifest::Image(ImageManifest {
                schema_version: 2,
                media_type: media_types::OCI_MANIFEST.to_owned(),
                config: descriptor(media_types::HELM_CONFIG),
                layers: vec![descriptor("application/octet-stream")],
                annotations: None,
            }),
            digest: "sha256:deadbeef".to_owned(),
            content_type: media_types::OCI_MANIFEST.to_owned(),
            raw: Vec::new(),
        };
        let chart = HelmChartConfig {
            name: Some("demo".to_owned()),
            version: version.map(str::to_owned),
            ..HelmChartConfig::default()
        };
        ImageDetail::from_manifest_and_config(
            &resp,
            None,
            Some(&chart),
            "charts/demo",
            tag,
            "https://ghcr.io",
        )
    }

    fn version_line(d: &ImageDetail) -> String {
        render_lines(d)
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .find(|l| l.starts_with("Version"))
            .unwrap_or_default()
    }

    #[test]
    fn helm_tag_for_version_encodes_build_metadata() {
        assert_eq!(helm_tag_for_version("1.2.3"), "1.2.3");
        assert_eq!(helm_tag_for_version("1.2.3+build5"), "1.2.3_build5");
    }

    /// The #159 finding: `0.2.0` retagged as `0.2.0-retag` still says 0.2.0.
    #[test]
    fn a_tag_that_differs_from_the_chart_version_is_flagged() {
        let d = chart_detail(Some("0.2.0"), "0.2.0-retag");
        assert_eq!(d.helm.as_ref().unwrap().tag_matches_version, Some(false));
        let line = version_line(&d);
        assert!(
            line.contains("0.2.0") && line.contains("(tag differs)"),
            "{line}"
        );
    }

    #[test]
    fn a_matching_tag_is_not_flagged() {
        let d = chart_detail(Some("0.2.0"), "0.2.0");
        assert_eq!(d.helm.as_ref().unwrap().tag_matches_version, Some(true));
        assert!(
            !version_line(&d).contains("tag differs"),
            "{}",
            version_line(&d)
        );
    }

    /// Helm writes `1.2.3+build5` as the tag `1.2.3_build5`: that is a match.
    #[test]
    fn build_metadata_encoded_the_helm_way_matches() {
        let d = chart_detail(Some("1.2.3+build5"), "1.2.3_build5");
        assert_eq!(d.helm.as_ref().unwrap().tag_matches_version, Some(true));
        assert!(!version_line(&d).contains("tag differs"));
    }

    /// With no version to compare against there is nothing to claim.
    #[test]
    fn an_unknown_version_makes_no_claim() {
        let d = chart_detail(None, "stable");
        assert_eq!(d.helm.as_ref().unwrap().tag_matches_version, None);
        assert!(version_line(&d).is_empty());
    }
}
