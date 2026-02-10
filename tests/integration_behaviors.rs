use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use shephard::apply;
use shephard::cli::{ApplyArgs, ApplyMethodArg};
use shephard::config::{
    FailurePolicy, ResolvedConfig, ResolvedRunConfig, RunMode, SideChannelConfig, TuiConfig,
};
use shephard::{discovery, workflow};

const SIDE_REMOTE_NAME: &str = "shephard";
const SIDE_BRANCH_NAME: &str = "shephard/sync";

#[test]
fn discovers_nested_repositories() {
    let workspace = temp_workspace();
    let root = workspace.path();

    let repo_a = root.join("a");
    let repo_b = root.join("nested").join("b");
    init_repo(&repo_a);
    init_repo(&repo_b);

    let repos = discovery::discover_repositories(&[root.to_path_buf()], false)
        .expect("discovery should work");
    let paths: Vec<PathBuf> = repos.into_iter().map(|r| r.path).collect();

    assert!(paths.contains(&repo_a.canonicalize().expect("canonical a")));
    assert!(paths.contains(&repo_b.canonicalize().expect("canonical b")));
}

#[test]
fn workflow_pull_only_success() {
    let workspace = temp_workspace();
    let (_, repo) = setup_origin_and_clone(workspace.path(), "pull-only-ok");

    let cfg = run_config(false, false, false, SIDE_REMOTE_NAME, SIDE_BRANCH_NAME);
    let results = workflow::run(std::slice::from_ref(&repo), &cfg);

    assert_eq!(results.len(), 1);
    assert!(matches!(results[0].status, workflow::RepoStatus::Success));
    assert!(results[0].message.contains("pull ok"));
}

#[test]
fn workflow_pull_ff_only_fails_when_local_tree_is_dirty() {
    let workspace = temp_workspace();
    let (origin, repo) = setup_origin_and_clone(workspace.path(), "pull-ff-fails");
    let peer = clone_repo(workspace.path(), &origin, "pull-ff-fails-peer");

    write_file(&repo, "tracked.txt", "local dirty change\n");

    write_file(&peer, "tracked.txt", "remote update\n");
    commit_all(&peer, "remote update");
    git(&peer, &["push"]);

    let cfg = run_config(false, false, false, SIDE_REMOTE_NAME, SIDE_BRANCH_NAME);
    let results = workflow::run(std::slice::from_ref(&repo), &cfg);

    assert_eq!(results.len(), 1);
    assert!(matches!(results[0].status, workflow::RepoStatus::Failed));
    assert!(results[0].message.contains("pull failed"));
}

#[test]
fn workflow_push_tracked_only_excludes_untracked_files() {
    let workspace = temp_workspace();
    let (_, repo) = setup_origin_and_clone(workspace.path(), "tracked-only");

    write_file(&repo, "tracked.txt", "tracked update\n");
    write_file(&repo, "new.txt", "should stay untracked\n");

    let cfg = run_config(true, false, false, SIDE_REMOTE_NAME, SIDE_BRANCH_NAME);
    let results = workflow::run(std::slice::from_ref(&repo), &cfg);

    assert!(matches!(results[0].status, workflow::RepoStatus::Success));
    let status = git(&repo, &["status", "--porcelain"]);
    assert!(status.contains("?? new.txt"));

    let tree = git(&repo, &["ls-tree", "--name-only", "HEAD"]);
    assert!(!tree.lines().any(|line| line == "new.txt"));
}

#[test]
fn workflow_push_include_untracked_adds_new_files() {
    let workspace = temp_workspace();
    let (_, repo) = setup_origin_and_clone(workspace.path(), "include-untracked");

    write_file(&repo, "tracked.txt", "tracked update\n");
    write_file(&repo, "new.txt", "include me\n");

    let cfg = run_config(true, true, false, SIDE_REMOTE_NAME, SIDE_BRANCH_NAME);
    let results = workflow::run(std::slice::from_ref(&repo), &cfg);

    assert!(matches!(results[0].status, workflow::RepoStatus::Success));
    let status = git(&repo, &["status", "--porcelain"]);
    assert!(!status.contains("?? new.txt"));

    let tree = git(&repo, &["ls-tree", "--name-only", "HEAD"]);
    assert!(tree.lines().any(|line| line == "new.txt"));
}

#[test]
fn workflow_push_with_no_local_changes_is_noop() {
    let workspace = temp_workspace();
    let (_, repo) = setup_origin_and_clone(workspace.path(), "noop");

    let cfg = run_config(true, false, false, SIDE_REMOTE_NAME, SIDE_BRANCH_NAME);
    let results = workflow::run(std::slice::from_ref(&repo), &cfg);

    assert!(matches!(results[0].status, workflow::RepoStatus::NoOp));
    assert!(results[0].message.contains("no local changes"));
}

#[test]
fn workflow_continues_after_repo_failure() {
    let workspace = temp_workspace();

    let (origin_fail, fail_repo) = setup_origin_and_clone(workspace.path(), "continue-fail");
    let fail_peer = clone_repo(workspace.path(), &origin_fail, "continue-fail-peer");

    write_file(&fail_repo, "tracked.txt", "dirty local\n");
    write_file(&fail_peer, "tracked.txt", "remote changed\n");
    commit_all(&fail_peer, "advance remote");
    git(&fail_peer, &["push"]);

    let (_, ok_repo) = setup_origin_and_clone(workspace.path(), "continue-ok");

    let cfg = run_config(false, false, false, SIDE_REMOTE_NAME, SIDE_BRANCH_NAME);
    let results = workflow::run(&[fail_repo, ok_repo], &cfg);

    assert_eq!(results.len(), 2);
    assert!(matches!(results[0].status, workflow::RepoStatus::Failed));
    assert!(matches!(results[1].status, workflow::RepoStatus::Success));
}

#[test]
fn workflow_side_channel_missing_remote_fails_with_hint() {
    let workspace = temp_workspace();
    let (_, repo) = setup_origin_and_clone(workspace.path(), "missing-side-remote");

    write_file(&repo, "tracked.txt", "local changes\n");

    let cfg = run_config(true, false, true, SIDE_REMOTE_NAME, SIDE_BRANCH_NAME);
    let results = workflow::run(std::slice::from_ref(&repo), &cfg);

    assert!(matches!(results[0].status, workflow::RepoStatus::Failed));
    assert!(results[0].message.contains("missing side-channel remote"));
}

#[test]
fn workflow_side_channel_pushes_without_local_branch_commit() {
    let workspace = temp_workspace();
    let (_, repo) = setup_origin_and_clone(workspace.path(), "side-no-pollute");
    let side_remote = create_bare_remote(workspace.path(), "side-no-pollute");

    add_remote(&repo, SIDE_REMOTE_NAME, &side_remote);
    seed_side_branch_from_head(&repo);

    let head_before = rev_parse_head(&repo);
    write_file(&repo, "tracked.txt", "unsaved local work\n");

    let cfg = run_config(true, false, true, SIDE_REMOTE_NAME, SIDE_BRANCH_NAME);
    let results = workflow::run(std::slice::from_ref(&repo), &cfg);

    assert!(matches!(results[0].status, workflow::RepoStatus::Success));

    let head_after = rev_parse_head(&repo);
    assert_eq!(head_before, head_after);

    let status = git(&repo, &["status", "--porcelain"]);
    assert!(!status.trim().is_empty());
    assert!(status.contains("tracked.txt"));

    let remote_heads = git(
        workspace.path(),
        &[
            "ls-remote",
            "--heads",
            &path_str(&side_remote),
            SIDE_BRANCH_NAME,
        ],
    );
    assert!(!remote_heads.trim().is_empty());
}

#[test]
fn apply_merge_cherry_pick_and_squash_behaviors() {
    let workspace = temp_workspace();
    let (origin, dev_repo) = setup_origin_and_clone(workspace.path(), "apply-all");
    let side_remote = create_bare_remote(workspace.path(), "apply-all-side");

    add_remote(&dev_repo, SIDE_REMOTE_NAME, &side_remote);
    seed_side_branch_from_head(&dev_repo);

    write_file(&dev_repo, "tracked.txt", "side branch content\n");
    let cfg = run_config(true, false, true, SIDE_REMOTE_NAME, SIDE_BRANCH_NAME);
    let side_results = workflow::run(std::slice::from_ref(&dev_repo), &cfg);
    assert!(matches!(
        side_results[0].status,
        workflow::RepoStatus::Success
    ));

    let apply_cfg = resolved_apply_config(SIDE_REMOTE_NAME, SIDE_BRANCH_NAME);

    let merge_clone = clone_repo(workspace.path(), &origin, "apply-merge-clone");
    add_remote(&merge_clone, SIDE_REMOTE_NAME, &side_remote);
    let merge_head_before = rev_parse_head(&merge_clone);
    apply::run(
        &ApplyArgs {
            repo: Some(merge_clone.clone()),
            method: ApplyMethodArg::Merge,
        },
        &apply_cfg,
    )
    .expect("merge apply should succeed");
    let merge_head_after = rev_parse_head(&merge_clone);
    assert_ne!(merge_head_before, merge_head_after);
    assert_eq!(
        read_file(&merge_clone, "tracked.txt"),
        "side branch content\n"
    );

    let cherry_clone = clone_repo(workspace.path(), &origin, "apply-cherry-clone");
    add_remote(&cherry_clone, SIDE_REMOTE_NAME, &side_remote);
    apply::run(
        &ApplyArgs {
            repo: Some(cherry_clone.clone()),
            method: ApplyMethodArg::CherryPick,
        },
        &apply_cfg,
    )
    .expect("cherry-pick apply should succeed");
    assert_eq!(
        read_file(&cherry_clone, "tracked.txt"),
        "side branch content\n"
    );

    let squash_clone = clone_repo(workspace.path(), &origin, "apply-squash-clone");
    add_remote(&squash_clone, SIDE_REMOTE_NAME, &side_remote);
    let squash_head_before = rev_parse_head(&squash_clone);
    apply::run(
        &ApplyArgs {
            repo: Some(squash_clone.clone()),
            method: ApplyMethodArg::Squash,
        },
        &apply_cfg,
    )
    .expect("squash apply should succeed");
    let squash_head_after = rev_parse_head(&squash_clone);
    assert_eq!(squash_head_before, squash_head_after);
    let squash_status = git(&squash_clone, &["status", "--porcelain"]);
    assert!(squash_status.contains("M  tracked.txt"));
}

fn temp_workspace() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("shephard-int-")
        .tempdir_in("/tmp")
        .expect("failed to create temp dir in /tmp")
}

fn setup_origin_and_clone(root: &Path, name: &str) -> (PathBuf, PathBuf) {
    let seed = root.join(format!("{name}-seed"));
    init_repo(&seed);
    write_file(&seed, "tracked.txt", "initial\n");
    commit_all(&seed, "initial commit");

    let origin = root.join(format!("{name}-origin.git"));
    git(root, &["init", "--bare", &path_str(&origin)]);

    git(&seed, &["remote", "add", "origin", &path_str(&origin)]);
    git(&seed, &["push", "-u", "origin", "main"]);

    let clone = clone_repo(root, &origin, &format!("{name}-clone"));
    (origin, clone)
}

fn clone_repo(root: &Path, origin: &Path, name: &str) -> PathBuf {
    let path = root.join(name);
    git(
        root,
        &[
            "clone",
            "--branch",
            "main",
            &path_str(origin),
            &path_str(&path),
        ],
    );
    configure_user(&path);
    path
}

fn create_bare_remote(root: &Path, name: &str) -> PathBuf {
    let path = root.join(format!("{name}.git"));
    git(root, &["init", "--bare", &path_str(&path)]);
    path
}

fn add_remote(repo: &Path, name: &str, remote_path: &Path) {
    git(repo, &["remote", "add", name, &path_str(remote_path)]);
}

fn seed_side_branch_from_head(repo: &Path) {
    git(
        repo,
        &[
            "push",
            SIDE_REMOTE_NAME,
            &format!("HEAD:{SIDE_BRANCH_NAME}"),
        ],
    );
}

fn init_repo(path: &Path) {
    fs::create_dir_all(path).expect("failed to create repo directory");
    git(path, &["init", "-b", "main"]);
    configure_user(path);
}

fn configure_user(repo: &Path) {
    git(repo, &["config", "user.name", "Shephard Test User"]);
    git(repo, &["config", "user.email", "shephard-test@example.com"]);
    git(repo, &["config", "commit.gpgsign", "false"]);
}

fn commit_all(repo: &Path, message: &str) {
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-m", message]);
}

fn rev_parse_head(repo: &Path) -> String {
    git(repo, &["rev-parse", "HEAD"])
}

fn read_file(repo: &Path, relative: &str) -> String {
    fs::read_to_string(repo.join(relative)).expect("failed to read file")
}

fn write_file(repo: &Path, relative: &str, content: &str) {
    let path = repo.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("failed to create file parent directory");
    }
    fs::write(path, content).expect("failed to write file");
}

fn run_config(
    push_enabled: bool,
    include_untracked: bool,
    side_channel_enabled: bool,
    remote_name: &str,
    branch_name: &str,
) -> ResolvedRunConfig {
    ResolvedRunConfig {
        workspace_roots: Vec::new(),
        descend_hidden_dirs: false,
        push_enabled,
        include_untracked,
        side_channel: SideChannelConfig {
            enabled: side_channel_enabled,
            remote_name: remote_name.to_string(),
            branch_name: branch_name.to_string(),
        },
        commit_template: "shephard sync: {timestamp} {hostname} [{scope}]".to_string(),
        failure_policy: FailurePolicy::Continue,
    }
}

fn resolved_apply_config(remote_name: &str, branch_name: &str) -> ResolvedConfig {
    ResolvedConfig {
        workspace_roots: Vec::new(),
        descend_hidden_dirs: false,
        default_mode: RunMode::SyncAll,
        push_enabled: true,
        include_untracked: false,
        side_channel: SideChannelConfig {
            enabled: true,
            remote_name: remote_name.to_string(),
            branch_name: branch_name.to_string(),
        },
        commit_template: "shephard sync: {timestamp} {hostname} [{scope}]".to_string(),
        failure_policy: FailurePolicy::Continue,
        tui: TuiConfig {
            persist_selection: true,
        },
    }
}

fn path_str(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn git(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|err| panic!("failed to run git {:?} in {}: {err}", args, cwd.display()));

    if !output.status.success() {
        panic!(
            "git {:?} failed in {}:\nstdout: {}\nstderr: {}",
            args,
            cwd.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    String::from_utf8_lossy(&output.stdout).trim().to_string()
}
