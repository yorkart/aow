import base64
import copy
import json
from pathlib import Path
import subprocess
import sys
import unittest
from unittest.mock import patch
import github

REQUEST = {"version": 2, "operation": "list", "repository": {
    "root": "/tmp", "host": "github.example.com", "path": "team/project", "remote": "upstream"}, "params": {}}
PR = {"number": 42, "state": "open", "draft": False, "title": "Example", "body": "Description",
      "head": {"ref": "feature", "sha": "head-sha", "repo": None},
      "base": {"ref": "main", "sha": "base-sha", "repo": {"full_name": "team/project"}},
      "html_url": "https://github.example.com/team/project/pull/42", "created_at": "", "updated_at": "",
      "user": {"id": 1, "login": "alice"}, "changed_files": 1, "commits": 2, "labels": []}
COMMENT = {"id": "comment", "author": {"login": "alice"}, "body": "hello", "createdAt": "", "updatedAt": ""}


class AdapterTests(unittest.TestCase):
    def setUp(self):
        self.client = github.GitHub(copy.deepcopy(REQUEST))

    def test_describe_does_not_need_cli_or_repository(self):
        self.assertEqual(github.dispatch({"version": 2, "operation": "describe", "repository": None}),
                         {"operations": ["list", "detail", "diff", "commit_links", "repository_info"]})

    def test_repository_info_uses_owner_avatar_for_users_and_organizations(self):
        request = dict(REQUEST, operation="repository_info")
        for kind in ("User", "Organization"):
            with self.subTest(kind=kind), patch.object(github.GitHub, "api", return_value={
                    "owner": {"type": kind, "login": "team", "avatar_url": "https://avatars.example.com/team"}}) as api:
                self.assertEqual(github.dispatch(request), {"avatar_url": "https://avatars.example.com/team"})
                api.assert_called_once_with("repos/team/project")
        with patch.object(self.client, "api", return_value={"owner": {}}):
            self.assertEqual(self.client.repository_info(), {"avatar_url": None})
        with patch.object(self.client, "api", side_effect=RuntimeError("auth required")):
            with self.assertRaisesRegex(RuntimeError, "auth required"):
                self.client.repository_info()

    def test_commit_links_use_configured_host_without_cli_or_network(self):
        request = copy.deepcopy(REQUEST)
        request.update(operation="commit_links", params={"commit": "a" * 40})
        with patch("subprocess.run", side_effect=AssertionError("links do not need gh")):
            self.assertEqual(github.dispatch(request), {
                "remote_url": "https://github.example.com/team/project",
                "commit_url": "https://github.example.com/team/project/commit/" + "a" * 40,
            })

    def test_gh_command_hostname_and_paginated_pages(self):
        with patch.object(self.client, "command", return_value=[[1, 2], [3]]) as command:
            self.assertEqual(self.client.api("repos/team/project/pulls?per_page=100", pages=True), [1, 2, 3])
            self.assertEqual(command.call_args.args, ("api", "--hostname", "github.example.com", "repos/team/project/pulls?per_page=100", "--paginate", "--slurp"))
        with patch("subprocess.run", return_value=subprocess.CompletedProcess([], 0, b'{}', b'')) as run:
            self.client.command("api", "user")
            self.assertEqual(run.call_args.args[0], ["gh", "api", "user"])
            self.assertNotIn("shell", run.call_args.kwargs)

    def test_list_filters_author_after_pagination(self):
        other = dict(PR, user={"id": 2})
        with patch.object(self.client, "api", side_effect=[PR["user"], [other, PR]]), patch("subprocess.run", return_value=subprocess.CompletedProcess([], 0, "feature\n", "")):
            result = self.client.list()
        self.assertEqual([p["number"] for p in result["pull_requests"]], [42])
        self.assertEqual(result["current_branch"], "feature")
        self.assertEqual(result["current_user"]["username"], "alice")

    def test_detail_failure_in_optional_sections_is_visible(self):
        with patch.object(self.client, "api", side_effect=[PR, RuntimeError("comments unavailable")]), patch.object(self.client, "files", side_effect=RuntimeError("files unavailable")), patch.object(self.client, "command", side_effect=RuntimeError("checks unavailable")), patch.object(self.client, "review_threads", side_effect=RuntimeError("threads unavailable")):
            result = self.client.detail(42)
        self.assertEqual(result["number"], 42)
        self.assertGreaterEqual(len(result["warnings"]), 4)
        self.assertIsNone(result["mergeable"])
        self.assertEqual(result["check_summary_status"], "unknown")

    def test_review_thread_and_nested_comment_pagination(self):
        first = {"id": "t1", "path": "file", "line": 1, "isResolved": False, "comments": {
            "nodes": [COMMENT], "pageInfo": {"hasNextPage": True, "endCursor": "comments-2"}}}
        second = {"id": "t2", "path": "file", "line": 2, "isResolved": True, "comments": {
            "nodes": [COMMENT], "pageInfo": {"hasNextPage": False}}}
        responses = [
            {"repository": {"pullRequest": {"reviewThreads": {"nodes": [first], "pageInfo": {"hasNextPage": True, "endCursor": "threads-2"}}}}},
            {"node": {"comments": {"nodes": [dict(COMMENT, id="reply")], "pageInfo": {"hasNextPage": False}}}},
            {"repository": {"pullRequest": {"reviewThreads": {"nodes": [second], "pageInfo": {"hasNextPage": False}}}}},
        ]
        with patch.object(self.client, "graphql", side_effect=responses) as graphql:
            result = self.client.review_threads(42)
        self.assertEqual([t["status"] for t in result], ["open", "resolved"])
        self.assertEqual(len(result[0]["comments"]), 2)
        self.assertEqual(graphql.call_args_list[1].args[1]["cursor"], "comments-2")
        self.assertEqual(graphql.call_args_list[2].args[1]["cursor"], "threads-2")

    def test_diff_uses_merge_base_and_handles_rename_deleted_fork_and_no_newline(self):
        with patch.object(self.client, "api", side_effect=[PR, {"merge_base_commit": {"sha": "merge-base"}}]), patch.object(self.client, "files", return_value=[{"filename": "new name", "previous_filename": "old name", "status": "renamed"}]), patch.object(self.client, "content", side_effect=[("before", False, False), ("after", False, False)]) as content:
            result = self.client.diff(42, "new name", patch_only=True)
        self.assertEqual(content.call_args_list[0].args, ("team/project", "old name", "merge-base"))
        self.assertEqual(content.call_args_list[1].args, ("team/project", "new name", "head-sha"))
        self.assertEqual(result["original_path"], "old name")
        self.assertIsNone(result["original"])
        self.assertIn("-before\n\\ No newline at end of file\n+after\n", result["patch"])

    def test_added_removed_binary_and_oversize_diff(self):
        for status, content_value in [("added", ("new\n", False, False)), ("removed", ("old\n", False, False)), ("added", (None, True, False)), ("added", (None, False, True))]:
            with self.subTest(status=status, content=content_value), patch.object(self.client, "api", side_effect=[PR, {"merge_base_commit": {"sha": "merge-base"}}]), patch.object(self.client, "files", return_value=[{"filename": "file", "status": status}]), patch.object(self.client, "content", return_value=content_value) as content:
                result = self.client.diff(42, "file")
            self.assertEqual(content.call_count, 1)
            self.assertEqual(result["binary"], content_value[1])
            self.assertEqual(result["truncated"], content_value[2])
            if status == "removed":
                self.assertEqual(result["modified"], "")
            else:
                self.assertEqual(result["original"], "")

    def test_content_encodes_paths_and_distinguishes_binary_from_missing(self):
        cases = [(b"hello", ("hello", False, False)), (b"a\0b", (None, True, False)), (b"\xff", (None, True, False))]
        for data, expected in cases:
            with patch.object(self.client, "api", return_value={"size": len(data), "encoding": "base64", "content": base64.b64encode(data).decode()}) as api:
                self.assertEqual(self.client.content("team/project", "a #?.txt", "sha"), expected)
                self.assertIn("a%20%23%3F.txt?ref=sha", api.call_args.args[0])
        with patch.object(self.client, "api", return_value={"size": 2000000, "encoding": "none"}):
            self.assertEqual(self.client.content("team/project", "file", "sha"), (None, False, True))
        with patch.object(self.client, "api", side_effect=RuntimeError("404")):
            with self.assertRaisesRegex(RuntimeError, "404"):
                self.client.content("team/project", "file", "sha")

    def test_process_emits_error_envelope_without_debug_stdout(self):
        script = Path(github.__file__)
        result = subprocess.run([sys.executable, str(script)], input='{"version":9}', capture_output=True, text=True, check=True)
        response = json.loads(result.stdout)
        self.assertEqual(response["version"], 2)
        self.assertIn("error", response)
        self.assertEqual(result.stderr, "")


if __name__ == '__main__':
    unittest.main()
