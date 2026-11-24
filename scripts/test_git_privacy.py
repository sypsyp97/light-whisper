import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
PUBLIC_EMAIL = "123456+privacy-fixture@users.noreply.github.com"
PRIVATE_EMAIL = "private-fixture@example.invalid"
TOKEN = "gh" + "p_" + "F" * 36


class GitPrivacyTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.base = Path(temporary.name)
        self.repo = self.base / "checkout"
        self.remote = self.base / "upstream.git"
        self.repo.mkdir()
        home = self.base / "fixture-home"
        home.mkdir()
        self.env = {k: v for k, v in os.environ.items() if not k.upper().startswith("GIT_")}
        self.env.update(
            HOME=str(home),
            USERPROFILE=r"Q:\Profiles\FixtureBuildÉditeur",
            GIT_CONFIG_NOSYSTEM="1",
            GIT_CONFIG_GLOBAL=os.devnull,
            GIT_ALLOW_PROTOCOL="file",
            GIT_TERMINAL_PROMPT="0",
            GIT_AUTHOR_DATE="2000-01-01T00:00:00+00:00",
            GIT_COMMITTER_DATE="2000-01-01T00:00:00+00:00",
        )
        self.git("init", "-q", "-b", "main")
        self.git("init", "-q", "--bare", "-b", "main", str(self.remote))
        self.git("config", "user.name", "Privacy Fixture")
        self.git("config", "user.email", PUBLIC_EMAIL)
        self.git("config", "core.autocrlf", "false")
        self.git("remote", "add", "origin", str(self.remote))
        self.git("config", "core.hooksPath", ".githooks")
        for relative in (".githooks/pre-commit", ".githooks/pre-push", "scripts/git_privacy.py"):
            source = ROOT / relative
            if source.is_file():
                destination = self.repo / relative
                destination.parent.mkdir(exist_ok=True)
                shutil.copy2(source, destination)
                if relative.startswith(".githooks/"):
                    destination.chmod(0o755)

    def git(self, *args, check=True, env=None, stdin=None):
        return subprocess.run(
            ["git", *args], cwd=self.repo, env=self.env | (env or {}),
            input=stdin, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            check=check, timeout=30,
        )

    def stage(self, data):
        (self.repo / "notes").write_bytes(data)
        self.git("add", "notes")

    def denied(self, result, *redactions):
        self.assertNotEqual(result.returncode, 0, "Git accepted private data")
        for value in redactions:
            self.assertNotIn(value.encode(), result.stdout)

    def assert_unpublished(self, ref="refs/heads/main"):
        self.assertEqual(self.git("ls-remote", "origin", ref).stdout, b"")

    def test_staged_blob_rejected_even_if_worktree_was_cleaned(self):
        self.stage(TOKEN.encode())
        (self.repo / "notes").write_text("redacted", encoding="utf-8")
        self.denied(self.git("commit", "-qm", "snapshot", check=False), TOKEN)
        self.assertNotEqual(self.git("rev-parse", "--verify", "HEAD", check=False).returncode, 0)

    def test_push_rejects_intermediate_blob_absent_from_tip(self):
        self.stage(TOKEN.encode())
        self.git("-c", "core.hooksPath=disabled", "commit", "-qm", "intermediate")
        self.git("rm", "-q", "notes")
        self.git("-c", "core.hooksPath=disabled", "commit", "-qm", "clean tip")
        self.denied(self.git("push", "-q", "origin", "main", check=False), TOKEN)
        self.assert_unpublished()

    def test_local_paths_and_credential_formats_are_redacted(self):
        profile = self.env["USERPROFILE"]
        cases = [
            (self.env["HOME"], "utf-8"),
            (profile, "utf-8"),
            (profile.replace("\\", "/"), "utf-8"),
            (profile.replace("\\", "\\\\"), "utf-8"),
            (profile, "utf-16-le"),
            (profile, "utf-16-be"),
            ("github" + "_pat_" + "Q" * 82, "utf-8"),
            ("-----BEGIN " + "OPENSSH PRIVATE KEY-----", "utf-8"),
        ]
        for value, encoding in cases:
            with self.subTest(encoding=encoding, category=cases.index((value, encoding))):
                self.stage(("prefix " + value + " suffix").encode(encoding))
                self.denied(self.git("commit", "-qm", "snapshot", check=False), value)

    def test_unstaged_token_does_not_prevent_clean_commit(self):
        self.stage(b"public")
        (self.repo / "notes").write_bytes(TOKEN.encode())
        self.git("commit", "-qm", "public")
        self.assertEqual(self.git("show", "HEAD:notes").stdout, b"public")

    def test_current_private_commit_identity_is_blocked(self):
        for role in ("AUTHOR", "COMMITTER"):
            with self.subTest(role=role):
                self.denied(self.git("commit", "--allow-empty", "-qm", "metadata", check=False,
                                     env={f"GIT_{role}_EMAIL": PRIVATE_EMAIL}), PRIVATE_EMAIL)

    def test_outgoing_private_commit_identities_and_signoff_are_blocked(self):
        cases = [
            ("author", {"GIT_AUTHOR_EMAIL": PRIVATE_EMAIL}, "metadata"),
            ("committer", {"GIT_COMMITTER_EMAIL": PRIVATE_EMAIL}, "metadata"),
            ("signoff", {}, f"metadata\n\nSigned-off-by: Fixture <{PRIVATE_EMAIL}>"),
        ]
        for label, env, message in cases:
            with self.subTest(source=label):
                self.git("checkout", "-q", "--orphan", label)
                self.git("-c", "core.hooksPath=disabled", "commit", "--allow-empty",
                         "-qm", message, env=env)
                self.git("-c", "core.hooksPath=disabled", "commit", "--allow-empty",
                         "-qm", "public tip")
                self.denied(self.git("push", "-q", "origin", f"HEAD:refs/heads/{label}",
                                     check=False), PRIVATE_EMAIL)
                self.assert_unpublished(f"refs/heads/{label}")

    def test_annotated_tag_on_published_commit_checks_its_own_tagger(self):
        self.git("commit", "--allow-empty", "-qm", "public")
        self.git("push", "-q", "origin", "main")
        self.git("tag", "-am", "annotation", "version", env={"GIT_COMMITTER_EMAIL": PRIVATE_EMAIL})
        self.denied(self.git("push", "-q", "origin", "refs/tags/version", check=False),
                    PRIVATE_EMAIL)
        self.assert_unpublished("refs/tags/version")

    def test_clean_branch_and_annotated_tag_can_be_pushed_and_deleted(self):
        self.git("commit", "--allow-empty", "-qm", "public")
        self.git("branch", "topic")
        self.git("tag", "-am", "release", "release")
        self.git("push", "-q", "origin", "topic", "refs/tags/release")
        for ref in ("refs/heads/topic", "refs/tags/release"):
            expected = self.git("rev-parse", ref).stdout.strip()
            self.assertEqual(self.git("ls-remote", "origin", ref).stdout.split()[0], expected)
        self.git("push", "-q", "--delete", "origin", "topic", "release")
        self.assert_unpublished("refs/heads/topic")
        self.assert_unpublished("refs/tags/release")

    def test_previously_published_private_history_does_not_block_clean_ref_updates(self):
        self.stage(TOKEN.encode())
        self.git("-c", "core.hooksPath=disabled", "commit", "-qm", "published history",
                 env={"GIT_AUTHOR_EMAIL": PRIVATE_EMAIL})
        self.git("-c", "core.hooksPath=disabled", "push", "-qu", "origin", "main")
        (self.repo / "public").write_bytes(b"public")
        self.git("add", "public")
        self.git("commit", "-qm", "new public object")
        self.git("push", "-q", "origin", "HEAD:refs/heads/topic", "main")
        for ref in ("refs/heads/topic", "refs/heads/main"):
            self.assertEqual(self.git("ls-remote", "origin", ref).stdout.split()[0],
                             self.git("rev-parse", "HEAD").stdout.strip())

    def test_hook_rejects_credentialed_remote_url_without_echoing_password(self):
        password = "synthetic-fixture-password"
        result = self.git("hook", "run", "--ignore-missing", "pre-push", "--", "origin",
                          f"https://fixture:{password}@example.invalid/project", check=False)
        self.denied(result, password)

    def test_missing_verifier_prevents_commit_and_push(self):
        self.git("-c", "core.hooksPath=disabled", "commit", "--allow-empty", "-qm", "base")
        (self.repo / "scripts" / "git_privacy.py").unlink(missing_ok=True)
        self.denied(self.git("commit", "--allow-empty", "-qm", "clean", check=False))
        self.denied(self.git("push", "-q", "origin", "main", check=False))
        self.assert_unpublished()

    def test_private_filenames_are_checked_at_commit_and_push(self):
        (self.repo / TOKEN).write_bytes(b"public")
        self.git("add", TOKEN)
        with self.subTest(operation="commit"):
            self.denied(self.git("commit", "-qm", "filename", check=False), TOKEN)
        self.git("-c", "core.hooksPath=disabled", "commit", "--allow-empty", "-qm", "filename")
        with self.subTest(operation="push"):
            self.denied(self.git("push", "-q", "origin", "main", check=False), TOKEN)
            self.assert_unpublished()

    def test_private_destination_ref_rejected_without_echoing_name(self):
        self.git("commit", "--allow-empty", "-qm", "public")
        ref = f"refs/heads/{TOKEN}"
        self.denied(self.git("push", "-q", "origin", f"HEAD:{ref}", check=False), TOKEN)
        self.assert_unpublished(ref)

    def test_nested_annotation_checks_inner_unpublished_tagger(self):
        self.git("commit", "--allow-empty", "-qm", "public")
        self.git("push", "-q", "origin", "main")
        self.git("tag", "-am", "inner", "first", env={"GIT_COMMITTER_EMAIL": PRIVATE_EMAIL})
        self.git("tag", "-am", "outer", "second", "first")
        self.denied(self.git("push", "-q", "origin", "refs/tags/second", check=False),
                    PRIVATE_EMAIL)
        self.assert_unpublished("refs/tags/second")

    def test_stale_tracking_ref_cannot_hide_unpublished_objects(self):
        self.stage(TOKEN.encode())
        self.git("-c", "core.hooksPath=disabled", "commit", "-qm", "private")
        self.git("update-ref", "refs/remotes/origin/obsolete", "HEAD")
        self.denied(self.git("push", "-q", "origin", "main", check=False), TOKEN)
        self.assert_unpublished()

    def test_third_party_license_contact_can_be_published(self):
        (self.repo / "LICENSE").write_bytes(b"Copyright Example Vendor <legal@vendor.example>\n")
        self.git("add", "LICENSE")
        self.git("commit", "-qm", "upstream license")
        self.git("push", "-q", "origin", "main")
        self.assertEqual(self.git("ls-remote", "origin", "refs/heads/main").stdout.split()[0],
                         self.git("rev-parse", "HEAD").stdout.strip())

    def test_configured_private_emails_and_embedded_url_credentials(self):
        contacts = ("former-contact@example.invalid", "new-contact@example.invalid")
        for email in contacts:
            self.git("config", "--add", "privacy.privateEmail", email)
        password = "fixture-password"
        private_url = "https://" + "fixture:" + password + "@example.invalid/path"
        api_key = "s" + "k-" + "A" * 40
        cases = [(email, email) for email in contacts] + [(private_url, password), (api_key, api_key)]
        for index, (data, private) in enumerate(cases):
            self.git("checkout", "-q", "--orphan", f"case-{index}")
            self.stage(data.encode())
            with self.subTest(case=index, hook="commit"):
                self.denied(self.git("commit", "-qm", "content", check=False), private)
            self.git("-c", "core.hooksPath=disabled", "commit", "--allow-empty", "-qm", "content")
            with self.subTest(case=index, hook="push"):
                ref = f"refs/heads/case-{index}"
                self.denied(self.git("push", "-q", "origin", f"HEAD:{ref}", check=False), private)
                self.assert_unpublished(ref)

    def test_css_mask_and_noreply_signoff_are_public(self):
        self.stage(b".icon-mask-controlled-by-user-preferences { -webkit-mask-image: none; }\n")
        self.git("commit", "-qm", f"public\n\nSigned-off-by: Fixture <{PUBLIC_EMAIL}>")
        self.git("push", "-q", "origin", "main")
        self.assertEqual(self.git("ls-remote", "origin", "refs/heads/main").stdout.split()[0],
                         self.git("rev-parse", "HEAD").stdout.strip())


if __name__ == "__main__":
    unittest.main()
