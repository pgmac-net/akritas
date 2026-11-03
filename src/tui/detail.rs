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
    /// `helm pull oci://<host>/<repo> --version <tag>` — the `--version` form
    /// is the unambiguous one; tag-in-reference support varies by Helm version.
    pub pull_command: String,
}

#[derive(Debug, Clone)]
pub struct ImageDetail {
    /// `Some` when this tag is a Helm chart rather than a container image.
    pub helm: Option<HelmInfo>,
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
    pub fn copy_text(&self) -> &str {
        self.helm
            .as_ref()
            .map_or(self.pull_url.as_str(), |h| h.pull_command.as_str())
    }

    pub fn from_manifest_and_config(
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
                Self {
                    helm: Some(HelmInfo {
                        name: chart.name,
                        version: chart.version,
                        app_version: chart.app_version,
                        description: chart.description,
                        pull_command: format!("helm pull oci://{host}/{repo} --version {tag}"),
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
        Span::styled(detail.copy_text().to_owned(), URL_STYLE),
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
                kv(&mut lines, key, Span::styled(v.clone(), VAL_STYLE));
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
        assert_eq!(d.copy_text(), cmd);
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
        assert_eq!(d.copy_text(), "helm pull oci://ghcr.io/a/b --version 1.0.0");
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
        assert_eq!(d.copy_text(), "ghcr.io/a/b:1.0.0");
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
