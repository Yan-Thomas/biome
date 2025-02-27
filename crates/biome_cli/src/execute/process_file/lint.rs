use crate::execute::diagnostics::ResultExt;
use crate::execute::process_file::workspace_file::WorkspaceFile;
use crate::execute::process_file::{FileResult, FileStatus, Message, SharedTraversalOptions};
use biome_diagnostics::{category, Error};
use biome_service::file_handlers::{AstroFileHandler, VueFileHandler};
use biome_service::workspace::RuleCategories;
use std::path::Path;
use std::sync::atomic::Ordering;

/// Lints a single file and returns a [FileResult]
pub(crate) fn lint<'ctx>(ctx: &'ctx SharedTraversalOptions<'ctx, '_>, path: &Path) -> FileResult {
    let mut workspace_file = WorkspaceFile::new(ctx, path)?;
    lint_with_guard(ctx, &mut workspace_file)
}

pub(crate) fn lint_with_guard<'ctx>(
    ctx: &'ctx SharedTraversalOptions<'ctx, '_>,
    workspace_file: &mut WorkspaceFile,
) -> FileResult {
    tracing::info_span!("Processes linting", path =? workspace_file.path.display()).in_scope(
        move || {
            let mut errors = 0;
            let mut input = workspace_file.input()?;

            if let Some(fix_mode) = ctx.execution.as_fix_file_mode() {
                let fix_result = workspace_file
                    .guard()
                    .fix_file(*fix_mode, false)
                    .with_file_path_and_code(
                        workspace_file.path.display().to_string(),
                        category!("lint"),
                    )?;

                ctx.push_message(Message::SkippedFixes {
                    skipped_suggested_fixes: fix_result.skipped_suggested_fixes,
                });

                let mut output = fix_result.code;
                if output != input {
                    if workspace_file.as_extension() == Some("astro") {
                        if output.is_empty() {
                            return Ok(FileStatus::Ignored);
                        }
                        output = AstroFileHandler::astro_output(input.as_str(), output.as_str());
                    }

                    if workspace_file.as_extension() == Some("vue") {
                        if output.is_empty() {
                            return Ok(FileStatus::Ignored);
                        }
                        output = VueFileHandler::vue_output(input.as_str(), output.as_str());
                    }

                    workspace_file.update_file(output)?;
                    input = workspace_file.input()?;
                }
                errors = fix_result.errors;
            }

            let max_diagnostics = ctx.remaining_diagnostics.load(Ordering::Relaxed);
            let pull_diagnostics_result = workspace_file
                .guard()
                .pull_diagnostics(
                    RuleCategories::LINT | RuleCategories::SYNTAX,
                    max_diagnostics.into(),
                )
                .with_file_path_and_code(
                    workspace_file.path.display().to_string(),
                    category!("lint"),
                )?;

            let no_diagnostics = pull_diagnostics_result.diagnostics.is_empty()
                && pull_diagnostics_result.skipped_diagnostics == 0;
            errors += pull_diagnostics_result.errors;

            if !no_diagnostics {
                let input = match workspace_file.as_extension() {
                    Some("astro") => AstroFileHandler::astro_input(input.as_str()).to_string(),
                    Some("vue") => VueFileHandler::vue_input(input.as_str()).to_string(),
                    _ => input,
                };

                ctx.push_message(Message::Diagnostics {
                    name: workspace_file.path.display().to_string(),
                    content: input,
                    diagnostics: pull_diagnostics_result
                        .diagnostics
                        .into_iter()
                        .map(Error::from)
                        .collect(),
                    skipped_diagnostics: pull_diagnostics_result.skipped_diagnostics,
                });
            }

            if errors > 0 {
                Ok(FileStatus::Message(Message::Failure))
            } else {
                Ok(FileStatus::Success)
            }
        },
    )
}
