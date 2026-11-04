use super::{SearchError, search_files};
use aow_protocol::{FileSearchMode, FileSearchRequest};
use std::{fs, time::Duration};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

fn request(root: &TempDir, query: &str) -> FileSearchRequest {
    FileSearchRequest {
        root: root.path().to_string_lossy().into_owned(),
        query: query.into(),
        mode: FileSearchMode::Content,
        case_sensitive: false,
        whole_word: false,
        use_regex: false,
        include_ignored: false,
        include_pattern: String::new(),
        exclude_pattern: String::new(),
    }
}

#[tokio::test]
async fn content_options_and_utf16_coordinates_use_real_ripgrep() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("unicode.txt"),
        "😀 中文 term terminal TERM\nterm\n",
    )
    .unwrap();
    let mut query = request(&root, "term");
    let result = search_files(query.clone(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(result.total_matches, 4);
    assert_eq!(result.files[0].matches[0].column, 7);
    assert_eq!(result.files[0].matches[0].match_length, 4);
    query.whole_word = true;
    assert_eq!(
        search_files(query.clone(), CancellationToken::new())
            .await
            .unwrap()
            .total_matches,
        3
    );
    query.case_sensitive = true;
    assert_eq!(
        search_files(query.clone(), CancellationToken::new())
            .await
            .unwrap()
            .total_matches,
        2
    );
    query.use_regex = true;
    query.query = "term(inal)?".into();
    assert_eq!(
        search_files(query, CancellationToken::new())
            .await
            .unwrap()
            .total_matches,
        3
    );
}

#[tokio::test]
async fn regex_anchors_handle_crlf_and_zero_width_matches() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("lines.txt"), "term\r\n\r\nterm\n\n").unwrap();
    let mut query = request(&root, "term$");
    query.use_regex = true;
    let result = search_files(query.clone(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(result.total_matches, 2);
    assert!(!result.truncated);
    query.query = "^$".into();
    let result = search_files(query, CancellationToken::new()).await.unwrap();
    assert_eq!(result.total_matches, 2);
    assert_eq!(result.files[0].matches[0].line, 2);
    assert_eq!(result.files[0].matches[0].column, 1);
    assert_eq!(result.files[0].matches[0].match_length, 0);
    assert!(!result.truncated);
}

#[tokio::test]
async fn names_scan_unexpanded_paths_and_ignored_files_without_content_matching() {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join(".git")).unwrap();
    fs::create_dir_all(root.path().join("deep/generated")).unwrap();
    fs::write(root.path().join(".gitignore"), "deep/generated/\n").unwrap();
    fs::write(root.path().join("deep/generated/Needle.txt"), "haystack").unwrap();
    fs::write(root.path().join(".git/needle"), "needle").unwrap();
    let mut query = request(&root, "DEEP needle");
    query.mode = FileSearchMode::Name;
    let result = search_files(query, CancellationToken::new()).await.unwrap();
    assert_eq!(result.files.len(), 1);
    assert_eq!(result.files[0].relative_path, "deep/generated/Needle.txt");
    assert!(result.files[0].matches.is_empty());
    let mut query = request(&root, "haystack");
    assert!(
        search_files(query.clone(), CancellationToken::new())
            .await
            .unwrap()
            .files
            .is_empty()
    );
    query.include_ignored = true;
    assert_eq!(
        search_files(query, CancellationToken::new())
            .await
            .unwrap()
            .files
            .len(),
        1
    );
}

#[tokio::test]
async fn globs_hidden_files_and_invalid_regex_have_distinct_results() {
    let root = TempDir::new().unwrap();
    for name in [".hidden.ts", "main.ts", "main.rs", "skip.ts"] {
        fs::write(root.path().join(name), "-needle a.b\n").unwrap();
    }
    let mut query = request(&root, "-needle");
    query.include_pattern = "*.{ts,rs}".into();
    query.exclude_pattern = "skip.ts, *.rs".into();
    let result = search_files(query.clone(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(result.files.len(), 2);
    query.query = "a.b".into();
    assert_eq!(
        search_files(query.clone(), CancellationToken::new())
            .await
            .unwrap()
            .files
            .len(),
        2
    );
    query.query = "not-present".into();
    assert!(
        search_files(query.clone(), CancellationToken::new())
            .await
            .unwrap()
            .files
            .is_empty()
    );
    query.query = "[".into();
    query.use_regex = true;
    assert!(matches!(
        search_files(query, CancellationToken::new()).await,
        Err(SearchError::Failed(_))
    ));
}

#[tokio::test]
async fn limits_keep_partial_results_and_long_previews_keep_navigation_coordinates() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("dense.txt"), "term ".repeat(2500)).unwrap();
    let result = search_files(request(&root, "term"), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(result.total_matches, 2000);
    assert!(result.truncated);
    let last = result.files[0].matches.last().unwrap();
    assert_eq!(last.column, 9996);
    let preview: Vec<u16> = last.text.encode_utf16().collect();
    assert_eq!(
        String::from_utf16(
            &preview[last.preview_column..last.preview_column + last.preview_length]
        )
        .unwrap(),
        "term"
    );
    assert!(last.text.chars().count() <= 402);
}

#[tokio::test]
async fn cancellation_timeout_and_missing_executable_are_reported() {
    let root = TempDir::new().unwrap();
    let cancel = CancellationToken::new();
    let signal = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(30)).await;
        signal.cancel();
    });
    let result = super::process::execute(
        "/bin/sleep".into(),
        vec!["60".into()],
        request(&root, "x"),
        cancel,
        Duration::from_secs(5),
    )
    .await;
    assert!(matches!(result, Err(SearchError::Cancelled)));
    let result = super::process::execute(
        "/bin/sleep".into(),
        vec!["60".into()],
        request(&root, "x"),
        CancellationToken::new(),
        Duration::from_millis(30),
    )
    .await
    .unwrap();
    assert!(result.truncated);
    let result = super::process::execute(
        root.path().join("missing-rg"),
        vec![],
        request(&root, "x"),
        CancellationToken::new(),
        Duration::from_secs(1),
    )
    .await;
    assert!(matches!(result, Err(SearchError::Unavailable)));
}

#[tokio::test]
async fn skips_large_binary_and_symlinked_files() {
    let root = TempDir::new().unwrap();
    let large = fs::File::create(root.path().join("large.txt")).unwrap();
    large.set_len(6 * 1024 * 1024).unwrap();
    fs::write(root.path().join("binary"), b"\0needle\0").unwrap();
    fs::write(root.path().join("text.txt"), "needle").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.path().join("text.txt"), root.path().join("linked.txt"))
        .unwrap();
    let result = search_files(request(&root, "needle"), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(result.files.len(), 1);
    assert_eq!(result.files[0].relative_path, "text.txt");
}

#[tokio::test]
async fn timeout_also_stops_a_process_that_closed_stdout() {
    let root = TempDir::new().unwrap();
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        super::process::execute(
            "/bin/sh".into(),
            vec!["-c".into(), "exec 1>&-; exec sleep 60".into()],
            request(&root, "x"),
            CancellationToken::new(),
            Duration::from_millis(30),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(result.truncated);
}
