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
        for relative in (".githooks/pre-commit", ".githooks/pre-push",
                         ".githooks/reference-transaction", "scripts/git_privacy.py"):
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
        self.git("-c", "core.hooksPath=disabled", "tag", "-am", "annotation", "version",
                 env={"GIT_COMMITTER_EMAIL": PRIVATE_EMAIL})
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
        self.git("-c", "core.hooksPath=disabled", "tag", "-am", "inner", "first",
                 env={"GIT_COMMITTER_EMAIL": PRIVATE_EMAIL})
        self.git("-c", "core.hooksPath=disabled", "tag", "-am", "outer", "second", "first")
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

    def test_binary_bytes_after_http_prefix_are_not_url_authority(self):
        self.stage(b"\x7fPE\0http://" + b"\x01\xff\x02" * 80 + b"@\x80\x03")
        self.git("commit", "-qm", "public binary")
        self.git("push", "-q", "origin", "main")
        password = "synthetic-url-password"
        credential = "https://" + "fixture:" + password + "@example.invalid/"
        self.stage(credential.encode("utf-16-le"))
        self.denied(self.git("commit", "-qm", "wide URL", check=False), password)

    def historical_commit(self):
        tree = self.git("mktree", stdin=b"").stdout.strip().decode()
        return self.git("commit-tree", tree, "-m", "historical",
                        env={"GIT_AUTHOR_EMAIL": PRIVATE_EMAIL}).stdout.strip().decode()

    def test_local_tag_rejects_private_metadata_before_creating_ref(self):
        target = self.historical_commit()
        profile = self.env["USERPROFILE"]
        for name, message, env, secret in (
            ("tagger", "release", {"GIT_COMMITTER_EMAIL": PRIVATE_EMAIL}, PRIVATE_EMAIL),
            ("contact", "Contact " + PRIVATE_EMAIL, {}, PRIVATE_EMAIL),
            ("path", "Built at " + profile, {}, profile),
            ("token", "Build " + TOKEN, {}, TOKEN),
        ):
            with self.subTest(name=name):
                result = self.git("tag", "-am", message, name, target, check=False, env=env)
                self.denied(result, secret)
                self.assertEqual(self.git("tag", "--list", name).stdout, b"")

    def test_local_tag_accepts_clean_annotations_and_lightweight_history(self):
        target = self.historical_commit()
        self.git("tag", "light", target, env={"GIT_COMMITTER_EMAIL": PRIVATE_EMAIL})
        note = "Docs https://docs.example.invalid/users/guide\n\ntagger examples follow\n\nRelease notes"
        self.git("tag", "-am", note, "annotated", target)
        self.git("tag", "-am", "release notes", "outer", "annotated")
        for name, kind in (("light", b"commit"), ("annotated", b"tag"), ("outer", b"tag")):
            with self.subTest(name=name):
                self.assertEqual(self.git("cat-file", "-t", name).stdout.strip(), kind)
                self.assertEqual(self.git("rev-parse", name + "^{}").stdout.strip().decode(), target)

    def test_local_tag_rejects_other_machines_profile_paths_in_annotation(self):
        target = self.historical_commit()
        for name, path in (("windows", "C:/Users/AnotherFixture/source"),
                           ("linux", "/home/AnotherFixture/work"),
                           ("mac", "/Users/AnotherFixture/work")):
            with self.subTest(name=name):
                self.denied(self.git("tag", "-am", "Built at " + path + "\n\nRelease notes", name, target,
                                     check=False), path)
                self.assertEqual(self.git("tag", "--list", name).stdout, b"")

    def test_local_tag_rejects_private_names_on_lightweight_tags(self):
        target = self.historical_commit()
        for name in (PRIVATE_EMAIL, TOKEN):
            with self.subTest(category="contact" if name == PRIVATE_EMAIL else "token"):
                self.denied(self.git("tag", name, target, check=False), name)
                self.assertEqual(self.git("tag", "--list", name).stdout, b"")

    def test_local_tag_missing_verifier_blocks_tag_and_allows_branch(self):
        target = self.historical_commit()
        (self.repo / "scripts" / "git_privacy.py").unlink()
        self.git("branch", "fixture-topic", target)
        self.assertEqual(self.git("rev-parse", "fixture-topic").stdout.strip().decode(), target)
        self.denied(self.git("tag", "clean", target, check=False))
        self.assertEqual(self.git("tag", "--list", "clean").stdout, b"")

    def test_local_tag_rejects_private_inner_annotation(self):
        target = self.historical_commit()
        self.git("-c", "core.hooksPath=disabled", "tag", "-am", TOKEN, "legacy", target)
        previous = self.git("rev-parse", "refs/tags/legacy").stdout
        result = self.git("tag", "-am", "public outer", "outer", "legacy", check=False)
        self.denied(result, TOKEN)
        self.assertEqual(self.git("tag", "--list", "outer").stdout, b"")
        self.assertEqual(self.git("rev-parse", "refs/tags/legacy").stdout, previous)

    def test_local_tag_transaction_rejection_preserves_all_refs(self):
        target = self.historical_commit()
        self.git("tag", "stable", target)
        self.git("-c", "core.hooksPath=disabled", "tag", "-am", TOKEN, "legacy", target)
        private_oid = self.git("rev-parse", "refs/tags/legacy").stdout.strip().decode()
        updates = (f"create refs/tags/companion {target}\n"
                   f"update refs/tags/stable {private_oid} {target}\n").encode()
        self.denied(self.git("update-ref", "--stdin", stdin=updates, check=False), TOKEN)
        self.assertEqual(self.git("tag", "--list", "companion").stdout, b"")
        self.assertEqual(self.git("rev-parse", "refs/tags/stable").stdout.strip().decode(), target)

    def test_local_tag_ignores_unrelated_verification_deletion_and_other_refs(self):
        target = self.historical_commit()
        self.git("-c", "core.hooksPath=disabled", "tag", "-am", TOKEN, "legacy", target)
        old = self.git("rev-parse", "refs/tags/legacy").stdout.strip().decode()
        updates = (f"verify refs/tags/legacy {old}\n"
                   f"create refs/fixture/import {old}\n"
                   f"create refs/tags/clean {target}\n").encode()
        self.git("update-ref", "--stdin", stdin=updates)
        self.assertEqual(self.git("rev-parse", "refs/fixture/import").stdout.strip().decode(), old)
        self.assertEqual(self.git("rev-parse", "refs/tags/clean").stdout.strip().decode(), target)
        self.git("pack-refs", "--all")
        self.assertEqual(self.git("rev-parse", "refs/tags/legacy").stdout.strip().decode(), old)
        self.git("tag", "-d", "legacy")
        self.assertEqual(self.git("tag", "--list", "legacy").stdout, b"")


if __name__ == "__main__":
    unittest.main()
