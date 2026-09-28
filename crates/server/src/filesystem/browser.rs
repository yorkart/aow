use std::path::{Path, PathBuf};

use aow_filesystem::list_directory;
use axum::{
    extract::{Path as AxumPath, State},
    http::{HeaderMap, StatusCode, header::LOCATION},
    response::{IntoResponse, Response},
};

use super::{
    paths::{PROCESS_HOME, decode_absolute, encode_absolute, escape_html},
    raw::raw_response,
};
use crate::{AppState, BasePath, HttpError};

pub(crate) async fn fs_root_redirect() -> impl IntoResponse {
    (StatusCode::PERMANENT_REDIRECT, [(LOCATION, "/fs/")])
}

pub(crate) async fn fs_root(State(state): State<AppState>) -> Result<Response, HttpError> {
    fs_browser(PathBuf::from("/"), &state.base_path).await
}

pub(crate) async fn fs_path(
    State(state): State<AppState>,
    AxumPath(path): AxumPath<String>,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let path = decode_absolute(&path)?;
    if tokio::fs::metadata(&path).await?.is_file() {
        return raw_response(path, headers, true).await;
    }
    fs_browser(path, &state.base_path).await
}

async fn fs_browser(path: PathBuf, base_path: &BasePath) -> Result<Response, HttpError> {
    let base = base_path.as_str();
    let listing = list_directory(&path).await?;
    let home = PROCESS_HOME.to_string_lossy();
    let rows = listing
        .entries
        .iter()
        .map(|entry| {
            let encoded = encode_absolute(&entry.path);
            let kind = match (entry.is_symlink, &entry.kind) {
                (true, _) => "符号链接",
                (_, aow_protocol::FileKind::Directory) => "目录",
                (_, aow_protocol::FileKind::File) => "文件",
                (_, aow_protocol::FileKind::Symlink) => "符号链接",
                (_, aow_protocol::FileKind::Other) => "其他",
            };
            let directory = matches!(
                entry.kind,
                aow_protocol::FileKind::Directory
            );
            let slash = if directory { "/" } else { "" };
            let icon = if directory { "📁" } else { "📄" };
            let link = entry
                .link_target
                .as_deref()
                .map(|target| format!(" → {}", escape_html(target)))
                .unwrap_or_default();
            format!(
                "<tr><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{kind}</td><td><a href=\"{base}/fs{encoded}{slash}\">{icon} {}{slash}</a>{link}</td><td><button class=\"copy\" data-path=\"{}\" title=\"复制服务器路径\">⧉</button></td></tr>",
                mode_string(entry.mode),
                entry.links,
                entry.uid,
                entry.gid,
                entry.size,
                entry
                    .modified_ms
                    .and_then(|value| chrono::DateTime::from_timestamp_millis(value as i64))
                    .map(|value| value.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S").to_string())
                    .unwrap_or_else(|| "-".to_owned()),
                escape_html(&entry.name),
                escape_html(&entry.path)
            )
        })
        .collect::<String>();
    let parent = path.parent().unwrap_or(Path::new("/"));
    let current = escape_html(&path.to_string_lossy());
    let encoded_current = encode_absolute(&path.to_string_lossy());
    let upload_base = format!(
        "{base}/api/fs/file{}",
        encoded_current.trim_end_matches('/')
    );
    let html = format!(
        r#"<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>文件浏览器 - {current}</title><style>
body{{margin:0;background:#1e1e1e;color:#ccc;font:12px system-ui}}a{{color:#4daafc;text-decoration:none}}header{{display:flex;align-items:center;gap:5px;padding:7px;border-bottom:1px solid #444}}header code{{flex:1;overflow:hidden;text-overflow:ellipsis}}button,.button{{padding:4px 8px;border:1px solid #555;border-radius:4px;background:#2d2d2d;color:#ddd;cursor:pointer}}main{{padding:6px}}table{{width:100%;border-collapse:collapse}}th,td{{padding:4px 7px;border-bottom:1px solid #393939;text-align:left}}tr:hover{{background:#292929}}.copy{{padding:2px 7px}}#status{{color:#9cdcfe}}
</style></head><body><header><a class="button" href="{base}/fs{parent}/">..</a><a class="button" href="{base}/fs{home}/">Home</a><a class="button" href="{base}/fs/tmp/">/tmp</a><a class="button" href="{base}/aow/">Project AoW</a><code>{current}</code><input id="files" type="file" multiple><button id="upload">上传</button><span id="status"></span></header><main><table><thead><tr><th>权限</th><th>链接</th><th>UID</th><th>GID</th><th>大小</th><th>修改时间</th><th>类型</th><th>名称</th><th>操作</th></tr></thead><tbody>{rows}</tbody></table></main><script>
document.addEventListener('click',async e=>{{const b=e.target.closest('.copy');if(b){{await navigator.clipboard.writeText(b.dataset.path);b.textContent='✓';setTimeout(()=>b.textContent='⧉',1000)}}}});
document.getElementById('upload').onclick=async()=>{{const files=[...document.getElementById('files').files];for(const file of files){{document.getElementById('status').textContent='上传 '+file.name;const response=await fetch('{upload_base}/'+encodeURIComponent(file.name),{{method:'PUT',body:file}});if(!response.ok)throw new Error(await response.text())}}location.reload()}};
</script></body></html>"#,
        parent = encode_absolute(&parent.to_string_lossy()).trim_end_matches('/'),
        home = encode_absolute(&home).trim_end_matches('/')
    );
    Ok(axum::response::Html(html).into_response())
}

fn mode_string(mode: u32) -> String {
    let kind = match mode & 0o170000 {
        0o040000 => 'd',
        0o120000 => 'l',
        0o100000 => '-',
        0o060000 => 'b',
        0o020000 => 'c',
        0o010000 => 'p',
        0o140000 => 's',
        _ => '?',
    };
    let mut value = String::with_capacity(10);
    value.push(kind);
    for (mask, character) in [
        (0o400, 'r'),
        (0o200, 'w'),
        (0o100, 'x'),
        (0o040, 'r'),
        (0o020, 'w'),
        (0o010, 'x'),
        (0o004, 'r'),
        (0o002, 'w'),
        (0o001, 'x'),
    ] {
        value.push(if mode & mask != 0 { character } else { '-' });
    }
    value
}
