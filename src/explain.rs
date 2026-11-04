//! `rules explain <file>`: which rules apply to a file, and why, for the technical artist.
//!
//! Built from the same matching as [`resolve`](crate::resolve), so it can never give another
//! answer.

use std::path::Path;

use crate::resolve::{FilePath, branch_rule, file_type, names_match, paths_match, stage_of};
use crate::{ANY_STAGE, Context, Mode, Pattern, Profile, Rules, StageFrom};

/// A plain-text account of how [`resolve`](crate::resolve) answers for `path`: its type, the
/// profiles tried and why each did or didn't match, the stage and where it came from, and the
/// rules file. `path` is taken as `resolve` takes it.
pub fn explain(rules: &Rules, ctx: &Context, path: &Path) -> String {
    let file = FilePath::new(&ctx.repo_root, path);
    let mut out = Vec::new();
    out.push(match &file.relative {
        Some(relative) => relative.clone(),
        None => format!("{} (outside the repository)", path.display()),
    });
    out.push(String::new());

    let Some(kind) = file_type(rules, &file) else {
        ignored(rules, &file, &mut out);
        return finish(out);
    };
    let tool = &rules.types()[kind].tool;
    out.push(format!("Type: {kind}"));
    out.push(format!(
        "    Files with the extension \"{}\" are of type \"{kind}\", which {tool} checks.",
        file.extension.as_deref().unwrap_or_default()
    ));
    out.push(String::new());

    let Some(profile) = profiles(rules, kind, &file, &mut out) else {
        let severity = match rules.mode() {
            Mode::Advise => "a warning, since the rules are in advise mode",
            Mode::Enforce => "an error, since the rules are in enforce mode",
        };
        out.push(format!(
            "    {tool} reports this as file.no_profile: {severity}."
        ));
        return finish(out);
    };
    out.push(String::new());

    let (stage, from) = stage_of(rules, ctx, profile);
    stage_section(rules, ctx, profile, stage, from, &mut out);
    out.push(String::new());
    rules_file_section(rules, profile, stage, &mut out);
    finish(out)
}

fn finish(mut out: Vec<String>) -> String {
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    let mut text = out.join("\n");
    text.push('\n');
    text
}

fn ignored(rules: &Rules, file: &FilePath, out: &mut Vec<String>) {
    out.push(match &file.extension {
        Some(extension) => format!("Ignored: no type has the extension \"{extension}\"."),
        None => "Ignored: the file has no extension.".to_owned(),
    });
    let types: Vec<String> = rules
        .types()
        .iter()
        .map(|(name, file_type)| format!("{name} ({})", file_type.extensions.join(", ")))
        .collect();
    if types.is_empty() {
        out.push("    There are no types.".to_owned());
    } else {
        out.push(format!("    Types: {}.", types.join("; ")));
    }
}

/// Writes the profile section and returns the profile that matched, if any.
fn profiles<'a>(
    rules: &'a Rules,
    kind: &str,
    file: &FilePath,
    out: &mut Vec<String>,
) -> Option<&'a Profile> {
    let candidates: Vec<(usize, &Profile)> = rules
        .profiles()
        .iter()
        .enumerate()
        .filter(|(_, profile)| profile.kind == kind)
        .collect();
    let matched = candidates
        .iter()
        .find(|(_, profile)| names_match(profile, file) && paths_match(profile, file));
    out.push(match matched {
        Some((_, profile)) => format!("Profile: {} ({})", profile.label, profile.id),
        None => "Profile: none matches".to_owned(),
    });
    if candidates.is_empty() {
        out.push(format!("    There are no {kind} profiles."));
        return None;
    }
    out.push(format!(
        "    The {kind} profiles are tried in file order, and the first that matches wins:"
    ));
    for (index, profile) in &candidates {
        let reason = reason(profile, file);
        out.push(format!(
            "    - profiles[{index}] \"{}\": {reason}",
            profile.id
        ));
        if matched.is_some_and(|(at, _)| at == index) {
            break;
        }
    }
    matched.map(|(_, profile)| *profile)
}

/// Why `profile` does or doesn't match `file`.
fn reason(profile: &Profile, file: &FilePath) -> String {
    let Some(name) = first_match(&profile.names, &file.stem) else {
        return format!(
            "no; the name \"{}\" matches none of {}.",
            file.stem,
            patterns(&profile.names)
        );
    };
    let name = format!("the name matches {}", name.as_str());
    let Some(paths) = &profile.paths else {
        return format!("yes; {name}, and the profile has no paths.");
    };
    match &file.relative {
        None => format!(
            "no; {name}, but the file is outside the repository, so it can't match the paths {}.",
            patterns(paths)
        ),
        Some(relative) => match first_match(paths, relative) {
            Some(path) => format!("yes; {name}, and the path matches {}.", path.as_str()),
            None => format!(
                "no; {name}, but the path matches none of {}.",
                patterns(paths)
            ),
        },
    }
}

fn first_match<'a>(patterns: &'a [Pattern], text: &str) -> Option<&'a Pattern> {
    patterns.iter().find(|pattern| pattern.is_match(text))
}

/// `SK_*, CH_*`, marking any that isn't a valid glob.
fn patterns(patterns: &[Pattern]) -> String {
    if patterns.is_empty() {
        return "(none)".to_owned();
    }
    let texts: Vec<String> = patterns
        .iter()
        .map(|pattern| match pattern.error() {
            Some(_) => format!("{} (not a valid glob)", pattern.as_str()),
            None => pattern.as_str().to_owned(),
        })
        .collect();
    texts.join(", ")
}

fn stage_section(
    rules: &Rules,
    ctx: &Context,
    profile: &Profile,
    stage: &str,
    from: StageFrom,
    out: &mut Vec<String>,
) {
    let source = match from {
        StageFrom::Asset => "the asset",
        StageFrom::Branch => "the branch",
        StageFrom::Profile => "the profile",
        StageFrom::Project => "the project",
    };
    out.push(format!("Stage: {stage}, from {source}"));

    let branch = match (&ctx.branch, branch_rule(rules, ctx)) {
        (_, Some((index, rule))) => {
            out.push(format!(
                "    The branch \"{}\" matches branches[{index}] ({}). The branch outranks the \
                 profile and the project.",
                ctx.branch.as_deref().unwrap_or_default(),
                rule.pattern.as_str()
            ));
            return;
        }
        (None, None) => "HEAD is detached, so no branch rule applies.".to_owned(),
        (Some(branch), None) if rules.branches().is_empty() => {
            format!("The branch is \"{branch}\", but there are no branch rules.")
        }
        (Some(branch), None) => {
            let tried: Vec<&str> = rules
                .branches()
                .iter()
                .map(|rule| rule.pattern.as_str())
                .collect();
            format!(
                "The branch \"{branch}\" matches no branch rule ({}).",
                tried.join(", ")
            )
        }
    };
    out.push(match &profile.stage {
        Some(own) => format!("    {branch} The profile sets its own stage, {own}."),
        None => format!(
            "    {branch} The profile sets no stage, so the project's stage, {}, applies.",
            rules.stage()
        ),
    });
}

fn rules_file_section(rules: &Rules, profile: &Profile, stage: &str, out: &mut Vec<String>) {
    let (file, why) = if let Some(file) = profile.rules.get(stage) {
        (file, format!("It is the profile's file for \"{stage}\"."))
    } else if let Some(file) = profile.rules.get(ANY_STAGE) {
        (
            file,
            format!(
                "The profile has no file for \"{stage}\", so its \"{ANY_STAGE}\" file applies."
            ),
        )
    } else {
        out.push("Rules file: none".to_owned());
        out.push(format!(
            "    The profile has no file for \"{stage}\" and no \"{ANY_STAGE}\" file, so the \
             file can't be checked. rules check reports this."
        ));
        return;
    };
    out.push(format!("Rules file: {file}"));
    out.push(format!("    {why}"));
    let path = rules.file_path(file);
    if !path.is_file() {
        out.push(format!(
            "    It doesn't exist: looked for {}.",
            path.display()
        ));
    }
}
