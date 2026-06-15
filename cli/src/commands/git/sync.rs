// Copyright 2025 The Jujutsu Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::io::Write as _;

use itertools::Itertools as _;
use jj_lib::git;
use jj_lib::git::GitSyncOptions;
use jj_lib::ref_name::RemoteName;
use jj_lib::str_util::StringPattern;

use super::fetch::fetch_and_import_refs;
use super::fetch::resolve_fetch_remotes;
use crate::cli_util::CommandHelper;
use crate::command_error::CommandError;
use crate::command_error::internal_error;
use crate::command_error::user_error;
use crate::git_util::print_git_import_stats;
use crate::ui::Ui;

/// Sync local bookmarks with a remote.
///
/// Fetches from the remote(s) and then rebases your local changes onto the
/// updated bookmark heads, all in a single (undoable) operation.
#[derive(clap::Args, Clone, Debug)]
pub struct GitSyncArgs {
    /// Only rebase work belonging to these bookmarks (can be repeated)
    ///
    /// All branches are still fetched; this only narrows which local changes
    /// are rebased onto their moved heads.
    #[arg(long = "bookmark", short = 'b', value_name = "BOOKMARK")]
    bookmarks: Option<Vec<String>>,

    /// The remote to fetch from (only named remotes are supported, can be
    /// repeated)
    #[arg(long = "remote", value_name = "REMOTE")]
    remotes: Option<Vec<String>>,

    /// Fetch from all remotes
    #[arg(long, conflicts_with = "remotes")]
    all_remotes: bool,
}

pub async fn cmd_git_sync(
    ui: &mut Ui,
    command: &CommandHelper,
    args: &GitSyncArgs,
) -> Result<(), CommandError> {
    let mut workspace_command = command.workspace_helper(ui).await?;
    let remotes = resolve_fetch_remotes(
        ui,
        &workspace_command,
        args.all_remotes,
        args.remotes.as_deref(),
    )?;
    if remotes.is_empty() {
        return Err(user_error("No git remotes to sync with"));
    }
    let matching_remotes: Vec<&RemoteName> = remotes.iter().map(AsRef::as_ref).collect();

    let mut tx = workspace_command.start_transaction();

    // Record where the bookmarks point before the fetch moves them.
    let old_targets = git::snapshot_local_bookmark_targets(tx.repo());

    // Sync always fetches every branch (so an already-merged feature branch
    // doesn't linger and cause phantom rebase conflicts); the rebase scope is
    // narrowed separately by `--bookmark`.
    let import_stats =
        fetch_and_import_refs(ui, &mut tx, &matching_remotes, None, None, false).await?;
    print_git_import_stats(ui, &tx, &import_stats)?;

    let rebase_bookmarks = args.bookmarks.as_ref().map(|texts| {
        texts
            .iter()
            .map(|text| StringPattern::exact(text.clone()))
            .collect()
    });
    let options = GitSyncOptions {
        rebase_bookmarks,
        ..Default::default()
    };
    let stats = git::rebase_descendants_for_sync(tx.repo_mut(), &old_targets, &options)
        .await
        .map_err(internal_error)?;

    if let Some(mut formatter) = ui.status_formatter() {
        for bookmark in &stats.bookmarks {
            writeln!(
                formatter,
                "Rebased {} commit(s) onto bookmark {}{}",
                bookmark.rebased,
                bookmark.bookmark.as_str(),
                if bookmark.diverged { " (diverged)" } else { "" }
            )?;
        }
    }

    tx.finish(
        ui,
        format!(
            "sync with git remote(s) {}",
            matching_remotes.iter().map(|n| n.as_symbol()).join(",")
        ),
    )
    .await?;
    Ok(())
}
