#![cfg(unix)]

use crate::common::global_state::{pin_data_dir, pin_path_with_dir_first};
use crate::common::harness::{copy_plugin, copy_plugin_lib, EditorTestHarness};
use fresh::config::Config;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

struct MultiRepoFixture {
    _temp: TempDir,
    root_alias: PathBuf,
    root: PathBuf,
    git_log: PathBuf,
    bin_dir: PathBuf,
}

impl MultiRepoFixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let root_alias = temp.path().join("workspace");
        fs::create_dir_all(&root_alias).unwrap();
        let root = fs::canonicalize(&root_alias).unwrap();
        let git_log = temp.path().join("git-status.log");
        let bin_dir = temp.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        Self {
            _temp: temp,
            root_alias,
            root,
            git_log,
            bin_dir,
        }
    }

    fn init_workspace_repo(&self) {
        fs::write(self.root.join("workspace.txt"), "workspace\n").unwrap();
        run_git(&self.root, &["init"]);
        run_git(&self.root, &["add", "."]);
        run_git(&self.root, &["commit", "-m", "initial workspace"]);
    }

    fn init_nested_repo(&self, relative_root: &str, files: &[&str], modified: Option<&str>) {
        let repo_root = self.root.join(relative_root);
        fs::create_dir_all(&repo_root).unwrap();
        run_git(&repo_root, &["init"]);
        for relative_file in files {
            let path = repo_root.join(relative_file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "initial content\n").unwrap();
        }
        run_git(&repo_root, &["add", "."]);
        run_git(&repo_root, &["commit", "-m", "initial nested repository"]);
        if let Some(relative_file) = modified {
            fs::write(repo_root.join(relative_file), "modified content\n").unwrap();
        }
    }

    fn install_git_wrapper(&self) -> crate::common::global_state::PathPin {
        write_git_wrapper(&self.bin_dir, &self.git_log, &self.root_alias, &self.root);
        pin_path_with_dir_first(&self.bin_dir)
    }

    fn harness_with_git_explorer(&self) -> EditorTestHarness {
        let plugins_dir = self.root.join("plugins");
        fs::create_dir_all(&plugins_dir).unwrap();
        // Same source-plugin setup as GitTestRepo::setup_git_explorer_plugin.
        copy_plugin(&plugins_dir, "git_explorer");
        copy_plugin_lib(&plugins_dir);
        EditorTestHarness::with_config_and_working_dir(
            120,
            40,
            Config::default(),
            self.root.clone(),
        )
        .unwrap()
    }

    fn root(&self) -> PathBuf {
        self.root.clone()
    }

    fn repo_root(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    fn status_cwds(&self) -> Vec<PathBuf> {
        let contents = fs::read_to_string(&self.git_log).unwrap_or_default();
        contents
            .lines()
            .filter_map(|line| {
                let (cwd, args) = line.split_once('\t')?;
                if args != "status --porcelain -z" {
                    return None;
                }
                let path = PathBuf::from(cwd);
                Some(fs::canonicalize(&path).unwrap_or(path))
            })
            .collect()
    }

    fn wait_for_status_cwds(&self, harness: &mut EditorTestHarness, expected: &[PathBuf]) {
        let mut expected = expected.to_vec();
        expected.sort();
        let mut stable_frames = 0usize;
        let mut frames = 0usize;
        harness
            .wait_until(|h| {
                frames += 1;
                let mut actual = self.status_cwds();
                actual.sort();
                if actual == expected {
                    stable_frames += 1;
                } else {
                    stable_frames = 0;
                }
                (stable_frames >= 12 && h.screen_to_string().contains("File Explorer"))
                    || frames >= 300
            })
            .unwrap();

        let mut actual = self.status_cwds();
        actual.sort();
        assert_eq!(actual, expected, "unexpected Git status processes");
    }
}

fn run_git(cwd: &Path, args: &[&str]) {
    let output = Command::new("/usr/bin/git")
        .current_dir(cwd)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", cwd.join("nonexistent-gitconfig"))
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(["-c", "user.name=Test User"])
        .args(["-c", "user.email=test@example.com"])
        .args(["-c", "commit.gpgsign=false"])
        .args(["-c", "tag.gpgsign=false"])
        .args(args)
        .output()
        .expect("run /usr/bin/git");
    assert!(
        output.status.success(),
        "git {:?} failed in {}: {}",
        args,
        cwd.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

fn write_git_wrapper(bin_dir: &Path, log: &Path, root_alias: &Path, root: &Path) {
    let root_alias = shell_quote(root_alias);
    let root = shell_quote(root);
    let log = shell_quote(log);
    let root_patterns = format!("{root}|{root_alias}|{root}/*|{root_alias}/*");
    let wrapper = format!(
        "#!/bin/sh\ncase \"$PWD\" in\n  {root_patterns})\n    printf '%s\\t%s\\n' \"$PWD\" \"$*\" >> {log}\n    ;;\nesac\nexec /usr/bin/git \"$@\"\n"
    );
    let path = bin_dir.join("git");
    fs::write(&path, wrapper).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn git_explorer_checks_only_visible_or_expanded_nested_repositories() {
    let fixture = MultiRepoFixture::new();
    fixture.init_workspace_repo();
    fixture.init_nested_repo(
        "packages/visible",
        &["src/edited.ts"],
        Some("src/edited.ts"),
    );
    fixture.init_nested_repo("packages/hidden/repo", &["src/new.ts"], Some("src/new.ts"));
    let data_dir = fixture._temp.path().join("test-data/fresh");
    let _data_dir_pin = pin_data_dir(data_dir);
    let _path_pin = fixture.install_git_wrapper();
    let mut harness = fixture.harness_with_git_explorer();

    harness.editor_mut().focus_file_explorer();
    harness.wait_for_file_explorer().unwrap();
    let root = fixture.root();
    fixture.wait_for_status_cwds(&mut harness, std::slice::from_ref(&root));
    assert!(!fixture
        .status_cwds()
        .contains(&fixture.repo_root("packages/hidden/repo")));

    // Root is selected after opening. Its first visible directory is packages.
    harness.editor_mut().file_explorer_navigate_down();
    harness.editor_mut().file_explorer_toggle_expand();
    let visible_repo = fixture.repo_root("packages/visible");
    fixture.wait_for_status_cwds(&mut harness, &[root.clone(), visible_repo.clone()]);
    assert!(!fixture
        .status_cwds()
        .contains(&fixture.repo_root("packages/hidden/repo")));

    // Expand the visible repository row. Compact-folder expansion also opens
    // its single `src` directory, so the modified file and parent badges paint.
    harness.editor_mut().file_explorer_navigate_down();
    harness.editor_mut().file_explorer_navigate_down();
    harness.editor_mut().file_explorer_toggle_expand();
    harness
        .wait_until(|h| {
            h.screen_to_string()
                .lines()
                .any(|line| line.contains("edited.ts") && line.contains('M'))
        })
        .unwrap();
    fixture.wait_for_status_cwds(&mut harness, &[root.clone(), visible_repo.clone()]);

    let screen = harness.screen_to_string();
    let root_name = fixture.root.file_name().unwrap().to_string_lossy();
    let root_line = screen
        .lines()
        .find(|line| line.starts_with('│') && line.contains(root_name.as_ref()))
        .expect("workspace root row should be visible");
    let visible_line = screen
        .lines()
        .find(|line| line.starts_with('│') && line.contains("visible"))
        .expect("visible nested repository row should be visible");
    let file_line = screen
        .lines()
        .find(|line| line.starts_with('│') && line.contains("edited.ts"))
        .expect("modified nested file should be visible");
    assert!(root_line.contains('M'), "root badge missing: {root_line}");
    assert!(
        visible_line.contains('M'),
        "parent badge missing: {visible_line}"
    );
    assert!(file_line.contains('M'), "file badge missing: {file_line}");
}
