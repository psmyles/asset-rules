//! Rule catalogs: each checker's description of every rule it knows (format 1).
//!
//! A checker keeps its catalog in `rules-catalog.toml` at its repository root. The catalog is the
//! only description of a rule outside its check: its id, category, the release it came in, the
//! texts the artist reads, its parameters and its defaults. The format is specified in the rules
//! repo, `docs/rule-editor-plan.md`, "The catalog file (format 1)".
//!
//! - [`Catalog::from_toml`] reads the file; [`Catalog::check`] lists its problems (an app's
//!   `build.rs` fails on any).
//! - [`Catalog::to_json`] is what `<cli> rules catalog` prints; [`Catalog::from_json`] reads it
//!   back, for Rule Editor.
//! - [`Rule::settings`] works out a rule's settings for a rules file, leniently, as the checkers
//!   load rules files; [`Catalog::check_rules_file`] lists everything wrong in one, strictly.
//! - [`Rule::texts`] and [`fill`] give the texts a finding shows.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::Pattern;
use crate::results::Severity;
use crate::version::Version;

/// The catalog format this crate reads and writes.
pub const CATALOG_VERSION: u64 = 1;

/// The rules-file format the checkers read (`"version"` in a rules file).
pub const RULES_FILE_VERSION: u64 = 1;

/// A checker's rule catalog.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    /// The format version: [`CATALOG_VERSION`].
    pub catalog: u64,
    /// The checker, as `rules.json`'s types name it (`fire-cli`).
    pub tool: String,
    /// The checker's own version. Only in what `rules catalog` prints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// The default sets, e.g. one per engine. Empty: one unnamed set.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bases: Vec<String>,
    /// In display order.
    #[serde(rename = "category")]
    pub categories: Vec<Category>,
    /// In display order.
    #[serde(rename = "rule")]
    pub rules: Vec<Rule>,
    /// Fields this crate doesn't know, found on load: (where, key).
    #[serde(skip)]
    unknown: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Category {
    pub id: String,
    pub title: String,
}

/// One rule.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    /// Stable: rules files and results key on it.
    pub id: String,
    /// The identifier code uses; see [`Rule::code_name`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub category: String,
    /// The first release of the checker with this rule.
    pub since: String,
    pub title: String,
    pub what: String,
    pub why: String,
    pub how: String,
    /// The values the check passes in for the texts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub measures: Vec<String>,
    pub default: RuleDefault,
    /// What differs per default set, by base name.
    #[serde(default, rename = "base", skip_serializing_if = "BTreeMap::is_empty")]
    pub bases: BTreeMap<String, BaseOverride>,
    /// In display order.
    #[serde(default, rename = "param", skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<Param>,
    /// Named variants of the texts, which the check picks.
    #[serde(default, rename = "case", skip_serializing_if = "BTreeMap::is_empty")]
    pub cases: BTreeMap<String, Texts>,
    /// Anything only the app reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<Value>,
}

/// A rule's default in every set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleDefault {
    pub enabled: bool,
    pub severity: Severity,
}

/// What differs for a rule in one default set.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BaseOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<Severity>,
    /// Parameter values by key.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, Value>,
    #[serde(flatten)]
    pub texts: Texts,
}

/// Texts that replace a rule's own: any of them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Texts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub what: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub how: Option<String>,
}

impl Texts {
    fn is_empty(&self) -> bool {
        self.title.is_none() && self.what.is_none() && self.why.is_none() && self.how.is_none()
    }

    /// `(field, text)` for the texts given.
    fn given(&self) -> impl Iterator<Item = (&'static str, &str)> {
        [
            ("title", &self.title),
            ("what", &self.what),
            ("why", &self.why),
            ("how", &self.how),
        ]
        .into_iter()
        .filter_map(|(field, text)| text.as_deref().map(|text| (field, text)))
    }
}

/// The texts a finding shows, worked out: see [`Rule::texts`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FindingTexts {
    pub title: String,
    pub what: String,
    pub why: String,
    pub how: String,
}

/// One of a rule's settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Param {
    /// Unique within the rule; rules files use it under `params`.
    pub key: String,
    pub kind: ParamKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// For `number`: shown as the value times this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// For `choice` and `choices`, the values allowed; for `count` and `number`, suggestions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<ParamOption>,
    /// The value in every set unless a set's `params` gives another.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
}

/// What a parameter holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParamKind {
    /// A whole number.
    Count,
    Number,
    /// `true` or `false`.
    Flag,
    /// One of `options`.
    Choice,
    /// Any of `options`, as a list.
    Choices,
    /// A list of globs.
    Patterns,
}

impl ParamKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ParamKind::Count => "count",
            ParamKind::Number => "number",
            ParamKind::Flag => "flag",
            ParamKind::Choice => "choice",
            ParamKind::Choices => "choices",
            ParamKind::Patterns => "patterns",
        }
    }

    fn numeric(self) -> bool {
        matches!(self, ParamKind::Count | ParamKind::Number)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParamOption {
    pub value: Value,
    pub label: String,
    /// Other values the checker accepts for this option.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<Value>,
}

/// A rule's settings for one rules file: its defaults, with what the file sets over them.
#[derive(Clone, Debug, PartialEq)]
pub struct RuleSettings {
    pub enabled: bool,
    pub severity: Severity,
    /// Every parameter, by key.
    pub params: BTreeMap<String, Value>,
    /// The technical artist's note, shown beside the rule's findings.
    pub note: Option<String>,
}

/// Why a catalog couldn't be read.
#[derive(Debug)]
pub enum CatalogError {
    /// Not TOML.
    Toml(String),
    /// Not JSON.
    Json(serde_json::Error),
    /// Not a catalog: a required field is missing or a value has the wrong type.
    Shape(serde_json::Error),
    /// `catalog` is missing or isn't [`CATALOG_VERSION`].
    Version(Option<String>),
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CatalogError::Toml(error) => write!(f, "isn't valid TOML: {error}"),
            CatalogError::Json(error) => write!(f, "isn't valid JSON: {error}"),
            CatalogError::Shape(error) => write!(f, "isn't a rules catalog: {error}"),
            CatalogError::Version(None) => {
                write!(
                    f,
                    "has no \"catalog\"; this reads catalog format {CATALOG_VERSION}"
                )
            }
            CatalogError::Version(Some(found)) => write!(
                f,
                "is catalog format {found}, but this reads format {CATALOG_VERSION}"
            ),
        }
    }
}

impl Error for CatalogError {}

/// One problem in a catalog or in a rules file read against one. Always an error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogProblem {
    /// Where: `rule "texture.max_size" param "max"`, `rules["texture.max_size"].params.max`.
    pub at: String,
    pub message: String,
}

impl fmt::Display for CatalogProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.at, self.message)
    }
}

impl Catalog {
    /// Reads a `rules-catalog.toml`.
    pub fn from_toml(text: &str) -> Result<Catalog, CatalogError> {
        let value: Value =
            toml::from_str(text).map_err(|error| CatalogError::Toml(error.to_string()))?;
        Catalog::from_value(value)
    }

    /// Reads what `rules catalog` printed.
    pub fn from_json(text: &str) -> Result<Catalog, CatalogError> {
        let value: Value = serde_json::from_str(text).map_err(CatalogError::Json)?;
        Catalog::from_value(value)
    }

    fn from_value(value: Value) -> Result<Catalog, CatalogError> {
        match value.get("catalog") {
            Some(found) if found.as_u64() == Some(CATALOG_VERSION) => {}
            found => return Err(CatalogError::Version(found.map(Value::to_string))),
        }
        let unknown = unknown_fields(&value);
        let mut catalog: Catalog = serde_json::from_value(value).map_err(CatalogError::Shape)?;
        catalog.unknown = unknown;
        Ok(catalog)
    }

    /// The catalog as JSON, for `rules catalog` to print. `version` is the checker's own.
    pub fn to_json(&self, version: &str) -> String {
        let mut catalog = self.clone();
        catalog.version = Some(version.to_owned());
        serde_json::to_string_pretty(&catalog).unwrap_or_default()
    }

    pub fn rule(&self, id: &str) -> Option<&Rule> {
        self.rules.iter().find(|rule| rule.id == id)
    }

    pub fn category(&self, id: &str) -> Option<&Category> {
        self.categories.iter().find(|category| category.id == id)
    }

    /// Every problem in the catalog. `app_version`, if given, is the newest `since` allowed: the
    /// app's own version.
    pub fn check(&self, app_version: Option<&str>) -> Vec<CatalogProblem> {
        let mut out = Problems::default();
        for (at, key) in &self.unknown {
            out.push(at, format!("unknown field \"{key}\""));
        }
        if self.tool.trim().is_empty() {
            out.push("tool", "is empty".to_owned());
        }
        let app_version = app_version.and_then(|v| {
            let parsed = Version::parse(v);
            if parsed.is_none() {
                out.push("app version", format!("\"{v}\" isn't a version like 0.9.0"));
            }
            parsed
        });
        for (i, base) in self.bases.iter().enumerate() {
            if !is_word(base) {
                out.push("bases", format!("\"{base}\" isn't a lowercase word"));
            }
            if self.bases[..i].contains(base) {
                out.push("bases", format!("\"{base}\" is listed more than once"));
            }
        }
        if self.categories.is_empty() {
            out.push("category", "there are no categories".to_owned());
        }
        for (i, category) in self.categories.iter().enumerate() {
            let at = format!("category \"{}\"", category.id);
            if !is_word(&category.id) {
                out.push(&at, "the id isn't a lowercase word".to_owned());
            }
            if self.categories[..i].iter().any(|c| c.id == category.id) {
                out.push(&at, "the id is used more than once".to_owned());
            }
            if category.title.trim().is_empty() {
                out.push(&at, "has no title".to_owned());
            }
        }
        let mut names: BTreeMap<String, &str> = BTreeMap::new();
        for (i, rule) in self.rules.iter().enumerate() {
            if self.rules[..i].iter().any(|r| r.id == rule.id) {
                out.push(
                    &format!("rule \"{}\"", rule.id),
                    "the id is used more than once".to_owned(),
                );
            }
            let name = rule.code_name();
            match names.get(&name) {
                Some(other) => out.push(
                    &format!("rule \"{}\"", rule.id),
                    format!(
                        "its code name \"{name}\" is also rule \"{other}\"'s; give one a \"name\""
                    ),
                ),
                None => {
                    names.insert(name, &rule.id);
                }
            }
            self.check_rule(rule, app_version.as_ref(), &mut out);
        }
        out.0
    }

    fn check_rule(&self, rule: &Rule, app_version: Option<&Version>, out: &mut Problems) {
        let at = format!("rule \"{}\"", rule.id);
        if !is_rule_id(&rule.id) {
            out.push(
                &at,
                "the id isn't lowercase words joined by \".\", like texture.max_size".to_owned(),
            );
        }
        if let Some(name) = &rule.name
            && !is_code_name(name)
        {
            out.push(
                &at,
                format!("\"name\" \"{name}\" isn't a CamelCase identifier"),
            );
        }
        if self.category(&rule.category).is_none() {
            let known: Vec<&str> = self.categories.iter().map(|c| c.id.as_str()).collect();
            out.push(
                &at,
                format!(
                    "category \"{}\" isn't one of the categories ({})",
                    rule.category,
                    known.join(", ")
                ),
            );
        }
        match Version::parse(&rule.since) {
            None => out.push(
                &at,
                format!("since \"{}\" isn't a version like 0.9.0", rule.since),
            ),
            Some(since) => {
                if let Some(app) = app_version
                    && since > *app
                {
                    out.push(
                        &at,
                        format!(
                            "since \"{}\" is newer than the app's own version",
                            rule.since
                        ),
                    );
                }
            }
        }

        // Measures: identifiers, once each, and never a parameter's key.
        for (i, measure) in rule.measures.iter().enumerate() {
            if !is_word(measure) {
                out.push(&at, format!("measure \"{measure}\" isn't a lowercase word"));
            }
            if rule.measures[..i].contains(measure) {
                out.push(
                    &at,
                    format!("measure \"{measure}\" is listed more than once"),
                );
            }
            if rule.param(measure).is_some() {
                out.push(
                    &at,
                    format!("measure \"{measure}\" has the same name as a parameter"),
                );
            }
        }

        // Texts.
        let own = Texts {
            title: Some(rule.title.clone()),
            what: Some(rule.what.clone()),
            why: Some(rule.why.clone()),
            how: Some(rule.how.clone()),
        };
        self.check_texts(rule, &at, &own, out);
        for (name, texts) in &rule.cases {
            let at = format!("{at} case \"{name}\"");
            if !is_word(name) {
                out.push(&at, "the case name isn't a lowercase word".to_owned());
            }
            if texts.is_empty() {
                out.push(&at, "gives no text".to_owned());
            }
            self.check_texts(rule, &at, texts, out);
        }

        // Parameters.
        for (i, param) in rule.params.iter().enumerate() {
            let at = format!("{at} param \"{}\"", param.key);
            if !is_word(&param.key) {
                out.push(&at, "the key isn't a lowercase word".to_owned());
            }
            if rule.params[..i].iter().any(|p| p.key == param.key) {
                out.push(&at, "the key is used more than once".to_owned());
            }
            check_param(param, &at, out);
            match &param.default {
                Some(value) => {
                    if let Err(why) = param.check_value(value, true) {
                        out.push(&at, format!("default: {why}"));
                    }
                }
                None => {
                    let missing: Vec<&str> = if self.bases.is_empty() {
                        vec![""]
                    } else {
                        self.bases
                            .iter()
                            .filter(|base| {
                                !rule
                                    .bases
                                    .get(*base)
                                    .is_some_and(|o| o.params.contains_key(&param.key))
                            })
                            .map(String::as_str)
                            .collect()
                    };
                    if missing == [""] {
                        out.push(&at, "has no default".to_owned());
                    } else if !missing.is_empty() {
                        out.push(
                            &at,
                            format!(
                                "has no default, and these sets give it none: {}",
                                missing.join(", ")
                            ),
                        );
                    }
                }
            }
        }

        // What differs per set.
        for (base, over) in &rule.bases {
            let at = format!("{at} base \"{base}\"");
            if self.bases.is_empty() {
                out.push(
                    &at,
                    "the catalog has no \"bases\", so a rule can't differ per set".to_owned(),
                );
            } else if !self.bases.contains(base) {
                out.push(
                    &at,
                    format!(
                        "\"{base}\" isn't one of the bases ({})",
                        self.bases.join(", ")
                    ),
                );
            }
            for (key, value) in &over.params {
                match rule.param(key) {
                    None => out.push(&at, format!("params: \"{key}\" isn't a parameter")),
                    Some(param) => {
                        if let Err(why) = param.check_value(value, true) {
                            out.push(&at, format!("params \"{key}\": {why}"));
                        }
                    }
                }
            }
            self.check_texts(rule, &at, &over.texts, out);
        }
    }

    /// Placeholders name a parameter or a measure; `title` has none; given texts aren't empty.
    fn check_texts(&self, rule: &Rule, at: &str, texts: &Texts, out: &mut Problems) {
        for (field, text) in texts.given() {
            if text.trim().is_empty() {
                out.push(at, format!("\"{field}\" is empty"));
                continue;
            }
            match placeholders(text) {
                Err(why) => out.push(at, format!("\"{field}\": {why}")),
                Ok(names) => {
                    if field == "title" && !names.is_empty() {
                        out.push(at, "\"title\" can't have placeholders".to_owned());
                    }
                    for name in names {
                        if rule.param(&name).is_none() && !rule.measures.contains(&name) {
                            out.push(
                                at,
                                format!(
                                    "\"{field}\" uses {{{name}}}, which is neither a parameter \
                                     nor one of \"measures\""
                                ),
                            );
                        }
                    }
                }
            }
        }
    }

    /// Everything wrong in a rules file (its JSON) read against this catalog. The checkers load
    /// rules files leniently ([`Rule::settings`]); this is the strict check for `rules check`,
    /// Rule Editor and the rules repo's own check.
    pub fn check_rules_file(&self, doc: &Value) -> Vec<CatalogProblem> {
        let mut out = Problems::default();
        match doc.get("version") {
            Some(v) if v.as_u64() == Some(RULES_FILE_VERSION) => {}
            Some(v) => out.push(
                "version",
                format!("is {v}, but rules files are version {RULES_FILE_VERSION}"),
            ),
            None => out.push(
                "version",
                format!("is missing; rules files are version {RULES_FILE_VERSION}"),
            ),
        }
        let Some(profile) = doc.get("profile").and_then(Value::as_object) else {
            out.push("profile", "is missing or isn't an object".to_owned());
            return out.0;
        };
        match (profile.get("base"), self.bases.is_empty()) {
            (None, _) => {}
            (Some(_), true) => out.push(
                "profile.base",
                format!("{} has no default sets to choose from", self.tool),
            ),
            (Some(base), false) => {
                if !base
                    .as_str()
                    .is_some_and(|b| self.bases.iter().any(|x| x == b))
                {
                    out.push(
                        "profile.base",
                        format!("{base} isn't one of {}", self.bases.join(", ")),
                    );
                }
            }
        }
        let Some(rules) = profile.get("rules").and_then(Value::as_object) else {
            out.push("profile.rules", "is missing or isn't an object".to_owned());
            return out.0;
        };
        for (id, entry) in rules {
            let at = format!("rules[\"{id}\"]");
            let Some(rule) = self.rule(id) else {
                out.push(&at, format!("{} has no rule \"{id}\"", self.tool));
                continue;
            };
            let Some(entry) = entry.as_object() else {
                out.push(&at, "isn't an object".to_owned());
                continue;
            };
            for (key, value) in entry {
                match key.as_str() {
                    "enabled" if !value.is_boolean() => {
                        out.push(&at, "\"enabled\" isn't true or false".to_owned());
                    }
                    "severity" if serde_json::from_value::<Severity>(value.clone()).is_err() => {
                        out.push(
                            &at,
                            format!("severity {value} isn't error, warning or info"),
                        );
                    }
                    "note" if !value.is_string() => {
                        out.push(&at, "\"note\" isn't text".to_owned());
                    }
                    "params" => match value.as_object() {
                        None => out.push(&at, "\"params\" isn't an object".to_owned()),
                        Some(params) => {
                            for (key, value) in params {
                                match rule.param(key) {
                                    None => out.push(
                                        &at,
                                        format!("\"{key}\" isn't a parameter of this rule"),
                                    ),
                                    Some(param) => {
                                        if let Err(why) = param.check_value(value, false) {
                                            out.push(&format!("{at}.params.{key}"), why);
                                        }
                                    }
                                }
                            }
                        }
                    },
                    "enabled" | "severity" | "note" => {}
                    other => out.push(&at, format!("unknown field \"{other}\"")),
                }
            }
        }
        out.0
    }
}

impl Rule {
    /// The identifier code uses: `name`, else the part of `id` after the first dot in CamelCase
    /// (`texture.max_size` → `MaxSize`).
    pub fn code_name(&self) -> String {
        if let Some(name) = &self.name {
            return name.clone();
        }
        let rest = self
            .id
            .split_once('.')
            .map_or(self.id.as_str(), |(_, rest)| rest);
        rest.split(['.', '_'])
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                    None => String::new(),
                }
            })
            .collect()
    }

    pub fn param(&self, key: &str) -> Option<&Param> {
        self.params.iter().find(|param| param.key == key)
    }

    /// The rule's defaults in `base`'s set (`None`: the unnamed set, or a base the catalog
    /// doesn't have).
    pub fn defaults(&self, base: Option<&str>) -> RuleSettings {
        let over = base.and_then(|base| self.bases.get(base));
        let mut params = BTreeMap::new();
        for param in &self.params {
            let value = over
                .and_then(|o| o.params.get(&param.key))
                .or(param.default.as_ref());
            if let Some(value) = value {
                params.insert(param.key.clone(), value.clone());
            }
        }
        RuleSettings {
            enabled: over.and_then(|o| o.enabled).unwrap_or(self.default.enabled),
            severity: over
                .and_then(|o| o.severity)
                .unwrap_or(self.default.severity),
            params,
            note: None,
        }
    }

    /// The rule's settings for a rules file: the defaults in `base`'s set, with what `entry` (the
    /// rule's object in the file, if listed) sets over them. Lenient, as the checkers load rules
    /// files: a value of the wrong kind keeps the default, and an alias reads as its option's
    /// value. [`Catalog::check_rules_file`] reports what was passed over.
    pub fn settings(&self, base: Option<&str>, entry: Option<&Value>) -> RuleSettings {
        let mut settings = self.defaults(base);
        let Some(entry) = entry.and_then(Value::as_object) else {
            return settings;
        };
        if let Some(enabled) = entry.get("enabled").and_then(Value::as_bool) {
            settings.enabled = enabled;
        }
        if let Some(severity) = entry
            .get("severity")
            .and_then(|v| serde_json::from_value::<Severity>(v.clone()).ok())
        {
            settings.severity = severity;
        }
        if let Some(params) = entry.get("params").and_then(Value::as_object) {
            for (key, value) in params {
                if let Some(param) = self.param(key)
                    && param.check_value(value, false).is_ok()
                {
                    settings.params.insert(key.clone(), param.canonical(value));
                }
            }
        }
        settings.note = entry
            .get("note")
            .and_then(Value::as_str)
            .filter(|note| !note.trim().is_empty())
            .map(str::to_owned);
        settings
    }

    /// The texts a finding shows, before placeholders are filled: the rule's own, replaced by
    /// `base`'s, replaced by `case`'s, one text at a time.
    pub fn texts(&self, base: Option<&str>, case: Option<&str>) -> FindingTexts {
        let mut texts = FindingTexts {
            title: self.title.clone(),
            what: self.what.clone(),
            why: self.why.clone(),
            how: self.how.clone(),
        };
        let layers = [
            base.and_then(|b| self.bases.get(b)).map(|o| &o.texts),
            case.and_then(|c| self.cases.get(c)),
        ];
        for layer in layers.into_iter().flatten() {
            for (field, text) in layer.given() {
                let slot = match field {
                    "title" => &mut texts.title,
                    "what" => &mut texts.what,
                    "why" => &mut texts.why,
                    _ => &mut texts.how,
                };
                *slot = text.to_owned();
            }
        }
        texts
    }

    /// `texts` with placeholders filled: parameters from `settings`, shown as [`Param::show`]
    /// shows them, and measures from `measures`.
    pub fn fill_texts(
        &self,
        texts: &FindingTexts,
        settings: &RuleSettings,
        measures: &BTreeMap<&str, String>,
    ) -> FindingTexts {
        let value = |name: &str| -> Option<String> {
            if let Some(measure) = measures.get(name) {
                return Some(measure.clone());
            }
            let param = self.param(name)?;
            settings.params.get(name).map(|v| param.show(v))
        };
        FindingTexts {
            title: texts.title.clone(),
            what: fill(&texts.what, value),
            why: fill(&texts.why, value),
            how: fill(&texts.how, value),
        }
    }
}

impl Param {
    /// Whether `value` is one this parameter can hold. `strict` (the catalog's own values) allows
    /// only option values, not aliases.
    pub fn check_value(&self, value: &Value, strict: bool) -> Result<(), String> {
        match self.kind {
            ParamKind::Count | ParamKind::Number => {
                let Some(n) = value.as_f64() else {
                    return Err(format!("{value} isn't a number"));
                };
                if self.kind == ParamKind::Count && !(value.is_i64() || value.is_u64()) {
                    return Err(format!("{value} isn't a whole number"));
                }
                self.in_range(n)
            }
            ParamKind::Flag => match value {
                Value::Bool(_) => Ok(()),
                _ => Err(format!("{value} isn't true or false")),
            },
            ParamKind::Choice => self.option_for(value, strict).map(|_| ()),
            ParamKind::Choices => {
                let Some(items) = value.as_array() else {
                    return Err(format!("{value} isn't a list"));
                };
                items
                    .iter()
                    .try_for_each(|item| self.option_for(item, strict).map(|_| ()))
            }
            ParamKind::Patterns => {
                let Some(items) = value.as_array() else {
                    return Err(format!("{value} isn't a list"));
                };
                for item in items {
                    let Some(text) = item.as_str() else {
                        return Err(format!("{item} isn't a pattern"));
                    };
                    if let Some(error) = Pattern::from(text).error() {
                        return Err(format!("\"{text}\" isn't a valid glob: {error}"));
                    }
                }
                Ok(())
            }
        }
    }

    fn in_range(&self, n: f64) -> Result<(), String> {
        if let Some(min) = self.min
            && n < min
        {
            return Err(format!("{n} is less than the least allowed, {min}"));
        }
        if let Some(max) = self.max
            && n > max
        {
            return Err(format!("{n} is more than the most allowed, {max}"));
        }
        Ok(())
    }

    /// The option `value` names: its value, or (unless `strict`) an alias. Text matches ignoring
    /// case.
    fn option_for(&self, value: &Value, strict: bool) -> Result<&ParamOption, String> {
        let same = |a: &Value, b: &Value| match (a.as_str(), b.as_str()) {
            (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
            _ => numbers_equal(a, b),
        };
        self.options
            .iter()
            .find(|option| {
                same(&option.value, value)
                    || (!strict && option.aliases.iter().any(|alias| same(alias, value)))
            })
            .ok_or_else(|| {
                let values: Vec<String> =
                    self.options.iter().map(|o| o.value.to_string()).collect();
                format!("{value} isn't one of {}", values.join(", "))
            })
    }

    /// `value` as the catalog writes it: an alias or another capitalisation becomes its option's
    /// value. Anything else is returned as it is.
    pub fn canonical(&self, value: &Value) -> Value {
        match self.kind {
            ParamKind::Choice => self
                .option_for(value, false)
                .map_or_else(|_| value.clone(), |o| o.value.clone()),
            ParamKind::Choices => match value.as_array() {
                Some(items) => Value::Array(
                    items
                        .iter()
                        .map(|item| {
                            self.option_for(item, false)
                                .map_or_else(|_| item.clone(), |o| o.value.clone())
                        })
                        .collect(),
                ),
                None => value.clone(),
            },
            _ => value.clone(),
        }
    }

    /// `value` as a text shows it: numbers without a trailing `.0`, a choice as its option's
    /// label, lists joined with ", ".
    pub fn show(&self, value: &Value) -> String {
        match self.kind {
            ParamKind::Choice => self
                .option_for(value, false)
                .map_or_else(|_| plain(value), |o| o.label.clone()),
            ParamKind::Choices => value
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .map(|item| {
                            self.option_for(item, false)
                                .map_or_else(|_| plain(item), |o| o.label.clone())
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_else(|| plain(value)),
            ParamKind::Patterns => value
                .as_array()
                .map(|items| items.iter().map(plain).collect::<Vec<_>>().join(", "))
                .unwrap_or_else(|| plain(value)),
            ParamKind::Flag => match value.as_bool() {
                Some(true) => "yes".to_owned(),
                Some(false) => "no".to_owned(),
                None => plain(value),
            },
            ParamKind::Count | ParamKind::Number => plain(value),
        }
    }
}

/// The checks on a parameter's own fields.
fn check_param(param: &Param, at: &str, out: &mut Problems) {
    if param.label.trim().is_empty() {
        out.push(at, "has no label".to_owned());
    }
    let numeric = param.kind.numeric();
    if !numeric {
        for (field, set) in [
            ("unit", param.unit.is_some()),
            ("min", param.min.is_some()),
            ("max", param.max.is_some()),
        ] {
            if set {
                out.push(
                    at,
                    format!(
                        "\"{field}\" is only for count and number, not {}",
                        param.kind.as_str()
                    ),
                );
            }
        }
    }
    if let Some(scale) = param.scale {
        if param.kind != ParamKind::Number {
            out.push(at, "\"scale\" is only for number".to_owned());
        } else if scale.is_nan() || scale <= 0.0 {
            out.push(at, "\"scale\" must be more than 0".to_owned());
        }
    }
    if let (Some(min), Some(max)) = (param.min, param.max)
        && min > max
    {
        out.push(at, format!("min {min} is more than max {max}"));
    }
    match param.kind {
        ParamKind::Choice | ParamKind::Choices if param.options.is_empty() => {
            out.push(at, "a choice needs \"options\"".to_owned());
        }
        ParamKind::Flag | ParamKind::Patterns if !param.options.is_empty() => {
            out.push(
                at,
                format!("\"options\" are not for {}", param.kind.as_str()),
            );
        }
        _ => {}
    }
    // Options: all text or all numbers; each value and alias once; in range for suggestions.
    let mut seen: Vec<&Value> = Vec::new();
    let text = param.options.first().is_some_and(|o| o.value.is_string());
    for option in &param.options {
        if option.label.trim().is_empty() {
            out.push(at, format!("option {} has no label", option.value));
        }
        for value in std::iter::once(&option.value).chain(&option.aliases) {
            let ok_kind = if numeric {
                value.is_number()
            } else {
                value.is_string() == text && (value.is_string() || value.is_number())
            };
            if !ok_kind {
                out.push(
                    at,
                    format!("option value {value} isn't the same kind as the others"),
                );
            }
            if seen.iter().any(|s| same_value(s, value)) {
                out.push(at, format!("option value {value} is listed more than once"));
            }
            seen.push(value);
        }
        // A suggestion only has to be the right kind and in range.
        if numeric && let Err(why) = param.check_value(&option.value, true) {
            out.push(at, format!("suggested option: {why}"));
        }
    }
}

/// The `{placeholder}` names in `text`, in order, or why the braces don't balance. `{{` and `}}`
/// are literal braces.
pub fn placeholders(text: &str) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
            }
            '{' => {
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(c) => name.push(c),
                        None => return Err("a \"{\" has no \"}\"".to_owned()),
                    }
                }
                if !is_word(&name) {
                    return Err(format!(
                        "{{{name}}} isn't a placeholder: use lowercase words"
                    ));
                }
                names.push(name);
            }
            '}' => return Err("a \"}\" has no \"{\"; write \"}}\" for a brace".to_owned()),
            _ => {}
        }
    }
    Ok(names)
}

/// `template` with each `{name}` replaced by `value(name)`. A name `value` doesn't know is left as
/// it is; `{{` and `}}` become braces.
pub fn fill(template: &str, value: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                out.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                out.push('}');
            }
            '{' => {
                let mut name = String::new();
                let mut closed = false;
                for c in chars.by_ref() {
                    if c == '}' {
                        closed = true;
                        break;
                    }
                    name.push(c);
                }
                match (closed, value(&name)) {
                    (true, Some(text)) => out.push_str(&text),
                    (true, None) => {
                        out.push('{');
                        out.push_str(&name);
                        out.push('}');
                    }
                    (false, _) => {
                        out.push('{');
                        out.push_str(&name);
                    }
                }
            }
            c => out.push(c),
        }
    }
    out
}

/// A value as text: strings without quotes, numbers without a trailing `.0`.
fn plain(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(n) => {
            let text = n.to_string();
            text.strip_suffix(".0").map(str::to_owned).unwrap_or(text)
        }
        other => other.to_string(),
    }
}

fn numbers_equal(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(a), Some(b)) => a == b,
        _ => a == b,
    }
}

fn same_value(a: &Value, b: &Value) -> bool {
    match (a.as_str(), b.as_str()) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        _ => numbers_equal(a, b),
    }
}

/// A lowercase word: `[a-z0-9_]+`, starting with a letter.
fn is_word(text: &str) -> bool {
    text.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && text
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Lowercase words joined by dots, at least two.
fn is_rule_id(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    parts.len() >= 2 && parts.iter().all(|part| is_word(part))
}

/// `UvMissing`: an ASCII letter first, uppercase, then letters and digits.
fn is_code_name(text: &str) -> bool {
    text.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && text.chars().all(|c| c.is_ascii_alphanumeric())
}

#[derive(Default)]
struct Problems(Vec<CatalogProblem>);

impl Problems {
    fn push(&mut self, at: &str, message: String) {
        self.0.push(CatalogProblem {
            at: at.to_owned(),
            message,
        });
    }
}

const TOP_FIELDS: &[&str] = &["catalog", "tool", "version", "bases", "category", "rule"];
const CATEGORY_FIELDS: &[&str] = &["id", "title"];
const RULE_FIELDS: &[&str] = &[
    "id", "name", "category", "since", "title", "what", "why", "how", "measures", "default",
    "base", "param", "case", "app",
];
const DEFAULT_FIELDS: &[&str] = &["enabled", "severity"];
const BASE_FIELDS: &[&str] = &[
    "enabled", "severity", "params", "title", "what", "why", "how",
];
const PARAM_FIELDS: &[&str] = &[
    "key", "kind", "unit", "scale", "min", "max", "options", "default", "label", "help",
];
const OPTION_FIELDS: &[&str] = &["value", "label", "aliases"];
const TEXT_FIELDS: &[&str] = &["title", "what", "why", "how"];

/// Every field in a catalog this crate doesn't read: (where, key).
fn unknown_fields(root: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let scan = |object: &Map<String, Value>, at: &str, known: &[&str], out: &mut Vec<_>| {
        for key in object.keys() {
            if !known.contains(&key.as_str()) {
                out.push((at.to_owned(), key.clone()));
            }
        }
    };
    let Some(root) = root.as_object() else {
        return out;
    };
    scan(root, "top level", TOP_FIELDS, &mut out);
    let each = |key: &str| -> Vec<&Map<String, Value>> {
        root.get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_object)
            .collect()
    };
    for category in each("category") {
        let at = format!("category \"{}\"", text_of(category, "id"));
        scan(category, &at, CATEGORY_FIELDS, &mut out);
    }
    for rule in each("rule") {
        let at = format!("rule \"{}\"", text_of(rule, "id"));
        scan(rule, &at, RULE_FIELDS, &mut out);
        if let Some(default) = rule.get("default").and_then(Value::as_object) {
            scan(default, &format!("{at} default"), DEFAULT_FIELDS, &mut out);
        }
        for (name, over) in objects_in(rule, "base") {
            scan(
                over,
                &format!("{at} base \"{name}\""),
                BASE_FIELDS,
                &mut out,
            );
        }
        for (name, case) in objects_in(rule, "case") {
            scan(
                case,
                &format!("{at} case \"{name}\""),
                TEXT_FIELDS,
                &mut out,
            );
        }
        for param in rule
            .get("param")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_object)
        {
            let at = format!("{at} param \"{}\"", text_of(param, "key"));
            scan(param, &at, PARAM_FIELDS, &mut out);
            for option in param
                .get("options")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_object)
            {
                scan(option, &format!("{at} option"), OPTION_FIELDS, &mut out);
            }
        }
    }
    out
}

fn text_of<'a>(object: &'a Map<String, Value>, key: &str) -> &'a str {
    object.get(key).and_then(Value::as_str).unwrap_or("?")
}

/// The object-valued entries of the table `object[key]`.
fn objects_in<'a>(
    object: &'a Map<String, Value>,
    key: &str,
) -> impl Iterator<Item = (&'a String, &'a Map<String, Value>)> {
    object
        .get(key)
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(name, value)| value.as_object().map(|v| (name, v)))
}
