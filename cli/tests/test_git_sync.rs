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

use indoc::indoc;
use testutils::git;

use crate::common::CommandOutput;
use crate::common::TestEnvironment;
use crate::common::TestWorkDir;

fn add_commit_to_branch(git_repo: &gix::Repository, branch: &str, message: &str) -> gix::ObjectId {
    let parents = git_repo
        .find_reference(&format!("refs/heads/{branch}"))
        .ok()
        .and_then(|mut r| r.peel_to_commit().ok())
        .map(|c| vec![c.id().detach()])
        .unwrap_or_default();
    git::add_commit(
        git_repo,
        &format!("refs/heads/{branch}"),
        branch,            // file name
        branch.as_bytes(), // content
        message,
        &parents,
    )
    .commit_id
}

/// Adds a remote with a bookmark of the same name (initial commit "message").
fn add_git_remote(
    test_env: &TestEnvironment,
    work_dir: &TestWorkDir,
    remote: &str,
) -> gix::Repository {
    let git_repo = git::init(test_env.env_root().join(remote));
    add_commit_to_branch(&git_repo, remote, "message");
    work_dir
        .run_jj(["git", "remote", "add", remote, &format!("../{remote}")])
        .success();
    git_repo
}

#[must_use]
fn get_log_output(work_dir: &TestWorkDir) -> CommandOutput {
    let template = indoc! {r#"
        separate(" ", description.first_line(), bookmarks) ++ "\n"
    "#};
    work_dir.run_jj(["log", "-T", template, "-r", "all()"])
}

/// `jj git sync` fetches and then rebases local work onto the bookmark's new
/// head, in a single operation.
#[test]
fn test_git_sync_rebases_onto_advanced_bookmark() {
    let test_env = TestEnvironment::default();
    test_env.add_config("remotes.origin.auto-track-bookmarks = '*'");
    test_env.run_jj_in(".", ["git", "init", "repo"]).success();
    let work_dir = test_env.work_dir("repo");
    let remote_repo = add_git_remote(&test_env, &work_dir, "origin");
    work_dir.run_jj(["git", "fetch"]).success();

    // A local change on top of the fetched bookmark.
    work_dir
        .run_jj(["new", "origin", "-m", "local change"])
        .success();

    // The remote bookmark advances.
    add_commit_to_branch(&remote_repo, "origin", "upstream advance");

    // Sync: fetch the advance and rebase the local change onto it.
    let output = work_dir.run_jj(["git", "sync"]);
    insta::assert_snapshot!(output, @r"
    ------- stderr -------
    bookmark: origin@origin [updated] tracked
    Rebased 1 commit(s) onto bookmark origin
    Working copy  (@) now at: zsuskuln 033ce353 (empty) local change
    Parent commit (@-)      : qrrkpxyu afd3ed92 origin | (empty) upstream advance
    [EOF]
    ");

    // The local change now sits on top of the upstream advance.
    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  local change
    ○  upstream advance origin
    ○  message
    ◆
    [EOF]
    ");
}
