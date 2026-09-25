"""AOW review protocol v2 — GitHub adapter using gh.

One JSON request on stdin; one {version, result|error} response on stdout.
Upload this script in Settings → Pull Requests. Python 3 standard library only.
"""
import base64
import difflib
import json
import os
import subprocess
import sys
from urllib.parse import quote

COMMENT_FIELDS = "id body createdAt updatedAt author { login }"
PAGE_INFO = "pageInfo { hasNextPage endCursor }"
CONTENT_LIMIT = 1024 * 1024


def user(value):
    value = value or {}
    return {"id": str(value.get("id", "")), "username": value.get("login", ""),
            "display_name": value.get("name") or value.get("login", "")}


def summary(pr):
    return {"number": pr["number"], "status": "merged" if pr.get("merged_at") else pr["state"],
            "draft": pr.get("draft", False), "title": pr["title"],
            "source_branch": pr["head"]["ref"], "target_branch": pr["base"]["ref"],
            "url": pr["html_url"], "created_at": pr["created_at"], "updated_at": pr["updated_at"]}


def comment(value):
    return {"id": str(value["id"]), "author": (value.get("author") or value.get("user") or {}).get("login", ""),
            "body": value.get("body") or "", "created_at": value.get("createdAt", value.get("created_at", "")),
            "updated_at": value.get("updatedAt", value.get("updated_at", ""))}


def thread(identifier, comments, path=None, line=None, status="comment"):
    first = comments[0] if comments else {}
    return {"id": str(identifier), "path": path, "line": line, "status": status,
            "author": first.get("author", ""), "body": first.get("body", ""),
            "updated_at": comments[-1]["updated_at"] if comments else None, "comments": comments}


class GitHub:
    def __init__(self, request):
        self.request = request
        self.repo = request["repository"]
        self.prefix = "repos/" + quote(self.repo["path"], safe="/")

    def commit_links(self, commit):
        repository_url = "https://" + self.repo["host"] + "/" + quote(self.repo["path"], safe="/")
        return {"remote_url": repository_url,
                "commit_url": repository_url + "/commit/" + quote(commit, safe="")}

    def repository_info(self):
        repository = self.api(self.prefix)
        return {"avatar_url": (repository.get("owner") or {}).get("avatar_url") or None}

    def command(self, *args):
        env = dict(os.environ, GH_PROMPT_DISABLED="1", GH_PAGER="cat")
        result = subprocess.run(["gh", *args], stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                stdin=subprocess.DEVNULL, timeout=45, env=env, check=False)
        if result.returncode:
            raise RuntimeError(result.stderr.decode("utf-8", errors="replace").strip() or f"gh exited {result.returncode}")
        return json.loads(result.stdout)

    def api(self, endpoint, pages=False):
        args = ["api", "--hostname", self.repo["host"], endpoint]
        if pages:
            args += ["--paginate", "--slurp"]
        result = self.command(*args)
        return [item for page in result for item in page] if pages else result

    def graphql(self, query, variables):
        args = ["api", "--hostname", self.repo["host"], "graphql", "-f", "query=" + query]
        for key, value in variables.items():
            if value is not None:
                args += ["-F" if isinstance(value, int) else "-f", f"{key}={value}"]
        result = self.command(*args)
        if result.get("errors"):
            raise RuntimeError("; ".join(e["message"] for e in result["errors"]))
        return result["data"]

    def list(self):
        current = self.api("user")
        items = self.api(self.prefix + "/pulls?state=open&per_page=100", pages=True)
        branch = subprocess.run(["git", "branch", "--show-current"], capture_output=True, text=True, check=False).stdout.strip()
        return {"repository": self.repo["root"], "current_branch": branch or "HEAD", "current_user": user(current),
                "pull_requests": [summary(pr) for pr in items if pr["user"]["id"] == current["id"]]}

    def files(self, number):
        return self.api(f"{self.prefix}/pulls/{number}/files?per_page=100", pages=True)

    def review_threads(self, number):
        owner, name = self.repo["path"].split("/", 1)
        variables = {"owner": owner, "name": name, "number": number}
        query = """query($owner:String!,$name:String!,$number:Int!,$cursor:String) {
          repository(owner:$owner,name:$name) { pullRequest(number:$number) {
            reviewThreads(first:100,after:$cursor) { nodes { id path line isResolved
              comments(first:100) { nodes { %s } %s } } %s }
          } }
        }""" % (COMMENT_FIELDS, PAGE_INFO, PAGE_INFO)
        result = []
        while True:
            connection = self.graphql(query, variables)["repository"]["pullRequest"]["reviewThreads"]
            for item in connection["nodes"]:
                comments = item["comments"]["nodes"][:]
                page = item["comments"]["pageInfo"]
                while page["hasNextPage"]:
                    extra = self.graphql("""query($id:ID!,$cursor:String) {
                      node(id:$id) { ... on PullRequestReviewThread {
                        comments(first:100,after:$cursor) { nodes { %s } %s }
                      } }
                    }""" % (COMMENT_FIELDS, PAGE_INFO), {"id": item["id"], "cursor": page["endCursor"]})["node"]["comments"]
                    comments.extend(extra["nodes"])
                    page = extra["pageInfo"]
                result.append(thread(item["id"], [comment(c) for c in comments], item["path"], item["line"],
                                     "resolved" if item["isResolved"] else "open"))
            if not connection["pageInfo"]["hasNextPage"]:
                return result
            variables["cursor"] = connection["pageInfo"]["endCursor"]

    def detail(self, number):
        pr = self.api(f"{self.prefix}/pulls/{number}")
        warnings = []

        def section(label, fn, fallback):
            try:
                return fn()
            except (RuntimeError, subprocess.SubprocessError, ValueError, KeyError) as error:
                warnings.append(f"{label}: {error}")
                return fallback

        files = section("文件变更", lambda: self.files(number), [])
        if len(files) < pr["changed_files"]:
            warnings.append(f"文件列表不完整：{len(files)} / {pr['changed_files']}（GitHub API 最多返回 3000 个文件）。")
        metadata = section("评审和检查", lambda: self.command(
            "pr", "view", str(number), "--repo", self.repo["host"] + "/" + self.repo["path"],
            "--json", "reviewDecision,statusCheckRollup,mergeStateStatus"), {})
        checks = []
        for index, check in enumerate(metadata.get("statusCheckRollup") or []):
            checks.append({"id": str(check.get("databaseId", index)), "name": check.get("name", check.get("context", "")),
                           "status": (check.get("status") or check.get("state") or "unknown").lower(),
                           "conclusion": (check.get("conclusion") or check.get("state") or "").lower(),
                           "details_url": check.get("detailsUrl", check.get("targetUrl")),
                           "description": check.get("description") or "", "text": "", "required": False,
                           "started_at": check.get("startedAt"), "completed_at": check.get("completedAt")})
        statuses = [c["conclusion"] or c["status"] for c in checks]
        ci = ("failed" if any(s in ("failure", "error", "cancelled", "timed_out", "action_required", "startup_failure") for s in statuses)
              else "passed" if statuses and all(s in ("success", "neutral", "skipped") for s in statuses)
              else "pending" if statuses else "unknown")
        threads = section("代码讨论", lambda: self.review_threads(number), [])
        general = section("一般讨论", lambda: self.api(f"{self.prefix}/issues/{number}/comments?per_page=100", pages=True), [])
        threads += [thread("issue-" + str(c["id"]), [comment(c)]) for c in general]
        # mergeable reports conflicts only; CLEAN is the platform's merge gate.
        merge_state = metadata.get("mergeStateStatus", "UNKNOWN")
        mergeable = True if merge_state == "CLEAN" and not pr.get("draft") else None if merge_state == "UNKNOWN" else False
        return dict(summary(pr), description=pr.get("body") or "", changes_count=pr["changed_files"], commits_count=pr["commits"],
                    review_status=(metadata.get("reviewDecision") or "unknown").lower(), check_summary_status=ci,
                    mergeable=mergeable, reviewers=[user(u) for u in pr.get("requested_reviewers", [])], checks=checks,
                    unresolved_threads=[t for t in threads if t["status"] == "open"],
                    files=[{"path": f["filename"], "change_type": {"added": "A", "removed": "D", "renamed": "R"}.get(f["status"], "M"),
                            "additions": f.get("additions"), "deletions": f.get("deletions")} for f in files],
                    author=user(pr["user"]), labels=[label["name"] for label in pr.get("labels", [])],
                    merge_checks=[{"name": "merge_state", "passed": mergeable, "reason": merge_state}], threads=threads,
                    warnings=warnings, diverged_commits_count=0, milestone=(pr.get("milestone") or {}).get("title"))

    def content(self, repository, path, ref):
        endpoint = f"repos/{quote(repository, safe='/')}/contents/{quote(path, safe='/')}?ref={quote(ref, safe='')}"
        value = self.api(endpoint)
        if not isinstance(value, dict) or value.get("size", 0) > CONTENT_LIMIT or value.get("encoding") != "base64":
            return None, False, True
        raw = base64.b64decode(value["content"])
        try:
            return (None, True, False) if b"\0" in raw else (raw.decode("utf-8"), False, False)
        except UnicodeDecodeError:
            return None, True, False

    def diff(self, number, path, patch_only=False):
        pr = self.api(f"{self.prefix}/pulls/{number}")
        file = next((f for f in self.files(number) if f["filename"] == path), None)
        if file is None:
            raise ValueError("File is not part of this pull request")
        original_path = file.get("previous_filename", path)
        original, modified, binary, truncated = "", "", False, False
        if file["status"] != "added":
            comparison = self.api(f"{self.prefix}/compare/{pr['base']['sha']}...{pr['head']['sha']}?per_page=1")
            original, b, t = self.content(pr["base"]["repo"]["full_name"], original_path, comparison["merge_base_commit"]["sha"])
            binary |= b
            truncated |= t
        if file["status"] != "removed":
            # Head commits of fork PRs remain addressable through the base repo,
            # including after the source fork has been deleted.
            modified, b, t = self.content(self.repo["path"], path, pr["head"]["sha"])
            binary |= b
            truncated |= t
        patch = None
        if not binary and not truncated:
            lines = difflib.unified_diff(original.splitlines(keepends=True), modified.splitlines(keepends=True),
                                         fromfile="a/" + original_path, tofile="b/" + path)
            patch = "".join(line if line.endswith("\n") else line + "\n\\ No newline at end of file\n" for line in lines)
        return {"repository": self.repo["root"], "number": number, "path": path,
                "original_path": original_path if original_path != path else None,
                "original": None if patch_only else original, "modified": None if patch_only else modified,
                "patch": patch, "binary": binary, "truncated": truncated}


def dispatch(request):
    if request["version"] != 2:
        raise ValueError("Unsupported protocol version")
    operation = request["operation"]
    if operation == "describe":
        return {"operations": ["list", "detail", "diff", "commit_links", "repository_info"]}
    github = GitHub(request)
    params = request["params"]
    if operation == "repository_info":
        return github.repository_info()
    if operation == "commit_links":
        return github.commit_links(params["commit"])
    if operation == "list":
        return github.list()
    if operation == "detail":
        return github.detail(params["number"])
    if operation == "diff":
        return github.diff(params["number"], params["path"], params.get("patch_only", False))
    raise ValueError("Unsupported operation: " + operation)


if __name__ == "__main__":
    try:
        response = {"version": 2, "result": dispatch(json.load(sys.stdin))}
    except Exception as error:
        response = {"version": 2, "error": {"code": "github_error", "message": str(error)}}
    print(json.dumps(response, ensure_ascii=False))
