//! Rules from the 2026-11 review against a real vault: `drift` against a named revision and its
//! list of new folders, `drift` and `stale-summary` for the notes a change touches, `note-size`,
//! and names inside `owner/repo#number` references. The real binary and real git, temp folders.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use serde_json::Value;
use tempfile::TempDir;

/// git isolated from the user's config and hooks.
fn git(dir: &Path, args: &[&str]) -> String {
    let mut c = std::process::Command::new("git");
    c.current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com");
    for v in [
        "GIT_DIR",
        "GIT_INDEX_FILE",
        "GIT_WORK_TREE",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
    ] {
        c.env_remove(v);
    }
    let out = c.args(args).output().unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn write(dir: &Path, path: &str, body: &str) {
    let p = dir.join(path);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

fn commit(dir: &Path, message: &str) {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", message]);
}

/// A git repo holding `wikiknit.toml` (with `config`) and a vault in `v/`.
fn repo(config: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    write(
        dir.path(),
        "wikiknit.toml",
        &format!("vault = \"v\"\n{config}"),
    );
    fs::create_dir_all(dir.path().join("v")).unwrap();
    dir
}

fn bin() -> PathBuf {
    assert_cmd::cargo::cargo_bin("wikiknit")
}

fn wk(dir: &Path, args: &[&str]) -> Output {
    let mut c = std::process::Command::new(bin());
    c.current_dir(dir);
    for v in ["GIT_DIR", "GIT_INDEX_FILE", "GIT_WORK_TREE"] {
        c.env_remove(v);
    }
    c.args(args).output().unwrap()
}

/// `wikiknit --format json <args>`: the items.
fn items(dir: &Path, args: &[&str]) -> Vec<Value> {
    let mut full = vec!["--format", "json"];
    full.extend_from_slice(args);
    let out = wk(dir, &full);
    let v: Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "{e}: {} / {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    });
    v["items"].as_array().unwrap().clone()
}

/// The messages of the findings of `rule` on `note`.
fn messages(found: &[Value], rule: &str, note: &str) -> Vec<String> {
    found
        .iter()
        .filter(|f| f["rule"] == rule && f["note"] == note)
        .map(|f| f["message"].as_str().unwrap().to_string())
        .collect()
}

/// A project repository: `main` has three commits; the checkout is parked on `parked`, one
/// commit after the first that `main` does not have. Returns it and the first commit's id.
fn parked_project() -> (TempDir, String) {
    let p = TempDir::new().unwrap();
    let d = p.path();
    git(d, &["init", "-q", "-b", "main"]);
    for n in ["one", "two", "three"] {
        write(d, "f.txt", n);
        commit(d, n);
    }
    let first = git(d, &["rev-list", "--max-parents=0", "HEAD"])[..7].to_string();
    git(d, &["checkout", "-q", "-b", "parked", &first]);
    write(d, "g.txt", "side");
    commit(d, "side work");
    (p, first)
}

fn project_note(path: &str, commit: &str, extra: &str) -> String {
    format!("---\npath: {path}\nverified_commit: {commit}\n{extra}---\n# p\n[[other]]\n")
}

#[test]
fn drift_compares_with_the_revision_the_note_names() {
    let (p, first) = parked_project();
    let pp = p.path().display().to_string();
    let r = repo("[[drift]]\n");
    let d = r.path();
    write(d, "v/other.md", "# other\n[[a]] [[b]] [[c]] [[e]] [[n]]\n");
    // YAML reads this one as a number; it is still the note's revision, and there is none.
    write(
        d,
        "v/n.md",
        &project_note(&pp, &first, "drift_ref: 123456789\n"),
    );
    // No revision named: the checkout's HEAD, which is the parked branch.
    write(d, "v/a.md", &project_note(&pp, &first, ""));
    // The note names the branch the project lives on.
    write(d, "v/b.md", &project_note(&pp, &first, "drift_ref: main\n"));
    write(d, "v/c.md", &project_note(&pp, &first, "drift_ref: nope\n"));
    // Not a branch name: it never reaches git.
    write(
        d,
        "v/e.md",
        &project_note(&pp, &first, "drift_ref: \"--output=/tmp/x\"\n"),
    );
    let found = items(d, &["check"]);
    let a = &messages(&found, "drift", "a")[0];
    assert!(
        a.starts_with("1 commit(s) in ") && a.contains("(HEAD "),
        "{a}"
    );
    let b = &messages(&found, "drift", "b")[0];
    assert!(
        b.starts_with("2 commit(s) in ") && b.contains("(main "),
        "{b}"
    );
    let main = git(p.path(), &["rev-parse", "--short", "main"]);
    let fix = found
        .iter()
        .find(|f| f["rule"] == "drift" && f["note"] == "b")
        .unwrap()["fix"]
        .as_str()
        .unwrap();
    assert!(
        fix.contains(&format!("log --oneline {first}..main"))
            && fix.contains(&format!("`verified_commit: {main}`")),
        "{fix}"
    );
    for (note, rev) in [("c", "nope"), ("e", "--output=/tmp/x"), ("n", "123456789")] {
        let m = &messages(&found, "drift", note)[0];
        assert!(
            m.contains(&format!("`drift_ref` {rev} is not a branch or ref in")),
            "{m}"
        );
    }
    assert!(found.iter().all(|f| f["severity"] == "warn"), "{found:?}");
    // The note's line of the field that is wrong.
    let line = |note: &str| {
        found
            .iter()
            .find(|f| f["rule"] == "drift" && f["note"] == note)
            .unwrap()["line"]
            .clone()
    };
    assert_eq!(line("b"), 3);
    assert_eq!(line("c"), 4);
}

#[test]
fn drift_rule_names_a_revision_for_every_repository_that_has_it() {
    let (p, first) = parked_project();
    let pp = p.path().display().to_string();
    let note = project_note(&pp, &first, "");
    // The rule's revision is in the repository: it is compared with.
    let r = repo("[[drift]]\nref = \"main\"\n");
    write(r.path(), "v/other.md", "# other\n[[a]]\n");
    write(r.path(), "v/a.md", &note);
    let m = &messages(&items(r.path(), &["check"]), "drift", "a")[0];
    assert!(
        m.starts_with("2 commit(s) in ") && m.contains("(main "),
        "{m}"
    );
    // The repository has no such revision (no remote): HEAD.
    let r = repo("[[drift]]\nref = \"origin/HEAD\"\n");
    write(r.path(), "v/other.md", "# other\n[[a]]\n");
    write(r.path(), "v/a.md", &note);
    let m = &messages(&items(r.path(), &["check"]), "drift", "a")[0];
    assert!(
        m.starts_with("1 commit(s) in ") && m.contains("(HEAD "),
        "{m}"
    );
    // The note's own revision wins over the rule's.
    let r = repo("[[drift]]\nref = \"main\"\nref_field = \"branch\"\n");
    write(r.path(), "v/other.md", "# other\n[[a]]\n");
    write(
        r.path(),
        "v/a.md",
        &project_note(&pp, &first, "branch: parked\n"),
    );
    let m = &messages(&items(r.path(), &["check"]), "drift", "a")[0];
    assert!(
        m.starts_with("1 commit(s) in ") && m.contains("(parked "),
        "{m}"
    );
    // A revision in the config that is not a name does not load.
    let r = repo("[[drift]]\nref = \"main..other\"\n");
    write(r.path(), "v/a.md", "# a\n");
    let out = wk(r.path(), &["check"]);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("drift.ref"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn drift_lists_new_folders_the_note_does_not_name() {
    let p = TempDir::new().unwrap();
    let d = p.path();
    git(d, &["init", "-q", "-b", "main"]);
    write(d, "crates/core/a.rs", "a");
    commit(d, "one");
    let first = git(d, &["rev-parse", "--short", "HEAD"]);
    write(d, "crates/alpha/a.rs", "a");
    write(d, "crates/beta/b.rs", "b");
    write(d, "crates/core/deep/c.rs", "c");
    write(d, "ios/App/x.swift", "x");
    write(d, "docs/a/b/c/z.md", "z");
    write(d, "api/x", "x");
    write(d, ".github/workflows/ci.yml", "ci");
    commit(d, "two");
    let pp = d.display().to_string();
    let r = repo("[[drift]]\n");
    write(r.path(), "v/other.md", "# other\n[[a]]\n");
    let body = format!(
        "---\npath: {pp}\nverified_commit: {first}\n---\n# p\n[[other]]\n- `crates/alpha`: the alpha crate\n- The Docs name the capital.\n"
    );
    write(r.path(), "v/a.md", &body);
    let m = messages(&items(r.path(), &["check"]), "drift", "a")[0].clone();
    // `alpha` and `Docs` are in the note (any case); `api` is not, though `capital` has the
    // letters; `crates/core/deep` is below the depth; `.github` is hidden; `ios/App` is
    // inside a new folder.
    assert!(
        m.ends_with("; new folders the note does not name: `api`, `crates/beta`, `ios`"),
        "{m}"
    );
    // Depth 3 reaches `crates/core/deep`; depth 0 lists nothing.
    let r3 = repo("[[drift]]\ndir_depth = 3\n");
    write(r3.path(), "v/other.md", "# other\n[[a]]\n");
    write(r3.path(), "v/a.md", &body);
    let m = messages(&items(r3.path(), &["check"]), "drift", "a")[0].clone();
    assert!(m.contains("`crates/core/deep`"), "{m}");
    let r0 = repo("[[drift]]\ndir_depth = 0\n");
    write(r0.path(), "v/other.md", "# other\n[[a]]\n");
    write(r0.path(), "v/a.md", &body);
    let m = messages(&items(r0.path(), &["check"]), "drift", "a")[0].clone();
    assert!(
        m.starts_with("1 commit(s) in ") && !m.contains("new folders"),
        "{m}"
    );
    // More than eight: the rest are counted.
    for i in 0..10 {
        write(d, &format!("more/m{i}/x"), "x");
    }
    write(d, "more/keep", "x");
    commit(d, "three");
    write(d, "top/x", "x");
    commit(d, "four");
    let m = messages(&items(r.path(), &["check"]), "drift", "a")[0].clone();
    assert!(m.starts_with("3 commit(s) in "), "{m}");
    assert!(
        m.contains("`more`") && m.contains("`top`") && !m.contains("`more/m1`"),
        "{m}"
    );
}

#[test]
fn a_change_shows_the_drift_of_the_notes_it_touches() {
    let (p, first) = parked_project();
    let pp = p.path().display().to_string();
    let r = repo("[[drift]]\n");
    let d = r.path();
    write(d, "v/other.md", "# other\n[[a]] [[b]]\n");
    write(d, "v/a.md", &project_note(&pp, &first, ""));
    write(d, "v/b.md", &project_note(&pp, &first, ""));
    commit(d, "base");
    // Nothing changed: the drift is not this change's.
    assert_eq!(items(d, &["check", "--changed"]), Vec::<Value>::new());
    // The change edits `a`: its drift is shown, `b`'s is not.
    write(
        d,
        "v/a.md",
        &(project_note(&pp, &first, "") + "One more fact.\n"),
    );
    let found = items(d, &["check", "--changed"]);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0]["rule"], "drift");
    assert_eq!(found[0]["note"], "a");
    assert_eq!(found[0]["severity"], "warn");
    // The same for the commit, and a warning does not block it.
    git(d, &["add", "-A"]);
    let found = items(d, &["check", "--staged"]);
    assert_eq!(messages(&found, "drift", "a").len(), 1, "{found:?}");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(wk(d, &["check", "--staged"]).status.success());
    // A new commit id that is still behind is reported once, not twice.
    let side = git(p.path(), &["rev-parse", "--short", "main~1"]);
    write(d, "v/a.md", &project_note(&pp, &side, "drift_ref: main\n"));
    let found = items(d, &["check", "--changed"]);
    assert_eq!(messages(&found, "drift", "a").len(), 1, "{found:?}");
}

const SUMMARY: &str =
    "hubs = [\"INDEX\"]\n[[summary]]\nfrom = \"INDEX\"\ntarget = \"^Projects/\"\ncommits = 2\n";

/// A vault whose INDEX describes three project notes; `a` then changes in two commits, `b` in
/// one, `c` (a bare link in INDEX) in three.
fn described() -> TempDir {
    let r = repo(SUMMARY);
    let d = r.path();
    write(
        d,
        "v/INDEX.md",
        "# Index\n\n- [[Projects/a]] — project a: the first version is live\n- [[Projects/b]] — project b: a draft, nothing is deployed\n- [[Projects/c]]\n- [[Other/x]] — a note outside the rule, with words\n",
    );
    for n in ["a", "b", "c"] {
        write(d, &format!("v/Projects/{n}.md"), "# p\n[[INDEX]]\n");
    }
    write(d, "v/Other/x.md", "# x\n[[INDEX]]\n");
    commit(d, "base");
    for (n, times) in [("a", 2), ("b", 1), ("c", 3)] {
        for i in 0..times {
            write(
                d,
                &format!("v/Projects/{n}.md"),
                &format!("# p\n[[INDEX]]\nfact {i}\n"),
            );
            commit(d, &format!("{n} {i}"));
        }
    }
    for i in 0..3 {
        write(d, "v/Other/x.md", &format!("# x\n[[INDEX]]\n{i}\n"));
        commit(d, &format!("x {i}"));
    }
    r
}

#[test]
fn stale_summary_counts_commits_to_the_described_note() {
    let r = described();
    let d = r.path();
    let found = items(d, &["check"]);
    let stale: Vec<&Value> = found
        .iter()
        .filter(|f| f["rule"] == "stale-summary")
        .collect();
    // `a` has two commits after its line; `b` one; `c`'s line says nothing; `Other/x` is
    // outside the rule.
    assert_eq!(stale.len(), 1, "{found:?}");
    assert_eq!(stale[0]["note"], "INDEX");
    assert_eq!(stale[0]["line"], 3);
    assert_eq!(stale[0]["severity"], "warn");
    let m = stale[0]["message"].as_str().unwrap();
    assert!(
        m.starts_with(
            "this line describes [[Projects/a]]; 2 commit(s) changed that note after the line last changed ("
        ),
        "{m}"
    );
    // Shown for the index or the described note; not for another note.
    for (arg, n) in [("INDEX", 1), ("Projects/a", 1), ("Projects/b", 0)] {
        let found = items(d, &["check", arg]);
        assert_eq!(
            found
                .iter()
                .filter(|f| f["rule"] == "stale-summary")
                .count(),
            n,
            "{arg}: {found:?}"
        );
    }
    // The line is rewritten: it is current, before and after the commit.
    let index = fs::read_to_string(d.join("v/INDEX.md")).unwrap();
    write(
        d,
        "v/INDEX.md",
        &index.replace("the first version is live", "the second version is live"),
    );
    let none = |found: Vec<Value>| found.iter().all(|f| f["rule"] != "stale-summary");
    assert!(none(items(d, &["check"])));
    commit(d, "index");
    assert!(none(items(d, &["check"])));
    // `severity` turns it off.
    write(
        d,
        "wikiknit.toml",
        &format!("vault = \"v\"\n{SUMMARY}[severity]\nstale-summary = \"off\"\n"),
    );
    write(d, "v/Projects/a.md", "# p\n[[INDEX]]\nfact 8\n");
    commit(d, "a 8");
    write(d, "v/Projects/a.md", "# p\n[[INDEX]]\nfact 9\n");
    commit(d, "a 9");
    assert!(none(items(d, &["check"])));
}

#[test]
fn a_change_shows_the_summaries_of_the_notes_it_touches() {
    let r = described();
    let d = r.path();
    // Nothing changed: `a`'s old finding is not this change's.
    assert_eq!(items(d, &["check", "--changed"]), Vec::<Value>::new());
    // The change edits `b`: one commit before it and this change reach the limit of 2.
    write(d, "v/Projects/b.md", "# p\n[[INDEX]]\nnow deployed\n");
    for mode in ["--changed", "--staged"] {
        git(d, &["add", "-A"]);
        let found = items(d, &["check", mode]);
        assert_eq!(found.len(), 1, "{mode}: {found:?}");
        assert_eq!(found[0]["rule"], "stale-summary");
        assert_eq!(found[0]["note"], "INDEX");
        assert_eq!(found[0]["line"], 4);
        let m = found[0]["message"].as_str().unwrap();
        assert!(
            m.starts_with(
                "this line describes [[Projects/b]]; this change and 1 commit(s) before it changed that note"
            ),
            "{m}"
        );
    }
    assert!(wk(d, &["check", "--staged"]).status.success());
    // The same change rewrites the line: nothing to say.
    let index = fs::read_to_string(d.join("v/INDEX.md")).unwrap();
    write(
        d,
        "v/INDEX.md",
        &index.replace("a draft, nothing is deployed", "deployed"),
    );
    git(d, &["add", "-A"]);
    for mode in ["--changed", "--staged"] {
        assert_eq!(items(d, &["check", mode]), Vec::<Value>::new(), "{mode}");
    }
}

#[test]
fn stale_summary_needs_a_repository_and_a_valid_rule() {
    // Not in a repository: the rule is silent.
    let dir = TempDir::new().unwrap();
    write(
        dir.path(),
        "wikiknit.toml",
        SUMMARY.trim_start_matches("hubs = [\"INDEX\"]\n"),
    );
    write(
        dir.path(),
        "INDEX.md",
        "# i\n- [[Projects/a]] — a line with some words\n",
    );
    write(dir.path(), "Projects/a.md", "# a\n[[INDEX]]\n");
    let out = wk(dir.path(), &["--format", "json", "check"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // Before the first commit, and with the index not committed yet: silent too.
    let r = repo(SUMMARY);
    write(
        r.path(),
        "v/INDEX.md",
        "# i\n- [[Projects/a]] — a line with some words\n",
    );
    write(r.path(), "v/Projects/a.md", "# a\n[[INDEX]]\n");
    assert_eq!(items(r.path(), &["check"]), Vec::<Value>::new());
    assert_eq!(
        items(r.path(), &["check", "--changed"]),
        Vec::<Value>::new()
    );
    for bad in [
        "[[summary]]\nfrom = \"INDEX\"\ntarget = \"^P\"\ncommits = 0\n",
        "[[summary]]\nfrom = \"INDEX\"\ntarget = \"(\"\ncommits = 2\n",
        "[[summary]]\nfrom = \"\"\ntarget = \"^P\"\ncommits = 2\n",
        "[[size]]\nmax_kb = 0\n",
    ] {
        let r = repo(bad);
        write(r.path(), "v/a.md", "# a\n");
        let out = wk(r.path(), &["check"]);
        assert!(!out.status.success(), "{bad}");
    }
}

#[test]
fn note_size_limit_by_folder() {
    let big = format!("# big\n[[small]]\n{}\n", "word ".repeat(500)); // about 2.5 KB
    let dir = TempDir::new().unwrap();
    let d = dir.path();
    write(
        d,
        "wikiknit.toml",
        "[[size]]\nmax_kb = 1\npath = \"^Projects/\"\nfix = \"split it\"\n\n[[size]]\nmax_kb = 2\n",
    );
    write(d, "Projects/big.md", &big);
    write(d, "Projects/small.md", "# small\n[[big]]\n");
    write(d, "Logs/big.md", &big);
    write(
        d,
        "Logs/mid.md",
        &format!("# mid\n[[small]]\n{}\n", "word ".repeat(300)),
    );
    write(
        d,
        "small.md",
        "# small\n[[Projects/big]] [[Projects/small]] [[Logs/big]] [[Logs/mid]]\n",
    );
    let found = items(d, &["check"]);
    let sized: Vec<(&str, &str, &str)> = found
        .iter()
        .filter(|f| f["rule"] == "note-size")
        .map(|f| {
            (
                f["note"].as_str().unwrap(),
                f["message"].as_str().unwrap(),
                f["fix"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(sized.len(), 2, "{found:?}");
    // The first rule that matches a note applies: 1 KB under Projects/, 2 KB elsewhere.
    assert_eq!(sized[0].0, "Logs/big");
    assert!(
        sized[0].1.starts_with("note is 3 KB; its limit is 2 KB"),
        "{}",
        sized[0].1
    );
    assert!(
        sized[0].2.starts_with("move history and detail"),
        "{}",
        sized[0].2
    );
    assert_eq!(sized[1].0, "Projects/big");
    assert!(
        sized[1].1.starts_with("note is 3 KB; its limit is 1 KB"),
        "{}",
        sized[1].1
    );
    assert_eq!(sized[1].2, "split it");
    assert!(
        found
            .iter()
            .all(|f| f["rule"] != "note-size" || f["severity"] == "warn")
    );
}

#[test]
fn note_size_is_a_change_s_finding_only_when_the_change_crosses_the_limit() {
    let r = repo("[[size]]\nmax_kb = 1\n");
    let d = r.path();
    let filler = |n: usize| format!("# a\n[[b]]\n{}\n", "word ".repeat(n));
    write(d, "v/a.md", &filler(100));
    write(d, "v/b.md", "# b\n[[a]]\n");
    commit(d, "base");
    write(d, "v/a.md", &filler(400));
    let found = items(d, &["check", "--changed"]);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0]["rule"], "note-size");
    commit(d, "grow");
    // Already over: growing more is not a new finding.
    write(d, "v/a.md", &filler(800));
    assert_eq!(items(d, &["check", "--changed"]), Vec::<Value>::new());
}

#[test]
fn mentions_skip_names_inside_issue_references() {
    let dir = TempDir::new().unwrap();
    let d = dir.path();
    write(d, "wikiknit.toml", "");
    write(
        d,
        "Reference/acme.md",
        "---\naliases: [Acme17]\n---\n# acme\n[[widgets]]\n",
    );
    write(d, "widgets.md", "# widgets\n[[Reference/acme]]\n");
    // Pull request references, bare and with punctuation around them.
    write(
        d,
        "refs.md",
        "# refs\n[[widgets]]\nSee PR Acme17/widgets#44. Also (Acme17/widgets#45), Acme17/widgets#46: merged.\n",
    );
    // No link to `widgets` here: both halves of a reference are part of it. Nested groups
    // and merge requests too.
    write(
        d,
        "more.md",
        "# more\n[[refs]]\nAcme17/widgets#44, team/Acme17/widgets#9 and Acme17/widgets!7.\n",
    );
    // Not references: a repository path without a number, and the name by itself.
    write(
        d,
        "prose.md",
        "# prose\n[[widgets]]\nThe repository Acme17/widgets has it. Issue #12 is for Acme17.\n",
    );
    let terms = |note: &str| -> Vec<(String, u64)> {
        items(d, &["mentions", note])
            .iter()
            .map(|m| {
                (
                    m["term"].as_str().unwrap().to_string(),
                    m["count"].as_u64().unwrap(),
                )
            })
            .collect()
    };
    assert_eq!(terms("refs"), Vec::<(String, u64)>::new());
    assert_eq!(terms("more"), Vec::<(String, u64)>::new());
    assert_eq!(terms("prose"), vec![("Acme17".to_string(), 2)]);
}

/// Review 1: findings a change did not add are warnings whatever `[severity]` says.
#[test]
fn findings_a_change_did_not_add_never_block_a_commit() {
    let (p, first) = parked_project();
    let pp = p.path().display().to_string();
    let r = repo(
        "hubs = [\"INDEX\"]\n[[drift]]\n[[summary]]\nfrom = \"INDEX\"\ntarget = \"^a$\"\ncommits = 1\n[severity]\ndrift = \"error\"\nstale-summary = \"error\"\n",
    );
    let d = r.path();
    write(
        d,
        "v/INDEX.md",
        "# Index\n\n- [[a]] — the project, as it was at the start\n",
    );
    write(
        d,
        "v/a.md",
        &project_note(&pp, &first, "").replace("[[other]]", "[[INDEX]]"),
    );
    commit(d, "base");
    // The whole vault: the configured severity, and exit 1.
    let all = items(d, &["check"]);
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(
        (&all[0]["rule"], &all[0]["severity"]),
        (&"drift".into(), &"error".into())
    );
    assert!(!wk(d, &["check"]).status.success());
    // A commit that edits the note: both findings are shown as warnings and it goes through.
    let text = fs::read_to_string(d.join("v/a.md")).unwrap();
    write(d, "v/a.md", &(text + "One more fact.\n"));
    git(d, &["add", "-A"]);
    for mode in ["--changed", "--staged"] {
        let found = items(d, &["check", mode]);
        let got: Vec<(&str, &str)> = found
            .iter()
            .map(|f| (f["rule"].as_str().unwrap(), f["severity"].as_str().unwrap()))
            .collect();
        assert_eq!(
            got,
            [("stale-summary", "warn"), ("drift", "warn")],
            "{mode}"
        );
        let out = wk(d, &["check", mode]);
        assert!(out.status.success(), "{mode}");
        assert!(
            !String::from_utf8_lossy(&out.stderr).contains("link error"),
            "{mode}"
        );
    }
    // A change that makes the note fall behind adds the finding: that one is an error.
    let side = git(p.path(), &["rev-parse", "--short", "main~1"]);
    write(
        d,
        "v/a.md",
        &project_note(&pp, &side, "drift_ref: main\n").replace("[[other]]", "[[INDEX]]"),
    );
    let found = items(d, &["check", "--changed"]);
    assert_eq!(
        found
            .iter()
            .filter(|f| f["rule"] == "drift" && f["severity"] == "error")
            .count(),
        1,
        "{found:?}"
    );
}

/// Review 2: a work tree that differs from the last commit in line endings only (a checkout
/// with `eol=crlf`) has no changed notes.
#[test]
fn line_endings_alone_do_not_make_a_note_changed() {
    let (p, first) = parked_project();
    let pp = p.path().display().to_string();
    let r = repo("[[drift]]\n");
    let d = r.path();
    write(d, ".gitattributes", "*.md text eol=crlf\n");
    write(d, "v/other.md", "# other\n[[a]]\n");
    let note = project_note(&pp, &first, "");
    write(d, "v/a.md", &note);
    commit(d, "base");
    // A fresh checkout writes CRLF on disk; the commit has LF.
    fs::remove_file(d.join("v/a.md")).unwrap();
    git(d, &["checkout", "--", "v/a.md"]);
    let on_disk = fs::read_to_string(d.join("v/a.md")).unwrap();
    assert!(on_disk.contains("\r\n"), "{on_disk:?}");
    assert_eq!(git(d, &["status", "--porcelain"]), "");
    assert_eq!(items(d, &["check", "--changed"]), Vec::<Value>::new());
    // A real edit is still seen.
    write(d, "v/a.md", &(on_disk + "fact\r\n"));
    assert_eq!(items(d, &["check", "--changed"]).len(), 1);
}

/// Review 3, 4, 7: an index with CRLF line endings, two index notes, a vault at the
/// repository top, and lines whose first link goes somewhere else.
#[test]
fn stale_summary_with_crlf_two_indexes_and_a_vault_at_the_top() {
    let dir = TempDir::new().unwrap();
    let d = dir.path();
    git(d, &["init", "-q", "-b", "main"]);
    write(
        d,
        "wikiknit.toml",
        "hubs = [\"INDEX\", \"HOME\"]\n[[summary]]\nfrom = \"INDEX\"\ntarget = \"^Projects/\"\ncommits = 2\n[[summary]]\nfrom = \"HOME\"\ntarget = \"^Projects/\"\ncommits = 2\n",
    );
    write(
        d,
        "INDEX.md",
        "# Index\r\n\r\n- [[Projects/a]] — project a: the first version is live\r\n- See [[Projects/gone]] first; [[Projects/a]] is named in passing on this line\r\n- Read [the plan](plan.pdf), then [[Projects/a]] for the state of the work\r\n",
    );
    write(
        d,
        "HOME.md",
        "# Home\n\n- [[Projects/a]] — the same project, described once more\n",
    );
    write(d, "plan.pdf", "pdf");
    write(d, "Projects/a.md", "# a\n[[INDEX]]\n");
    commit(d, "base");
    for i in 0..2 {
        write(d, "Projects/a.md", &format!("# a\n[[INDEX]]\nfact {i}\n"));
        commit(d, &format!("a {i}"));
    }
    let found = items(d, &["check"]);
    let stale: Vec<(&str, u64)> = found
        .iter()
        .filter(|f| f["rule"] == "stale-summary")
        .map(|f| (f["note"].as_str().unwrap(), f["line"].as_u64().unwrap()))
        .collect();
    // One line in each index; not the lines whose first link is a missing note or a file.
    assert_eq!(stale, [("HOME", 3), ("INDEX", 3)], "{found:?}");
}

/// Review 5: the repository's `blame.ignoreRevsFile` list does not decide which commit a line
/// belongs to; a list that names a missing file (git itself fails then) is said, not hidden.
#[test]
fn stale_summary_does_not_use_the_repository_s_blame_ignore_list() {
    let r = repo(SUMMARY);
    let d = r.path();
    let index =
        |words: &str| format!("# Index\n\n- [[Projects/a]] — {words} words about the project\n");
    let project = |n: u32| format!("# p\n[[INDEX]]\nfact {n}\n");
    write(d, "v/INDEX.md", &index("first"));
    write(d, "v/Projects/a.md", &project(0));
    commit(d, "base");
    write(d, "v/Projects/a.md", &project(1));
    commit(d, "a 1");
    write(d, "v/INDEX.md", &index("second"));
    commit(d, "the line is rewritten");
    let rewrite = git(d, &["rev-parse", "HEAD"]);
    write(d, "v/Projects/a.md", &project(2));
    commit(d, "a 2");
    // One commit to `a` after the line last changed: under the limit of 2. With the rewrite
    // on an ignore list, git blame gives the line to the base commit (2 commits to `a`).
    fs::write(d.join(".git/ignore-revs"), format!("{rewrite}\n")).unwrap();
    git(d, &["config", "blame.ignoreRevsFile", ".git/ignore-revs"]);
    let blamed = git(d, &["blame", "-s", "-L3,3", "HEAD", "--", "v/INDEX.md"]);
    assert!(
        !rewrite.starts_with(blamed.split(' ').next().unwrap()),
        "{blamed}"
    );
    let out = wk(d, &["--format", "json", "check"]);
    assert!(out.status.success());
    assert!(!String::from_utf8_lossy(&out.stdout).contains("stale-summary"));
    assert_eq!(String::from_utf8_lossy(&out.stderr), "");
    // The list names a file that is not there: git blame fails, and wikiknit says so.
    git(d, &["config", "blame.ignoreRevsFile", ".git/no-such-file"]);
    let out = wk(d, &["--format", "json", "check"]);
    assert!(out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("stale-summary is off for INDEX: git blame failed"),
        "{err}"
    );
}
