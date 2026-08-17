//! Editor-facing configuration for the language server.
//!
//! Configuration is accepted from three sources (later sources win):
//!
//! 1. `initializationOptions` of `initialize`,
//! 2. the `workspace/configuration` response (section `pandoc`),
//! 3. `workspace/didChangeConfiguration` notifications (`settings.pandoc`).
//!
//! ```json
//! {
//!   "extensions": {
//!     "flavor": "markdown",
//!     "enabled": ["emoji"],
//!     "disabled": ["smart"],
//!     "format": "markdown+citations-smart"
//!   },
//!   "diagnostics": {
//!     "unresolvedReferences": true,
//!     "disabledExtensions": true,
//!     "pandoc": "off"
//!   },
//!   "completion": {
//!     "citations": true,
//!     "anchors": true,
//!     "referenceLabels": true
//!   }
//! }
//! ```

use pandocmd_extensions::{ExtensionConfig, Flavor};
use serde::{Deserialize, Serialize};

/// Root configuration object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PandocmdConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<ExtensionsSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<DiagnosticsSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion: Option<CompletionSection>,
}

/// Pandoc extension selection.
pub type ExtensionsSection = ExtensionConfig;

/// Control over diagnostic categories.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiagnosticsSection {
    /// Warn about unresolved references, footnotes, anchors, and citations.
    #[serde(default = "default_true")]
    pub unresolved_references: bool,
    /// Warn when a construct is used while its extension is disabled.
    #[serde(default = "default_true")]
    pub disabled_extensions: bool,
    /// How to use the external `pandoc` executable for validation.
    #[serde(default)]
    pub pandoc: PandocValidationMode,
}

impl Default for DiagnosticsSection {
    fn default() -> Self {
        DiagnosticsSection {
            unresolved_references: true,
            disabled_extensions: true,
            pandoc: PandocValidationMode::default(),
        }
    }
}

fn default_true() -> bool {
    true
}

/// External pandoc validation policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PandocValidationMode {
    /// Never run pandoc.
    #[default]
    Off,
    /// Run pandoc when a document is saved.
    OnSave,
}

impl PandocValidationMode {
    pub fn from_str_value(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" | "false" | "none" | "disabled" => Some(PandocValidationMode::Off),
            "onsave" | "on_save" | "save" | "on" | "true" | "enabled" => {
                Some(PandocValidationMode::OnSave)
            }
            _ => None,
        }
    }
}

/// Control over completion sources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompletionSection {
    #[serde(default = "default_true")]
    pub citations: bool,
    #[serde(default = "default_true")]
    pub anchors: bool,
    #[serde(default = "default_true")]
    pub reference_labels: bool,
}

impl Default for CompletionSection {
    fn default() -> Self {
        CompletionSection {
            citations: true,
            anchors: true,
            reference_labels: true,
        }
    }
}

impl PandocmdConfig {
    /// Merge `other` on top of `self` (fields set in `other` win).
    pub fn merge(&mut self, other: PandocmdConfig) {
        if other.extensions.is_some() {
            self.extensions = other.extensions;
        }
        if other.diagnostics.is_some() {
            self.diagnostics = other.diagnostics;
        }
        if other.completion.is_some() {
            self.completion = other.completion;
        }
    }

    /// Extract a config from a client `settings` object, looking under the
    /// `pandoc` (preferred) or `pandocmd` keys.
    pub fn from_settings(settings: &serde_json::Value) -> Option<PandocmdConfig> {
        for key in ["pandoc", "pandocmd"] {
            if let Some(section) = settings.get(key) {
                if section.is_null() {
                    continue;
                }
                return serde_json::from_value(section.clone()).ok();
            }
        }
        None
    }
}

/// Fully-resolved runtime settings derived from [`PandocmdConfig`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSettings {
    /// Base flavor name (used for display/telemetry).
    pub flavor: Flavor,
    pub extension_config: ExtensionConfig,
    pub unresolved_references: bool,
    pub disabled_extensions: bool,
    pub pandoc_validation: PandocValidationMode,
    pub completion_citations: bool,
    pub completion_anchors: bool,
    pub completion_reference_labels: bool,
}

impl Default for ResolvedSettings {
    fn default() -> Self {
        ResolvedSettings {
            flavor: Flavor::Markdown,
            extension_config: ExtensionConfig::default(),
            unresolved_references: true,
            disabled_extensions: true,
            pandoc_validation: PandocValidationMode::Off,
            completion_citations: true,
            completion_anchors: true,
            completion_reference_labels: true,
        }
    }
}

impl ResolvedSettings {
    pub fn from_config(config: &PandocmdConfig) -> Self {
        let mut settings = ResolvedSettings::default();
        settings.apply(config);
        settings
    }

    pub fn apply(&mut self, config: &PandocmdConfig) {
        if let Some(extensions) = &config.extensions {
            self.extension_config = extensions.clone();
            if let Some(flavor) = extensions.flavor.as_deref().and_then(Flavor::from_name) {
                self.flavor = flavor;
            }
        }
        if let Some(diagnostics) = &config.diagnostics {
            self.unresolved_references = diagnostics.unresolved_references;
            self.disabled_extensions = diagnostics.disabled_extensions;
            self.pandoc_validation = diagnostics.pandoc;
        }
        if let Some(completion) = &config.completion {
            self.completion_citations = completion.citations;
            self.completion_anchors = completion.anchors;
            self.completion_reference_labels = completion.reference_labels;
        }
    }
}

/// Accept `"onSave"` (enum) or legacy string forms for the pandoc mode.
pub fn deserialize_pandoc_mode_flexible(value: serde_json::Value) -> Option<PandocValidationMode> {
    if let Some(mode) = PandocValidationMode::from_str_value(value.as_str()?) {
        return Some(mode);
    }
    serde_json::from_value::<PandocValidationMode>(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_config() {
        let config: PandocmdConfig = serde_json::from_str(
            r#"{
              "extensions": { "flavor": "gfm", "enabled": ["citations"] },
              "diagnostics": { "unresolvedReferences": false, "pandoc": "onSave" },
              "completion": { "anchors": false }
            }"#,
        )
        .unwrap();

        assert_eq!(
            config.extensions.as_ref().unwrap().flavor.as_deref(),
            Some("gfm")
        );
        let diagnostics = config.diagnostics.clone().unwrap();
        assert!(!diagnostics.unresolved_references);
        assert!(diagnostics.disabled_extensions);
        assert_eq!(diagnostics.pandoc, PandocValidationMode::OnSave);
        let completion = config.completion.clone().unwrap();
        assert!(!completion.anchors);
        assert!(completion.citations);

        let settings = ResolvedSettings::from_config(&config);
        assert!(settings.completion_citations);
        assert!(!settings.completion_anchors);
        assert_eq!(settings.pandoc_validation, PandocValidationMode::OnSave);
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(serde_json::from_str::<PandocmdConfig>(r#"{"bogus": true}"#).is_err());
        assert!(serde_json::from_str::<DiagnosticsSection>(r#"{"pandoc": "sometimes"}"#).is_err());
    }

    #[test]
    fn merges_configs() {
        let mut base = PandocmdConfig::default();
        let overlay: PandocmdConfig =
            serde_json::from_str(r#"{"diagnostics": {"pandoc": "onSave"}}"#).unwrap();
        base.merge(overlay);
        assert_eq!(
            base.diagnostics.unwrap().pandoc,
            PandocValidationMode::OnSave
        );
    }

    #[test]
    fn reads_settings_sections() {
        let settings: serde_json::Value =
            serde_json::from_str(r#"{"pandoc": {"completion": {"citations": false}}}"#).unwrap();
        let config = PandocmdConfig::from_settings(&settings).unwrap();
        assert!(!config.completion.unwrap().citations);
    }

    #[test]
    fn flexible_pandoc_mode() {
        assert_eq!(
            deserialize_pandoc_mode_flexible(serde_json::json!("onSave")),
            Some(PandocValidationMode::OnSave)
        );
        assert_eq!(
            deserialize_pandoc_mode_flexible(serde_json::json!("off")),
            Some(PandocValidationMode::Off)
        );
        assert_eq!(
            deserialize_pandoc_mode_flexible(serde_json::json!("nonsense")),
            None
        );
    }
}
