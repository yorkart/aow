use aow_protocol::{FileSearchMode, FileSearchRequest};

use super::SearchError;

pub(super) fn arguments(request: &FileSearchRequest) -> Result<Vec<String>, SearchError> {
    if !std::path::Path::new(&request.root).is_absolute() {
        return Err(SearchError::Invalid("搜索目录必须是绝对路径".into()));
    }
    if [
        &request.query,
        &request.include_pattern,
        &request.exclude_pattern,
    ]
    .iter()
    .any(|text| text.len() > 8192 || text.contains('\0'))
    {
        return Err(SearchError::Invalid(
            "搜索条件过长或包含无效字符（最多 8 KiB）".into(),
        ));
    }
    let mut args: Vec<String> = [
        "--no-config",
        "--hidden",
        "--color",
        "never",
        "--threads",
        "2",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    if request.mode == FileSearchMode::Name {
        args.extend(["--files", "--null", "--no-ignore"].map(String::from));
    } else {
        args.extend(["--json", "--crlf", "--max-filesize", "5M"].map(String::from));
        if request.include_ignored {
            args.push("--no-ignore".into());
        }
        if !request.case_sensitive {
            args.push("--ignore-case".into());
        }
        if request.whole_word {
            args.push("--word-regexp".into());
        }
        if !request.use_regex {
            args.push("--fixed-strings".into());
        }
        for pattern in split_globs(&request.include_pattern)? {
            args.extend(["--glob".into(), pattern]);
        }
        for pattern in split_globs(&request.exclude_pattern)? {
            args.extend(["--glob".into(), format!("!{pattern}")]);
        }
    }
    args.extend(["--glob", "!.git", "--"].map(String::from));
    if request.mode == FileSearchMode::Content {
        args.push(request.query.clone());
    }
    args.push(".".into());
    Ok(args)
}

// A comma inside a brace group or character class belongs to that glob.
fn split_globs(input: &str) -> Result<Vec<String>, SearchError> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut braces = 0usize;
    let mut class = false;
    let mut escaped = false;
    for (index, ch) in input.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '[' => class = true,
            ']' => class = false,
            '{' if !class => braces += 1,
            '}' if !class => braces = braces.saturating_sub(1),
            ',' if !class && braces == 0 => {
                result.push(input[start..index].trim().to_owned());
                start = index + 1;
            }
            _ => {}
        }
    }
    result.push(input[start..].trim().to_owned());
    result.retain(|pattern| !pattern.is_empty());
    if result.len() > 64 {
        return Err(SearchError::Invalid("文件筛选规则最多 64 项".into()));
    }
    Ok(result)
}
